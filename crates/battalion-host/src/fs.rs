// SPDX-License-Identifier: MPL-2.0
//! FS-API-1: the OS file API, read side. BattalionOS has ONE root folder (the `fs.root` setting, by default `BattalionOS`
//! in the user's home folder); an app with `files.read` may list, stat, read and watch anything under it, nothing else.
//!
//! Every path an app passes is relative to the root, `/`-separated, and resolved here, the one place. The lexical rules
//! refuse absolute paths, drives, `\`, `.`/`..` segments, empty segments, control characters, device names (`CON`,
//! `NUL`, `COM1`...), trailing dots or spaces and over-long paths. Then the file is OPENED and the opened handle's final
//! path (links and junctions followed, `GetFinalPathNameByHandleW` on Windows) must still be under the root, so a link
//! swapped in between a check and the use cannot win (TOCTOU). The OS's own home is refused even when it lies under the
//! root. Every limit is an `os.json` setting (`fs.*`) with a default. Knows no app by name.

use crate::paths::Paths;
use crate::store;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, Metadata};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

/// The page-facing command names (docs/commands.md, "Added by FS-API-1").
pub const LIST: &str = "os_fs_list";
pub const STAT: &str = "os_fs_stat";
pub const READ: &str = "os_fs_read";
pub const WATCH: &str = "os_fs_watch";
pub const UNWATCH: &str = "os_fs_unwatch";
pub const COMMANDS: &[&str] = &[LIST, STAT, READ, WATCH, UNWATCH];
/// The manifest permission every read-side call needs.
pub const PERMISSION: &str = "files.read";

/// The folder name of the default root, under the user's home folder.
pub const DEFAULT_ROOT_NAME: &str = "BattalionOS";

/// `os.json` `{"fs": {...}}`. Every field has a default; a missing or broken `os.json` gives the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The OS root folder; empty = `<user home>/BattalionOS`.
    pub root: Option<String>,
    /// The most bytes one `os_fs_read` answers (a larger file is read in chunks with `offset`).
    pub read_max_bytes: u64,
    /// The most entries one `os_fs_list` answers (a larger folder is paged with `cursor`).
    pub list_max: usize,
    /// The most watches one app holds at once.
    pub watch_max: usize,
    /// How often the shell looks at the watched paths, in milliseconds.
    pub watch_poll_ms: u64,
    /// The longest relative path an app may pass, in characters.
    pub max_path_chars: usize,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { root: None, read_max_bytes: 4 * 1024 * 1024, list_max: 1000, watch_max: 32, watch_poll_ms: 1000, max_path_chars: 1024 }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Settings {
        #[derive(Deserialize, Default)]
        struct Os {
            #[serde(default)]
            fs: Settings,
        }
        match store::read_json::<Os>(&paths.os_json()) {
            Ok(Some(o)) => o.fs,
            _ => Settings::default(),
        }
    }

    /// The root this setting names: `fs.root` when set, else `<user home>/BattalionOS`.
    pub fn root_path(&self) -> Option<PathBuf> {
        match self.root.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            Some(r) => Some(PathBuf::from(r)),
            None => default_root(),
        }
    }
}

/// `<user home>/BattalionOS` (`USERPROFILE` on Windows, `HOME` elsewhere).
pub fn default_root() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(DEFAULT_ROOT_NAME))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    /// No root could be found or created.
    NoRoot(String),
    /// The relative path breaks a rule; the reason says which.
    BadPath(String),
    /// The path (after links) is outside the OS root, or is the OS's own home.
    LeavesRoot,
    NotFound,
    NotAFolder,
    NotAFile,
    /// `as: "text"` on bytes that are not UTF-8.
    NotText,
    TooManyWatches(usize),
    NoSuchWatch,
    BadArgs(String),
    Io(String),
}

impl FsError {
    /// A short stable code for a page (`{error, code}`).
    pub fn code(&self) -> &'static str {
        match self {
            FsError::NoRoot(_) => "no_root",
            FsError::BadPath(_) => "bad_path",
            FsError::LeavesRoot => "leaves_root",
            FsError::NotFound => "not_found",
            FsError::NotAFolder => "not_a_folder",
            FsError::NotAFile => "not_a_file",
            FsError::NotText => "not_text",
            FsError::TooManyWatches(_) => "too_many_watches",
            FsError::NoSuchWatch => "no_such_watch",
            FsError::BadArgs(_) => "bad_args",
            FsError::Io(_) => "io",
        }
    }
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::NoRoot(r) => write!(f, "the OS root folder is not available: {r}"),
            FsError::BadPath(r) => write!(f, "path refused: {r}"),
            FsError::LeavesRoot => write!(f, "path leaves the OS root"),
            FsError::NotFound => write!(f, "no such file or folder"),
            FsError::NotAFolder => write!(f, "not a folder"),
            FsError::NotAFile => write!(f, "not a file"),
            FsError::NotText => write!(f, "the file is not UTF-8 text; read it with as: \"base64\""),
            FsError::TooManyWatches(n) => write!(f, "an app holds at most {n} watches (fs.watchMax)"),
            FsError::NoSuchWatch => write!(f, "no such watch"),
            FsError::BadArgs(r) => write!(f, "{r}"),
            FsError::Io(r) => write!(f, "{r}"),
        }
    }
}
impl std::error::Error for FsError {}

