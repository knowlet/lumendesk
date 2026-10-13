use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Native,
    Web,
}
impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Web => "web",
        }
    }
}
impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "native" => Ok(Self::Native),
            "web" => Ok(Self::Web),
            _ => Err(format!("unsupported mode '{value}'; choose native or web")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub app: AppConfig,
    pub build: BuildConfig,
    #[serde(default)]
    pub bindings: BindingsConfig,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub name: String,
    pub id: String,
    pub version: String,
    pub mode: Mode,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    pub manifest: String,
    pub binary: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BindingsConfig {
    pub schema: String,
    pub output: String,
}
impl Default for BindingsConfig {
    fn default() -> Self {
        Self {
            schema: "bindings.json".into(),
            output: "web/lumendesk.generated.ts".into(),
        }
    }
}
impl Config {
    pub fn load(project: &Path) -> Result<Self> {
        let path = project.join("lumendesk.toml");
        let text = std::fs::read_to_string(&path).map_err(|error| format!("cannot read {}: {error}; run this command in a LumenDesk project or use --project DIR", path.display()))?;
        let config: Self = toml::from_str(&text)
            .map_err(|error| format!("invalid {}: {error}", path.display()))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported configuration schema_version {}; expected 1",
                self.schema_version
            ));
        }
        validate_name(&self.app.name)?;
        validate_name(&self.build.binary)?;
        let sections: Vec<_> = self.app.id.split('.').collect();
        if sections.len() < 2
            || sections.iter().any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
        {
            return Err(
                "app.id must be a reverse-domain identifier such as dev.example.hello".into(),
            );
        }
        if self.app.version.is_empty() || self.app.version.chars().any(char::is_whitespace) {
            return Err("app.version must be a nonempty version without whitespace".into());
        }
        for (key, value) in [
            ("build.manifest", &self.build.manifest),
            ("bindings.schema", &self.bindings.schema),
            ("bindings.output", &self.bindings.output),
        ] {
            let path = Path::new(value);
            if value.is_empty()
                || path
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
            {
                return Err(format!(
                    "{key} must be a relative path inside the project (no '..')"
                ));
            }
        }
        Ok(())
    }
}
pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("project and binary names must start with an ASCII letter and contain at most 64 letters, digits, '-' or '_'".into());
    }
    if matches!(name, "test" | "build" | "deps" | "examples" | "incremental") {
        return Err(format!(
            "'{name}' is reserved by Cargo; choose another project name"
        ));
    }
    Ok(())
}
