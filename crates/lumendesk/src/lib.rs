//! An independent Rust desktop SDK. Backends are opt-in Cargo features.
pub use lumendesk_core as core;
use serde::{Deserialize, Serialize};

#[cfg(feature = "native")]
pub mod native;
#[cfg(feature = "webview")]
pub mod webview;

/// Shared, device-independent window settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowOptions {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub resizable: bool,
    pub decorated: bool,
}
impl WindowOptions {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 800.0,
            height: 600.0,
            resizable: true,
            decorated: true,
        }
    }
    pub fn validate(&self) -> Result<(), DesktopError> {
        if !self.width.is_finite()
            || !self.height.is_finite()
            || self.width < 160.0
            || self.height < 120.0
            || self.width > 16384.0
            || self.height > 16384.0
        {
            return Err(DesktopError::InvalidGeometry);
        }
        if self.title.contains('\0') {
            return Err(DesktopError::InvalidTitle);
        }
        Ok(())
    }
}
#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    #[error("window dimensions must be finite and between 160×120 and 16384×16384 logical pixels")]
    InvalidGeometry,
    #[error("window title cannot contain a NUL character")]
    InvalidTitle,
    #[error("desktop backend failed: {0}")]
    Backend(String),
    #[error("unsupported desktop capability: {0}")]
    Unsupported(&'static str),
}
/// Explicitly unsupported rather than silently pretending to deliver notifications.
pub fn notify(_title: &str, _body: &str) -> Result<(), DesktopError> {
    Err(DesktopError::Unsupported(
        "notifications are not included in this milestone",
    ))
}

/// The same serializable state can be used by native and web frontends.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FormState {
    pub name: String,
    pub enthusiastic: bool,
    pub greeting: String,
    pub submissions: u64,
}
impl FormState {
    pub fn submit(&mut self) -> bool {
        let name = self.name.trim();
        if name.is_empty() {
            return false;
        }
        self.greeting = format!("Hello, {name}{}", if self.enthusiastic { "!" } else { "." });
        self.submissions = self.submissions.saturating_add(1);
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_geometry() {
        for width in [f32::NAN, f32::INFINITY, -1.0, 0.0, 20000.0] {
            let mut o = WindowOptions::new("test");
            o.width = width;
            assert!(o.validate().is_err());
        }
        assert!(WindowOptions::new("valid").validate().is_ok());
    }
    #[test]
    fn shared_form_unicode_and_disabled() {
        let mut state = FormState::default();
        assert!(!state.submit());
        state.name = " Orion 星 ".into();
        state.enthusiastic = true;
        assert!(state.submit());
        assert_eq!(state.greeting, "Hello, Orion 星!");
        assert_eq!(state.submissions, 1);
    }
    #[test]
    fn desktop_services_fail_explicitly() {
        assert!(matches!(
            notify("a", "b"),
            Err(DesktopError::Unsupported(_))
        ));
    }
}
