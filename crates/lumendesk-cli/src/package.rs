//! Deliberately bounded portable archives: no installer, credentials, or publishing.
use crate::{Config, Mode, Result, build};
use flate2::{Compression, GzBuilder};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_SOURCE_FILE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: usize = 512 * 1024 * 1024;
const NOTICE: &str = "Portable archive, not an installer. No signing, notarization, publishing, automatic update, or system installation is performed. Application assets referenced outside the allowlisted source tree or not embedded in the executable are not bundled. Review the file inventory before distribution. File filtering cannot detect secrets embedded in ordinary source code. Dependency license metadata is informational, not a compliance certification. Registry dependencies are NOT vendored; a source rebuild needs the locked registry packages (network access or an existing Cargo cache), Rust 1.95+, and platform development libraries. Included dependency license files cover the resolved Cargo graph; inspect DEPENDENCIES.json for missing notices or absent license declarations before redistribution. Cargo.lock pins the dependency resolution; archives contain no machine-specific Cargo configuration. Build scripts are executed by binary packaging, just as by cargo build.\n";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Source,
    Binary,
}
impl Kind {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "source" => Ok(Self::Source),
            "binary" => Ok(Self::Binary),
            _ => Err("--kind must be source or binary".into()),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Binary => "binary",
        }
    }
}
struct Payload {
    bytes: Vec<u8>,
    mode: u32,
}
type Files = BTreeMap<String, Payload>;

#[derive(Serialize)]
struct FileRecord<'a> {
    path: &'a str,
    size: usize,
    mode: u32,
    sha256: String,
}
#[derive(Serialize)]
struct Dependency {
    name: String,
    version: String,
    source: String,
    license: Option<String>,
    notices: Vec<String>,
}