fn io_err(e: io::Error) -> FsError {
    match e.kind() {
        io::ErrorKind::NotFound => FsError::NotFound,
        // Never the OS path in the message: only the kind.
        k => FsError::Io(format!("file system error ({k:?})")),
    }
}

const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$"];

/// A Windows device name (`CON`, `nul.txt`, `COM1`, `lpt9.log`...), whatever the case, with or without an extension.
pub fn device_name(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return true;
    }
    let digitish = |s: &str| matches!(s, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³");
    (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() > 3 && digitish(&stem[3..])
}

/// The lexical rules for an app's relative path: the segments, or why it is refused. `""` is the root itself.
pub fn check_rel(path: &str, max_chars: usize) -> Result<Vec<&str>, FsError> {
    if path.chars().count() > max_chars {
        return Err(FsError::BadPath(format!("longer than {max_chars} characters (fs.maxPathChars)")));
    }
    if path.is_empty() {
        return Ok(Vec::new());
    }
    if path.chars().any(|c| c.is_control()) {
        return Err(FsError::BadPath("control characters".into()));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(FsError::BadPath("absolute paths are not allowed; paths are relative to the OS root".into()));
    }
    if path.contains(':') {
        return Err(FsError::BadPath("':' (a drive or a stream) is not allowed".into()));
    }
    if path.contains('\\') {
        return Err(FsError::BadPath("use '/' between names, not '\\'".into()));
    }
    let mut segs = Vec::new();
    for seg in path.split('/') {
        if seg.is_empty() {
            return Err(FsError::BadPath("empty name (a doubled or trailing '/')".into()));
        }
        if seg == "." || seg == ".." {
            return Err(FsError::BadPath(format!("'{seg}' is not allowed")));
        }
        if seg.chars().any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*')) {
            return Err(FsError::BadPath(format!("'{seg}' holds a character Windows does not allow")));
        }
        if seg.ends_with('.') || seg.ends_with(' ') {
            return Err(FsError::BadPath(format!("'{seg}' ends with a dot or a space")));
        }
        if device_name(seg) {
            return Err(FsError::BadPath(format!("'{seg}' is a device name")));
        }
        segs.push(seg);
    }
    Ok(segs)
}

/// Open `p` for reading, a folder too (no share lock that would stop others).
fn open_any(p: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        std::fs::OpenOptions::new().read(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).share_mode(7).open(p)
    }
    #[cfg(not(windows))]
    {
        File::open(p)
    }
}

/// The final path of an OPEN handle, links and junctions followed.
#[cfg(windows)]
fn final_path(f: &File, _p: &Path) -> io::Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
    let mut buf = vec![0u16; 512];
    loop {
        // SAFETY: the handle is open for the life of `f`; the buffer length is passed with it.
        let n = unsafe { GetFinalPathNameByHandleW(f.as_raw_handle() as _, buf.as_mut_ptr(), buf.len() as u32, 0) } as usize;
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n < buf.len() {
            buf.truncate(n);
            return Ok(PathBuf::from(OsString::from_wide(&buf)));
        }
        buf.resize(n + 1, 0);
    }
}

#[cfg(target_os = "linux")]
fn final_path(f: &File, _p: &Path) -> io::Result<PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd()))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn final_path(_f: &File, p: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(p)
}

fn norm(c: Component<'_>) -> String {
    let s = c.as_os_str().to_string_lossy().to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// Is `p` equal to or under `root` (component-wise; case-insensitive on Windows)?
fn under(p: &Path, root: &Path) -> bool {
    let (mut a, mut b) = (p.components(), root.components());
    loop {
        match (b.next(), a.next()) {
            (None, _) => return true,
            (Some(rc), Some(pc)) if norm(rc) == norm(pc) => continue,
            _ => return false,
        }
    }
}

/// A path for a person to read (`stat.display`): no `\\?\` prefix. Never to be fed back as an argument.
pub fn display(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\UNC\").map(|r| format!(r"\\{r}")).or_else(|| s.strip_prefix(r"\\?\").map(str::to_string)).unwrap_or_else(|| s.to_string())
}

fn millis(t: io::Result<std::time::SystemTime>) -> Option<u64> {
    t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64)
}

