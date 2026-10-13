use crate::{Mode, Result, config::validate_name};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Create a fresh project without replacing existing files. Existing empty directories are allowed.
pub fn create_project(destination: &Path, mode: Mode, sdk_path: Option<&Path>) -> Result<PathBuf> {
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("destination must end in a valid project name")?;
    validate_name(name)?;
    let sdk = if let Some(path) = sdk_path {
        let path = path
            .canonicalize()
            .map_err(|error| format!("cannot find SDK path {}: {error}", path.display()))?;
        if !path.join("Cargo.toml").is_file() {
            return Err(format!(
                "SDK path {} must contain Cargo.toml",
                path.display()
            ));
        }
        Some(path)
    } else {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../lumendesk");
        path.canonicalize()
            .ok()
            .filter(|path| path.join("Cargo.toml").is_file())
    };
    let dependency = if let Some(path) = sdk {
        format!("path = {}, ", toml_string(&path.to_string_lossy()))
    } else {
        "version = \"0.1\", ".into()
    };
    let feature = if mode == Mode::Native {
        "native"
    } else {
        "webview"
    };
    let main = if mode == Mode::Native {
        format!(
            "use lumendesk::WindowOptions;\n\nfn main() -> Result<(), Box<dyn std::error::Error>> {{\n    lumendesk::native::run_form(WindowOptions::new({}))?;\n    Ok(())\n}}\n",
            toml_string(name)
        )
    } else {
        format!(
            "use lumendesk::WindowOptions;\n\nfn main() -> Result<(), Box<dyn std::error::Error>> {{\n    lumendesk::webview::run_html(WindowOptions::new({}), include_str!(\"../web/index.html\"))?;\n    Ok(())\n}}\n",
            toml_string(name)
        )
    };
    let config = format!(
        "schema_version = 1\n\n[app]\nname = {name:?}\nid = \"dev.lumendesk.{}\"\nversion = \"0.1.0\"\nmode = {:?}\n\n[build]\nmanifest = \"Cargo.toml\"\nbinary = {name:?}\n\n[bindings]\nschema = \"bindings.json\"\noutput = \"web/lumendesk.generated.ts\"\n",
        name.replace('_', "-"),
        mode.as_str()
    );
    let manifest = format!(
        "[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.95\"\n\n[workspace]\n\n[dependencies]\nlumendesk = {{ {dependency}default-features = false, features = [{feature:?}] }}\n"
    );
    let schema = serde_json::to_string_pretty(&crate::sample_schema())
        .map_err(|error| error.to_string())?
        + "\n";
    let client = crate::generate_typescript(&crate::sample_schema())?;
    let mut files = vec![
        ("Cargo.toml", manifest),
        ("lumendesk.toml", config),
        ("src/main.rs", main),
        ("bindings.json", schema),
        ("web/lumendesk.generated.ts", client),
        (".gitignore", "/target/\n/.lumendesk/\n".into()),
        (
            "README.md",
            format!(
                "# {name}\n\nAn independently generated LumenDesk {} application.\n\nRun `lumendesk doctor`, then `lumendesk dev`. Build a host release executable with\n`lumendesk build`. Regenerate the typed sample client with `lumendesk bindings`.\n\nThe native template uses the SDK's form. The web template embeds `web/index.html`\nand uses the reference greeting service. Edit `src/main.rs` to customize your app. The generated client describes `bindings.json`;\nregister matching backend services before using it with a custom frontend.\n\nDevelopment sets `LUMENDESK_APP_ID`, `LUMENDESK_ENV`, and `LUMENDESK_DATA_DIR`.\nUse those values for application identity and user data when adding persistence.\nThe CLI itself stores development data separately under `.lumendesk`.\n\nScaffolds created from a workspace use its absolute local SDK path. Use\n`lumendesk package --kind source --out app-source.tar.gz` to bundle the SDK\nwith relative paths before moving the project, or update that dependency manually.\n",
                mode.as_str()
            ),
        ),
    ];
    if mode == Mode::Web {
        files.push(("web/index.html", WEB_HTML.into()));
    }
    files.sort_by_key(|(path, _)| *path);
    let existed = match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!(
                    "destination {} must be a real, empty directory",
                    destination.display()
                ));
            }
            if fs::read_dir(destination)
                .map_err(|error| error.to_string())?
                .next()
                .is_some()
            {
                return Err(format!(
                    "refusing populated destination {}; no files were changed",
                    destination.display()
                ));
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "cannot inspect destination {}: {error}",
                destination.display()
            ));
        }
    };
    if !existed {
        fs::create_dir(destination).map_err(|error| {
            format!(
                "cannot create {}: {error}; its parent directory must already exist",
                destination.display()
            )
        })?;
    }
    let mut written = Vec::new();
    let mut directories = Vec::new();
    let result = (|| {
        for (relative, content) in files {
            let path = destination.join(relative);
            if let Some(parent) = path.parent().filter(|parent| *parent != destination)
                && !directories.iter().any(|directory| directory == parent)
            {
                fs::create_dir(parent).map_err(|error| {
                    format!(
                        "cannot create {} without replacing existing data: {error}",
                        parent.display()
                    )
                })?;
                directories.push(parent.to_path_buf());
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|error| {
                    format!(
                        "cannot create {} without replacing existing data: {error}",
                        path.display()
                    )
                })?;
            written.push(path.clone());
            file.write_all(content.as_bytes())
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        }
        destination
            .canonicalize()
            .map_err(|error| error.to_string())
    })();
    if result.is_err() {
        for path in written.iter().rev() {
            let _ = fs::remove_file(path);
        }
        for path in directories.iter().rev() {
            let _ = fs::remove_dir(path);
        }
        if !existed {
            let _ = fs::remove_dir(destination);
        }
    }
    result
}
fn toml_string(value: &str) -> String {
    // JSON quoted strings are also valid TOML basic strings for filesystem paths here.
    serde_json::to_string(value).expect("strings are serializable")
}

