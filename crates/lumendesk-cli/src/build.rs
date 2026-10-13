use crate::{Config, Result};
use serde_json::Value;
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(crate) fn compile(
    project: &Path,
    config: &Config,
    release: bool,
    offline: bool,
) -> Result<PathBuf> {
    let manifest = project.join(&config.build.manifest);
    if !manifest.is_file() {
        return Err(format!(
            "manifest {} does not exist; correct build.manifest in lumendesk.toml",
            manifest.display()
        ));
    }
    let mut command = Command::new("cargo");
    command
        .current_dir(project)
        .args(["build", "--manifest-path"])
        .arg(&manifest)
        .args([
            "--bin",
            &config.build.binary,
            "--message-format=json-render-diagnostics",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    if release {
        command.arg("--release");
    } else {
        command
            .arg("--target-dir")
            .arg(project.join("target/lumendesk-dev"));
    }
    if offline {
        command.arg("--offline");
    }
    let output = command.output().map_err(|error| {
        format!("could not run cargo: {error}; install Rust and make cargo available on PATH")
    })?;
    let mut executable = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if message["reason"] == "compiler-artifact"
            && message["target"]["name"] == config.build.binary
            && message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            && let Some(path) = message["executable"].as_str()
        {
            executable = Some(PathBuf::from(path));
        }
    }
    if !output.status.success() {
        return Err(format!(
            "cargo build failed with {}; the previous development app, if any, was left running",
            output.status
        ));
    }
    let executable = executable.ok_or_else(|| format!("cargo succeeded but reported no executable for '{}'; build.binary must name a Cargo binary target", config.build.binary))?;
    let executable = if executable.is_absolute() {
        executable
    } else {
        project.join(executable)
    };
    if !executable.is_file() {
        return Err(format!(
            "cargo reported an executable that does not exist: {}",
            executable.display()
        ));
    }
    Ok(executable)
}

struct RunningApp {
    child: Child,
    staged: PathBuf,
}
impl RunningApp {
    fn stop(&mut self) -> Result<()> {
        if self
            .child
            .try_wait()
            .map_err(|error| format!("cannot inspect running app: {error}"))?
            .is_none()
        {
            // The child is intentionally terminated, then reaped before any replacement starts.
            // Graceful application-specific shutdown and descendant processes are out of scope.
            self.child.kill().map_err(|error| {
                format!(
                    "cannot stop old app (pid {}): {error}; no replacement was started",
                    self.child.id()
                )
            })?;
        }
        self.child
            .wait()
            .map_err(|error| format!("cannot wait for old app to exit: {error}"))?;
        Ok(())
    }
}
impl Drop for RunningApp {
    fn drop(&mut self) {
        let _ = self.stop();
        let _ = fs::remove_file(&self.staged);
    }
}

fn stage(project: &Path, executable: &Path) -> Result<PathBuf> {
    let directory = project.join(".lumendesk/run");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create development runtime directory: {error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path = directory.join(format!(
        "app-{}-{stamp}{}",
        std::process::id(),
        std::env::consts::EXE_SUFFIX
    ));
    fs::copy(executable, &path).map_err(|error| {
        format!("cannot stage new executable: {error}; the previous app was left running")
    })?;
    Ok(path)
}
fn launch(project: &Path, config: &Config, staged: PathBuf) -> Result<RunningApp> {
    let data = project.join(".lumendesk/development-data");
    fs::create_dir_all(&data)
        .map_err(|error| format!("cannot create isolated development data directory: {error}"))?;
    let child = Command::new(&staged)
        .current_dir(project)
        .env("LUMENDESK_ENV", "development")
        .env("LUMENDESK_APP_ID", format!("{}.development", config.app.id))
        .env("LUMENDESK_DATA_DIR", data)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", staged.display()))?;
    eprintln!(
        "Started development app (pid {}, identity {}.development)",
        child.id(),
        config.app.id
    );
    Ok(RunningApp { child, staged })
}

pub(crate) fn develop(
    project: &Path,
    mut config: Config,
    once: bool,
    offline: bool,
    poll_ms: u64,
) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = stop.clone();
    ctrlc::set_handler(move || handler_stop.store(true, Ordering::SeqCst))
        .map_err(|error| format!("cannot install Ctrl-C handler: {error}"))?;
    let mut previous_hash = fingerprint(project)?;
    let executable = compile(project, &config, false, offline)?;
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut running = launch(project, &config, stage(project, &executable)?)?;
    if once {
        loop {
            if stop.load(Ordering::SeqCst) {
                running.stop()?;
                return Ok(());
            }
            if let Some(status) = running
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
            {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("development app exited with {status}"))
                };
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    eprintln!(
        "Watching project sources every {poll_ms} ms. Ctrl-C stops the app. Replacement terminates the old child and waits for its exit."
    );
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(poll_ms));
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let next = match fingerprint(project) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("Watch read deferred: {error}");
                continue;
            }
        };
        if next == previous_hash {
            continue;
        }
        previous_hash = next;
        eprintln!(
            "Source change detected; building replacement while the current app keeps running."
        );
        let replacement = Config::load(project).and_then(|updated| {
            let executable = compile(project, &updated, false, offline)?;
            let staged = stage(project, &executable)?;
            Ok((updated, staged))
        });
        match replacement {
            Err(error) => {
                eprintln!("Rebuild rejected: {error}");
            }
            Ok((updated, staged)) => {
                if stop.load(Ordering::SeqCst) {
                    let _ = fs::remove_file(staged);
                    break;
                }
                running.stop()?;
                let old_path = running.staged.clone();
                // launch before assignment so the old staged executable remains available
                // for recovery if the new process cannot be started.
                match launch(project, &updated, staged.clone()) {
                    Ok(new) => {
                        running = new;
                        config = updated;
                    }
                    Err(error) => {
                        let _ = fs::remove_file(staged);
                        eprintln!("{error}; restarting the last successfully launched executable");
                        let recovery = stage(project, &old_path)?;
                        running = launch(project, &config, recovery)?;
                    }
                }
            }
        }
    }
    running.stop()?;
    Ok(())
}

fn fingerprint(project: &Path) -> Result<u64> {
    fn walk(directory: &Path, hasher: &mut DefaultHasher) -> Result<()> {
        let mut entries = fs::read_dir(directory)
            .map_err(|error| format!("cannot watch {}: {error}", directory.display()))?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            if matches!(
                name.to_str(),
                Some("target" | ".git" | ".lumendesk" | "node_modules" | "Cargo.lock")
            ) {
                continue;
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            path.hash(hasher);
            if kind.is_dir() {
                walk(&path, hasher)?;
            } else if kind.is_file() {
                let mut file = fs::File::open(&path)
                    .map_err(|error| format!("cannot watch {}: {error}", path.display()))?;
                let mut buffer = [0_u8; 8192];
                loop {
                    let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
                    if count == 0 {
                        break;
                    }
                    hasher.write(&buffer[..count]);
                }
            }
        }
        Ok(())
    }
    let mut hasher = DefaultHasher::new();
    walk(project, &mut hasher)?;
    Ok(hasher.finish())
}
