// SPDX-License-Identifier: MPL-2.0
//! OPEN-WITH-1: "open with" between apps. An app declares the files it opens (`fileTypes` in app.json); any app's page may
//! ask the OS to open a file under the OS root (`os_open_with {path, appId?}`). The OS picks the app (the one named, the
//! remembered default for the type, or the person's choice in a chooser), opens it and hands it the file.
//!
//! The receiver gets the path (`os_open_file_take`, and an `os_open_file` event while its frame is open) and, when it lacks
//! `files.read`, a ONE-FILE grant: `os_fs_read` and `os_fs_stat` of exactly that path, nothing else, for as long as the
//! OS runs and the receiver's frame stays open. Least privilege: an editor that is only ever handed files needs no
//! permission over the whole root; writing back still needs `files.write`.
//!
//! The type of a file is its extension (lower case); its MIME type is looked up from the extension in a small table, so
//! `mime` entries of `fileTypes` match too. Remembered defaults live in `<home>/state/open-with.json`
//! (`{"schema": 1, "defaults": {"md": "notes"}}`), keyed by extension. Knows no app by name.

use crate::fs::{check_rel, Fs, FsError, READ, STAT};
use crate::paths::Paths;
use crate::store;
use battalion_app_api::manifest::FileType;
use battalion_app_api::Manifest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

/// The page calls (docs/commands.md, "Open with (OPEN-WITH-1)").
pub const OPEN_WITH: &str = "os_open_with";
pub const OPEN_FILE_TAKE: &str = "os_open_file_take";
pub const COMMANDS: &[&str] = &[OPEN_WITH, OPEN_FILE_TAKE];

/// The lower-case extension of the last name of `rel` (`"notes/a.MD"` -> `"md"`), or `None` (no dot, a leading dot only).
pub fn extension_of(rel: &str) -> Option<String> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// The MIME type of an extension, for the common kinds; `None` for the rest (then only `extensions` can match).
pub fn mime_of(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "txt" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => return None,
    })
}

/// Does `t` cover a file with extension `ext`? By extension, or by its MIME type (`text/plain`, or `text/*`).
pub fn type_matches(t: &FileType, ext: &str) -> bool {
    if t.extensions.iter().any(|e| e == ext) {
        return true;
    }
    let Some(mime) = mime_of(ext) else { return false };
    let major = mime.split('/').next().unwrap_or("");
    t.mime.iter().any(|m| m == mime || m.strip_suffix("/*").is_some_and(|mj| mj == major))
}

/// One app the chooser offers: its id, name and the label of the first of its types that matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub label: String,
}

/// The installed apps (with a page) that declare a type covering `ext`, in the order given (the caller sorts by name).
pub fn candidates<'a>(apps: impl IntoIterator<Item = &'a Manifest>, ext: &str) -> Vec<Candidate> {
    apps.into_iter()
        .filter(|m| m.ui.is_some())
        .filter_map(|m| m.file_types.iter().find(|t| type_matches(t, ext)).map(|t| Candidate { id: m.id.clone(), name: m.name.clone(), label: t.label.clone() }))
        .collect()
}

/// A refused open. `code()` is the stable name a client maps to a typed error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    BadArgs(String),
    /// The named app is not installed, has no page, or does not open this type.
    NoSuchApp(String),
    /// No installed app opens this type.
    NoApp(String),
    /// The path broke a file rule, left the OS root, or is missing (the `fs` code: `leaves_root`, `not_found`...).
    Fs(FsError),
    Io(String),
}

impl OpenError {
    pub fn code(&self) -> &'static str {
        match self {
            OpenError::BadArgs(_) => "bad_args",
            OpenError::NoSuchApp(_) => "no_such_app",
            OpenError::NoApp(_) => "no_app",
            OpenError::Fs(e) => e.code(),
            OpenError::Io(_) => "io",
        }
    }
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::BadArgs(r) | OpenError::Io(r) => write!(f, "{r}"),
            OpenError::NoSuchApp(id) => write!(f, "app '{id}' does not open this kind of file"),
            OpenError::Fs(e) => write!(f, "{e}"),
            OpenError::NoApp(ext) => write!(f, "no installed app opens '.{ext}' files"),
        }
    }
}
impl std::error::Error for OpenError {}

/// What the OS does with one `os_open_with`: open this app now, or ask the person to choose among these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Open(String),
    Choose(Vec<Candidate>),
}

/// Pick the app for a file of extension `ext`: `app_id` when given (it must be a candidate), else the remembered default
/// (while it is still a candidate), else the chooser. The chooser is shown even for one candidate: the person decides
/// which app receives the file, never the OS on its own.
pub fn decide(cands: &[Candidate], ext: &str, app_id: Option<&str>, default: Option<&str>) -> Result<Decision, OpenError> {
    if let Some(id) = app_id {
        return if cands.iter().any(|c| c.id == id) { Ok(Decision::Open(id.to_string())) } else { Err(OpenError::NoSuchApp(id.to_string())) };
    }
    if cands.is_empty() {
        return Err(OpenError::NoApp(ext.to_string()));
    }
    match default.filter(|d| cands.iter().any(|c| c.id == *d)) {
        Some(d) => Ok(Decision::Open(d.to_string())),
        None => Ok(Decision::Choose(cands.to_vec())),
    }
}