const WEB_HTML: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>LumenDesk greeting workshop</title>
<style>
:root { font: 17px/1.5 system-ui,sans-serif; color: #203143; background: #f5f8fc; }
main { max-width: 38rem; margin: 4rem auto; padding: 2rem; background: white; border-radius: 1rem; }
h1 { margin-top: 0; } label { display: block; margin: 1rem 0; } input[type=text] { display:block; padding:.6rem; width:90%; font:inherit; }
button { border:0; padding:.7rem 1.2rem; border-radius:.4rem; color:white; background:#22558e; font:inherit; cursor:pointer; }
button:disabled { opacity:.5; cursor:wait; } output { display:block; margin-top:1.4rem; font-weight:600; }
</style></head><body><main>
<h1>Greeting workshop</h1><p>This form calls a typed Rust service in your desktop app.</p>
<form id="form"><label>Your name<input name="name" type="text" value="Orion" required></label>
<label><input name="enthusiastic" type="checkbox"> Add a little enthusiasm</label>
<button id="submit" disabled>Create greeting</button></form>
<output id="result" aria-live="polite">Waiting for the desktop connection…</output>
<p id="count"></p></main><script>
'use strict';
const form = document.querySelector('#form');
const result = document.querySelector('#result');
let connected = false;
function show(state) { result.textContent = state.greeting; document.querySelector('#count').textContent = `Greetings created: ${state.submissions}`; }
function ready() {
  if (connected || !window.lumen) return;
  connected = true;
  document.querySelector('#submit').disabled = false;
  result.textContent = 'Ready when you are.';
  window.lumen.on('greeting', show);
}
window.addEventListener('lumendesk-ready', ready);
ready();
form.addEventListener('submit', async event => {
  event.preventDefault();
  if (!connected) return;
  try { show(await window.lumen.call('greet', {name: form.elements.name.value, enthusiastic: form.elements.enthusiastic.checked})); }
  catch (error) { result.textContent = `Could not create greeting: ${error.message || error}`; }
});
</script></body></html>
"#;
