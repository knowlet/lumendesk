use crate::Result;
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Replace a generated file atomically, never following its final symlink.
/// Config-derived paths additionally reject symlinks beneath the canonical project.
pub(crate) fn write_generated(path: &Path, contents: &str, project: Option<&Path>) -> Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    let path = if let Some(project) = project {
        let project_absolute = if project.is_absolute() {
            project.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|error| error.to_string())?
                .join(project)
        };
        let root = project
            .canonicalize()
            .map_err(|error| format!("cannot open project: {error}"))?;
        let relative = absolute
            .strip_prefix(&project_absolute)
            .map_err(|_| "configured output must be inside the project".to_string())?;
        let mut checked = root.clone();
        for component in relative.components() {
            match component {
                Component::Normal(part) => checked.push(part),
                Component::CurDir => continue,
                _ => return Err("configured output must be inside the project without '..'".into()),
            }
            reject_symlink(&checked)?;
        }
        let parent = checked.parent().ok_or("output needs a parent directory")?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        if !parent
            .canonicalize()
            .map_err(|error| error.to_string())?
            .starts_with(root)
        {
            return Err("configured output resolved outside the project".into());
        }
        checked
    } else {
        absolute
    };
    reject_symlink(&path)?;
    let parent = path.parent().ok_or("output needs a parent directory")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary: PathBuf = parent.join(format!(
        ".lumendesk-generated-{}-{stamp}.tmp",
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("cannot stage generated output: {error}"))?;
    let result = (|| {
        file.write_all(contents.as_bytes())
            .map_err(|error| format!("cannot write generated output: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot flush generated output: {error}"))?;
        drop(file);
        // Replacing the directory entry also avoids modifying an outside hard-link target.
        reject_symlink(&path)?;
        fs::rename(&temporary, &path)
            .map_err(|error| format!("cannot replace {}: {error}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "refusing symlink in generated output path: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect output {}: {error}", path.display())),
    }
}