/// The remembered default app per extension (`<home>/state/open-with.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default = "schema_one")]
    pub schema: u32,
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
}
fn schema_one() -> u32 {
    1
}

impl Defaults {
    pub fn file(paths: &Paths) -> PathBuf {
        paths.root().join("state").join("open-with.json")
    }
    /// The saved defaults; none when the file is missing or broken (a broken file only loses the remembered choices).
    pub fn load(paths: &Paths) -> Defaults {
        store::read_json::<Defaults>(&Self::file(paths)).ok().flatten().unwrap_or(Defaults { schema: 1, defaults: BTreeMap::new() })
    }
    pub fn get(&self, ext: &str) -> Option<&str> {
        self.defaults.get(ext).map(String::as_str)
    }
    /// Remember `app` for `ext` (`None` forgets it), written atomically.
    pub fn set(paths: &Paths, ext: &str, app: Option<&str>) -> Result<Defaults, OpenError> {
        if !battalion_app_api::manifest::valid_extension(ext) {
            return Err(OpenError::BadArgs(format!("'{ext}' is not an extension")));
        }
        let mut d = Self::load(paths);
        d.schema = 1;
        match app {
            Some(a) if battalion_app_api::manifest::valid_id(a) => {
                d.defaults.insert(ext.to_string(), a.to_string());
            }
            Some(a) => return Err(OpenError::BadArgs(format!("'{a}' is not an app id"))),
            None => {
                d.defaults.remove(ext);
            }
        }
        let f = Self::file(paths);
        if let Some(dir) = f.parent() {
            std::fs::create_dir_all(dir).map_err(|e| OpenError::Io(format!("cannot save the default app ({:?})", e.kind())))?;
        }
        store::write_json(&f, &d).map_err(|e| OpenError::Io(format!("cannot save the default app ({:?})", e.kind())))?;
        Ok(d)
    }
    pub fn to_json(&self) -> Value {
        json!(self.defaults)
    }
}

/// The files handed to apps in this OS session: the paths each app has yet to take, and its one-file grants.
#[derive(Debug, Default)]
pub struct Opens {
    pending: HashMap<String, Vec<String>>,
    grants: HashMap<String, BTreeSet<String>>,
}

impl Opens {
    /// Hand `rel` to `app`: queue it for `os_open_file_take`; and grant that one file when the app lacks `files.read`.
    pub fn deliver(&mut self, app: &str, rel: &str, has_files_read: bool) {
        let q = self.pending.entry(app.to_string()).or_default();
        if !q.iter().any(|p| p == rel) {
            q.push(rel.to_string());
        }
        if !has_files_read {
            self.grants.entry(app.to_string()).or_default().insert(rel.to_string());
        }
    }
    /// The paths handed to `app` since it last asked, oldest first; then none until the next open.
    pub fn take(&mut self, app: &str) -> Vec<String> {
        self.pending.remove(app).unwrap_or_default()
    }
    /// May `app` make file call `cmd` with `args` on a grant? Only `os_fs_read` / `os_fs_stat` of exactly a granted path.
    pub fn grant_allows(&self, app: &str, cmd: &str, args: &Value) -> bool {
        if cmd != READ && cmd != STAT {
            return false;
        }
        let Some(path) = args.get("path").and_then(Value::as_str) else { return false };
        self.grants.get(app).is_some_and(|g| g.contains(path))
    }
    /// The app's frame closed: its grants end (pending paths stay, for when it opens again).
    pub fn close_app(&mut self, app: &str) {
        self.grants.remove(app);
    }
}

/// The file of an `os_open_with`: `args.path` must be a relative path that resolves (links followed) to a FILE under the OS
/// root (`battalion-host::fs`, the one place paths are checked). Returns the path and its extension.
pub fn file_to_open(fs: &Fs, args: &Value) -> Result<(String, String), OpenError> {
    let p = args.get("path").and_then(Value::as_str).ok_or_else(|| OpenError::BadArgs("os_open_with needs a path".into()))?;
    let segs = check_rel(p, fs.settings.max_path_chars).map_err(OpenError::Fs)?;
    if segs.is_empty() {
        return Err(OpenError::Fs(FsError::NotAFile));
    }
    let st = fs.stat(p).map_err(OpenError::Fs)?;
    if st["target"] != "file" {
        return Err(OpenError::Fs(FsError::NotAFile));
    }
    let ext = extension_of(p).ok_or_else(|| OpenError::BadArgs("the file has no extension, so no app is known to open it".into()))?;
    Ok((p.to_string(), ext))
}
