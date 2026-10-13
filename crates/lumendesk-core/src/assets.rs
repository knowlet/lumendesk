//! Filesystem-independent embedded-asset lookup.
//!
//! Paths are decoded repeatedly (at most eight rounds), normalized to relative
//! slash-separated keys, and rejected if any round contains a parent segment,
//! backslash, control character, malformed escape, or URI delimiter. Literal
//! query strings and fragments are discarded before decoding. A literal `%`
//! filename is intentionally unsupported: every percent sign must introduce an
//! escape, and nested escapes must resolve fully within the decoding limit.
//!
//! Only an extensionless missing final path component can use the entry page.
//! In particular a missing `.js`, `.css`, image, or dotfile never returns HTML.
//! This module opens no files and has no access to paths outside its own map.

use std::collections::BTreeMap;
use std::fmt;

const MAX_DECODE_ROUNDS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetError {
    InvalidPath(&'static str),
    InvalidMime,
    MissingAsset(String),
    MissingEntry(String),
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(reason) => write!(f, "invalid asset path: {reason}"),
            Self::InvalidMime => {
                write!(f, "asset MIME type must be a valid, single-line media type")
            }
            Self::MissingAsset(path) => write!(f, "embedded asset not found: {path}"),
            Self::MissingEntry(path) => write!(f, "embedded entry page not found: {path}"),
        }
    }
}

impl std::error::Error for AssetError {}

/// Owned embedded bytes and an explicitly supplied response media type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    bytes: Vec<u8>,
    mime: String,
}

