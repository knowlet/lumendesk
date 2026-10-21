use lumendesk_cli::{Config, Mode, create_project, generate_typescript, sample_schema};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lumendesk-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lumendesk"))
        .args(args)
        .output()
        .unwrap()
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn scaffolds_both_modes_in_fresh_and_empty_directories() {
    let temp = Temp::new();
    for (name, mode) in [("native-form", Mode::Native), ("web-form", Mode::Web)] {
        let destination = temp.join(name);
        if mode == Mode::Web {
            fs::create_dir(&destination).unwrap();
        }
        create_project(&destination, mode, None).unwrap();
        let config = Config::load(&destination).unwrap();
        assert_eq!(config.app.mode, mode);
        assert_eq!(config.app.name, name);
        let main = fs::read_to_string(destination.join("src/main.rs")).unwrap();
        if mode == Mode::Native {
            assert!(main.contains("run_form"));
            assert!(!main.contains("webview"));
        } else {
            assert!(main.contains("run_html"));
            assert!(destination.join("web/index.html").is_file());
        }
        let schema: serde_json::Value =
            serde_json::from_slice(&fs::read(destination.join("bindings.json")).unwrap()).unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("web/lumendesk.generated.ts")).unwrap(),
            generate_typescript(&schema).unwrap()
        );
    }
}

#[test]
fn populated_destination_is_preserved_exactly() {
    let temp = Temp::new();
    let target = temp.join("existing");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), "user work").unwrap();
    assert!(
        create_project(&target, Mode::Native, None)
            .unwrap_err()
            .contains("populated")
    );
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).unwrap(),
        "user work"
    );
    assert_eq!(fs::read_dir(target).unwrap().count(), 1);
}

