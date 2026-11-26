// SPDX-License-Identifier: Apache-2.0
//! Catalog v2 (`catalog.json`, one per Armory source) and feed v1 (one per app and channel).

use crate::manifest::{valid_id, ContractRange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Catalog {
    pub schema: u32,
    #[serde(default)]
    pub apps: Vec<CatalogApp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogApp {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// minisign public key; pinned at first install.
    #[serde(rename = "publicKey")]
    pub public_key: String,
    pub contract: ContractRange,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// channel name -> feed url (https, or a file for tests and the Sandbox).
    pub channels: BTreeMap<String, String>,
    /// AND-2, optional: the platforms (`windows-x86_64`, `android-aarch64`, ...) the app's feeds carry a
    /// package for, so a device can tell what it cannot install without reading every feed.
    /// Absent = not said; the feed decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platforms: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Feed {
    pub version: String,
    #[serde(default)]
    pub notes: Option<String>,
    pub contract: u32,
    #[serde(default = "one", rename = "stateSchema")]
    pub state_schema: u32,
    pub platforms: BTreeMap<String, FeedPlatform>,
}
fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeedPlatform {
    pub url: String,
    pub signature: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    NotJson(String),
    UnsupportedSchema(u32),
    DuplicateId(String),
    BadId(String),
    NoKey(String),
    NoChannel(String),
    InsecureUrl { id: String, url: String },
    /// The entry says it is another kind of thing (`kind` is set and is not `app`).
    NotAnApp { id: String, kind: String },
}
impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use CatalogError::*;
        match self {
            NotJson(e) => write!(f, "catalog is not valid: {e}"),
            UnsupportedSchema(s) => write!(f, "catalog schema {s} is not supported"),
            DuplicateId(i) => write!(f, "app id '{i}' appears twice"),
            BadId(i) => write!(f, "app id '{i}' is not allowed"),
            NoKey(i) => write!(f, "app '{i}' has no public key"),
            NoChannel(i) => write!(f, "app '{i}' has no channel"),
            InsecureUrl { id, url } => write!(f, "app '{id}' channel url '{url}' is not https, file or loopback http"),
            NotAnApp { id, kind } => write!(f, "'{id}' is not an app (its kind is '{kind}'); the Armory lists apps only, so it is skipped"),
        }
    }
}
impl std::error::Error for CatalogError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    NotJson(String),
    BadVersion(String),
    NoPlatforms,
    BadSha256 { platform: String },
    InsecureUrl { platform: String, url: String },
    EmptySignature { platform: String },
}
impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use FeedError::*;
        match self {
            NotJson(e) => write!(f, "feed is not valid: {e}"),
            BadVersion(v) => write!(f, "feed version '{v}' is not major.minor.patch"),
            NoPlatforms => write!(f, "feed lists no platform"),
            BadSha256 { platform } => write!(f, "sha256 for {platform} is not 64 hex characters"),
            InsecureUrl { platform, url } => write!(f, "package url for {platform} '{url}' is not https, file or loopback http"),
            EmptySignature { platform } => write!(f, "signature for {platform} is empty"),
        }
    }
}
impl std::error::Error for FeedError {}

fn url_ok(u: &str) -> bool {
    // A loopback http url is well formed here; the Armory fetches it only with its developer setting (AND-2).
    u.starts_with("https://") || u.starts_with("file:") || is_loopback_http(u)
}

impl Catalog {
    /// Parse and check one catalog; a broken entry fails the whole file. The Armory uses
    /// [`Catalog::parse_lenient`] so one bad entry does not hide the rest.
    pub fn parse(json: &str) -> Result<Catalog, CatalogError> {
        let c: Catalog = serde_json::from_str(json).map_err(|e| CatalogError::NotJson(e.to_string()))?;
        if c.schema != 2 {
            return Err(CatalogError::UnsupportedSchema(c.schema));
        }
        let mut seen = std::collections::BTreeSet::new();
        for a in &c.apps {
            a.check()?;
            if !seen.insert(a.id.clone()) {
                return Err(CatalogError::DuplicateId(a.id.clone()));
            }
        }
        Ok(c)
    }