pub(crate) fn create(project: &Path, kind: Kind, output: &Path, offline: bool) -> Result<()> {
    let project = checked_absolute(project, false)?;
    let output = checked_absolute(output, true)?;
    if fs::symlink_metadata(&output).is_ok() {
        return Err(format!(
            "refusing existing archive {}; choose a new --out path",
            output.display()
        ));
    }
    if output.extension().and_then(|v| v.to_str()) != Some("gz")
        || !output.to_string_lossy().ends_with(".tar.gz")
    {
        return Err("--out must end in .tar.gz".into());
    }
    let config = Config::load(&project)?;
    if config.build.manifest != "Cargo.toml" {
        return Err("portable packaging currently requires build.manifest = \"Cargo.toml\" at the project root".into());
    }
    // Inspect the selected source tree before invoking Cargo; do not follow symlink manifests.
    let mut app_files = Files::new();
    collect(&project, Path::new(""), "", &mut app_files)?;
    if !app_files.contains_key("Cargo.toml") || !app_files.contains_key("lumendesk.toml") {
        return Err("portable package requires real Cargo.toml and lumendesk.toml files".into());
    }
    let host = rust_host()?;
    let metadata = metadata(&project, offline)?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("Cargo metadata omitted packages")?;
    let root_id = metadata["resolve"]["root"]
        .as_str()
        .ok_or("portable packaging requires a single application Cargo package")?;
    let root = packages
        .iter()
        .find(|p| p["id"] == root_id)
        .ok_or("Cargo metadata omitted the root application")?;
    let root_manifest = Path::new(
        root["manifest_path"]
            .as_str()
            .ok_or("missing manifest path")?,
    );
    if root_manifest != project.join("Cargo.toml") {
        return Err("Cargo resolved a different application root".into());
    }
    let mut local = BTreeMap::<PathBuf, String>::new();
    for package in packages.iter().filter(|p| p["source"].is_null()) {
        let manifest = checked_absolute(
            Path::new(
                package["manifest_path"]
                    .as_str()
                    .ok_or("missing local manifest")?,
            ),
            false,
        )?;
        let name = package["name"].as_str().ok_or("missing package name")?;
        let prefix = if package["id"] == root_id {
            String::new()
        } else if matches!(name, "lumendesk" | "lumendesk-core") {
            format!("_sdk/crates/{name}/")
        } else {
            return Err(format!(
                "unsupported local path dependency '{name}'; this bounded package format only bundles lumendesk and lumendesk-core"
            ));
        };
        if local.values().any(|existing| *existing == prefix) {
            return Err(format!("duplicate bundled local crate '{name}'"));
        }
        local.insert(manifest, prefix);
    }
    let mut files = Files::new();
    if kind == Kind::Source {
        files = app_files;
        // Cargo metadata creates/updates the application's resolution without executing build scripts.
        add_file(
            &mut files,
            "Cargo.lock",
            &project.join("Cargo.lock"),
            MAX_SOURCE_FILE,
        )?;
        for package in packages.iter().filter(|p| p["source"].is_null()) {
            let manifest = Path::new(
                package["manifest_path"]
                    .as_str()
                    .ok_or("missing local manifest")?,
            );
            let directory = manifest.parent().ok_or("local crate lacks a directory")?;
            let prefix = local.get(manifest).ok_or("local crate was not selected")?;
            if !prefix.is_empty() {
                collect(directory, Path::new(""), prefix, &mut files)?;
                // Workspace SDK crates inherit the repository's license texts.
                let workspace = Path::new(metadata["workspace_root"].as_str().unwrap_or(""));
                let sdk_workspace =
                    find_workspace(directory).unwrap_or_else(|| workspace.to_path_buf());
                if sdk_workspace.join("assets").is_dir() {
                    collect(&sdk_workspace, Path::new("assets"), "_sdk/", &mut files)?;
                }
                for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
                    let source = sdk_workspace.join(name);
                    if source.exists() {
                        add_file(
                            &mut files,
                            &format!("{prefix}{name}"),
                            &source,
                            MAX_SOURCE_FILE,
                        )?;
                    }
                }
            }
            let rewritten = rewrite_manifest(manifest, package, prefix, &local)?;
            insert(
                &mut files,
                &format!("{prefix}Cargo.toml"),
                rewritten.into_bytes(),
                0o644,
            )?;
        }
        validate_declared_paths(&files, &local)?;
    } else {
        let executable = build::compile_for_target(&project, &config, true, offline, Some(&host))?;
        add_file_with_links(
            &mut files,
            &format!(
                "bin/{}{}",
                config.build.binary,
                std::env::consts::EXE_SUFFIX
            ),
            &executable,
            MAX_TOTAL as u64,
            true,
        )?;
        files
            .get_mut(&format!(
                "bin/{}{}",
                config.build.binary,
                std::env::consts::EXE_SUFFIX
            ))
            .expect("inserted executable")
            .mode = 0o755;
        for (name, value) in app_files.into_iter().filter(|(name, _)| is_license(name)) {
            insert(&mut files, &name, value.bytes, 0o644)?;
        }
    }
    let dependencies = license_inventory(packages, root_id, &mut files)?;
    insert(
        &mut files,
        "DEPENDENCIES.json",
        pretty(&dependencies)?,
        0o644,
    )?;
    let runtime = runtime_notes(config.app.mode);
    let instructions = if kind == Kind::Source {
        "Extract into an empty directory. From that directory run: cargo build --release --locked\nUse --offline only when every locked registry dependency is already cached. The bundled SDK paths are relative and independent of the originating workspace. Review DEPENDENCIES.json and dependency-licenses/ before redistribution.\n"
    } else {
        "Extract into an empty directory on the declared build target. Run the executable in bin/. Install the required host runtime libraries separately. No runtime libraries, installer, icons registration, or uninstaller are bundled. Review DEPENDENCIES.json and dependency-licenses/ before redistribution.\n"
    };
    insert(
        &mut files,
        "PACKAGE-NOTES.txt",
        format!("{NOTICE}\n{instructions}\n{runtime}\n").into_bytes(),
        0o644,
    )?;
    let records: Vec<_> = files
        .iter()
        .map(|(path, file)| FileRecord {
            path,
            size: file.bytes.len(),
            mode: file.mode,
            sha256: format!("{:x}", Sha256::digest(&file.bytes)),
        })
        .collect();
    let manifest = serde_json::json!({
        "format_version": 1,
        "kind": kind.name(),
        "app": config.app,
        "app_license": root["license"],
        "platform": {"build_host": host, "binary_target": if kind == Kind::Binary { Some(&host) } else { None }, "source_portability": "Other hosts require their own toolchain/system development libraries and remain unverified."},
        "runtime": {"self_contained": false, "notes": runtime},
        "dependencies": {"inventory": "DEPENDENCIES.json", "registry_sources_vendored": false, "license_review_required": true},
        "checksum": {"algorithm": "SHA-256", "scope": "Every payload file; MANIFEST.json excludes its own self-referential checksum."},
        "files": records,
    });
    insert(&mut files, "MANIFEST.json", pretty(&manifest)?, 0o644)?;
    write_archive(&output, &files)
}