fn kind_of(m: &Metadata) -> &'static str {
    if m.file_type().is_symlink() {
        "link"
    } else if m.is_dir() {
        "dir"
    } else {
        "file"
    }
}

/// The OS root and the rules. Built once per call from the settings (cheap); `open` creates the root when missing.
#[derive(Debug, Clone)]
pub struct Fs {
    root: PathBuf,
    os_home: Option<PathBuf>,
    pub settings: Settings,
}

/// What `open_checked` hands back: the open handle, its final path, its metadata.
struct Opened {
    file: File,
    real: PathBuf,
    meta: Metadata,
}

impl Fs {
    /// The root from `os.json` (`fs.root`, else the default), created when missing; the OS home is kept out.
    pub fn open(paths: &Paths) -> Result<Fs, FsError> {
        let settings = Settings::load(paths);
        let root = settings.root_path().ok_or_else(|| FsError::NoRoot("no home folder to put it in; set fs.root in os.json".into()))?;
        Fs::with_root(&root, Some(paths.root()), settings)
    }

    /// The root `root` (created when missing). `os_home`, when it lies under the root, stays out of reach.
    pub fn with_root(root: &Path, os_home: Option<&Path>, settings: Settings) -> Result<Fs, FsError> {
        std::fs::create_dir_all(root).map_err(|e| FsError::NoRoot(format!("cannot create it ({:?})", e.kind())))?;
        let f = open_any(root).map_err(|e| FsError::NoRoot(format!("cannot open it ({:?})", e.kind())))?;
        let real = final_path(&f, root).map_err(|e| FsError::NoRoot(format!("cannot resolve it ({:?})", e.kind())))?;
        let os_home = os_home.and_then(|h| std::fs::canonicalize(h).ok());
        Ok(Fs { root: real, os_home, settings })
    }

    /// The root's real path (shown in the OS settings).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `root` joined with the checked segments (not yet opened, so not yet trusted).
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, FsError> {
        let segs = check_rel(rel, self.settings.max_path_chars)?;
        Ok(segs.iter().fold(self.root.clone(), |p, s| p.join(s)))
    }

    fn allowed(&self, real: &Path) -> bool {
        under(real, &self.root) && !self.os_home.as_deref().is_some_and(|h| under(real, h))
    }

    /// Open the path, THEN check where the open handle really is (the TOCTOU-safe check).
    fn open_checked(&self, rel: &str) -> Result<Opened, FsError> {
        let p = self.resolve(rel)?;
        let file = open_any(&p).map_err(io_err)?;
        let real = final_path(&file, &p).map_err(io_err)?;
        if !self.allowed(&real) {
            return Err(FsError::LeavesRoot);
        }
        let meta = file.metadata().map_err(io_err)?;
        Ok(Opened { file, real, meta })
    }

    /// `os_fs_list`: a folder's entries, sorted by name, paged by `cursor` (an opaque string), at most `fs.listMax`.
    pub fn list(&self, rel: &str, cursor: Option<&str>, limit: Option<usize>) -> Result<Value, FsError> {
        let o = self.open_checked(rel)?;
        if !o.meta.is_dir() {
            return Err(FsError::NotAFolder);
        }
        let start: usize = match cursor.filter(|c| !c.is_empty()) {
            Some(c) => c.parse().map_err(|_| FsError::BadArgs("cursor is not one this call gave".into()))?,
            None => 0,
        };
        let limit = limit.unwrap_or(self.settings.list_max).clamp(1, self.settings.list_max.max(1));
        let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(&o.real)
            .map_err(io_err)?
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
            .filter(|(_, p)| !self.os_home.as_deref().is_some_and(|h| under(p, h)))
            .collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        let total = names.len();
        let entries: Vec<Value> = names
            .into_iter()
            .skip(start)
            .take(limit)
            .map(|(name, p)| match std::fs::symlink_metadata(&p) {
                Ok(m) => json!({"name": name, "kind": kind_of(&m), "size": if m.is_dir() { 0 } else { m.len() }, "modified": millis(m.modified())}),
                Err(_) => json!({"name": name, "kind": "file", "size": 0, "modified": null}),
            })
            .collect();
        let next = start + entries.len();
        Ok(json!({"entries": entries, "cursor": if next < total { Value::from(next.to_string()) } else { Value::Null }}))
    }

