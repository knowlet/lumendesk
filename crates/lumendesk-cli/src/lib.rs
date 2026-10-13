//! Project tooling for independently designed LumenDesk applications.
mod build;
mod config;
mod doctor;
mod output;
mod package;
mod scaffold;
mod typescript;

pub use config::{Config, Mode};
pub use scaffold::create_project;
pub use typescript::{generate_typescript, sample_schema};

use std::path::PathBuf;

type Result<T> = std::result::Result<T, String>;

const HELP: &str = "LumenDesk — independent Rust desktop project tools

Usage:
  lumendesk new NAME [--mode native|web] [--sdk-path PATH]
  lumendesk doctor [--mode native|web] [--project DIR]
  lumendesk build [--project DIR] [--offline]
  lumendesk package --kind source|binary --out FILE.tar.gz [--project DIR] [--offline]
  lumendesk dev [--project DIR] [--offline] [--once] [--poll-ms 500]
  lumendesk bindings [--project DIR] [--schema FILE] [--out FILE]
  lumendesk bindings --sample [--out FILE]
  lumendesk --version

new refuses populated directories. build produces a release executable for this host.
dev rebuilds source changes, preserving the running app on compiler failure. It stops
and waits for the old child before starting a successfully built replacement.
--once builds, launches once, and waits for that app to exit. Ctrl-C stops the app.
bindings --sample prints a sample client unless --out is given.
package creates a portable archive with checksums and dependency/runtime notes.
Installers, signing, publishing, and cross-compilation are outside this milestone.";

struct Arguments {
    values: Vec<String>,
}
impl Arguments {
    fn flag(&mut self, key: &str) -> bool {
        if let Some(index) = self.values.iter().position(|arg| arg == key) {
            self.values.remove(index);
            true
        } else {
            false
        }
    }
    fn value(&mut self, key: &str) -> Result<Option<String>> {
        let Some(index) = self.values.iter().position(|arg| arg == key) else {
            return Ok(None);
        };
        self.values.remove(index);
        if index >= self.values.len() || self.values[index].starts_with("--") {
            return Err(format!("{key} requires a value; run lumendesk --help"));
        }
        Ok(Some(self.values.remove(index)))
    }
    fn finish(self) -> Result<()> {
        if self.values.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "unexpected argument(s): {}; run lumendesk --help",
                self.values.join(" ")
            ))
        }
    }
}

/// Execute one CLI command. Errors are suitable for a diagnostic and nonzero exit.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<()> {
    let mut args = Arguments {
        values: args.into_iter().collect(),
    };
    if args.values.is_empty() {
        println!("{HELP}");
        return Ok(());
    }
    let command = args.values.remove(0);
    if matches!(command.as_str(), "--help" | "-h" | "help") {
        args.finish()?;
        println!("{HELP}");
        return Ok(());
    }
    if matches!(command.as_str(), "--version" | "-V" | "version") {
        args.finish()?;
        println!("lumendesk {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.flag("--help") || args.flag("-h") {
        println!("{HELP}");
        return Ok(());
    }
    match command.as_str() {
        "new" => {
            let mode = args
                .value("--mode")?
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(Mode::Native);
            let sdk = args.value("--sdk-path")?.map(PathBuf::from);
            if args.values.len() != 1 {
                return Err(
                    "new requires one directory name; try lumendesk new hello --mode native".into(),
                );
            }
            let destination = PathBuf::from(args.values.remove(0));
            let path = create_project(&destination, mode, sdk.as_deref())?;
            println!("Created {} project at {}", mode.as_str(), path.display());
            println!(
                "Next: cd {} && lumendesk doctor && lumendesk dev",
                destination.display()
            );
            Ok(())
        }
        "doctor" => {
            let project = PathBuf::from(args.value("--project")?.unwrap_or_else(|| ".".into()));
            let mode = args
                .value("--mode")?
                .map(|value| value.parse())
                .transpose()?;
            args.finish()?;
            let mode = if let Some(mode) = mode {
                mode
            } else if project.join("lumendesk.toml").exists() {
                Config::load(&project)?.app.mode
            } else {
                Mode::Native
            };
            doctor::check(mode)
        }
        "build" | "dev" => {
            let project = PathBuf::from(args.value("--project")?.unwrap_or_else(|| ".".into()));
            let offline = args.flag("--offline");
            let once = command == "dev" && args.flag("--once");
            let poll = if command == "dev" {
                args.value("--poll-ms")?
            } else {
                None
            };
            let poll = poll
                .map(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|_| "--poll-ms must be a positive integer".to_string())
                })
                .transpose()?
                .unwrap_or(500);
            if !(50..=60_000).contains(&poll) {
                return Err("--poll-ms must be between 50 and 60000".into());
            }
            args.finish()?;
            let project = project
                .canonicalize()
                .map_err(|error| format!("cannot open project {}: {error}", project.display()))?;
            let config = Config::load(&project)?;
            if command == "build" {
                let binary = build::compile(&project, &config, true, offline)?;
                println!("Release executable: {}", binary.display());
                Ok(())
            } else {
                build::develop(&project, config, once, offline, poll)
            }
        }
        "package" => {
            let project = PathBuf::from(args.value("--project")?.unwrap_or_else(|| ".".into()));
            let kind =
                package::Kind::parse(&args.value("--kind")?.unwrap_or_else(|| "source".into()))?;
            let output = PathBuf::from(
                args.value("--out")?
                    .ok_or("package requires --out FILE.tar.gz")?,
            );
            let offline = args.flag("--offline");
            args.finish()?;
            package::create(&project, kind, &output, offline)?;
            println!("Portable archive: {}", output.display());
            Ok(())
        }
        "bindings" => {
            let project = PathBuf::from(args.value("--project")?.unwrap_or_else(|| ".".into()));
            let schema = args.value("--schema")?.map(PathBuf::from);
            let output = args.value("--out")?.map(PathBuf::from);
            let sample = args.flag("--sample");
            args.finish()?;
            if sample && schema.is_some() {
                return Err("--sample and --schema cannot be used together".into());
            }
            let (input, output, root) = if sample {
                (sample_schema(), output, None)
            } else if let Some(schema) = schema {
                (read_schema(&schema)?, output, None)
            } else {
                let config = Config::load(&project)?;
                let root = if output.is_none() {
                    Some(project.clone())
                } else {
                    None
                };
                (
                    read_schema(&project.join(&config.bindings.schema))?,
                    Some(output.unwrap_or_else(|| project.join(&config.bindings.output))),
                    root,
                )
            };
            let text = generate_typescript(&input)?;
            if let Some(path) = output {
                output::write_generated(&path, &text, root.as_deref())?;
                println!("Generated {}", path.display());
            } else {
                print!("{text}")
            }
            Ok(())
        }
        _ => Err(format!("unknown command '{command}'; run lumendesk --help")),
    }
}

fn read_schema(path: &std::path::Path) -> Result<serde_json::Value> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read bindings schema {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("invalid bindings schema {}: {error}", path.display()))
}