fn pretty(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn runtime_notes(mode: Mode) -> String {
    let renderer = match mode {
        Mode::Native => {
            "Native eframe rendering needs the platform window system and graphics stack; Linux needs an X11 or Wayland session, compatible graphics/OpenGL drivers and system libraries."
        }
        Mode::Web => {
            "Web mode dynamically uses the system web renderer: Linux GTK 3 and WebKitGTK 4.1 (plus their transitive runtime libraries and X11/Wayland session); macOS Cocoa/WKWebView; Windows WebView2 Runtime."
        }
    };
    format!(
        "{renderer} Linux executables are dynamically linked, not self-contained. A compatible libc, loader, C/C++ and other linked system libraries may be required. No clean-machine runtime installation or cross-platform GUI launch has been validated by this archive command."
    )
}
fn rust_host() -> Result<String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| format!("cannot inspect Rust host: {e}"))?;
    if !output.status.success() {
        return Err("rustc -vV failed".into());
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or("rustc did not identify its host".into())
}
fn metadata(project: &Path, offline: bool) -> Result<Value> {
    let mut command = Command::new("cargo");
    command
        .current_dir(project)
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    if offline {
        command.arg("--offline");
    }
    let output = command
        .output()
        .map_err(|e| format!("cannot resolve package dependencies with cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed; ensure registry dependencies are available (or omit --offline to download them)".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("invalid cargo metadata: {e}"))
}

fn safe_relative(path: &str) -> Result<()> {
    if path.is_empty() || path.contains('\\') || path.contains(':') || path.starts_with('/') {
        return Err(format!("unsafe archive path: {path:?}"));
    }
    for part in path.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with([' ', '.'])
            || part
                .chars()
                .any(|c| c.is_control() || matches!(c, '<' | '>' | '|' | '?' | '*'))
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(format!("unsafe archive path: {path:?}"));
        }
    }
    Ok(())
}
fn checked_absolute(path: &Path, missing_final: bool) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    let mut checked = PathBuf::new();
    let count = absolute.components().count();
    for (index, part) in absolute.components().enumerate() {
        if part == Component::ParentDir {
            return Err(format!("path must not contain '..': {}", path.display()));
        }
        if part == Component::CurDir {
            continue;
        }
        checked.push(part);
        match fs::symlink_metadata(&checked) {
            Ok(info) if info.file_type().is_symlink() => {
                return Err(format!("refusing symlink: {}", checked.display()));
            }
            Ok(_) => {}
            Err(error)
                if missing_final
                    && index + 1 == count
                    && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect {}: {error}", checked.display())),
        }
    }
    Ok(checked)
}
fn is_license(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["LICENSE", "LICENCE", "COPYING", "NOTICE", "AUTHORS"]
        .iter()
        .any(|prefix| {
            upper == *prefix
                || upper.starts_with(&format!("{prefix}-"))
                || upper.starts_with(&format!("{prefix}."))
        })
}
fn excluded(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.starts_with('.')
        || matches!(
            lower.as_str(),
            "target"
                | "dist"
                | "build"
                | "node_modules"
                | "vendor"
                | "cache"
                | "caches"
                | "__pycache__"
        )
        || lower.split(['.', '-', '_']).any(|part| {
            matches!(
                part,
                "secret" | "secrets" | "credential" | "credentials" | "password" | "passwords"
            )
        })
        || matches!(lower.as_str(), "id_rsa" | "id_ed25519")
}
fn allowed_file(relative: &Path) -> bool {
    let name = relative.file_name().and_then(|p| p.to_str()).unwrap_or("");
    if relative.components().count() == 1 {
        return matches!(
            name,
            "Cargo.toml"
                | "Cargo.lock"
                | "lumendesk.toml"
                | "bindings.json"
                | "build.rs"
                | "package.json"
                | "package-lock.json"
                | "tsconfig.json"
                | "README"
                | "README.md"
                | "README.txt"
        ) || is_license(name);
    }
    matches!(
        relative.extension().and_then(|e| e.to_str()),
        Some(
            "rs" | "js"
                | "mjs"
                | "ts"
                | "tsx"
                | "jsx"
                | "html"
                | "css"
                | "json"
                | "svg"
                | "png"
                | "jpg"
                | "jpeg"
                | "webp"
                | "gif"
                | "ico"
                | "woff"
                | "woff2"
                | "ttf"
                | "otf"
                | "wgsl"
                | "glsl"
                | "txt"
                | "md"
        )
    )
}
fn collect(root: &Path, relative: &Path, prefix: &str, files: &mut Files) -> Result<()> {
    checked_absolute(&root.join(relative), false)?;
    let mut entries = fs::read_dir(root.join(relative))
        .map_err(|e| e.to_string())?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "package filenames must be Unicode")?;
        safe_relative(&name)?;
        if excluded(&name) {
            continue;
        }
        let child = relative.join(&name);
        let info = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        let included_dir = !relative.as_os_str().is_empty()
            || matches!(
                name.as_str(),
                "src" | "web" | "assets" | "examples" | "tests"
            );
        if info.file_type().is_symlink() && (included_dir || allowed_file(&child)) {
            return Err(format!(
                "refusing symlink in package input: {}",
                entry.path().display()
            ));
        }
        if info.is_dir() {
            if included_dir {
                collect(root, &child, prefix, files)?;
            }
        } else if allowed_file(&child) {
            if !info.is_file() {
                return Err(format!(
                    "package input must be a regular file: {}",
                    entry.path().display()
                ));
            }
            let name = child
                .to_str()
                .ok_or("package path must be Unicode")?
                .replace(std::path::MAIN_SEPARATOR, "/");
            add_file(
                files,
                &format!("{prefix}{name}"),
                &entry.path(),
                MAX_SOURCE_FILE,
            )?;
        }
    }
    Ok(())
}
fn add_file(files: &mut Files, name: &str, path: &Path, limit: u64) -> Result<()> {
    add_file_with_links(files, name, path, limit, false)
}
fn add_file_with_links(
    files: &mut Files,
    name: &str,
    path: &Path,
    limit: u64,
    allow_hardlinks: bool,
) -> Result<()> {
    let path = checked_absolute(path, false)?;
    let info = fs::metadata(&path).map_err(|e| e.to_string())?;
    if !info.is_file() || info.len() > limit {
        return Err(format!(
            "package input must be a regular file of at most {limit} bytes: {}",
            path.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if !allow_hardlinks && info.nlink() != 1 {
            return Err(format!(
                "refusing hard-linked package input: {}",
                path.display()
            ));
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("package input exceeded file limit while reading".into());
    }
    insert(files, name, bytes, 0o644)
}
fn insert(files: &mut Files, name: &str, bytes: Vec<u8>, mode: u32) -> Result<()> {
    safe_relative(name)?;
    if files.values().map(|f| f.bytes.len()).sum::<usize>() + bytes.len() > MAX_TOTAL {
        return Err("package contents exceed the 512 MiB safety limit".into());
    }
    if let Some(other) = files
        .keys()
        .find(|key| key.eq_ignore_ascii_case(name) && *key != name)
    {
        return Err(format!(
            "case-insensitive archive path collision: {other} and {name}"
        ));
    }
    files.insert(name.into(), Payload { bytes, mode });
    Ok(())
}

fn find_workspace(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|parent| {
            fs::read_to_string(parent.join("Cargo.toml"))
                .ok()
                .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
                .is_some_and(|value| value.get("workspace").is_some())
        })
        .map(Path::to_path_buf)
}
fn rewrite_manifest(
    path: &Path,
    package: &Value,
    prefix: &str,
    local: &BTreeMap<PathBuf, String>,
) -> Result<String> {
    let mut manifest: toml::Value =
        toml::from_str(&fs::read_to_string(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let package_table = manifest
        .get_mut("package")
        .and_then(toml::Value::as_table_mut)
        .ok_or("manifest lacks package table")?;
    for (key, value) in package_table.iter_mut() {
        if value.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
            let metadata_key = if key == "rust-version" {
                "rust_version"
            } else {
                key
            };
            let resolved = package
                .get(metadata_key)
                .ok_or_else(|| format!("cannot resolve inherited package.{key}"))?;
            *value = match resolved {
                Value::String(value) => toml::Value::String(value.clone()),
                Value::Array(values) => toml::Value::Array(
                    values
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .map(|s| toml::Value::String(s.into()))
                                .ok_or_else(|| "unsupported inherited package array".to_string())
                        })
                        .collect::<Result<_>>()?,
                ),
                _ => return Err(format!("unsupported inherited package.{key}")),
            };
        }
    }
    package_table.remove("workspace");
    let table = manifest.as_table_mut().ok_or("invalid Cargo manifest")?;
    if table.contains_key("patch") || table.contains_key("replace") {
        return Err("portable packaging does not support Cargo patch/replace overrides".into());
    }
    table.remove("workspace");
    if prefix.is_empty() {
        let members = local
            .values()
            .filter(|value| !value.is_empty())
            .map(|value| toml::Value::String(value.trim_end_matches('/').into()))
            .collect();
        let mut workspace = toml::map::Map::new();
        workspace.insert("exclude".into(), toml::Value::Array(members));
        workspace.insert("resolver".into(), toml::Value::String("3".into()));
        table.insert("workspace".into(), toml::Value::Table(workspace));
    }
    rewrite_dependency_tables(
        &mut manifest,
        path.parent().ok_or("manifest lacks parent")?,
        prefix,
        local,
    )?;
    toml::to_string_pretty(&manifest).map_err(|e| e.to_string())
}
fn rewrite_dependency_tables(
    value: &mut toml::Value,
    directory: &Path,
    prefix: &str,
    local: &BTreeMap<PathBuf, String>,
) -> Result<()> {
    let Some(table) = value.as_table_mut() else {
        return Ok(());
    };
    for (key, child) in table {
        if matches!(
            key.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        ) {
            for (name, dependency) in child.as_table_mut().ok_or("invalid dependencies table")? {
                if dependency.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                    return Err(format!(
                        "workspace-inherited dependency '{name}' must be explicit before packaging"
                    ));
                }
                if let Some(path) = dependency.get("path").and_then(toml::Value::as_str) {
                    // Dependency paths legitimately contain '..'; resolve then validate every actual component.
                    let original = directory.join(path).join("Cargo.toml");
                    reject_symlink_components_allow_parent(&original)?;
                    let canonical = original
                        .canonicalize()
                        .map_err(|e| format!("cannot resolve local dependency {name}: {e}"))?;
                    let target = local
                        .get(&canonical)
                        .ok_or_else(|| format!("unbundled local dependency '{name}'"))?;
                    let relative = if prefix.is_empty() {
                        target.trim_end_matches('/').to_owned()
                    } else {
                        format!("../../../{}", target.trim_end_matches('/'))
                    };
                    if relative.is_empty() {
                        return Err(
                            "local dependencies on the application itself are unsupported".into(),
                        );
                    }
                    dependency
                        .as_table_mut()
                        .ok_or("invalid path dependency")?
                        .insert("path".into(), toml::Value::String(relative));
                }
            }
        } else {
            rewrite_dependency_tables(child, directory, prefix, local)?;
        }
    }
    Ok(())
}
fn reject_symlink_components_allow_parent(path: &Path) -> Result<()> {
    let mut checked = PathBuf::new();
    for component in path.components() {
        if component == Component::ParentDir {
            checked.pop();
            continue;
        }
        checked.push(component);
        if fs::symlink_metadata(&checked)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err(format!(
                "refusing symlink in local dependency: {}",
                checked.display()
            ));
        }
    }
    Ok(())
}
fn validate_declared_paths(files: &Files, local: &BTreeMap<PathBuf, String>) -> Result<()> {
    for prefix in local.values() {
        let manifest: toml::Value = toml::from_str(
            std::str::from_utf8(&files[&format!("{prefix}Cargo.toml")].bytes)
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let mut paths = Vec::new();
        if let Some(package) = manifest.get("package") {
            for key in ["build", "readme", "license-file"] {
                if let Some(path) = package.get(key).and_then(toml::Value::as_str) {
                    paths.push(path);
                }
            }
        }
        if let Some(path) = manifest
            .get("lib")
            .and_then(|v| v.get("path"))
            .and_then(toml::Value::as_str)
        {
            paths.push(path);
        }
        for key in ["bin", "example", "test", "bench"] {
            for target in manifest
                .get(key)
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(path) = target.get("path").and_then(toml::Value::as_str) {
                    paths.push(path);
                }
            }
        }
        for path in paths {
            safe_relative(path)?;
            if !files.contains_key(&format!("{prefix}{path}")) {
                return Err(format!(
                    "declared Cargo file '{prefix}{path}' is outside the supported source allowlist or is missing"
                ));
            }
        }
    }
    Ok(())
}
fn license_inventory(
    packages: &[Value],
    root_id: &str,
    files: &mut Files,
) -> Result<Vec<Dependency>> {
    let mut result = Vec::new();
    let mut identities = std::collections::BTreeSet::new();
    for package in packages.iter().filter(|p| p["id"] != root_id) {
        let name = package["name"].as_str().ok_or("dependency lacks name")?;
        let version = package["version"]
            .as_str()
            .ok_or("dependency lacks version")?;
        if !identities.insert((name, version)) {
            return Err(format!(
                "multiple dependency sources share {name}-{version}; unique license archive paths are required"
            ));
        }
        let directory = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or("dependency lacks manifest")?,
        )
        .parent()
        .ok_or("dependency manifest lacks directory")?;
        let base = format!("dependency-licenses/{name}-{version}");
        let mut notices = Vec::new();
        let mut candidates = BTreeMap::new();
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let filename = entry
                .file_name()
                .into_string()
                .map_err(|_| "dependency notice filename must be Unicode")?;
            if is_license(&filename) && entry.file_type().map_err(|e| e.to_string())?.is_file() {
                candidates.insert(filename, entry.path());
            }
        }
        if let Some(path) = package["license_file"].as_str() {
            safe_relative(path)?;
            candidates.insert(path.into(), directory.join(path));
        }
        if package["source"].is_null()
            && let Some(workspace) = find_workspace(directory)
        {
            for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
                if workspace.join(name).exists() {
                    candidates.insert(name.into(), workspace.join(name));
                }
            }
        }
        for (relative, source) in candidates {
            let target = format!("{base}/{relative}");
            add_file(files, &target, &source, MAX_SOURCE_FILE)?;
            notices.push(target);
        }
        result.push(Dependency {
            name: name.into(),
            version: version.into(),
            source: package["source"]
                .as_str()
                .unwrap_or("local SDK dependency")
                .into(),
            license: package["license"].as_str().map(str::to_owned),
            notices,
        });
    }
    result.sort_by(|a, b| (&a.name, &a.version, &a.source).cmp(&(&b.name, &b.version, &b.source)));
    Ok(result)
}
fn write_archive(path: &Path, files: &Files) -> Result<()> {
    checked_absolute(path, true)?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("cannot create archive without replacing data: {e}"))?;
    let result = (|| {
        let gzip = GzBuilder::new()
            .mtime(0)
            .operating_system(255)
            .write(file, Compression::default());
        let mut archive = tar::Builder::new(gzip);
        archive.mode(tar::HeaderMode::Deterministic);
        for (path, payload) in files {
            safe_relative(path)?;
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.bytes.len() as u64);
            header.set_mode(payload.mode);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            archive
                .append_data(&mut header, path, payload.bytes.as_slice())
                .map_err(|e| e.to_string())?;
        }
        let mut file = archive
            .into_inner()
            .map_err(|e| e.to_string())?
            .finish()
            .map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_paths_are_portable_and_cannot_escape() {
        for path in [
            "../outside",
            "/absolute",
            "a/../../b",
            "C:/windows",
            "a\\b",
            "a//b",
            "a/./b",
            "a/",
            "CON",
            "src/NUL.txt",
            "a/COM1",
            "trail.",
            "space ",
            "a\nfile",
        ] {
            assert!(safe_relative(path).is_err(), "{path}");
        }
        for path in [
            "src/main.rs",
            "_sdk/lumendesk-core/Cargo.toml",
            "dependency-licenses/foo-1.0/LICENSE-MIT",
        ] {
            assert!(safe_relative(path).is_ok());
        }
    }
    #[test]
    fn source_filter_is_an_allowlist_with_sensitive_name_exclusions() {
        for path in [
            ".env",
            ".git",
            "credentials.json",
            "config.secret.json",
            "passwords.txt",
            "target",
            "node_modules",
        ] {
            assert!(excluded(path), "{path}");
        }
        for path in [
            "settings.toml",
            "identity.pem",
            "private.key",
            "db.sqlite",
            "random.zip",
        ] {
            assert!(!allowed_file(Path::new(path)));
        }
        assert!(allowed_file(Path::new("src/main.rs")));
        assert!(allowed_file(Path::new("web/index.html")));
    }
}