    /// `os_fs_stat`: one file or folder. A link inside the root is followed (`kind: "link"`, the rest is the target's).
    pub fn stat(&self, rel: &str) -> Result<Value, FsError> {
        let o = self.open_checked(rel)?;
        let p = self.resolve(rel)?;
        let is_link = std::fs::symlink_metadata(&p).map(|m| m.file_type().is_symlink()).unwrap_or(false)
            // A junction is a reparse point that is not a symlink to std on every version: its real path differs.
            || !rel.is_empty() && !under(&o.real, &p) && !under(&p, &o.real);
        let name = rel.rsplit('/').next().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_default();
        Ok(json!({
            "name": name,
            "kind": if is_link { "link" } else if o.meta.is_dir() { "dir" } else { "file" },
            "target": if o.meta.is_dir() { "dir" } else { "file" },
            "size": if o.meta.is_dir() { 0 } else { o.meta.len() },
            "modified": millis(o.meta.modified()),
            "created": millis(o.meta.created()),
            "readonly": o.meta.permissions().readonly(),
            "display": display(&o.real),
        }))
    }

    /// `os_fs_read`: at most `fs.readMaxBytes` from `offset`, as UTF-8 text (cut at a character boundary) or base64.
    pub fn read(&self, rel: &str, offset: u64, length: Option<u64>, as_text: bool) -> Result<Value, FsError> {
        let mut o = self.open_checked(rel)?;
        if o.meta.is_dir() {
            return Err(FsError::NotAFile);
        }
        let size = o.meta.len();
        let want = length.unwrap_or(self.settings.read_max_bytes).min(self.settings.read_max_bytes);
        o.file.seek(SeekFrom::Start(offset.min(size))).map_err(io_err)?;
        let mut buf = Vec::with_capacity(want.min(size.saturating_sub(offset)) as usize);
        (&mut o.file).take(want).read_to_end(&mut buf).map_err(io_err)?;
        let data = if as_text {
            match std::str::from_utf8(&buf) {
                Ok(_) => {}
                // A character cut by the chunk end: answer up to it; the next read starts there.
                Err(e) if e.error_len().is_none() && offset + (buf.len() as u64) < size && e.valid_up_to() > 0 => buf.truncate(e.valid_up_to()),
                Err(_) => return Err(FsError::NotText),
            }
            Value::from(String::from_utf8(buf.clone()).map_err(|_| FsError::NotText)?)
        } else {
            use base64::Engine;
            Value::from(base64::engine::general_purpose::STANDARD.encode(&buf))
        };
        let next = offset.min(size) + buf.len() as u64;
        Ok(json!({"data": data, "size": size, "offset": offset.min(size), "length": buf.len(), "eof": next >= size}))
    }

    /// A first look at a watched path (`os_fs_watch`): checked like any call.
    pub fn watch_start(&self, rel: &str) -> Result<Watch, FsError> {
        self.open_checked(rel)?;
        let snap = self.snapshot(rel)?;
        Ok(Watch { rel: rel.to_string(), snap })
    }

    /// What changed under a watch since its last look: `[{path, change: "created"|"changed"|"removed"}]`.
    /// A watched path that now leaves the root (a link swapped in) reads as removed.
    pub fn watch_poll(&self, w: &mut Watch) -> Vec<Value> {
        let now = self.snapshot(&w.rel).unwrap_or_default();
        let mut out = Vec::new();
        for (k, v) in &now {
            match w.snap.get(k) {
                None => out.push(json!({"path": k, "change": "created"})),
                Some(old) if old != v => out.push(json!({"path": k, "change": "changed"})),
                _ => {}
            }
        }
        for k in w.snap.keys().filter(|k| !now.contains_key(*k)) {
            out.push(json!({"path": k, "change": "removed"}));
        }
        w.snap = now;
        out
    }