#[test]
fn invalid_names_modes_and_sdk_fail_before_creating_files() {
    let temp = Temp::new();
    for name in ["invalid name", "123start", "test"] {
        let destination = temp.join(name);
        assert!(create_project(&destination, Mode::Native, None).is_err());
        assert!(!destination.exists());
    }
    let destination = temp.join("valid");
    assert!(create_project(&destination, Mode::Web, Some(&temp.join("no-sdk"))).is_err());
    assert!(!destination.exists());
    let output = cli(&["new", destination.to_str().unwrap(), "--mode", "magic"]);
    assert!(!output.status.success());
    assert!(text(&output).contains("choose native or web"));
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn destination_symlink_is_rejected() {
    let temp = Temp::new();
    fs::create_dir(temp.join("real")).unwrap();
    std::os::unix::fs::symlink(temp.join("real"), temp.join("alias")).unwrap();
    assert!(create_project(&temp.join("alias"), Mode::Native, None).is_err());
    assert_eq!(fs::read_dir(temp.join("real")).unwrap().count(), 0);
}

#[test]
fn help_version_unknown_commands_and_missing_config() {
    assert!(text(&cli(&["--help"])).contains("lumendesk bindings"));
    assert_eq!(
        String::from_utf8(cli(&["--version"]).stdout)
            .unwrap()
            .trim(),
        "lumendesk 0.1.0"
    );
    let output = cli(&["unexpected"]);
    assert!(!output.status.success());
    assert!(text(&output).contains("unknown command"));
    let temp = Temp::new();
    let output = cli(&["build", "--project", temp.0.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(text(&output).contains("lumendesk.toml"));
    assert!(!cli(&["build", "--target", "other-os"]).status.success());
    assert!(!cli(&["new", "--mode"]).status.success());
}

#[test]
fn config_rejects_unknown_fields_unsupported_versions_and_escaping_paths() {
    let temp = Temp::new();
    let target = temp.join("configuration");
    create_project(&target, Mode::Native, None).unwrap();
    let config = Config::load(&target).unwrap();
    let mut unsupported = config.clone();
    unsupported.schema_version = 99;
    assert!(unsupported.validate().is_err());
    let mut escaping = config.clone();
    escaping.build.manifest = "../Cargo.toml".into();
    assert!(escaping.validate().is_err());
    let mut identity = config.clone();
    identity.app.id = "not-an-id".into();
    assert!(identity.validate().is_err());
    let original = fs::read_to_string(target.join("lumendesk.toml")).unwrap();
    fs::write(
        target.join("lumendesk.toml"),
        format!("unknown = 1\n{original}"),
    )
    .unwrap();
    assert!(Config::load(&target).is_err());
}

#[test]
fn bindings_are_deterministic_and_validate_registry() {
    let a = cli(&["bindings", "--sample"]);
    let b = cli(&["bindings", "--sample"]);
    assert!(a.status.success());
    assert_eq!(a.stdout, b.stdout);
    let generated = String::from_utf8(a.stdout).unwrap();
    assert!(generated.contains("\"greet\""));
    assert!(generated.contains("\"submissions\": number"));
    assert!(generated.contains("AbortSignal"));
    let mut invalid = sample_schema();
    invalid["methods"]["greet"]["name"] = serde_json::json!("wrong");
    assert!(
        generate_typescript(&invalid)
            .unwrap_err()
            .contains("does not match")
    );
    invalid = sample_schema();
    invalid["unknown"] = serde_json::json!(true);
    assert!(generate_typescript(&invalid).is_err());
    let temp = Temp::new();
    let file = temp.join("client.ts");
    assert!(
        cli(&["bindings", "--sample", "--out", file.to_str().unwrap()])
            .status
            .success()
    );
    assert_eq!(fs::read_to_string(file).unwrap(), generated);
}

fn fixture(temp: &Temp, source: &str) -> PathBuf {
    let project = temp.join("fixture-app");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname=\"fixture-app\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n",
    )
    .unwrap();
    fs::write(project.join("lumendesk.toml"), "schema_version=1\n[app]\nname=\"fixture-app\"\nid=\"dev.example.fixture\"\nversion=\"0.1.0\"\nmode=\"native\"\n[build]\nmanifest=\"Cargo.toml\"\nbinary=\"fixture-app\"\n").unwrap();
    fs::write(project.join("src/main.rs"), source).unwrap();
    project
}

#[test]
fn release_build_produces_real_executable() {
    let temp = Temp::new();
    let project = fixture(&temp, "fn main() { println!(\"built-by-real-cargo\"); }");
    let output = cli(&["build", "--project", project.to_str().unwrap(), "--offline"]);
    assert!(output.status.success(), "{}", text(&output));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let binary = stdout
        .lines()
        .find_map(|line| line.strip_prefix("Release executable: "))
        .unwrap();
    assert!(Path::new(binary).is_file());
    let launched = Command::new(binary).output().unwrap();
    assert!(launched.status.success());
    assert_eq!(
        String::from_utf8(launched.stdout).unwrap().trim(),
        "built-by-real-cargo"
    );
}

#[test]
fn dev_once_sets_separate_identity_and_propagates_app_failure() {
    let temp = Temp::new();
    let project = fixture(
        &temp,
        "fn main() { println!(\"{}|{}|{}\", std::env::var(\"LUMENDESK_APP_ID\").unwrap(), std::env::var(\"LUMENDESK_ENV\").unwrap(), std::env::var(\"LUMENDESK_DATA_DIR\").unwrap()); }",
    );
    let output = cli(&[
        "dev",
        "--project",
        project.to_str().unwrap(),
        "--once",
        "--offline",
    ]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("dev.example.fixture.development|development|"));
    assert!(text(&output).contains(".lumendesk/development-data"));
    fs::write(
        project.join("src/main.rs"),
        "fn main() { std::process::exit(7); }",
    )
    .unwrap();
    let failed = cli(&[
        "dev",
        "--project",
        project.to_str().unwrap(),
        "--once",
        "--offline",
    ]);
    assert!(!failed.status.success());
    assert!(text(&failed).contains("development app exited"));
}

#[cfg(unix)]
#[test]
fn dev_preserves_working_process_on_failure_then_releases_resources_before_swap() {
    let temp = Temp::new();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let source = |version: &str| {
        format!(
            r#"fn main() {{
        let _listener = std::net::TcpListener::bind("127.0.0.1:{port}").expect("old process must release its socket before replacement starts");
        loop {{ println!("{version} {{}}", std::process::id()); std::thread::sleep(std::time::Duration::from_millis(80)); }}
    }}"#
        )
    };
    let project = fixture(&temp, &source("VERSION_ONE"));
    fs::create_dir_all(project.join(".lumendesk")).unwrap();
    let stdout_path = project.join(".lumendesk/stdout.log");
    let stderr_path = project.join(".lumendesk/stderr.log");
    let child = Command::new(env!("CARGO_BIN_EXE_lumendesk"))
        .args([
            "dev",
            "--project",
            project.to_str().unwrap(),
            "--offline",
            "--poll-ms",
            "50",
        ])
        .stdout(Stdio::from(fs::File::create(&stdout_path).unwrap()))
        .stderr(Stdio::from(fs::File::create(&stderr_path).unwrap()))
        .spawn()
        .unwrap();
    struct Cleanup(std::process::Child);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = Command::new("kill")
                    .args(["-INT", &self.0.id().to_string()])
                    .status();
            }
            let _ = self.0.wait();
        }
    }
    let mut child = Cleanup(child);
    let wait_for = |path: &Path, needle: &str| {
        let until = Instant::now() + Duration::from_secs(45);
        loop {
            let contents = fs::read_to_string(path).unwrap_or_default();
            if contents.contains(needle) {
                return contents;
            }
            assert!(
                Instant::now() < until,
                "timed out waiting for {needle}\nstdout:{}\nstderr:{}",
                fs::read_to_string(&stdout_path).unwrap_or_default(),
                fs::read_to_string(&stderr_path).unwrap_or_default()
            );
            thread::sleep(Duration::from_millis(50));
        }
    };
    wait_for(&stdout_path, "VERSION_ONE");
    fs::write(
        project.join("src/main.rs"),
        "compile_error!(\"deliberate-rebuild-failure\");",
    )
    .unwrap();
    wait_for(&stderr_path, "Rebuild rejected");
    let before = fs::read_to_string(&stdout_path)
        .unwrap()
        .matches("VERSION_ONE")
        .count();
    thread::sleep(Duration::from_millis(250));
    let after = fs::read_to_string(&stdout_path)
        .unwrap()
        .matches("VERSION_ONE")
        .count();
    assert!(
        after > before,
        "previous app must keep running while compilation fails"
    );
    fs::write(project.join("src/main.rs"), source("VERSION_TWO")).unwrap();
    wait_for(&stdout_path, "VERSION_TWO");
    thread::sleep(Duration::from_millis(150));
    let contents = fs::read_to_string(&stdout_path).unwrap();
    let (_, after_replacement) = contents.split_once("VERSION_TWO").unwrap();
    assert!(!after_replacement.contains("VERSION_ONE"));
    assert!(
        !fs::read_to_string(&stderr_path)
            .unwrap()
            .contains("old process must release")
    );
    assert!(
        Command::new("kill")
            .args(["-INT", &child.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(child.0.wait().unwrap().success());
}

#[test]
fn missing_cargo_and_malformed_schema_are_actionable_failures() {
    let temp = Temp::new();
    let project = fixture(&temp, "fn main() {}");
    let output = Command::new(env!("CARGO_BIN_EXE_lumendesk"))
        .args(["build", "--project", project.to_str().unwrap()])
        .env("PATH", temp.join("nonexistent"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("could not run cargo"));
    let malformed = temp.join("malformed.json");
    fs::write(&malformed, "this is not JSON").unwrap();
    let output = cli(&["bindings", "--schema", malformed.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(text(&output).contains("invalid bindings schema"));
}

#[test]
fn doctor_reports_missing_tools_without_claiming_success() {
    let temp = Temp::new();
    let output = Command::new(env!("CARGO_BIN_EXE_lumendesk"))
        .args(["doctor", "--mode", "native"])
        .env("PATH", &temp.0)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("MISSING cargo"));
    assert!(text(&output).contains("MISSING rustc"));
    assert!(text(&output).contains("https://rustup.rs/"));
}

#[cfg(unix)]
#[test]
fn doctor_rejects_old_rust_compiler() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new();
    for (name, body) in [
        ("cargo", "echo cargo 1.94.0"),
        ("rustc", "echo rustc 1.94.0"),
        ("cc", "exit 0"),
        ("pkg-config", "exit 0"),
        ("xcrun", "exit 0"),
    ] {
        let path = temp.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_lumendesk"))
        .args(["doctor", "--mode", "native"])
        .env("PATH", &temp.0)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("Rust 1.95 or newer is required"));
    assert!(text(&output).contains("rustup update stable"));
}

#[cfg(unix)]
#[test]
fn configured_bindings_reject_symlink_files_and_directories_without_touching_external_data() {
    use std::os::unix::fs::symlink;
    for directory_link in [false, true] {
        let temp = Temp::new();
        let project = fixture(&temp, "fn main() {}");
        fs::write(
            project.join("bindings.json"),
            serde_json::to_vec(&sample_schema()).unwrap(),
        )
        .unwrap();
        let external = temp.join("outside");
        fs::create_dir(&external).unwrap();
        let sentinel = external.join("lumendesk.generated.ts");
        fs::write(&sentinel, "do not touch my data").unwrap();
        if directory_link {
            symlink(&external, project.join("web")).unwrap();
        } else {
            fs::create_dir(project.join("web")).unwrap();
            symlink(&sentinel, project.join("web/lumendesk.generated.ts")).unwrap();
        }
        let output = cli(&["bindings", "--project", project.to_str().unwrap()]);
        assert!(!output.status.success(), "{}", text(&output));
        assert!(text(&output).contains("symlink"));
        assert_eq!(
            fs::read_to_string(&sentinel).unwrap(),
            "do not touch my data"
        );
    }
}

#[cfg(unix)]
#[test]
fn explicit_bindings_output_rejects_symlinks_and_replaces_hardlinks_safely() {
    let temp = Temp::new();
    let external = temp.join("important.ts");
    let output_path = temp.join("generated.ts");
    fs::write(&external, "valuable original").unwrap();
    std::os::unix::fs::symlink(&external, &output_path).unwrap();
    let result = cli(&[
        "bindings",
        "--sample",
        "--out",
        output_path.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(&external).unwrap(), "valuable original");
    fs::remove_file(&output_path).unwrap();
    fs::hard_link(&external, &output_path).unwrap();
    let result = cli(&[
        "bindings",
        "--sample",
        "--out",
        output_path.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{}", text(&result));
    assert_eq!(fs::read_to_string(&external).unwrap(), "valuable original");
    assert!(
        fs::read_to_string(&output_path)
            .unwrap()
            .contains("export interface Client")
    );
}

#[test]
fn bindings_default_project_path_replaces_existing_generated_file() {
    let temp = Temp::new();
    let project = temp.join("relative-project");
    create_project(&project, Mode::Web, None).unwrap();
    fs::write(
        project.join("web/lumendesk.generated.ts"),
        "old generated content",
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_lumendesk"))
        .arg("bindings")
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", text(&result));
    assert_eq!(
        fs::read_to_string(project.join("web/lumendesk.generated.ts")).unwrap(),
        generate_typescript(&sample_schema()).unwrap()
    );
}

#[test]
fn bindings_accept_runtime_stream_registry_and_validate_names() {
    let temp = Temp::new();
    let source = include_str!("../../lumendesk-core/tests/typescript/schema.json");
    let schema = temp.join("schema.json");
    fs::write(&schema, source).unwrap();
    let result = cli(&["bindings", "--schema", schema.to_str().unwrap()]);
    assert!(result.status.success(), "{}", text(&result));
    let registry: lumendesk_core::SchemaRegistry = serde_json::from_str(source).unwrap();
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        registry.typescript()
    );
    let mut value: serde_json::Value = serde_json::from_str(source).unwrap();
    let streams = value["streams"].as_object_mut().unwrap();
    let key = streams.keys().next().unwrap().clone();
    streams.get_mut(&key).unwrap()["name"] = "different".into();
    assert!(
        generate_typescript(&value)
            .unwrap_err()
            .contains("does not match")
    );
    let mut duplicate: serde_json::Value = serde_json::from_str(source).unwrap();
    let mut stream = duplicate["streams"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    stream["name"] = "inspect".into();
    duplicate["streams"]["inspect"] = stream;
    assert!(generate_typescript(&duplicate).is_err());
    let mut malformed: serde_json::Value = serde_json::from_str(source).unwrap();
    malformed["streams"] = serde_json::json!({"broken": {"name": "broken"}});
    assert!(generate_typescript(&malformed).is_err());
    let empty = serde_json::to_value(lumendesk_core::SchemaRegistry::default()).unwrap();
    assert!(generate_typescript(&empty).is_ok());
}
