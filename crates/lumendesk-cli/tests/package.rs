//! Real archive/metadata/build tests. None of these fixtures initialize a GUI.
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        // macOS commonly exposes TMPDIR through /var -> /private/var. Use the
        // real fixture root so archive tests do not accidentally request symlinks.
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "lumendesk-package-{}-{}",
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
fn fixture(temp: &Temp) -> PathBuf {
    let app = temp.join("app");
    fs::create_dir_all(app.join("src")).unwrap();
    fs::write(
        app.join("Cargo.toml"),
        "[package]\nname=\"package-fixture\"\nversion=\"1.2.3\"\nedition=\"2024\"\n[workspace]\n",
    )
    .unwrap();
    fs::write(app.join("lumendesk.toml"), "schema_version=1\n[app]\nname=\"package-fixture\"\nid=\"dev.example.package\"\nversion=\"1.2.3\"\nmode=\"native\"\n[build]\nmanifest=\"Cargo.toml\"\nbinary=\"package-fixture\"\n").unwrap();
    fs::write(
        app.join("src/main.rs"),
        "fn main() { println!(\"portable-executable\"); }\n",
    )
    .unwrap();
    app
}
fn package(app: &Path, out: &Path, kind: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lumendesk"))
        .args(["package", "--offline", "--project"])
        .arg(app)
        .args(["--kind", kind, "--out"])
        .arg(out)
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
fn entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = tar::Archive::new(GzDecoder::new(fs::File::open(path).unwrap()));
    archive
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            assert!(entry.header().entry_type().is_file());
            assert_eq!(entry.header().mtime().unwrap(), 0);
            assert_eq!(entry.header().uid().unwrap(), 0);
            assert_eq!(entry.header().gid().unwrap(), 0);
            assert!(entry.link_name().unwrap().is_none());
            let name = entry.path().unwrap().to_str().unwrap().to_string();
            assert!(!name.starts_with('/') && !name.split('/').any(|part| part == ".."));
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
fn verify_manifest(files: &BTreeMap<String, Vec<u8>>) -> serde_json::Value {
    let manifest: serde_json::Value = serde_json::from_slice(&files["MANIFEST.json"]).unwrap();
    let records = manifest["files"].as_array().unwrap();
    assert_eq!(records.len(), files.len() - 1);
    for record in records {
        let bytes = &files[record["path"].as_str().unwrap()];
        assert_eq!(record["size"], bytes.len());
        assert_eq!(record["sha256"], format!("{:x}", Sha256::digest(bytes)));
    }
    assert_eq!(manifest["runtime"]["self_contained"], false);
    assert!(
        manifest["runtime"]["notes"]
            .as_str()
            .unwrap()
            .contains("dynamically linked")
    );
    manifest
}
#[test]
fn source_is_deterministic_checksummed_and_excludes_secrets_and_caches() {
    let temp = Temp::new();
    let app = fixture(&temp);
    for (path, content) in [
        (".env", "SECRET_ENV"),
        (".git/config", "SECRET_GIT"),
        ("target/debug/cache.rs", "SECRET_TARGET"),
        ("node_modules/a/index.js", "SECRET_NODE"),
        ("web/credentials.json", "SECRET_CREDENTIALS"),
        ("web/private.pem", "SECRET_KEY"),
        ("src/.private/data.rs", "SECRET_HIDDEN"),
        ("web/config.secret.json", "SECRET_NESTED"),
        ("web/index.html", "<h1>Public</h1>"),
    ] {
        let path = app.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    let a = temp.join("one.tar.gz");
    let b = temp.join("two.tar.gz");
    let result = package(&app, &a, "source");
    assert!(result.status.success(), "{}", text(&result));
    // Source filesystem timestamps and output filenames are not archive inputs.
    let source = fs::read(app.join("src/main.rs")).unwrap();
    fs::write(app.join("src/main.rs"), source).unwrap();
    let result = package(&app, &b, "source");
    assert!(result.status.success(), "{}", text(&result));
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
    let files = entries(&a);
    let manifest = verify_manifest(&files);
    assert_eq!(manifest["app"]["id"], "dev.example.package");
    assert_eq!(manifest["kind"], "source");
    assert!(files.contains_key("web/index.html"));
    assert!(files.contains_key("Cargo.lock"));
    assert!(
        !files
            .values()
            .any(|bytes| String::from_utf8_lossy(bytes).contains("SECRET_"))
    );
    let previous = fs::read(&a).unwrap();
    let overwrite = package(&app, &a, "source");
    assert!(!overwrite.status.success());
    assert_eq!(fs::read(&a).unwrap(), previous);
}
#[test]
fn source_rebuilds_after_original_app_and_sdk_are_removed() {
    let temp = Temp::new();
    let app = fixture(&temp);
    let sdk = temp.join("original-sdk");
    fs::create_dir_all(sdk.join("crates/lumendesk/src")).unwrap();
    fs::create_dir_all(sdk.join("crates/lumendesk-core/src")).unwrap();
    fs::create_dir_all(sdk.join("assets")).unwrap();
    fs::write(sdk.join("Cargo.toml"), "[workspace]\nmembers=[\"crates/lumendesk\",\"crates/lumendesk-core\"]\nresolver=\"3\"\n[workspace.package]\nversion=\"0.1.0\"\nedition=\"2024\"\nlicense=\"MIT OR Apache-2.0\"\nrust-version=\"1.95\"\n").unwrap();
    fs::write(sdk.join("LICENSE-MIT"), "fixture SDK MIT notice").unwrap();
    fs::write(sdk.join("LICENSE-APACHE"), "fixture SDK Apache notice").unwrap();
    fs::write(sdk.join("assets/message.txt"), "bundled-sdk-asset").unwrap();
    fs::create_dir(app.join("assets")).unwrap();
    fs::write(app.join("assets/message.txt"), "separate-app-asset").unwrap();
    fs::write(sdk.join("crates/lumendesk/Cargo.toml"), "[package]\nname=\"lumendesk\"\nversion.workspace=true\nedition.workspace=true\nlicense.workspace=true\nrust-version.workspace=true\n[dependencies]\nlumendesk-core={path=\"../lumendesk-core\"}\n").unwrap();
    fs::write(sdk.join("crates/lumendesk/src/lib.rs"), "pub fn message() -> &'static str { let _ = lumendesk_core::NUMBER; include_str!(\"../../../assets/message.txt\") }\n").unwrap();
    fs::write(sdk.join("crates/lumendesk-core/Cargo.toml"), "[package]\nname=\"lumendesk-core\"\nversion.workspace=true\nedition.workspace=true\nlicense.workspace=true\nrust-version.workspace=true\n").unwrap();
    fs::write(
        sdk.join("crates/lumendesk-core/src/lib.rs"),
        "pub const NUMBER: u32 = 7;\n",
    )
    .unwrap();
    let mut manifest = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    manifest.push_str(&format!(
        "[dependencies]\nlumendesk={{path={:?}}}\n",
        sdk.join("crates/lumendesk").to_str().unwrap()
    ));
    fs::write(app.join("Cargo.toml"), manifest).unwrap();
    fs::write(
        app.join("src/main.rs"),
        "fn main() { println!(\"{}\", lumendesk::message()); }\n",
    )
    .unwrap();
    let archive = temp.join("portable.tar.gz");
    let result = package(&app, &archive, "source");
    assert!(result.status.success(), "{}", text(&result));
    let files = entries(&archive);
    verify_manifest(&files);
    assert!(files.contains_key("_sdk/crates/lumendesk-core/src/lib.rs"));
    assert!(files.contains_key("_sdk/assets/message.txt"));
    assert!(!String::from_utf8_lossy(&files["Cargo.toml"]).contains(temp.0.to_str().unwrap()));
    assert!(
        !String::from_utf8_lossy(&files["_sdk/crates/lumendesk/Cargo.toml"])
            .contains("workspace = true")
    );
    fs::remove_dir_all(&sdk).unwrap();
    fs::remove_dir_all(&app).unwrap();
    let extracted = temp.join("extracted-elsewhere");
    fs::create_dir(&extracted).unwrap();
    tar::Archive::new(GzDecoder::new(fs::File::open(&archive).unwrap()))
        .unpack(&extracted)
        .unwrap();
    let build = Command::new("cargo")
        .args(["run", "--release", "--offline", "--locked", "--quiet"])
        .current_dir(&extracted)
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", text(&build));
    assert_eq!(
        String::from_utf8_lossy(&build.stdout).trim(),
        "bundled-sdk-asset"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let assets = extracted.join("_sdk/assets");
        let saved = temp.join("saved-sdk-assets");
        fs::rename(&assets, &saved).unwrap();
        symlink(&saved, &assets).unwrap();
        let rejected = temp.join("linked-assets.tar.gz");
        let result = package(&extracted, &rejected, "source");
        assert!(!result.status.success());
        assert!(text(&result).contains("symlink"), "{}", text(&result));
        assert!(!rejected.exists());
        fs::remove_file(&assets).unwrap();
        fs::rename(&saved, &assets).unwrap();
    }
    let repacked = temp.join("repacked.tar.gz");
    let result = package(&extracted, &repacked, "source");
    assert!(result.status.success(), "{}", text(&result));
    let second = entries(&repacked);
    verify_manifest(&second);
    assert_eq!(
        second.get("assets/message.txt"),
        files.get("assets/message.txt")
    );
    assert_eq!(
        second.get("_sdk/assets/message.txt"),
        files.get("_sdk/assets/message.txt")
    );
    fs::remove_dir_all(&extracted).unwrap();
    let relocated = temp.join("repacked-elsewhere");
    fs::create_dir(&relocated).unwrap();
    tar::Archive::new(GzDecoder::new(fs::File::open(&repacked).unwrap()))
        .unpack(&relocated)
        .unwrap();
    let build = Command::new("cargo")
        .args(["run", "--release", "--offline", "--locked", "--quiet"])
        .current_dir(&relocated)
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", text(&build));
    assert_eq!(
        String::from_utf8_lossy(&build.stdout).trim(),
        "bundled-sdk-asset"
    );
}
#[test]
fn source_preserves_root_workspace_lints_after_relocation() {
    let temp = Temp::new();
    let app = fixture(&temp);
    let mut manifest = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    manifest.push_str("[workspace.lints.rust]\nunsafe_code=\"forbid\"\n[lints]\nworkspace=true\n");
    fs::write(app.join("Cargo.toml"), manifest).unwrap();
    let original = Command::new("cargo")
        .args(["check", "--offline"])
        .current_dir(&app)
        .output()
        .unwrap();
    assert!(original.status.success(), "{}", text(&original));
    let archive = temp.join("lints.tar.gz");
    let result = package(&app, &archive, "source");
    assert!(result.status.success(), "{}", text(&result));
    verify_manifest(&entries(&archive));
    fs::remove_dir_all(&app).unwrap();
    let extracted = temp.join("relocated");
    fs::create_dir(&extracted).unwrap();
    tar::Archive::new(GzDecoder::new(fs::File::open(&archive).unwrap()))
        .unpack(&extracted)
        .unwrap();
    let metadata = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--offline", "--locked"])
        .current_dir(&extracted)
        .output()
        .unwrap();
    assert!(metadata.status.success(), "{}", text(&metadata));
    let build = Command::new("cargo")
        .args(["build", "--offline", "--locked"])
        .current_dir(&extracted)
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", text(&build));
    fs::write(extracted.join("src/main.rs"), "fn main() { unsafe {} }\n").unwrap();
    let forbidden = Command::new("cargo")
        .args(["check", "--offline", "--locked"])
        .current_dir(&extracted)
        .output()
        .unwrap();
    assert!(!forbidden.status.success(), "unsafe_code forbid was lost");
    assert!(
        text(&forbidden).contains("usage of an `unsafe` block"),
        "{}",
        text(&forbidden)
    );
}
#[test]
fn source_rejects_sdk_workspace_lint_inheritance_before_writing_archive() {
    let temp = Temp::new();
    let app = fixture(&temp);
    let sdk = temp.join("sdk");
    fs::create_dir_all(sdk.join("crates/lumendesk/src")).unwrap();
    fs::write(
        sdk.join("Cargo.toml"),
        "[workspace]\nmembers=[\"crates/lumendesk\"]\nresolver=\"3\"\n[workspace.lints.rust]\nunsafe_code=\"forbid\"\n",
    ).unwrap();
    fs::write(
        sdk.join("crates/lumendesk/Cargo.toml"),
        "[package]\nname=\"lumendesk\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[lints]\nworkspace=true\n",
    ).unwrap();
    fs::write(
        sdk.join("crates/lumendesk/src/lib.rs"),
        "pub fn safe() {}\n",
    )
    .unwrap();
    let mut manifest = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    manifest.push_str(&format!(
        "[dependencies]\nlumendesk={{path={:?}}}\n",
        sdk.join("crates/lumendesk").to_str().unwrap()
    ));
    fs::write(app.join("Cargo.toml"), manifest).unwrap();
    let original = Command::new("cargo")
        .args(["check", "--offline"])
        .current_dir(&app)
        .output()
        .unwrap();
    assert!(original.status.success(), "{}", text(&original));
    let archive = temp.join("unsupported-lints.tar.gz");
    let result = package(&app, &archive, "source");
    assert!(!result.status.success());
    assert!(
        text(&result).contains("workspace-inherited lints outside the application root"),
        "{}",
        text(&result)
    );
    assert!(!archive.exists());
}
#[test]
fn binary_archive_contains_a_real_host_executable_and_runtime_notes() {
    let temp = Temp::new();
    let app = fixture(&temp);
    let archive = temp.join("binary.tar.gz");
    let result = package(&app, &archive, "binary");
    assert!(result.status.success(), "{}", text(&result));
    let files = entries(&archive);
    let manifest = verify_manifest(&files);
    assert_eq!(manifest["kind"], "binary");
    assert_eq!(
        manifest["platform"]["binary_target"],
        manifest["platform"]["build_host"]
    );
    assert!(!files.contains_key("src/main.rs"));
    let extracted = temp.join("binary-extracted");
    fs::create_dir(&extracted).unwrap();
    tar::Archive::new(GzDecoder::new(fs::File::open(&archive).unwrap()))
        .unpack(&extracted)
        .unwrap();
    let executable = extracted.join(format!(
        "bin/package-fixture{}",
        std::env::consts::EXE_SUFFIX
    ));
    let result = Command::new(executable).output().unwrap();
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        "portable-executable"
    );
}
#[cfg(unix)]
#[test]
fn input_links_and_output_symlink_components_are_rejected() {
    use std::os::unix::fs::symlink;
    for directory in [false, true] {
        let temp = Temp::new();
        let app = fixture(&temp);
        let outside = temp.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("external.rs"), "private outside bytes").unwrap();
        if directory {
            symlink(&outside, app.join("src/link")).unwrap();
        } else {
            symlink(outside.join("external.rs"), app.join("src/external.rs")).unwrap();
        }
        let output = temp.join("output.tar.gz");
        let result = package(&app, &output, "source");
        assert!(!result.status.success());
        assert!(text(&result).contains("symlink"), "{}", text(&result));
        assert!(!output.exists());
    }
    let temp = Temp::new();
    let app = fixture(&temp);
    let outside = temp.join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, temp.join("link")).unwrap();
    let result = package(&app, &temp.join("link/output.tar.gz"), "source");
    assert!(!result.status.success());
    assert!(text(&result).contains("symlink"));
    assert!(!outside.join("output.tar.gz").exists());
    fs::write(outside.join("sentinel"), "untouched").unwrap();
    symlink(outside.join("sentinel"), temp.join("output.tar.gz")).unwrap();
    let result = package(&app, &temp.join("output.tar.gz"), "source");
    assert!(!result.status.success());
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).unwrap(),
        "untouched"
    );
}
#[cfg(unix)]
#[test]
fn cross_platform_path_escapes_fail_closed() {
    for name in ["..\\outside.rs", "quote\".rs", "COM¹.rs", "LPT².rs"] {
        let temp = Temp::new();
        let app = fixture(&temp);
        fs::write(app.join("src").join(name), "trick path").unwrap();
        let output = temp.join("unsafe.tar.gz");
        let result = package(&app, &output, "source");
        assert!(!result.status.success(), "{}", text(&result));
        assert!(!output.exists());
    }
}
#[test]
fn hardlink_inputs_fail_closed_on_each_supported_platform() {
    let temp = Temp::new();
    let app = fixture(&temp);
    fs::write(temp.join("outside.rs"), "external private data").unwrap();
    fs::hard_link(temp.join("outside.rs"), app.join("src/external.rs")).unwrap();
    let output = temp.join("unsafe.tar.gz");
    let result = package(&app, &output, "source");
    assert!(!result.status.success(), "{}", text(&result));
    assert!(text(&result).contains("hard-linked"), "{}", text(&result));
    assert!(!output.exists());
}
#[cfg(windows)]
#[test]
fn junction_inputs_and_output_parents_fail_closed() {
    for input in [true, false] {
        let temp = Temp::new();
        let app = fixture(&temp);
        let outside = temp.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("private.rs"), "outside bytes").unwrap();
        let link = if input {
            app.join("src/junction")
        } else {
            temp.join("junction")
        };
        // Directory junction creation does not require the symlink privilege.
        let result = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", text(&result));
        let archive = if input {
            temp.join("junction.tar.gz")
        } else {
            link.join("junction.tar.gz")
        };
        let result = package(&app, &archive, "source");
        // Remove the junction explicitly so fixture cleanup cannot recurse into it.
        fs::remove_dir(&link).unwrap();
        assert!(!result.status.success(), "{}", text(&result));
        assert!(text(&result).contains("reparse point"), "{}", text(&result));
        assert!(!outside.join("junction.tar.gz").exists());
        assert_eq!(
            fs::read_to_string(outside.join("private.rs")).unwrap(),
            "outside bytes"
        );
    }
}
#[cfg(unix)]
#[test]
fn fifo_configuration_is_rejected_without_blocking_on_a_reader() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let temp = Temp::new();
    let app = fixture(&temp);
    fs::remove_file(app.join("lumendesk.toml")).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(app.join("lumendesk.toml"))
            .status()
            .unwrap()
            .success()
    );
    let archive = temp.join("fifo.tar.gz");
    let mut child = Command::new(env!("CARGO_BIN_EXE_lumendesk"))
        .args(["package", "--offline", "--project"])
        .arg(&app)
        .args(["--kind", "source", "--out"])
        .arg(&archive)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let completed = loop {
        if child.try_wait().unwrap().is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            break false;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let result = child.wait_with_output().unwrap();
    assert!(
        completed,
        "packaging blocked while reading a FIFO configuration"
    );
    assert!(!result.status.success(), "{}", text(&result));
    assert!(text(&result).contains("regular file"), "{}", text(&result));
    assert!(!archive.exists());
}
#[test]
fn unsupported_local_dependencies_and_configured_escape_fail() {
    let temp = Temp::new();
    let app = fixture(&temp);
    let dependency = temp.join("unknown-dep");
    fs::create_dir_all(dependency.join("src")).unwrap();
    fs::write(
        dependency.join("Cargo.toml"),
        "[package]\nname=\"unknown-dep\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(dependency.join("src/lib.rs"), "").unwrap();
    let manifest = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    fs::write(
        app.join("Cargo.toml"),
        format!("{manifest}\n[dependencies]\nunknown-dep={{path=\"../unknown-dep\"}}\n"),
    )
    .unwrap();
    let result = package(&app, &temp.join("output.tar.gz"), "source");
    assert!(!result.status.success());
    assert!(
        text(&result).contains("unsupported local path dependency"),
        "{}",
        text(&result)
    );
    let config = fs::read_to_string(app.join("lumendesk.toml"))
        .unwrap()
        .replace("manifest=\"Cargo.toml\"", "manifest=\"../Cargo.toml\"");
    fs::write(app.join("lumendesk.toml"), config).unwrap();
    let result = package(&app, &temp.join("output.tar.gz"), "source");
    assert!(!result.status.success());
    assert!(text(&result).contains("relative path inside the project"));
}

/// Requires the platform WebKit/GTK development libraries and cached Rust dependencies.
/// It builds a real desktop executable but only runs display-free SDK unit tests.
#[test]
#[ignore = "requires configured WebKit/GTK development libraries and cached dependencies"]
fn real_web_source_archive_builds_and_retains_sdk_test_fixtures() {
    let temp = Temp::new();
    let app = temp.join("real-web-app");
    lumendesk_cli::create_project(&app, lumendesk_cli::Mode::Web, None).unwrap();
    let output = temp.join("real-web-source.tar.gz");
    let result = package(&app, &output, "source");
    assert!(result.status.success(), "{}", text(&result));
    let files = entries(&output);
    verify_manifest(&files);
    for required in [
        "_sdk/assets/index.html",
        "_sdk/assets/app.js",
        "_sdk/assets/stream.html",
        "_sdk/crates/lumendesk/src/web_streams.rs",
        "_sdk/crates/lumendesk/tests/stream-frames.json",
        "_sdk/crates/lumendesk/tests/session-delivery.json",
        "_sdk/crates/lumendesk-core/src/lib.rs",
        "_sdk/crates/lumendesk/LICENSE-MIT",
        "_sdk/crates/lumendesk-core/LICENSE-APACHE",
    ] {
        assert!(files.contains_key(required), "missing {required}");
    }
    let original_sdk = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("lumendesk")
        .canonicalize()
        .unwrap();
    for (name, contents) in files
        .iter()
        .filter(|(name, _)| name.ends_with("Cargo.toml"))
    {
        assert!(
            !String::from_utf8_lossy(contents).contains(original_sdk.to_str().unwrap()),
            "absolute SDK path in {name}"
        );
    }
    fs::remove_dir_all(&app).unwrap();
    let extracted = temp.join("extracted-away-from-original-app");
    fs::create_dir(&extracted).unwrap();
    tar::Archive::new(GzDecoder::new(fs::File::open(&output).unwrap()))
        .unpack(&extracted)
        .unwrap();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target")
                .canonicalize()
                .unwrap()
        });
    let result = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--locked",
            "--offline",
            "--manifest-path",
        ])
        .arg(extracted.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", text(&result));
    // The application dependency activates webview. Cargo disallows --features
    // for a selected dependency outside workspace membership, so use that graph.
    let result = Command::new("cargo")
        .args([
            "test",
            "-p",
            "lumendesk",
            "--locked",
            "--offline",
            "--manifest-path",
        ])
        .arg(extracted.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", text(&result));
    assert!(
        text(&result)
            .contains("webview::tests::bundled_assets_embed_all_resources_and_reject_traversal")
    );
    assert!(text(&result).contains("web_streams::tests::slow_consumer_has_no_unsolicited_values_and_completion_follows_last_item"));
    assert!(text(&result).contains(
        "webview::tests::delivery_scripts_bind_the_execution_to_the_original_page_session"
    ));
}

#[test]
fn source_preserves_resolver_one_feature_unification() {
    let temp = Temp::new();
    let app = fixture(&temp);
    let sdk = temp.join("lumendesk");
    fs::create_dir_all(sdk.join("src")).unwrap();
    fs::write(sdk.join("Cargo.toml"), "[package]\nname=\"lumendesk\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[features]\nneeded=[]\n").unwrap();
    fs::write(
        sdk.join("src/lib.rs"),
        "#[cfg(feature = \"needed\")] pub fn needed() {}\n",
    )
    .unwrap();
    let manifest = format!(
        "[package]\nname=\"package-fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\nresolver=\"1\"\n[dependencies]\nlumendesk={{path={:?}}}\n[dev-dependencies]\nlumendesk={{path={:?},features=[\"needed\"]}}\n",
        sdk.to_str().unwrap(),
        sdk.to_str().unwrap()
    );
    fs::write(app.join("Cargo.toml"), manifest).unwrap();
    fs::write(
        app.join("src/main.rs"),
        "fn main() { lumendesk::needed(); }\n",
    )
    .unwrap();
    let original = Command::new("cargo")
        .args(["build", "--release", "--offline"])
        .current_dir(&app)
        .output()
        .unwrap();
    assert!(original.status.success(), "{}", text(&original));
    let archive = temp.join("resolver.tar.gz");
    let result = package(&app, &archive, "source");
    assert!(result.status.success(), "{}", text(&result));
    fs::remove_dir_all(&app).unwrap();
    fs::remove_dir_all(&sdk).unwrap();
    let relocated = temp.join("relocated");
    fs::create_dir(&relocated).unwrap();
    tar::Archive::new(GzDecoder::new(fs::File::open(&archive).unwrap()))
        .unpack(&relocated)
        .unwrap();
    let build = Command::new("cargo")
        .args(["build", "--release", "--offline", "--locked"])
        .current_dir(&relocated)
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", text(&build));
}

#[test]
fn source_preserves_effective_workspace_resolver() {
    // Virtual roots ignore member editions. Nonvirtual roots, including a
    // different root package, derive the default from their own edition.
    for (workspace, member, expected) in [
        (None, "edition=\"2018\"\n", "1"),
        (None, "edition=\"2021\"\n", "2"),
        (None, "edition=\"2024\"\n", "3"),
        (None, "edition=\"2024\"\nresolver=\"1\"\n", "1"),
        (
            Some("[workspace]\nmembers=[\"app\"]\n"),
            "edition=\"2024\"\n",
            "1",
        ),
        (
            Some("[workspace]\nmembers=[\"app\"]\nresolver=\"2\"\n"),
            "edition=\"2024\"\n",
            "2",
        ),
        (
            Some(
                "[package]\nname=\"lumendesk-core\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\nmembers=[\"app\"]\n",
            ),
            "edition=\"2024\"\n",
            "2",
        ),
        (
            Some("[workspace]\nmembers=[\"app\"]\n[workspace.package]\nedition=\"2021\"\n"),
            "edition.workspace=true\n",
            "1",
        ),
    ] {
        let temp = Temp::new();
        let app = fixture(&temp);
        let mut manifest =
            format!("[package]\nname=\"package-fixture\"\nversion=\"1.2.3\"\n{member}");
        if let Some(workspace) = workspace {
            fs::write(temp.join("Cargo.toml"), workspace).unwrap();
            if workspace.contains("[package]") {
                fs::create_dir_all(temp.join("src")).unwrap();
                fs::write(temp.join("src/lib.rs"), "").unwrap();
            }
        } else {
            manifest.push_str("[workspace]\n");
        }
        fs::write(app.join("Cargo.toml"), manifest).unwrap();
        let archive = temp.join("resolver.tar.gz");
        let result = package(&app, &archive, "source");
        assert!(result.status.success(), "{}", text(&result));
        let files = entries(&archive);
        let manifest: toml::Value =
            toml::from_str(std::str::from_utf8(&files["Cargo.toml"]).unwrap()).unwrap();
        assert_eq!(manifest["workspace"]["resolver"].as_str(), Some(expected));
        let relocated = temp.join("relocated");
        fs::create_dir(&relocated).unwrap();
        tar::Archive::new(GzDecoder::new(fs::File::open(&archive).unwrap()))
            .unpack(&relocated)
            .unwrap();
        let build = Command::new("cargo")
            .args(["build", "--release", "--offline", "--locked"])
            .current_dir(&relocated)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "resolver {expected}: {}",
            text(&build)
        );
    }
}