    /// The watched path itself and, for a folder, its direct entries: relative path -> (kind, size, modified).
    fn snapshot(&self, rel: &str) -> Result<BTreeMap<String, (String, u64, Option<u64>)>, FsError> {
        let o = self.open_checked(rel)?;
        let mut m = BTreeMap::new();
        m.insert(rel.to_string(), (kind_of(&o.meta).to_string(), o.meta.len(), millis(o.meta.modified())));
        if o.meta.is_dir() {
            for e in std::fs::read_dir(&o.real).map_err(io_err)?.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let key = if rel.is_empty() { name } else { format!("{rel}/{name}") };
                if let Ok(md) = std::fs::symlink_metadata(e.path()) {
                    m.insert(key, (kind_of(&md).to_string(), if md.is_dir() { 0 } else { md.len() }, millis(md.modified())));
                }
            }
        }
        Ok(m)
    }

    /// One page call (`os_fs_list` / `os_fs_stat` / `os_fs_read`) with the page's `args`. Watches go through [`Watches`].
    pub fn call(&self, cmd: &str, args: &Value) -> Result<Value, FsError> {
        let path = match args.get("path") {
            None | Some(Value::Null) => "",
            Some(Value::String(s)) => s.as_str(),
            Some(_) => return Err(FsError::BadArgs("path must be a string".into())),
        };
        let num = |k: &str| -> Result<Option<u64>, FsError> {
            match args.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v.as_u64().map(Some).ok_or_else(|| FsError::BadArgs(format!("{k} must be a whole number"))),
            }
        };
        match cmd {
            LIST => self.list(path, args.get("cursor").and_then(Value::as_str), num("limit")?.map(|n| n as usize)),
            STAT => self.stat(path),
            READ => {
                let as_text = match args.get("as").and_then(Value::as_str) {
                    None | Some("text") => true,
                    Some("base64") => false,
                    Some(o) => return Err(FsError::BadArgs(format!("as must be \"text\" or \"base64\", not '{o}'"))),
                };
                self.read(path, num("offset")?.unwrap_or(0), num("length")?, as_text)
            }
            other => Err(FsError::BadArgs(format!("'{other}' is not a file call"))),
        }
    }
}

/// One watched path and what it looked like last time.
#[derive(Debug, Clone)]
pub struct Watch {
    pub rel: String,
    snap: BTreeMap<String, (String, u64, Option<u64>)>,
}

/// Every app's watches, at most `fs.watchMax` per app. The app id is the frame's, never an argument.
#[derive(Debug, Default)]
pub struct Watches {
    next: u64,
    by_app: HashMap<String, BTreeMap<String, Watch>>,
}

impl Watches {
    /// Start a watch for app `app` on `rel`: its id (`w-<n>`).
    pub fn add(&mut self, fs: &Fs, app: &str, rel: &str) -> Result<String, FsError> {
        let max = fs.settings.watch_max;
        if self.by_app.get(app).map_or(0, BTreeMap::len) >= max {
            return Err(FsError::TooManyWatches(max));
        }
        let w = fs.watch_start(rel)?;
        self.next += 1;
        let id = format!("w-{}", self.next);
        self.by_app.entry(app.to_string()).or_default().insert(id.clone(), w);
        Ok(id)
    }

    /// Stop app `app`'s watch `id` (another app's id is "no such watch").
    pub fn remove(&mut self, app: &str, id: &str) -> Result<(), FsError> {
        self.by_app.get_mut(app).and_then(|m| m.remove(id)).map(|_| ()).ok_or(FsError::NoSuchWatch)
    }

    /// Drop every watch of app `app` (its frame closed).
    pub fn clear_app(&mut self, app: &str) {
        self.by_app.remove(app);
    }

    pub fn is_empty(&self) -> bool {
        self.by_app.values().all(BTreeMap::is_empty)
    }

    /// Look at every watch: `(app, watch id, path, change)` for each change, for that app's frame only.
    pub fn poll(&mut self, fs: &Fs) -> Vec<(String, String, Value)> {
        let mut out = Vec::new();
        for (app, ws) in self.by_app.iter_mut() {
            for (id, w) in ws.iter_mut() {
                for c in fs.watch_poll(w) {
                    out.push((app.clone(), id.clone(), c));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_whatever_the_case_or_extension() {
        for d in ["CON", "con", "Nul.txt", "aux.tar.gz", "COM1", "lpt9.log", "PRN ", "conin$", "COM²"] {
            assert!(device_name(d), "{d}");
        }
        for ok in ["console", "nullish.txt", "COM", "COM10", "lpt", "auxiliary", "a.con"] {
            assert!(!device_name(ok), "{ok}");
        }
    }

    #[test]
    fn under_is_by_component() {
        assert!(under(Path::new("/a/b/c"), Path::new("/a/b")));
        assert!(under(Path::new("/a/b"), Path::new("/a/b")));
        assert!(!under(Path::new("/a/bc"), Path::new("/a/b")));
        assert!(!under(Path::new("/a"), Path::new("/a/b")));
    }

    #[test]
    fn display_drops_the_verbatim_prefix() {
        assert_eq!(display(Path::new(r"\\?\C:\x\y")), r"C:\x\y");
        assert_eq!(display(Path::new(r"\\?\UNC\srv\share\a")), r"\\srv\share\a");
    }
}
