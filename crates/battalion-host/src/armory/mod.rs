// SPDX-License-Identifier: MPL-2.0
//! The Armory engine: a catalog of apps, a signed package check, install, update with rollback,
//! and remove. It knows no app by name; everything it installs is data in a catalog.
//!
//! ```text
//! catalog   sources (os.json) -> catalog v2 (lenient) -> offline cache <home>/catalog/
//! package   download -> size + sha256 -> minisign (pinned key) -> unzip (path, size, count) -> app.json
//! install   journaled: download, verify, unpack, place, init  (a crash is undone by `recover`)
//! update    journaled: stop, switch, start; a start that never gets ready is rolled back
//! remove    journaled: stop, delete the versions; the state folder stays unless asked
//! ```
//!
//! **Key format** (one rule for every catalog and feed): `publicKey` is the base64 line of a
//! minisign `.pub` file (a whole `.pub` file is accepted too); `signature` is the text of the
//! package's `.minisig` file (prehashed `ED` signatures only; the legacy `Ed` mode is refused).
//! The signature covers the package bytes. The first install pins the key in `current.json`;
//! later updates are checked against the pinned key, never against a newer catalog key.
//! (The older daemon-side Armory verified a raw ed25519 key over a sha256 digest; that format is
//! not accepted here.)
//!
//! One Armory action runs per app at a time (`apps/<id>/armory.lock`). Every number is a named
//! setting in `os.json` under `"armory"` ([`Settings`]) with a default.

pub mod catalog;
pub mod exit;
pub mod info;
pub mod install;
pub mod osupdate;
pub mod package;
pub mod progress;
pub mod recover;
pub mod remove;
pub mod update;

pub use catalog::{CatalogEntry, Listing};
pub use install::{Installed, InstalledApp};
pub use progress::{Progress, ProgressSink};
pub use recover::Recovered;
pub use remove::RemoveOptions;
pub use update::{Available, UpdateOptions, Updated};