    /// Entries that pass, and the reason for each that does not (id or `?`, reason).
    #[allow(clippy::type_complexity)]
    pub fn parse_lenient(json: &str) -> Result<(Vec<CatalogApp>, Vec<(String, CatalogError)>), CatalogError> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| CatalogError::NotJson(e.to_string()))?;
        let schema = v.get("schema").and_then(|s| s.as_u64()).unwrap_or(0) as u32;
        if schema != 2 {
            return Err(CatalogError::UnsupportedSchema(schema));
        }
        let (mut ok, mut bad) = (Vec::new(), Vec::new());
        let mut seen = std::collections::BTreeSet::new();
        for raw in v.get("apps").and_then(|a| a.as_array()).cloned().unwrap_or_default() {
            let label = raw.get("id").and_then(|i| i.as_str()).unwrap_or("?").to_string();
            if let Some(kind) = raw.get("kind").and_then(|k| k.as_str()).filter(|k| *k != "app") {
                bad.push((label.clone(), CatalogError::NotAnApp { id: label, kind: kind.to_string() }));
                continue;
            }
            match serde_json::from_value::<CatalogApp>(raw) {
                Err(e) => bad.push((label, CatalogError::NotJson(e.to_string()))),
                Ok(a) => match a.check() {
                    Err(e) => bad.push((label, e)),
                    Ok(()) if !seen.insert(a.id.clone()) => bad.push((label, CatalogError::DuplicateId(a.id))),
                    Ok(()) => ok.push(a),
                },
            }
        }
        Ok((ok, bad))
    }
}

impl CatalogApp {
    pub fn check(&self) -> Result<(), CatalogError> {
        if !valid_id(&self.id) {
            return Err(CatalogError::BadId(self.id.clone()));
        }
        if self.public_key.trim().is_empty() {
            return Err(CatalogError::NoKey(self.id.clone()));
        }
        if self.channels.is_empty() {
            return Err(CatalogError::NoChannel(self.id.clone()));
        }
        for u in self.channels.values() {
            if !url_ok(u) {
                return Err(CatalogError::InsecureUrl { id: self.id.clone(), url: u.clone() });
            }
        }
        Ok(())
    }
}

impl Feed {
    pub fn parse(json: &str) -> Result<Feed, FeedError> {
        let f: Feed = serde_json::from_str(json).map_err(|e| FeedError::NotJson(e.to_string()))?;
        if crate::version::Version::parse(&f.version).is_none() {
            return Err(FeedError::BadVersion(f.version));
        }
        if f.platforms.is_empty() {
            return Err(FeedError::NoPlatforms);
        }
        for (name, p) in &f.platforms {
            if p.sha256.len() != 64 || !p.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(FeedError::BadSha256 { platform: name.clone() });
            }
            if !url_ok(&p.url) {
                return Err(FeedError::InsecureUrl { platform: name.clone(), url: p.url.clone() });
            }
            if p.signature.trim().is_empty() {
                return Err(FeedError::EmptySignature { platform: name.clone() });
            }
        }
        Ok(f)
    }
}

/// `http://127.0.0.1:8000/x`, `http://localhost/x`, `http://[::1]:9/x`: an http url to this device itself.
/// Anything with user info (a name and an at sign before the host) or another host is not.
pub fn is_loopback_http(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else { return false };
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    if auth.contains('@') {
        return false;
    }
    let host = if let Some(v6) = auth.strip_prefix('[') {
        match v6.split_once(']') {
            Some((h, port)) if port.is_empty() || port.starts_with(':') => h,
            _ => return false,
        }
    } else {
        auth.split(':').next().unwrap_or("")
    };
    matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "::1" | "localhost")
}