impl Asset {
    pub fn new(bytes: impl Into<Vec<u8>>, mime: impl Into<String>) -> Result<Self, AssetError> {
        let mime = mime.into();
        validate_mime(&mime)?;
        Ok(Self {
            bytes: bytes.into(),
            mime,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn mime(&self) -> &str {
        &self.mime
    }
}

/// A successful lookup, including the key actually served for route fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAsset<'a> {
    pub requested_path: String,
    pub served_path: &'a str,
    pub asset: &'a Asset,
    pub is_fallback: bool,
}

#[derive(Debug, Clone)]
pub struct AssetStore {
    entry_path: String,
    assets: BTreeMap<String, Asset>,
}

impl AssetStore {
    /// Creates an empty store. The entry page must subsequently be inserted.
    pub fn new(entry_path: impl AsRef<str>) -> Result<Self, AssetError> {
        let entry_path = normalize_asset_path(entry_path.as_ref())?;
        if entry_path.is_empty() {
            return Err(AssetError::InvalidPath("entry path must name an asset"));
        }
        Ok(Self {
            entry_path,
            assets: BTreeMap::new(),
        })
    }

    /// Inserts or replaces an asset under its canonical path, returning the old
    /// value if present. Both path and MIME validation precede any mutation.
    pub fn insert(
        &mut self,
        path: impl AsRef<str>,
        bytes: impl Into<Vec<u8>>,
        mime: impl Into<String>,
    ) -> Result<Option<Asset>, AssetError> {
        let path = normalize_asset_path(path.as_ref())?;
        if path.is_empty() {
            return Err(AssetError::InvalidPath("asset path must name an asset"));
        }
        let asset = Asset::new(bytes, mime)?;
        Ok(self.assets.insert(path, asset))
    }

    pub fn entry_path(&self) -> &str {
        &self.entry_path
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Resolves a request path. `/` and other extensionless misses use the
    /// entry page; literal existing assets always win over fallback.
    pub fn resolve(&self, request_path: &str) -> Result<ResolvedAsset<'_>, AssetError> {
        let requested_path = normalize_asset_path(request_path)?;
        if let Some((key, asset)) = self.assets.get_key_value(&requested_path) {
            return Ok(ResolvedAsset {
                requested_path,
                served_path: key,
                asset,
                is_fallback: false,
            });
        }
        let final_component = requested_path.rsplit('/').next().unwrap_or("");
        if final_component.contains('.') {
            return Err(AssetError::MissingAsset(requested_path));
        }
        let asset = self
            .assets
            .get(&self.entry_path)
            .ok_or_else(|| AssetError::MissingEntry(self.entry_path.clone()))?;
        Ok(ResolvedAsset {
            requested_path,
            served_path: &self.entry_path,
            asset,
            is_fallback: true,
        })
    }
}

/// Returns a canonical in-memory key, never a filesystem path.
///
/// Repeated slashes and `.` components are folded. Parent components are
/// rejected rather than resolved, even when resolving them would stay inside
/// the apparent root. Full URLs and Windows drive paths are not accepted.
pub fn normalize_asset_path(request_path: &str) -> Result<String, AssetError> {
    // Reject raw controls/backslashes even inside a query. This avoids unsafe
    // strings being interpreted differently by a later HTTP adapter.
    if request_path.chars().any(|c| c == '\\' || c.is_control()) {
        return Err(AssetError::InvalidPath(
            "backslashes and controls are forbidden",
        ));
    }
    let end = request_path.find(['?', '#']).unwrap_or(request_path.len());
    let mut path = request_path[..end].to_owned();
    for round in 0..=MAX_DECODE_ROUNDS {
        validate_path_round(&path)?;
        if !path.contains('%') {
            let parts: Vec<_> = path
                .split('/')
                .filter(|p| !p.is_empty() && *p != ".")
                .collect();
            return Ok(parts.join("/"));
        }
        if round == MAX_DECODE_ROUNDS {
            return Err(AssetError::InvalidPath(
                "percent encoding exceeds decoding limit",
            ));
        }
        path = decode_percent(&path)?;
    }
    unreachable!("decoding loop always returns at its final iteration")
}

fn validate_path_round(path: &str) -> Result<(), AssetError> {
    if path.chars().any(|c| c == '\\' || c.is_control()) {
        return Err(AssetError::InvalidPath(
            "backslashes and controls are forbidden",
        ));
    }
    if path.contains(['?', '#', ':']) {
        return Err(AssetError::InvalidPath(
            "URI delimiters and drive prefixes are forbidden",
        ));
    }
    if path.split('/').any(|part| part == "..") {
        return Err(AssetError::InvalidPath(
            "parent-directory traversal is forbidden",
        ));
    }
    Ok(())
}

fn decode_percent(path: &str) -> Result<String, AssetError> {
    let input = path.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] != b'%' {
            output.push(input[index]);
            index += 1;
            continue;
        }
        let high = input.get(index + 1).copied().and_then(hex_digit);
        let low = input.get(index + 2).copied().and_then(hex_digit);
        let (Some(high), Some(low)) = (high, low) else {
            return Err(AssetError::InvalidPath("malformed percent escape"));
        };
        output.push(high * 16 + low);
        index += 3;
    }
    String::from_utf8(output).map_err(|_| AssetError::InvalidPath("path is not valid UTF-8"))
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn validate_mime(mime: &str) -> Result<(), AssetError> {
    if mime.is_empty() || !mime.is_ascii() || mime.bytes().any(|b| b.is_ascii_control()) {
        return Err(AssetError::InvalidMime);
    }
    let media_type = mime.split(';').next().unwrap_or("").trim();
    let Some((major, minor)) = media_type.split_once('/') else {
        return Err(AssetError::InvalidMime);
    };
    let token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
    };
    if !token(major) || !token(minor) {
        return Err(AssetError::InvalidMime);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> AssetStore {
        let mut store = AssetStore::new("/index.html").unwrap();
        store
            .insert(
                "index.html",
                b"<main>Orion</main>".to_vec(),
                "text/html; charset=utf-8",
            )
            .unwrap();
        store
            .insert("styles/app.css", b"body{}".to_vec(), "text/css")
            .unwrap();
        store
            .insert("images/star.png", vec![0, 255, 42], "image/png")
            .unwrap();
        store
    }

    #[test]
    fn entry_css_and_binary_bytes_preserve_mime() {
        let store = store();
        let entry = store.resolve("/").unwrap();
        assert_eq!(entry.served_path, "index.html");
        assert_eq!(entry.asset.mime(), "text/html; charset=utf-8");
        assert!(entry.is_fallback);
        assert_eq!(
            store.resolve("/styles/app.css?v=1").unwrap().asset.bytes(),
            b"body{}"
        );
        let image = store.resolve("images/star.png").unwrap();
        assert_eq!(image.asset.bytes(), &[0, 255, 42]);
        assert_eq!(image.asset.mime(), "image/png");
        assert!(!image.is_fallback);
    }

    #[test]
    fn fallback_only_for_extensionless_missing_final_component() {
        let mut store = store();
        for route in [
            "/settings",
            "/profile/42/",
            "/v1.0/dashboard",
            "/settings?tab=2#item",
        ] {
            assert!(store.resolve(route).unwrap().is_fallback, "{route}");
        }
        for asset in [
            "/missing.js",
            "/missing.css",
            "/photo.png",
            "/favicon.ico",
            "/.env",
            "/a.",
        ] {
            assert!(
                matches!(store.resolve(asset), Err(AssetError::MissingAsset(_))),
                "{asset}"
            );
        }
        store
            .insert("health", b"ok".to_vec(), "text/plain")
            .unwrap();
        let exact = store.resolve("/health").unwrap();
        assert!(!exact.is_fallback);
        assert_eq!(exact.asset.bytes(), b"ok");
    }

    #[test]
    fn canonicalizes_slashes_dot_segments_and_unicode() {
        assert_eq!(
            normalize_asset_path("//assets/./icon.svg").unwrap(),
            "assets/icon.svg"
        );
        assert_eq!(normalize_asset_path("/caf%C3%A9/🌟").unwrap(), "café/🌟");
        assert_eq!(
            normalize_asset_path("/%2573tyles%252fapp.css").unwrap(),
            "styles/app.css"
        );
    }

    #[test]
    fn rejects_plain_encoded_mixed_and_multiencoded_traversal() {
        for path in [
            "../secret",
            "/../secret",
            "/safe/../../secret",
            "/%2e%2e/secret",
            "/.%2E/secret",
            "/%2e./secret",
            "/%252e%252e/secret",
            "/%25252e%25252e%25252fsecret",
            "/safe%2f..%2fsecret",
            "/safe/%252e.%252fsecret",
            "/..%2fsecret",
            "/safe/..",
        ] {
            assert!(
                matches!(normalize_asset_path(path), Err(AssetError::InvalidPath(_))),
                "{path}"
            );
            assert!(store().resolve(path).is_err(), "{path}");
        }
    }

    #[test]
    fn rejects_backslashes_controls_invalid_utf8_and_uri_confusion() {
        for path in [
            "/safe\\secret",
            "/%5csecret",
            "/%255Csecret",
            "/%00file",
            "/%2500file",
            "/a\0b",
            "/a\nb",
            "/%0afile",
            "/%7ffile",
            "/%C0%AE%C0%AE/secret",
            "/%FF",
            "/%",
            "/%2",
            "/%GG",
            "/C:/secret",
            "https://example.test/a",
            "/%3fsecret",
            "/%23secret",
            "/%253asecret",
        ] {
            assert!(normalize_asset_path(path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn bounded_decode_fails_closed() {
        let mut path = "%61".to_string();
        for _ in 0..7 {
            path = path.replace('%', "%25");
        }
        assert_eq!(normalize_asset_path(&path).unwrap(), "a");
        path = path.replace('%', "%25");
        assert!(matches!(
            normalize_asset_path(&path),
            Err(AssetError::InvalidPath(_))
        ));
    }

    #[test]
    fn failed_inserts_are_atomic_and_normalized_keys_replace() {
        let mut store = store();
        let old_count = store.len();
        assert!(store.insert("../secret", vec![], "text/plain").is_err());
        assert!(
            store
                .insert("styles/app.css", vec![], "text/css\r\nInjected: yes")
                .is_err()
        );
        assert!(store.insert("new", vec![], "not a mime").is_err());
        assert_eq!(store.len(), old_count);
        assert_eq!(
            store.resolve("styles/app.css").unwrap().asset.bytes(),
            b"body{}"
        );
        let old = store
            .insert("/styles/./app.css", b"new".to_vec(), "text/css")
            .unwrap()
            .unwrap();
        assert_eq!(old.bytes(), b"body{}");
        assert_eq!(store.len(), old_count);
        assert_eq!(
            store.resolve("styles/app.css").unwrap().asset.bytes(),
            b"new"
        );
    }

    #[test]
    fn missing_entry_and_empty_asset_keys_are_explicit_errors() {
        assert!(AssetStore::new("/").is_err());
        let mut store = AssetStore::new("index.html").unwrap();
        assert!(store.is_empty());
        assert!(store.insert("/", vec![], "text/plain").is_err());
        assert_eq!(
            store.resolve("/route"),
            Err(AssetError::MissingEntry("index.html".into()))
        );
        assert_eq!(
            store.resolve("/missing.png"),
            Err(AssetError::MissingAsset("missing.png".into()))
        );
    }
}
