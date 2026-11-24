// SPDX-License-Identifier: Apache-2.0
//! `app.json`: the manifest at the root of a `.bosapp` package.

use crate::version::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MANIFEST_VERSION: u32 = 1;

/// Unknown fields are ignored on purpose (serde's default): a newer manifest must still load.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub manifest: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub contract: ContractRange,
    #[serde(default, rename = "minOs")]
    pub min_os: Option<String>,
    /// Per platform key (`windows-x86_64`). Empty = no process part (UI-only app).
    #[serde(default)]
    pub process: BTreeMap<String, ProcessSpec>,
    #[serde(default)]
    pub ui: Option<UiSpec>,
    #[serde(default)]
    pub init: Option<InitSpec>,
    #[serde(default)]
    pub state: Option<StateSpec>,
    #[serde(default)]
    pub cli: Vec<CliSpec>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractRange {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessSpec {
    /// The app's library inside the package (`.dll`, `.dylib` or `.so`, by target). BattalionOS loads
    /// it on the app's own worker and calls its `run`; see [`crate::abi`].
    #[serde(default)]
    pub library: String,
    /// Refused: an app is a library, not a program. Read only so the package check can say so.
    #[serde(default, skip_serializing)]
    pub exec: Option<String>,
    /// Name prefixes (`MYAPP_`) of the OS's own environment that the app must not inherit. Variables
    /// starting `BATTALION_` are always removed; the contract values are then set; the rest passes.
    #[serde(default, rename = "envScrub")]
    pub env_scrub: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_ready", rename = "readyTimeoutMs")]
    pub ready_timeout_ms: u64,
    #[serde(default = "default_stop", rename = "stopTimeoutMs")]
    pub stop_timeout_ms: u64,
}
fn default_ready() -> u64 {
    60_000
}
fn default_stop() -> u64 {
    10_000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiSpec {
    pub entry: String,
    #[serde(default)]
    pub window: Option<WindowSpec>,
    #[serde(default)]
    pub connect: Vec<String>,
    /// BOS-27: the app's page draws its own title bar (name, drag area, window buttons). Inside the OS window the OS then draws no
    /// bar of its own over the page once the page says its bar is there (`os_window_bar`). Absent = false.
    #[serde(default, rename = "ownTitleBar")]
    pub own_title_bar: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowSpec {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InitSpec {
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateSpec {
    /// Per OS (`windows`, `linux`, `macos`): a folder the app already owns. Absent = `<os>/data/<id>`.
    #[serde(default)]
    pub dir: BTreeMap<String, String>,
    #[serde(default = "default_schema")]
    pub schema: u32,
}
fn default_schema() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CliSpec {
    pub name: String,
    pub exec: String,
}

/// Why a manifest was refused. One variant per reason, so callers and tests can match on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    NotJson(String),
    UnsupportedManifest(u32),
    BadId(String),
    EmptyName,
    BadVersion(String),
    BadContractRange { min: u32, max: u32 },
    /// The OS speaks a contract outside the app's range.
    ContractNotSupported { os: u32, min: u32, max: u32 },
    BadMinOs(String),
    /// `process.<target>.exec`: an app is a library; BattalionOS never starts a program for it.
    ExecRefused(String),
    /// The library's extension does not fit the target (`.dll` windows, `.dylib` macos, `.so` linux).
    WrongLibraryKind { target: String, library: String },
    /// A path that is absolute, has `..`, a drive, or an empty segment.
    UnsafePath { field: &'static str, path: String },
    EmptyExec(&'static str),
    BadCliName(String),
    UnknownPermission(String),
    /// No process part for this platform and no UI part either: nothing to run or show.
    NothingToRun,
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ManifestError::*;
        match self {
            NotJson(e) => write!(f, "app.json is not valid: {e}"),
            UnsupportedManifest(v) => write!(f, "manifest version {v} is not supported"),
            BadId(i) => write!(f, "app id '{i}' is not allowed (a-z, 0-9, '-', 2 to 63 characters)"),
            EmptyName => write!(f, "app name is empty"),
            BadVersion(v) => write!(f, "version '{v}' is not major.minor.patch"),
            BadContractRange { min, max } => write!(f, "contract range {min}..{max} is empty or starts at 0"),
            ContractNotSupported { os, min, max } => write!(f, "app needs contract {min}..{max}, this OS speaks {os}"),
            BadMinOs(v) => write!(f, "minOs '{v}' is not major.minor.patch"),
            ExecRefused(t) => write!(f, "process.{t}.exec is not allowed: an app ships a library (process.{t}.library), not a program; BattalionOS runs it on its own worker"),
            WrongLibraryKind { target, library } => write!(f, "process.{target}.library '{library}' is not a library for that target (windows .dll, macos .dylib, linux .so)"),
            UnsafePath { field, path } => write!(f, "{field} path '{path}' leaves the package"),
            EmptyExec(w) => write!(f, "{w} has no library (or exec)"),
            BadCliName(n) => write!(f, "cli name '{n}' is not allowed"),
            UnknownPermission(p) => write!(f, "permission '{p}' is not known"),
            NothingToRun => write!(f, "app has neither a process part for this platform nor a UI part"),
        }
    }
}
impl std::error::Error for ManifestError {}

/// Permissions v1 knows. Unknown ones are refused: a typo must not silently grant nothing.
pub const PERMISSIONS: &[&str] = &["process.spawn", "network", "files.user", "window", "login.window"];

pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=63).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// A relative path that stays inside the package.
pub fn safe_rel_path(p: &str) -> bool {
    if p.is_empty() || p.starts_with('/') || p.starts_with('\\') || p.contains(':') || p.contains('\0') {
        return false;
    }
    !p.split(['/', '\\']).any(|seg| seg == ".." || seg.is_empty())
}

impl Manifest {
    /// Parse and validate. `platform` is the host's key, e.g. `windows-x86_64`; `os_contract` is
    /// the contract the OS speaks ([`crate::CONTRACT`]).
    pub fn parse(json: &str, platform: &str, os_contract: u32) -> Result<Manifest, ManifestError> {
        let m: Manifest = serde_json::from_str(json).map_err(|e| ManifestError::NotJson(e.to_string()))?;
        m.validate(platform, os_contract)?;
        Ok(m)
    }

    pub fn validate(&self, platform: &str, os_contract: u32) -> Result<(), ManifestError> {
        use ManifestError::*;
        if self.manifest != MANIFEST_VERSION {
            return Err(UnsupportedManifest(self.manifest));
        }
        if !valid_id(&self.id) {
            return Err(BadId(self.id.clone()));
        }
        if self.name.trim().is_empty() {
            return Err(EmptyName);
        }
        if Version::parse(&self.version).is_none() {
            return Err(BadVersion(self.version.clone()));
        }
        let c = &self.contract;
        if c.min == 0 || c.min > c.max {
            return Err(BadContractRange { min: c.min, max: c.max });
        }
        if os_contract < c.min || os_contract > c.max {
            return Err(ContractNotSupported { os: os_contract, min: c.min, max: c.max });
        }
        if let Some(v) = &self.min_os {
            if Version::parse(v).is_none() {
                return Err(BadMinOs(v.clone()));
            }
        }
        let check = |field: &'static str, p: &str| {
            if safe_rel_path(p) {
                Ok(())
            } else {
                Err(UnsafePath { field, path: p.to_string() })
            }
        };
        if let Some(i) = &self.icon {
            check("icon", i)?;
        }
        for (target, spec) in &self.process {
            if spec.exec.is_some() {
                return Err(ExecRefused(target.clone()));
            }
            if spec.library.trim().is_empty() {
                return Err(EmptyExec("process"));
            }
            check("process.library", &spec.library)?;
            if let Some(bad) = spec.env_scrub.iter().find(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')) {
                return Err(UnsafePath { field: "process.envScrub", path: bad.clone() });
            }
            let ext = if target.starts_with("windows") {
                Some(".dll")
            } else if target.starts_with("macos") {
                Some(".dylib")
            } else if target.starts_with("linux") {
                Some(".so")
            } else {
                None
            };
            if let Some(ext) = ext {
                if !spec.library.to_ascii_lowercase().ends_with(ext) {
                    return Err(WrongLibraryKind { target: target.clone(), library: spec.library.clone() });
                }
            }
        }
        if let Some(ui) = &self.ui {
            check("ui.entry", &ui.entry)?;
        }
        if let Some(init) = &self.init {
            if init.exec.trim().is_empty() {
                return Err(EmptyExec("init"));
            }
            check("init.exec", &init.exec)?;
        }
        for cli in &self.cli {
            if !valid_id(&cli.name) {
                return Err(BadCliName(cli.name.clone()));
            }
            check("cli.exec", &cli.exec)?;
        }
        for p in &self.permissions {
            if !PERMISSIONS.contains(&p.as_str()) {
                return Err(UnknownPermission(p.clone()));
            }
        }
        if !self.process.contains_key(platform) && self.ui.is_none() {
            return Err(NothingToRun);
        }
        Ok(())
    }

    /// The process part for a platform, if the app has one.
    pub fn process_for(&self, platform: &str) -> Option<&ProcessSpec> {
        self.process.get(platform)
    }
}