use crate::paths::{BadAppId, Paths};
use crate::supervisor::Supervisor;
use crate::{store, Lock, LockError};
use battalion_app_api::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `os.json` -> `"armory": { ... }`. Unknown keys are ignored; missing keys take the default.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Catalog urls (`https:` or `file:`), first wins on a duplicate id.
    pub sources: Vec<String>,
    pub max_catalog_bytes: u64,
    pub max_feed_bytes: u64,
    /// A package larger than this (or than its feed says) is refused before it is stored.
    pub max_package_bytes: u64,
    /// Total bytes after unzipping.
    pub max_unpacked_bytes: u64,
    pub max_file_bytes: u64,
    pub max_entries: usize,
    /// Versions kept on disk per app (current + previous = 2). Older ones are deleted.
    pub keep_versions: usize,
    pub init_timeout_ms: u64,
    /// Ask a running app whether it is busy before an update; a busy app is not updated.
    pub check_busy: bool,
    pub http_timeout_ms: u64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            sources: Vec::new(),
            max_catalog_bytes: 4 << 20,
            max_feed_bytes: 256 << 10,
            max_package_bytes: 2 << 30,
            max_unpacked_bytes: 8 << 30,
            max_file_bytes: 4 << 30,
            max_entries: 100_000,
            keep_versions: 2,
            init_timeout_ms: 120_000,
            check_busy: true,
            http_timeout_ms: 30_000,
        }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Settings {
        #[derive(Deserialize, Default)]
        struct Os {
            #[serde(default)]
            armory: Settings,
        }
        match store::read_json::<Os>(&paths.os_json()) {
            Ok(Some(o)) => o.armory,
            _ => Settings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArmoryError {
    BadId(String),
    /// No catalog could be read and nothing was cached.
    Offline(String),
    Fetch { url: String, reason: String },
    NotInCatalog(String),
    NoChannel { id: String, channel: String },
    Catalog(String),
    Feed(String),
    NoPlatform(String),
    /// The app's contract range (or the feed's contract) does not include what this OS speaks.
    ContractNotSupported { id: String, os: u32, min: u32, max: u32 },
    OsTooOld { need: String, have: String },
    TooLarge { what: &'static str, limit: u64 },
    SizeMismatch { expected: u64, got: u64 },
    ShaMismatch,
    BadSignature(String),
    /// The catalog now carries another key than the one pinned at the first install.
    KeyChanged,
    ZipPath(String),
    ZipLimit(String),
    Zip(String),
    Manifest(String),
    /// The package root holds another kind of manifest (`module.json`) instead of `app.json`.
    NotAnApp(String),
    IdMismatch { expected: String, got: String },
    VersionMismatch { expected: String, got: String },
    AlreadyInstalled(String),
    NotInstalled(String),
    UpToDate(String),
    /// Another Armory action holds this app.
    Busy,
    /// The app reports work in flight; update later or force.
    AppBusy(u32),
    WrongState(String),
    /// The new version never became ready. `rolled_back`: the old version runs again; otherwise
    /// the app is left failed (the state schema changed, or the old version did not start either).
    UpdateFailed { reason: String, rolled_back: bool },
    SchemaBump { from: u32, to: u32 },
    NoPrevious,
    Init(String),
    Io(String),
}

impl std::fmt::Display for ArmoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ArmoryError::*;
        match self {
            BadId(i) => write!(f, "app id '{i}' is not allowed"),
            Offline(m) => write!(f, "no catalog available (offline, nothing cached): {m}"),
            Fetch { url, reason } => write!(f, "could not fetch {url}: {reason}"),
            NotInCatalog(i) => write!(f, "app '{i}' is not in any catalog"),
            NoChannel { id, channel } => write!(f, "app '{id}' has no channel '{channel}'"),
            Catalog(m) => write!(f, "catalog: {m}"),
            Feed(m) => write!(f, "feed: {m}"),
            NoPlatform(p) => write!(f, "the feed has no package for {p}"),
            ContractNotSupported { id, os, min, max } => write!(f, "app '{id}' needs contract {min}..{max}, this OS speaks {os}"),
            OsTooOld { need, have } => write!(f, "the app needs OS {need} or newer, this is {have}"),
            TooLarge { what, limit } => write!(f, "{what} is larger than the limit of {limit} bytes"),
            SizeMismatch { expected, got } => write!(f, "package is {got} bytes, the feed says {expected}"),
            ShaMismatch => write!(f, "package sha256 does not match the feed"),
            BadSignature(m) => write!(f, "package signature check failed: {m}"),
            KeyChanged => write!(f, "the catalog key differs from the key pinned at install; refused"),
            ZipPath(p) => write!(f, "package entry '{p}' leaves the package"),
            ZipLimit(m) => write!(f, "package is too big or too odd to unpack: {m}"),
            Zip(m) => write!(f, "package is not a valid zip: {m}"),
            Manifest(m) => write!(f, "package manifest: {m}"),
            NotAnApp(m) => write!(f, "this package is not an app: {m}"),
            IdMismatch { expected, got } => write!(f, "package is app '{got}', expected '{expected}'"),
            VersionMismatch { expected, got } => write!(f, "package is version {got}, the feed says {expected}"),
            AlreadyInstalled(i) => write!(f, "app '{i}' is already installed"),
            NotInstalled(i) => write!(f, "app '{i}' is not installed"),
            UpToDate(v) => write!(f, "already at {v}"),
            Busy => write!(f, "the app is busy with another Armory action"),
            AppBusy(n) => write!(f, "the app reports {n} unit(s) of work in flight"),
            WrongState(m) => write!(f, "{m}"),
            UpdateFailed { reason, rolled_back } => write!(f, "update failed ({}): {reason}", if *rolled_back { "rolled back" } else { "not rolled back" }),
            SchemaBump { from, to } => write!(f, "state schema changes {from} -> {to}; the automatic rollback is refused"),
            NoPrevious => write!(f, "there is no previous version to go back to"),
            Init(m) => write!(f, "init failed: {m}"),
            Io(m) => write!(f, "{m}"),
        }
    }
}
impl std::error::Error for ArmoryError {}
impl From<BadAppId> for ArmoryError {
    fn from(e: BadAppId) -> ArmoryError {
        ArmoryError::BadId(e.0)
    }
}
impl From<io::Error> for ArmoryError {
    fn from(e: io::Error) -> ArmoryError {
        ArmoryError::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ArmoryError>;

/// Where bytes come from. The default speaks `https:` and `file:`; tests plug in their own
/// (a cut connection, a server that lies).
pub trait Fetcher: Send + Sync {
    fn open(&self, url: &str) -> io::Result<Box<dyn Read + Send>>;
}

pub struct DefaultFetcher {
    pub timeout_ms: u64,
}

/// `file:///C:/x/y`, `file:///x/y`, `file:C:\x\y` -> a path.
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file:")?;
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let b = rest.as_bytes();
    let rest = if b.len() >= 3 && b[0] == b'/' && b[2] == b':' { &rest[1..] } else { rest };
    Some(PathBuf::from(rest))
}

impl Fetcher for DefaultFetcher {
    fn open(&self, url: &str) -> io::Result<Box<dyn Read + Send>> {
        if let Some(p) = file_url_path(url) {
            return Ok(Box::new(std::fs::File::open(p)?));
        }
        if !url.starts_with("https://") {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "only https and file urls are fetched"));
        }
        let t = std::time::Duration::from_millis(self.timeout_ms);
        let agent = ureq::AgentBuilder::new().timeout_connect(t).timeout_read(t).build();
        let resp = agent.get(url).call().map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Box::new(resp.into_reader()))
    }
}

