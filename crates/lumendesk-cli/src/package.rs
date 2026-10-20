//! Deliberately bounded portable archives: no installer, credentials, or publishing.
use crate::{Config, Mode, Result, build};
use flate2::{Compression, GzBuilder};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};
use unicode_normalization::UnicodeNormalization;

const MAX_SOURCE_FILE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: usize = 512 * 1024 * 1024;
const MAX_NOTICE_ENTRIES: usize = 50_000;
const MAX_NOTICE_DEPTH: usize = 64;
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
    source_archive: Option<SourceArchive>,
    notice_provenance: Vec<NoticeProvenance>,
    unresolved_license_review: Vec<String>,
}
#[derive(Serialize)]
struct SourceArchive {
    url: String,
    sha256: String,
    local_source_verified: bool,
}
#[derive(Serialize)]
struct NoticeProvenance {
    path: String,
    url: String,
}

pub(crate) fn create(project: &Path, kind: Kind, output: &Path, offline: bool) -> Result<()> {
    let project = checked_absolute(project, false)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
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
    // Inspect and bound every selected input before parsing configuration or invoking
    // Cargo. In particular, reading lumendesk.toml first can block on a FIFO.
    let mut app_files = Files::new();
    collect(&project, Path::new(""), "", &mut app_files)?;
    if !app_files.contains_key("Cargo.toml") || !app_files.contains_key("lumendesk.toml") {
        return Err("portable package requires real Cargo.toml and lumendesk.toml files".into());
    }
    let config: Config = toml::from_str(
        std::str::from_utf8(&app_files["lumendesk.toml"].bytes)
            .map_err(|e| format!("invalid lumendesk.toml: {e}"))?,
    )
    .map_err(|e| format!("invalid lumendesk.toml: {e}"))?;
    config.validate()?;
    if config.build.manifest != "Cargo.toml" {
        return Err("portable packaging currently requires build.manifest = \"Cargo.toml\" at the project root".into());
    }
    let host = rust_host()?;
    let metadata = metadata(
        &project,
        offline,
        (kind == Kind::Binary).then_some(host.as_str()),
    )?;
    let workspace_root = Path::new(
        metadata["workspace_root"]
            .as_str()
            .ok_or("missing workspace root")?,
    );
    let lockfile_path = workspace_root.join("Cargo.lock");
    let selected = selected_packages(&metadata, kind)?;
    let packages = &selected;
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
    if checked_absolute(root_manifest, false)?
        .canonicalize()
        .map_err(|e| e.to_string())?
        != project.join("Cargo.toml")
    {
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
        )?
        .canonicalize()
        .map_err(|e| e.to_string())?;
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
        let resolver = workspace_resolver(&metadata)?;
        files = app_files;
        // Cargo metadata creates/updates the application's resolution without executing build scripts.
        add_file(&mut files, "Cargo.lock", &lockfile_path, MAX_SOURCE_FILE)?;
        for package in packages.iter().filter(|p| p["source"].is_null()) {
            let manifest = checked_absolute(
                Path::new(
                    package["manifest_path"]
                        .as_str()
                        .ok_or("missing local manifest")?,
                ),
                false,
            )?
            .canonicalize()
            .map_err(|e| e.to_string())?;
            let directory = manifest.parent().ok_or("local crate lacks a directory")?;
            let prefix = local.get(&manifest).ok_or("local crate was not selected")?;
            if !prefix.is_empty() {
                collect(directory, Path::new(""), prefix, &mut files)?;
                // Workspace SDK crates inherit the repository's license texts.
                let workspace = Path::new(metadata["workspace_root"].as_str().unwrap_or(""));
                // Repacked SDK crates live below _sdk without their old workspace
                // manifest. Their assets still belong to _sdk, not the app root.
                let sdk_workspace = bundled_sdk_root(directory)
                    .or_else(|| find_workspace(directory))
                    .unwrap_or_else(|| workspace.to_path_buf());
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
            let rewritten = rewrite_manifest(&manifest, package, prefix, &local, &resolver)?;
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
    let lockfile: toml::Value = toml::from_str(
        std::str::from_utf8(&read_regular_file(&lockfile_path, MAX_SOURCE_FILE, false)?)
            .map_err(|e| format!("invalid Cargo.lock: {e}"))?,
    )
    .map_err(|e| format!("invalid Cargo.lock: {e}"))?;
    let dependencies = license_inventory(packages, root_id, &lockfile, &mut files)?;
    insert(
        &mut files,
        "DEPENDENCY-SOURCES.txt",
        source_availability(&dependencies).into_bytes(),
        0o644,
    )?;
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
        "dependencies": {"inventory": "DEPENDENCIES.json", "scope": if kind == Kind::Binary { "Host-target normal/build dependency graph; Cargo metadata may still conservatively include features not linked into the executable." } else { "All resolved dependency targets; non-bundled registry dependencies are informational for source redistribution." }, "registry_sources_vendored": false, "license_review_required": true, "redistribution_review_required": true},
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
fn metadata(project: &Path, offline: bool, target: Option<&str>) -> Result<Value> {
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
    if let Some(target) = target {
        command.args(["--filter-platform", target]);
    }
    let output = command
        .output()
        .map_err(|e| format!("cannot resolve package dependencies with cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed; ensure registry dependencies are available (or omit --offline to download them)".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("invalid cargo metadata: {e}"))
}

fn selected_packages(metadata: &Value, kind: Kind) -> Result<Vec<Value>> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("Cargo metadata omitted packages")?;
    if kind == Kind::Source {
        return Ok(packages.clone());
    }
    let root = metadata["resolve"]["root"]
        .as_str()
        .ok_or("Cargo metadata omitted root")?;
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("Cargo metadata omitted resolve nodes")?;
    let mut selected = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if !selected.insert(id) {
            continue;
        }
        let node = nodes
            .iter()
            .find(|node| node["id"] == id)
            .ok_or("Cargo resolve node is missing")?;
        for dependency in node["deps"]
            .as_array()
            .ok_or("Cargo resolve node omitted deps")?
        {
            if dependency["dep_kinds"]
                .as_array()
                .ok_or("Cargo dependency omitted kinds")?
                .iter()
                .any(|kind| kind["kind"].is_null() || kind["kind"] == "build")
            {
                pending.push(
                    dependency["pkg"]
                        .as_str()
                        .ok_or("Cargo dependency omitted package ID")?,
                );
            }
        }
    }
    Ok(packages
        .iter()
        .filter(|package| {
            package["id"]
                .as_str()
                .is_some_and(|id| selected.contains(id))
        })
        .cloned()
        .collect())
}

fn safe_relative(path: &str) -> Result<()> {
    if path.is_empty() || path.contains('\\') || path.contains(':') || path.starts_with('/') {
        return Err(format!("unsafe archive path: {path:?}"));
    }
    for part in path.split('/') {
        let stem = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with([' ', '.'])
            || part
                .chars()
                .any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"))
                .is_some_and(|suffix| {
                    matches!(
                        suffix,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
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
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&checked) {
            Ok(info) if is_link(&info) => {
                return Err(format!(
                    "refusing symlink or reparse point: {}",
                    checked.display()
                ));
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
fn is_link(info: &fs::Metadata) -> bool {
    if info.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        return info.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    false
}
fn is_license(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "LICENSE",
        "LICENCE",
        "COPYING",
        "NOTICE",
        "AUTHORS",
        "COPYRIGHT",
        "UNLICENSE",
    ]
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
        if is_link(&info) && (included_dir || allowed_file(&child)) {
            return Err(format!(
                "refusing symlink or reparse point in package input: {}",
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
    insert(
        files,
        name,
        read_regular_file(path, limit, allow_hardlinks)?,
        0o644,
    )
}

fn read_regular_file(path: &Path, limit: u64, allow_hardlinks: bool) -> Result<Vec<u8>> {
    let path = checked_absolute(path, false)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Do not follow a replaced final symlink, or block opening a replaced FIFO.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ);
    }
    let file = options
        .open(&path)
        .map_err(|e| format!("cannot open package input {}: {e}", path.display()))?;
    // Validate the handle used for reading, not metadata from a separate path lookup.
    let info = file.metadata().map_err(|e| e.to_string())?;
    if is_link(&info) {
        return Err(format!(
            "refusing symlink or reparse point: {}",
            path.display()
        ));
    }
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
    #[cfg(windows)]
    {
        use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: file owns a live handle and information is writable storage for
        // the API's documented output structure. It is read only after success.
        let success =
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
        if success == 0 {
            return Err(format!(
                "cannot inspect package input links: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: GetFileInformationByHandle initialized the structure on success.
        let information = unsafe { information.assume_init() };
        if !allow_hardlinks && information.nNumberOfLinks != 1 {
            return Err(format!(
                "refusing hard-linked package input: {}",
                path.display()
            ));
        }
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("package input exceeded file limit while reading".into());
    }
    Ok(bytes)
}
fn insert(files: &mut Files, name: &str, bytes: Vec<u8>, mode: u32) -> Result<()> {
    safe_relative(name)?;
    if files.values().map(|f| f.bytes.len()).sum::<usize>() + bytes.len() > MAX_TOTAL {
        return Err("package contents exceed the 512 MiB safety limit".into());
    }
    // Check every shared directory component as well as filenames. Comparing only
    // complete paths misses both directory aliases and file/directory conflicts.
    // NFC + Unicode uppercase is a conservative collision policy, not a complete
    // emulation of every host filesystem's name-equivalence rules.
    let parts: Vec<_> = name.split('/').collect();
    let keys: Vec<_> = parts
        .iter()
        .map(|part| portable_component_key(part))
        .collect();
    for other in files.keys().filter(|other| *other != name) {
        let other_parts: Vec<_> = other.split('/').collect();
        let mut same_prefix = true;
        for ((part, key), other_part) in parts.iter().zip(&keys).zip(&other_parts) {
            if *key != portable_component_key(other_part) {
                same_prefix = false;
                break;
            }
            if part != other_part {
                return Err(format!(
                    "portable archive path collision: {other} and {name}"
                ));
            }
        }
        if same_prefix {
            return Err(format!(
                "archive file/directory path collision: {other} and {name}"
            ));
        }
    }
    files.insert(name.into(), Payload { bytes, mode });
    Ok(())
}
fn portable_component_key(value: &str) -> String {
    value.nfc().flat_map(char::to_uppercase).nfc().collect()
}

// The known bundled layout is deliberately narrower than an arbitrary ancestor
// containing an assets directory. collect() still validates all input paths.
fn bundled_sdk_root(directory: &Path) -> Option<PathBuf> {
    let crates = directory.parent()?;
    let sdk = crates.parent()?;
    (crates.file_name()? == "crates" && sdk.file_name()? == "_sdk").then(|| sdk.to_path_buf())
}

fn workspace_resolver(metadata: &Value) -> Result<String> {
    let root = Path::new(
        metadata["workspace_root"]
            .as_str()
            .ok_or("missing workspace root")?,
    );
    let bytes = read_regular_file(&root.join("Cargo.toml"), MAX_SOURCE_FILE, false)?;
    let manifest: toml::Value =
        toml::from_str(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if let Some(resolver) = manifest
        .get("workspace")
        .and_then(|v| v.get("resolver"))
        .or_else(|| manifest.get("package").and_then(|v| v.get("resolver")))
        .and_then(toml::Value::as_str)
    {
        return Ok(resolver.into());
    }
    // Cargo's metadata has already resolved inherited editions. A virtual
    // workspace defaults to 1, irrespective of any selected member's edition.
    let root_manifest = root.join("Cargo.toml");
    let edition = metadata["packages"]
        .as_array()
        .ok_or("missing Cargo packages")?
        .iter()
        .find(|p| {
            p["manifest_path"]
                .as_str()
                .is_some_and(|p| Path::new(p) == root_manifest)
        })
        .and_then(|p| p["edition"].as_str());
    Ok(match edition {
        Some("2024") => "3",
        Some("2021") => "2",
        _ => "1",
    }
    .into())
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
    resolver: &str,
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
    if prefix.is_empty() {
        // The effective value is emitted in [workspace]; Cargo rejects having
        // resolver in both the root package and workspace tables.
        package_table.remove("resolver");
    }
    let table = manifest.as_table_mut().ok_or("invalid Cargo manifest")?;
    if table.contains_key("patch") || table.contains_key("replace") {
        return Err("portable packaging does not support Cargo patch/replace overrides".into());
    }
    // Only the application root keeps a workspace in the portable archive.
    // Preserve its lint policy, and reject inheritance we cannot safely retain.
    let workspace_lints = table
        .get("workspace")
        .and_then(|workspace| workspace.get("lints"))
        .cloned();
    if table
        .get("lints")
        .and_then(|lints| lints.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true)
        && (!prefix.is_empty() || workspace_lints.is_none())
    {
        return Err(
            "workspace-inherited lints outside the application root must be explicit before packaging"
                .into(),
        );
    }
    table.remove("workspace");
    if prefix.is_empty() {
        let members = local
            .values()
            .filter(|value| !value.is_empty())
            .map(|value| toml::Value::String(value.trim_end_matches('/').into()))
            .collect();
        let mut workspace = toml::map::Map::new();
        if let Some(lints) = workspace_lints {
            workspace.insert("lints".into(), lints);
        }
        workspace.insert("exclude".into(), toml::Value::Array(members));
        workspace.insert("resolver".into(), toml::Value::String(resolver.into()));
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
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        if is_link(&fs::symlink_metadata(&checked).map_err(|e| e.to_string())?) {
            return Err(format!(
                "refusing symlink or reparse point in local dependency: {}",
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
    lockfile: &toml::Value,
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
        collect_dependency_notices(directory, Path::new(""), false, &mut 0, &mut candidates)?;
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
        let source = package["source"].as_str().unwrap_or("local SDK dependency");
        let license = package["license"].as_str();
        let mut source_archive = locked_source_archive(lockfile, name, version, source);
        let mut unresolved_license_review = Vec::new();
        let mut notice_provenance = Vec::new();
        let supplements = source_archive
            .as_ref()
            .map(|archive| {
                crate::license_overrides::notices(name, version, source, license, &archive.sha256)
            })
            .unwrap_or_default();
        if let Some(archive) = &source_archive {
            unresolved_license_review.extend(
                crate::license_overrides::review_notes(
                    name,
                    version,
                    source,
                    license,
                    &archive.sha256,
                )
                .into_iter()
                .map(str::to_owned),
            );
        }
        let needs_source_offer = license.is_some_and(|license| license.contains("MPL-2.0"));
        if needs_source_offer || !supplements.is_empty() {
            if let Some(archive) = source_archive.as_mut() {
                match verify_registry_sources(directory, name, version, &archive.sha256) {
                    Ok(()) => archive.local_source_verified = true,
                    Err(reason) => unresolved_license_review.push(format!("Registry source identity remains unverified: {reason}. Curated notice supplements and a verified source offer are omitted for this dependency.")),
                }
            } else {
                unresolved_license_review.push("MPL source availability requires an exact corresponding-source location; no supported locked registry archive is available.".into());
            }
        }
        if source_archive
            .as_ref()
            .is_some_and(|archive| archive.local_source_verified)
        {
            for notice in supplements {
                let target = format!("{base}/curated/{}", notice.path);
                if files.contains_key(&target) {
                    return Err(format!(
                        "curated notice would replace an existing dependency file: {target}"
                    ));
                }
                insert(files, &target, notice.bytes.to_vec(), 0o644)?;
                notice_provenance.push(NoticeProvenance {
                    path: target.clone(),
                    url: notice.provenance.into(),
                });
                notices.push(target);
            }
        }
        notices.sort();
        if notices.is_empty() {
            unresolved_license_review.push("No dependency license/notice files were found; determine and supply the applicable text and copyright notices before redistribution.".into());
        }
        if license.is_none() {
            unresolved_license_review.push("Cargo metadata contains no license expression; applicable terms require independent review.".into());
        }
        result.push(Dependency {
            name: name.into(),
            version: version.into(),
            source: source.into(),
            license: license.map(str::to_owned),
            notices,
            source_archive,
            notice_provenance,
            unresolved_license_review,
        });
    }
    result.sort_by(|a, b| (&a.name, &a.version, &a.source).cmp(&(&b.name, &b.version, &b.source)));
    Ok(result)
}

fn source_availability(dependencies: &[Dependency]) -> String {
    let mut result = String::from(
        "DEPENDENCY SOURCE AVAILABILITY\n\nThe versioned registry archives below are available from the listed public URLs. Their SHA-256 digests match Cargo.lock, and the local registry source files used for packaging were compared with the archive, including a check for added or missing files. Only Cargo's regular .cargo-ok extraction marker is excluded from that comparison. These entries provide exact corresponding source locations for the listed unmodified dependencies, including MPL-2.0 code where applicable. No registry source is vendored in this package.\n\nDEPENDENCIES.json may also list expected archive URLs with local_source_verified=false; those entries are not verified source-availability offers. Modified sources and unresolved license review items require separate handling. This file does not establish that every redistribution obligation has been satisfied.\n\n",
    );
    let mut count = 0;
    for dependency in dependencies {
        if let Some(archive) = &dependency.source_archive
            && archive.local_source_verified
        {
            count += 1;
            result.push_str(&format!(
                "{} {} ({})\nSource: {}\nSHA-256: {}\n\n",
                dependency.name,
                dependency.version,
                dependency
                    .license
                    .as_deref()
                    .unwrap_or("license undeclared"),
                archive.url,
                archive.sha256
            ));
        }
    }
    if count == 0 {
        result.push_str("No verified registry source-availability entries are included. Consult unresolved_license_review in DEPENDENCIES.json.\n");
    }
    result
}

fn is_notice_text(name: &str) -> bool {
    !matches!(
        Path::new(name)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "rs" | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "cc"
                | "js"
                | "ts"
                | "py"
                | "sh"
                | "a"
                | "lib"
                | "so"
                | "dll"
                | "dylib"
                | "exe"
                | "o"
                | "obj"
                | "wasm"
                | "bin"
                | "png"
                | "jpg"
                | "gif"
        )
    )
}

fn collect_dependency_notices(
    root: &Path,
    relative: &Path,
    in_notice_directory: bool,
    inspected: &mut usize,
    candidates: &mut BTreeMap<String, PathBuf>,
) -> Result<()> {
    if relative.components().count() > MAX_NOTICE_DEPTH {
        return Err("dependency notice scan exceeds the directory-depth limit".into());
    }
    checked_absolute(&root.join(relative), false)?;
    for entry in fs::read_dir(root.join(relative)).map_err(|e| e.to_string())? {
        *inspected += 1;
        if *inspected > MAX_NOTICE_ENTRIES {
            return Err("dependency notice scan exceeds the entry limit".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "dependency notice filename must be Unicode")?;
        // Never walk VCS metadata or build caches. Vendored dependency trees are
        // intentionally included: their notices can impose separate obligations.
        if matches!(
            name.as_str(),
            ".git" | ".hg" | ".svn" | "target" | "node_modules"
        ) {
            continue;
        }
        let path = relative.join(&name);
        let info = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if is_link(&info) {
            return Err(format!(
                "refusing symlink or reparse point in dependency notice tree: {}",
                entry.path().display()
            ));
        }
        if info.is_dir() {
            let notice_directory = matches!(
                name.to_ascii_uppercase().as_str(),
                "LICENSE"
                    | "LICENSES"
                    | "LICENCE"
                    | "LICENCES"
                    | "NOTICE"
                    | "NOTICES"
                    | "COPYRIGHTS"
                    | "THIRD-PARTY-LICENSES"
                    | "THIRD_PARTY_LICENSES"
            );
            collect_dependency_notices(
                root,
                &path,
                in_notice_directory || notice_directory,
                inspected,
                candidates,
            )?;
        } else if (in_notice_directory || is_license(&name)) && is_notice_text(&name) {
            if !info.is_file() {
                return Err(format!(
                    "dependency notice must be a regular file: {}",
                    entry.path().display()
                ));
            }
            let relative = path
                .to_str()
                .ok_or("dependency notice path must be Unicode")?
                .replace(std::path::MAIN_SEPARATOR, "/");
            safe_relative(&relative)?;
            candidates.insert(relative, entry.path());
        }
    }
    Ok(())
}

fn locked_source_archive(
    lockfile: &toml::Value,
    name: &str,
    version: &str,
    source: &str,
) -> Option<SourceArchive> {
    if !matches!(
        source,
        "registry+https://github.com/rust-lang/crates.io-index"
            | "registry+https://index.crates.io/"
            | "sparse+https://index.crates.io/"
    ) {
        return None;
    }
    let package = lockfile
        .get("package")?
        .as_array()?
        .iter()
        .find(|package| {
            package.get("name").and_then(toml::Value::as_str) == Some(name)
                && package.get("version").and_then(toml::Value::as_str) == Some(version)
                && package.get("source").and_then(toml::Value::as_str) == Some(source)
        })?;
    let checksum = package.get("checksum")?.as_str()?;
    if checksum.len() != 64 || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(SourceArchive {
        url: format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"),
        sha256: checksum.to_ascii_lowercase(),
        local_source_verified: false,
    })
}

// Check a registry source offer against the actual source tree used by Cargo.
// A Cargo.lock checksum alone authenticates the downloaded archive, not a local
// cache directory that a developer or build script may have modified afterwards.
fn verify_registry_sources(
    directory: &Path,
    name: &str,
    version: &str,
    checksum: &str,
) -> std::result::Result<(), &'static str> {
    let registry_id = directory
        .parent()
        .ok_or("unsupported registry cache layout")?;
    let source = registry_id
        .parent()
        .ok_or("unsupported registry cache layout")?;
    if source.file_name().and_then(|name| name.to_str()) != Some("src") {
        return Err("unsupported registry cache layout");
    }
    let registry = source.parent().ok_or("unsupported registry cache layout")?;
    let archive_path = registry
        .join("cache")
        .join(
            registry_id
                .file_name()
                .ok_or("unsupported registry cache layout")?,
        )
        .join(format!("{name}-{version}.crate"));
    let bytes = read_regular_file(&archive_path, MAX_TOTAL as u64, false)
        .map_err(|_| "cached registry archive is unavailable or unsafe")?;
    if format!("{:x}", Sha256::digest(&bytes)) != checksum {
        return Err("cached registry archive does not match Cargo.lock checksum");
    }
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice()));
    let mut expected = BTreeSet::new();
    let mut total = 0_u64;
    let mut count = 0;
    let crate_root = PathBuf::from(format!("{name}-{version}"));
    for entry in archive
        .entries()
        .map_err(|_| "cached registry archive is invalid")?
    {
        count += 1;
        if count > MAX_NOTICE_ENTRIES {
            return Err("registry archive exceeds entry limit");
        }
        let mut entry = entry.map_err(|_| "cached registry archive entry is invalid")?;
        let path = entry
            .path()
            .map_err(|_| "cached registry archive path is invalid")?
            .into_owned();
        let relative = path
            .strip_prefix(&crate_root)
            .map_err(|_| "registry archive has an unexpected root")?;
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err("registry archive contains an unsafe path");
        }
        if entry.header().entry_type().is_dir() {
            continue;
        }
        if !entry.header().entry_type().is_file()
            || relative.as_os_str().is_empty()
            || !expected.insert(relative.to_path_buf())
        {
            return Err("registry archive contains links or duplicate file entries");
        }
        let size = entry
            .header()
            .size()
            .map_err(|_| "invalid registry archive file size")?;
        total = total
            .checked_add(size)
            .ok_or("registry archive exceeds size limit")?;
        if size > MAX_SOURCE_FILE || total > MAX_TOTAL as u64 {
            return Err("registry archive exceeds size limit");
        }
        let actual = read_regular_file(&directory.join(relative), MAX_SOURCE_FILE, false)
            .map_err(|_| "registry source file is unavailable or unsafe")?;
        if actual.len() as u64 != size {
            return Err("registry source file differs from the locked archive");
        }
        let mut archived = Vec::new();
        entry
            .read_to_end(&mut archived)
            .map_err(|_| "cannot read registry source archive")?;
        if actual != archived {
            return Err("registry source file differs from the locked archive");
        }
    }
    let mut actual = BTreeSet::new();
    collect_registry_files(directory, Path::new(""), &mut 0, &mut actual)?;
    if actual != expected {
        return Err("registry source tree has added or missing files");
    }
    Ok(())
}

fn collect_registry_files(
    root: &Path,
    relative: &Path,
    inspected: &mut usize,
    files: &mut BTreeSet<PathBuf>,
) -> std::result::Result<(), &'static str> {
    if relative.components().count() > MAX_NOTICE_DEPTH {
        return Err("registry source tree exceeds directory-depth limit");
    }
    checked_absolute(&root.join(relative), false).map_err(|_| "registry source path is unsafe")?;
    for entry in
        fs::read_dir(root.join(relative)).map_err(|_| "cannot read registry source directory")?
    {
        *inspected += 1;
        if *inspected > MAX_NOTICE_ENTRIES {
            return Err("registry source tree exceeds entry limit");
        }
        let entry = entry.map_err(|_| "cannot inspect registry source entry")?;
        let path = relative.join(entry.file_name());
        // Cargo writes this extraction-complete marker; it is not crate source.
        if path == Path::new(".cargo-ok") {
            // A directory or link with this name must not hide extra source files.
            // Read through the same checked handle path as ordinary source inputs.
            read_regular_file(&entry.path(), 4096, false)
                .map_err(|_| "registry extraction marker is not a safe regular file")?;
            continue;
        }
        let info = fs::symlink_metadata(entry.path())
            .map_err(|_| "cannot inspect registry source file")?;
        if is_link(&info) || (!info.is_dir() && !info.is_file()) {
            return Err("registry source contains a link or special file");
        }
        if info.is_dir() {
            collect_registry_files(root, &path, inspected, files)?;
        } else {
            files.insert(path);
        }
    }
    Ok(())
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "lumendesk-package-unit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn write(&self, relative: &str, bytes: &[u8]) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn binary_inventory_excludes_unreachable_and_dev_only_packages() {
        let metadata = serde_json::json!({
            "packages": [
                {"id": "root"}, {"id": "normal"}, {"id": "build"},
                {"id": "dev"}, {"id": "other-platform"},
            ],
            "resolve": {"root": "root", "nodes": [
                {"id": "root", "deps": [
                    {"pkg": "normal", "dep_kinds": [{"kind": null}]},
                    {"pkg": "build", "dep_kinds": [{"kind": "build"}]},
                    {"pkg": "dev", "dep_kinds": [{"kind": "dev"}]},
                ]},
                {"id": "normal", "deps": []}, {"id": "build", "deps": []},
                {"id": "dev", "deps": []}, {"id": "other-platform", "deps": []},
            ]}
        });
        let binary = selected_packages(&metadata, Kind::Binary).unwrap();
        assert_eq!(
            binary
                .iter()
                .map(|package| package["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["root", "normal", "build"]
        );
        assert_eq!(selected_packages(&metadata, Kind::Source).unwrap().len(), 5);
    }

    #[test]
    fn dependency_notices_include_nested_vendored_texts_without_source_or_binary_lookalikes() {
        let fixture = Fixture::new();
        for path in [
            "COPYRIGHT-RUST.txt",
            "UNLICENSE",
            "vendor/dbus/COPYING",
            "vendor/dbus/cmake/COPYING-CMAKE-SCRIPTS",
            "licenses/MIT.txt",
            "src/copying.rs",
            "lib/libwinapi_oemlicense.a",
            "licenses/helper.rs",
        ] {
            fixture.write(path, path.as_bytes());
        }
        let mut candidates = BTreeMap::new();
        collect_dependency_notices(&fixture.0, Path::new(""), false, &mut 0, &mut candidates)
            .unwrap();
        assert_eq!(
            candidates.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "COPYRIGHT-RUST.txt",
                "UNLICENSE",
                "licenses/MIT.txt",
                "vendor/dbus/COPYING",
                "vendor/dbus/cmake/COPYING-CMAKE-SCRIPTS"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn dependency_notice_scans_reject_links_and_bound_directory_depth() {
        let fixture = Fixture::new();
        fixture.write("outside.txt", b"private data");
        fs::create_dir(fixture.0.join("dependency")).unwrap();
        std::os::unix::fs::symlink(
            fixture.0.join("outside.txt"),
            fixture.0.join("dependency/LICENSE"),
        )
        .unwrap();
        let mut candidates = BTreeMap::new();
        let result = collect_dependency_notices(
            &fixture.0.join("dependency"),
            Path::new(""),
            false,
            &mut 0,
            &mut candidates,
        );
        assert!(result.unwrap_err().contains("symlink"));
        let deep: PathBuf = std::iter::repeat_n("x", MAX_NOTICE_DEPTH + 1).collect();
        assert!(
            collect_dependency_notices(&fixture.0, &deep, false, &mut 0, &mut candidates)
                .unwrap_err()
                .contains("directory-depth")
        );
    }

    #[test]
    fn source_availability_requires_archive_checksum_and_unmodified_registry_tree() {
        let fixture = Fixture::new();
        let prefix = "registry/src/test-index/demo-1.0.0";
        let contents = [
            (
                "Cargo.toml",
                b"[package]\nname=\"demo\"\nversion=\"1.0.0\"\n".as_slice(),
            ),
            ("src/lib.rs", b"pub fn original() {}\n".as_slice()),
            ("LICENSE", b"fixture license text\n".as_slice()),
        ];
        let mut archived_files = Files::new();
        for (path, bytes) in contents {
            fixture.write(&format!("{prefix}/{path}"), bytes);
            insert(
                &mut archived_files,
                &format!("demo-1.0.0/{path}"),
                bytes.to_vec(),
                0o644,
            )
            .unwrap();
        }
        fs::create_dir_all(fixture.0.join("registry/cache/test-index")).unwrap();
        let archive_path = fixture.0.join("registry/cache/test-index/demo-1.0.0.crate");
        write_archive(&archive_path, &archived_files).unwrap();
        fixture.write(&format!("{prefix}/.cargo-ok"), b"cargo marker");
        let original_archive = fs::read(&archive_path).unwrap();
        let checksum = format!("{:x}", Sha256::digest(&original_archive));
        let source = "registry+https://github.com/rust-lang/crates.io-index";
        let package = serde_json::json!({"id": "demo", "name": "demo", "version": "1.0.0", "source": source, "license": "MPL-2.0", "manifest_path": fixture.0.join(prefix).join("Cargo.toml")});
        let lockfile: toml::Value = toml::from_str(&format!(
            "[[package]]\nname='demo'\nversion='1.0.0'\nsource='{source}'\nchecksum='{checksum}'\n"
        ))
        .unwrap();
        let inventory = || {
            license_inventory(
                std::slice::from_ref(&package),
                "root",
                &lockfile,
                &mut Files::new(),
            )
            .unwrap()
        };
        let verified = inventory();
        assert!(
            verified[0]
                .source_archive
                .as_ref()
                .unwrap()
                .local_source_verified
        );
        assert!(verified[0].unresolved_license_review.is_empty());
        assert!(
            source_availability(&verified)
                .contains("Source: https://static.crates.io/crates/demo/demo-1.0.0.crate")
        );

        fixture.write(&format!("{prefix}/src/lib.rs"), b"pub fn modified() {}\n");
        let changed = inventory();
        assert!(
            !changed[0]
                .source_archive
                .as_ref()
                .unwrap()
                .local_source_verified
        );
        assert!(!changed[0].unresolved_license_review.is_empty());
        assert!(
            !source_availability(&changed)
                .contains("Source: https://static.crates.io/crates/demo/demo-1.0.0.crate")
        );
        fixture.write(&format!("{prefix}/src/lib.rs"), contents[1].1);

        fixture.write(&format!("{prefix}/src/extra.rs"), b"added source");
        assert!(
            !inventory()[0]
                .source_archive
                .as_ref()
                .unwrap()
                .local_source_verified
        );
        fs::remove_file(fixture.0.join(prefix).join("src/extra.rs")).unwrap();

        fs::remove_file(fixture.0.join(prefix).join(".cargo-ok")).unwrap();
        fixture.write(
            &format!("{prefix}/.cargo-ok/hidden.rs"),
            b"added source under marker name",
        );
        assert!(
            !inventory()[0]
                .source_archive
                .as_ref()
                .unwrap()
                .local_source_verified
        );
        fs::remove_dir_all(fixture.0.join(prefix).join(".cargo-ok")).unwrap();
        fixture.write(&format!("{prefix}/.cargo-ok"), b"cargo marker");

        let mut changed_archive = original_archive;
        changed_archive.push(0);
        fs::write(&archive_path, changed_archive).unwrap();
        let changed = inventory();
        assert!(
            !changed[0]
                .source_archive
                .as_ref()
                .unwrap()
                .local_source_verified
        );
        assert!(changed[0].unresolved_license_review[0].contains("checksum"));
    }
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
            "src/NUL .txt",
            "a/COM1",
            "a/COM1 .txt",
            "a/COM¹.txt",
            "a/LPT²",
            "quote\".rs",
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
    fn archive_paths_reject_colliding_directory_components_and_file_prefixes() {
        for (first, second) in [
            ("web/A.rs", "web/a.rs/child.rs"),
            ("web/a.rs/child.rs", "web/A.rs"),
            ("src/Forms/one.rs", "src/forms/two.rs"),
            ("src/one.rs", "src/one.rs/two.rs"),
            ("src/café.rs", "src/cafe\u{301}.rs"),
            ("src/Ä.rs", "src/ä.rs"),
            (
                "dependency-licenses/example-1/LICENSE",
                "dependency-licenses/Example-1/AUTHORS",
            ),
            (
                "dependency-licenses/example-1/COPYING",
                "dependency-licenses/example-1/copying/MIT.txt",
            ),
            (
                "dependency-licenses/example-1/ÅUTHORS",
                "dependency-licenses/example-1/A\u{30a}UTHORS",
            ),
        ] {
            let mut files = Files::new();
            insert(&mut files, first, b"first".to_vec(), 0o644).unwrap();
            assert!(
                insert(&mut files, second, b"second".to_vec(), 0o644).is_err(),
                "archive must reject {first} and {second}"
            );
        }
        let mut files = Files::new();
        insert(&mut files, "src/main.rs", b"first".to_vec(), 0o644).unwrap();
        insert(&mut files, "src/main.rs", b"rewritten".to_vec(), 0o644).unwrap();
        insert(&mut files, "src/other.rs", b"other".to_vec(), 0o644).unwrap();
        assert_eq!(files["src/main.rs"].bytes, b"rewritten");
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