pub struct Armory {
    pub(crate) paths: Paths,
    pub(crate) sup: Supervisor,
    pub(crate) fetcher: Arc<dyn Fetcher>,
    pub(crate) settings: Settings,
    pub(crate) os_version: Version,
    pub(crate) sink: std::sync::Mutex<Option<ProgressSink>>,
}

impl Armory {
    /// `os_version` is the version of this BattalionOS (`major.minor.patch`), checked against
    /// each app's `minOs`.
    pub fn new(paths: Paths, sup: Supervisor, os_version: &str) -> Armory {
        let settings = Settings::load(&paths);
        let fetcher = Arc::new(DefaultFetcher { timeout_ms: settings.http_timeout_ms });
        Armory::with_parts(paths, sup, os_version, settings, fetcher)
    }

    /// The supervisor that runs this Armory's apps (shared, not a copy).
    pub fn supervisor(&self) -> Supervisor {
        self.sup.clone()
    }
    pub fn with_parts(paths: Paths, sup: Supervisor, os_version: &str, settings: Settings, fetcher: Arc<dyn Fetcher>) -> Armory {
        let os_version = Version::parse(os_version).unwrap_or(Version { parts: [0, 0, 0], pre: None });
        Armory { paths, sup, fetcher, settings, os_version, sink: std::sync::Mutex::new(None) }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// One Armory action per app. Not `app.lock`: the supervisor takes that while it starts.
    pub(crate) fn lock_app(&self, id: &str) -> Result<Lock> {
        let path = self.paths.app_dir(id)?.join("armory.lock");
        match Lock::try_acquire(&path) {
            Ok(l) => Ok(l),
            Err(LockError::Busy) => Err(ArmoryError::Busy),
            Err(LockError::Io(e)) => Err(e.into()),
        }
    }

    /// Journal one step, then (tests only) die here if asked: `BATTALION_KILL_AT=<step>` with the
    /// `crash-points` feature ends the process at once, with no cleanup, like a power cut.
    pub(crate) fn step(&self, j: &crate::journal::Journal, tx: u64, name: &str) -> Result<()> {
        j.step(tx, name)?;
        if let Some((app, op)) = j.who() {
            self.report(Progress::stage(app, op, name));
        }
        self.point(name);
        Ok(())
    }

    #[allow(unused_variables)]
    pub(crate) fn point(&self, name: &str) {
        #[cfg(feature = "crash-points")]
        if std::env::var("BATTALION_KILL_AT").is_ok_and(|v| v == name) {
            std::process::exit(99);
        }
    }

    /// Read a small remote document, capped.
    pub(crate) fn fetch_small(&self, url: &str, max: u64, what: &'static str) -> Result<Vec<u8>> {
        let rd = self.fetcher.open(url).map_err(|e| ArmoryError::Fetch { url: url.into(), reason: e.to_string() })?;
        let mut buf = Vec::new();
        rd.take(max + 1).read_to_end(&mut buf).map_err(|e| ArmoryError::Fetch { url: url.into(), reason: e.to_string() })?;
        if buf.len() as u64 > max {
            return Err(ArmoryError::TooLarge { what, limit: max });
        }
        Ok(buf)
    }
}

/// Per-app sidecar `apps/<id>/armory.json`: what the Armory needs to remember beyond `current.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub channel: String,
    /// State schema of the current version (from its manifest).
    #[serde(rename = "stateSchema")]
    pub state_schema: u32,
    /// State schema of the previous version, when there is one.
    #[serde(default, rename = "previousStateSchema")]
    pub previous_state_schema: Option<u32>,
    /// The app named its own state folder; the OS never deletes it.
    #[serde(default, rename = "customStateDir")]
    pub custom_state_dir: bool,
}

pub(crate) fn meta_path(paths: &Paths, id: &str) -> Result<PathBuf> {
    Ok(paths.app_dir(id)?.join("armory.json"))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Remove a folder tree; a missing folder is fine.
pub(crate) fn remove_tree(p: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
