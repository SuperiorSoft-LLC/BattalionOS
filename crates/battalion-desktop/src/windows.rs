// SPDX-License-Identifier: MPL-2.0
//! OS-WINDOW-1 (WM-1): the window model. One `WindowManager` actor (a tokio task behind an mpsc) owns the window table;
//! callers hold a cheap `Windows` handle. No Tauri here: the native step (WM-6) sits behind a trait later, and the frame
//! commands (WM-2) only send to this actor.
//!
//! Persistence: `(app, key) -> {kind, bounds, state, monitor?}` in `<root>/state/windows/<app>.json`. Writes are
//! debounced (`windows.saveDebounceMs` in os.json, the setting `bos.windows.saveDebounceMs`, default 500) and run on
//! `spawn_blocking`. Removing the app deletes the file; an update keeps it. On restore the bounds are clamped to the
//! surface the caller gives (its view, the work area or the monitor).

use battalion_app_api::geometry::{clamp_box, Rect, Snap, Surface};
use battalion_host::paths::Paths;
use battalion_host::store;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Inline,
    Float,
    Native,
}

/// `"normal" | "minimized" | "maximized" | {"snap": "left"|"right"|"tl"|"tr"|"bl"|"br"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WinState {
    Normal,
    Minimized,
    Maximized,
    Snap(Snap),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Badge {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// One row of the window table (design 2.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// `w-<app>-<n>`: given by the OS, scoped to its app.
    pub wid: String,
    pub app: String,
    pub key: String,
    pub kind: Kind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<Badge>,
    #[serde(default)]
    pub attention: bool,
    pub state: WinState,
    pub bounds: Rect,
    /// OS-owned stack order within the surface (higher is on top).
    pub z: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
}

/// What an app asks for when it opens a window. The app id comes from the caller's identity, never from here.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSpec {
    pub key: String,
    pub kind: Option<Kind>,
    #[serde(default)]
    pub title: String,
    pub icon: Option<String>,
    pub route: Option<String>,
    /// Used when nothing is saved for `(app, key)`.
    pub bounds: Option<Rect>,
    /// Used when nothing is saved for `(app, key)` (WM-2: `os_win_open`'s `state?`).
    pub state: Option<WinState>,
    pub monitor: Option<String>,
}

/// A change to an open window. Absent fields stay as they are.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub title: Option<String>,
    pub icon: Option<String>,
    pub badge: Option<Option<Badge>>,
    pub attention: Option<bool>,
    pub state: Option<WinState>,
    pub bounds: Option<Rect>,
    pub monitor: Option<String>,
}

/// What is kept per `(app, key)` across runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub kind: Kind,
    pub bounds: Rect,
    pub state: WinState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WinError {
    /// The wid names another app's window (or is not a wid at all). Refused by name.
    NotYours(String),
    NoSuchWindow(String),
    BadApp(String),
    /// The actor is gone (the OS is shutting down).
    Stopped,
}

impl std::fmt::Display for WinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WinError::NotYours(w) => write!(f, "window {w} belongs to another app"),
            WinError::NoSuchWindow(w) => write!(f, "no window {w}"),
            WinError::BadApp(a) => write!(f, "not a valid app id: {a}"),
            WinError::Stopped => write!(f, "the window manager has stopped"),
        }
    }
}
impl std::error::Error for WinError {}

/// WM-3: `bos.taskbar.groupWindows` (os.json `{"taskbar": {"groupWindows": false}}`, default true): an app's windows group under its
/// taskbar button; false gives one button per window.
pub fn taskbar_group_windows(paths: &Paths) -> bool {
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Taskbar {
        group_windows: Option<bool>,
    }
    #[derive(Deserialize, Default)]
    struct Os {
        #[serde(default)]
        taskbar: Taskbar,
    }
    match store::read_json::<Os>(&paths.os_json()) {
        Ok(Some(o)) => o.taskbar.group_windows.unwrap_or(true),
        _ => true,
    }
}

/// The window settings (os.json `windows` section).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowSettings {
    /// `bos.windows.saveDebounceMs`: how long geometry changes gather before one write.
    pub save_debounce_ms: u64,
}

impl Default for WindowSettings {
    fn default() -> Self {
        WindowSettings { save_debounce_ms: 500 }
    }
}

impl WindowSettings {
    pub fn load(paths: &Paths) -> WindowSettings {
        #[derive(Deserialize, Default)]
        struct Os {
            #[serde(default)]
            windows: WindowSettings,
        }
        match store::read_json::<Os>(&paths.os_json()) {
            Ok(Some(o)) => o.windows,
            _ => WindowSettings::default(),
        }
    }
}

/// `<root>/state/windows/<app>.json`.
pub fn windows_file(paths: &Paths, app: &str) -> Result<PathBuf, WinError> {
    if !battalion_app_api::manifest::valid_id(app) {
        return Err(WinError::BadApp(app.to_string()));
    }
    Ok(paths.root().join("state").join("windows").join(format!("{app}.json")))
}

/// The app a wid belongs to: `w-<app>-<n>` (app ids are `[a-z0-9-]`, so the number is after the last dash).
pub fn wid_app(wid: &str) -> Option<&str> {
    let rest = wid.strip_prefix("w-")?;
    let (app, n) = rest.rsplit_once('-')?;
    (!app.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then_some(app)
}

/// The access rule: an app may only name its own wids (`access::authorize_wid`, here as a `WinError`).
pub fn authorize_wid(app: &str, wid: &str) -> Result<(), WinError> {
    crate::access::authorize_wid(app, wid).map_err(|_| WinError::NotYours(wid.to_string()))
}

type Reply<T> = oneshot::Sender<T>;

enum Cmd {
    Open { app: String, spec: OpenSpec, surface: Surface, reply: Reply<Result<(Window, bool), WinError>> },
    Update { app: String, wid: String, patch: Patch, reply: Reply<Result<Window, WinError>> },
    Focus { app: String, wid: String, reply: Reply<Result<Window, WinError>> },
    Close { app: String, wid: String, reply: Reply<Result<(), WinError>> },
    List { app: Option<String>, reply: Reply<Vec<Window>> },
    AppRemoved { app: String, reply: Reply<()> },
    Flush { reply: Reply<()> },
}

/// A handle to the window manager. Clone it freely.
#[derive(Clone)]
pub struct Windows {
    tx: mpsc::Sender<Cmd>,
}

/// The mpsc depth between callers and the actor (a caller waits when it is full; nothing is dropped).
const QUEUE: usize = 256;

impl Windows {
    /// The handle and the actor's future; the caller spawns the future on its tokio runtime.
    pub fn start(paths: Paths, settings: WindowSettings) -> (Windows, impl Future<Output = ()> + Send + 'static) {
        let (tx, rx) = mpsc::channel(QUEUE);
        let wm = WindowManager { paths: Some(paths), debounce: Duration::from_millis(settings.save_debounce_ms), ..Default::default() };
        (Windows { tx }, wm.run(rx))
    }

    async fn ask<T>(&self, f: impl FnOnce(Reply<T>) -> Cmd) -> Result<T, WinError> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(f(reply)).await.map_err(|_| WinError::Stopped)?;
        rx.await.map_err(|_| WinError::Stopped)
    }

    /// Open (or, for an existing `(app, key)`, focus) a window. Saved geometry wins over `spec.bounds`, clamped to `surface`.
    pub async fn open(&self, app: &str, spec: OpenSpec, surface: Surface) -> Result<Window, WinError> {
        self.open_restored(app, spec, surface).await.map(|(w, _)| w)
    }
    /// `open`, and whether saved state for `(app, key)` was used (`restored`; false for a focus of an open window).
    pub async fn open_restored(&self, app: &str, spec: OpenSpec, surface: Surface) -> Result<(Window, bool), WinError> {
        self.ask(|reply| Cmd::Open { app: app.into(), spec, surface, reply }).await?
    }
    pub async fn update(&self, app: &str, wid: &str, patch: Patch) -> Result<Window, WinError> {
        self.ask(|reply| Cmd::Update { app: app.into(), wid: wid.into(), patch, reply }).await?
    }
    pub async fn focus(&self, app: &str, wid: &str) -> Result<Window, WinError> {
        self.ask(|reply| Cmd::Focus { app: app.into(), wid: wid.into(), reply }).await?
    }
    pub async fn close(&self, app: &str, wid: &str) -> Result<(), WinError> {
        self.ask(|reply| Cmd::Close { app: app.into(), wid: wid.into(), reply }).await?
    }
    /// One app's windows, or every window (`None`, the shell), in z order.
    pub async fn list(&self, app: Option<&str>) -> Result<Vec<Window>, WinError> {
        self.ask(|reply| Cmd::List { app: app.map(Into::into), reply }).await
    }
    /// The app was removed: its windows go and its file is deleted. (An update does not call this.)
    pub async fn app_removed(&self, app: &str) -> Result<(), WinError> {
        self.ask(|reply| Cmd::AppRemoved { app: app.into(), reply }).await
    }
    /// Write every pending change now (shutdown, tests).
    pub async fn flush(&self) -> Result<(), WinError> {
        self.ask(|reply| Cmd::Flush { reply }).await
    }
}

#[derive(Default)]
struct WindowManager {
    paths: Option<Paths>,
    debounce: Duration,
    table: BTreeMap<String, Window>,
    next_n: HashMap<String, u64>,
    z: u64,
    /// Saved geometry per app, loaded on first use.
    saved: HashMap<String, BTreeMap<String, Saved>>,
    dirty: HashSet<String>,
    due: Option<Instant>,
}

// `Paths` has no Default; the Option keeps `..Default::default()` usable above.
impl WindowManager {
    fn paths(&self) -> &Paths {
        self.paths.as_ref().expect("paths")
    }

    async fn run(mut self, mut rx: mpsc::Receiver<Cmd>) {
        loop {
            let cmd = match self.due {
                Some(due) => tokio::select! {
                    c = rx.recv() => c,
                    _ = tokio::time::sleep_until(due) => { self.write_dirty().await; continue; }
                },
                None => rx.recv().await,
            };
            let Some(cmd) = cmd else { break };
            self.handle(cmd).await;
        }
        self.write_dirty().await; // every handle dropped: keep what was pending
    }

    async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Open { app, spec, surface, reply } => {
                let r = self.open(app, spec, surface).await;
                let _ = reply.send(r);
            }
            Cmd::Update { app, wid, patch, reply } => {
                let r = self.update(&app, &wid, patch);
                let _ = reply.send(r);
            }
            Cmd::Focus { app, wid, reply } => {
                let r = self.own(&app, &wid).map(|_| ()).map(|_| {
                    self.z += 1;
                    let w = self.table.get_mut(&wid).expect("owned");
                    w.z = self.z;
                    w.clone()
                });
                let _ = reply.send(r);
            }
            Cmd::Close { app, wid, reply } => {
                let r = self.own(&app, &wid).map(|_| {
                    self.table.remove(&wid);
                });
                let _ = reply.send(r);
            }
            Cmd::List { app, reply } => {
                let mut v: Vec<Window> = self.table.values().filter(|w| app.as_deref().is_none_or(|a| w.app == a)).cloned().collect();
                v.sort_by_key(|w| w.z);
                let _ = reply.send(v);
            }
            Cmd::AppRemoved { app, reply } => {
                self.table.retain(|_, w| w.app != app);
                self.saved.remove(&app);
                self.dirty.remove(&app);
                if let Ok(p) = windows_file(self.paths(), &app) {
                    let _ = tokio::task::spawn_blocking(move || match std::fs::remove_file(&p) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                        _ => Ok(()),
                    })
                    .await;
                }
                let _ = reply.send(());
            }
            Cmd::Flush { reply } => {
                self.write_dirty().await;
                let _ = reply.send(());
            }
        }
    }

    /// The caller's own, existing window.
    fn own(&self, app: &str, wid: &str) -> Result<(), WinError> {
        authorize_wid(app, wid)?;
        match self.table.get(wid) {
            Some(w) if w.app == app => Ok(()),
            Some(_) => Err(WinError::NotYours(wid.into())),
            None => Err(WinError::NoSuchWindow(wid.into())),
        }
    }

    async fn saved_for(&mut self, app: &str) -> Result<&mut BTreeMap<String, Saved>, WinError> {
        if !self.saved.contains_key(app) {
            let p = windows_file(self.paths(), app)?;
            let m = tokio::task::spawn_blocking(move || store::read_json::<BTreeMap<String, Saved>>(&p))
                .await
                .ok()
                .and_then(|r| r.ok().flatten())
                .unwrap_or_default(); // no file yet, or an unreadable one: start empty (it is only geometry)
            self.saved.insert(app.to_string(), m);
        }
        Ok(self.saved.get_mut(app).expect("loaded"))
    }

    async fn open(&mut self, app: String, spec: OpenSpec, surface: Surface) -> Result<(Window, bool), WinError> {
        if !battalion_app_api::manifest::valid_id(&app) {
            return Err(WinError::BadApp(app));
        }
        // Focus instead of a duplicate.
        if let Some(w) = self.table.values_mut().find(|w| w.app == app && w.key == spec.key) {
            self.z += 1;
            w.z = self.z;
            return Ok((w.clone(), false));
        }
        let key = spec.key.clone();
        let saved = self.saved_for(&app).await?.get(&key).cloned();
        let default = Rect { x: 40.0, y: 40.0, w: 480.0, h: 360.0 };
        let restored = saved.is_some();
        let (kind, bounds, state) = match saved {
            Some(s) => (spec.kind.unwrap_or(s.kind), s.bounds, s.state),
            None => (spec.kind.unwrap_or(Kind::Inline), spec.bounds.unwrap_or(default), spec.state.unwrap_or(WinState::Normal)),
        };
        let n = self.next_n.entry(app.clone()).or_insert(0);
        *n += 1;
        self.z += 1;
        let w = Window {
            wid: format!("w-{app}-{n}"),
            app: app.clone(),
            key,
            kind,
            title: spec.title,
            icon: spec.icon,
            badge: None,
            attention: false,
            state,
            bounds: clamp_box(bounds, surface),
            z: self.z,
            route: if kind == Kind::Inline { None } else { spec.route },
        };
        self.table.insert(w.wid.clone(), w.clone());
        self.remember(&w, spec.monitor);
        Ok((w, restored))
    }

    fn update(&mut self, app: &str, wid: &str, p: Patch) -> Result<Window, WinError> {
        self.own(app, wid)?;
        let w = self.table.get_mut(wid).expect("owned");
        let geometry = p.bounds.is_some() || p.state.is_some();
        if let Some(t) = p.title { w.title = t }
        if let Some(i) = p.icon { w.icon = Some(i) }
        if let Some(b) = p.badge { w.badge = b }
        if let Some(a) = p.attention { w.attention = a }
        if let Some(s) = p.state { w.state = s }
        if let Some(b) = p.bounds { w.bounds = b }
        let w = w.clone();
        if geometry || p.monitor.is_some() {
            self.remember(&w, p.monitor);
        }
        Ok(w)
    }

    /// Record `(app, key)` geometry and arm the debounced write. A minimized window keeps its last open state.
    fn remember(&mut self, w: &Window, monitor: Option<String>) {
        let Some(m) = self.saved.get_mut(&w.app) else { return };
        let prev = m.get(&w.key).cloned();
        let state = match (w.state, &prev) {
            (WinState::Minimized, Some(p)) => p.state,
            (WinState::Minimized, None) => WinState::Normal,
            (s, _) => s,
        };
        let s = Saved { kind: w.kind, bounds: w.bounds, state, monitor: monitor.or(prev.and_then(|p| p.monitor)) };
        if m.get(&w.key) != Some(&s) {
            m.insert(w.key.clone(), s);
            self.dirty.insert(w.app.clone());
            if self.due.is_none() {
                self.due = Some(Instant::now() + self.debounce);
            }
        }
    }

    async fn write_dirty(&mut self) {
        self.due = None;
        for app in std::mem::take(&mut self.dirty) {
            let (Ok(p), Some(m)) = (windows_file(self.paths(), &app), self.saved.get(&app).cloned()) else { continue };
            // Awaited: the actor stays ordered (a later delete never races this write), but the main thread is never involved.
            let _ = tokio::task::spawn_blocking(move || {
                if let Some(d) = p.parent() {
                    std::fs::create_dir_all(d)?;
                }
                store::write_json(&p, &m)
            })
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, Paths) {
        let d = tempfile::tempdir().unwrap();
        let p = Paths::new(d.path());
        (d, p)
    }
    fn start(p: &Paths, ms: u64) -> Windows {
        let (w, fut) = Windows::start(p.clone(), WindowSettings { save_debounce_ms: ms });
        tokio::spawn(fut);
        w
    }
    fn spec(key: &str) -> OpenSpec {
        OpenSpec { key: key.into(), title: "Chat".into(), bounds: Some(Rect { x: 600.0, y: 400.0, w: 800.0, h: 600.0 }), ..Default::default() }
    }
    const BIG: Surface = Surface { x: 0.0, y: 0.0, w: 2000.0, h: 1200.0 };

    #[tokio::test]
    async fn persistence_round_trips_through_the_app_file() {
        let (_d, p) = home();
        let wm = start(&p, 10_000);
        let w = wm.open("infantry", spec("session.e7"), BIG).await.unwrap();
        assert_eq!(w.wid, "w-infantry-1");
        let r = Rect { x: 100.0, y: 120.0, w: 700.0, h: 500.0 };
        wm.update("infantry", &w.wid, Patch { bounds: Some(r), state: Some(WinState::Snap(Snap::Left)), monitor: Some("DISPLAY2".into()), ..Default::default() }).await.unwrap();
        let file = windows_file(&p, "infantry").unwrap();
        assert!(!file.exists(), "debounced: nothing written yet");
        wm.flush().await.unwrap();
        let on_disk: BTreeMap<String, Saved> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(on_disk["session.e7"], Saved { kind: Kind::Inline, bounds: r, state: WinState::Snap(Snap::Left), monitor: Some("DISPLAY2".into()) });

        // A fresh OS run restores the geometry.
        let wm2 = start(&p, 10_000);
        let w2 = wm2.open("infantry", OpenSpec { key: "session.e7".into(), ..Default::default() }, BIG).await.unwrap();
        assert_eq!((w2.bounds, w2.state), (r, WinState::Snap(Snap::Left)));
    }

    #[tokio::test]
    async fn the_debounced_write_happens_by_itself() {
        let (_d, p) = home();
        let wm = start(&p, 20);
        wm.open("infantry", spec("k"), BIG).await.unwrap();
        let file = windows_file(&p, "infantry").unwrap();
        for _ in 0..200 {
            if file.exists() { return; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no write after the debounce");
    }

    #[tokio::test]
    async fn restore_clamps_onto_a_smaller_surface() {
        let (_d, p) = home();
        let wm = start(&p, 10_000);
        let w = wm.open("infantry", spec("k"), BIG).await.unwrap();
        wm.update("infantry", &w.wid, Patch { bounds: Some(Rect { x: 1700.0, y: 1000.0, w: 1200.0, h: 900.0 }), ..Default::default() }).await.unwrap();
        wm.flush().await.unwrap();
        let small = Surface::sized(800.0, 600.0);
        let b = start(&p, 10_000).open("infantry", OpenSpec { key: "k".into(), ..Default::default() }, small).await.unwrap().bounds;
        assert!(b.w <= 800.0 && b.h <= 600.0 && b.x <= 800.0 - 80.0 && b.y <= 600.0 - 36.0, "{b:?}");
    }

    #[tokio::test]
    async fn same_key_focuses_instead_of_a_duplicate() {
        let (_d, p) = home();
        let wm = start(&p, 10_000);
        let a = wm.open("infantry", spec("k"), BIG).await.unwrap();
        wm.open("infantry", spec("other"), BIG).await.unwrap();
        let again = wm.open("infantry", spec("k"), BIG).await.unwrap();
        assert_eq!(a.wid, again.wid);
        assert!(again.z > a.z);
        assert_eq!(wm.list(Some("infantry")).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_wid_that_names_another_app_is_refused() {
        let (_d, p) = home();
        let wm = start(&p, 10_000);
        let theirs = wm.open("other-app", spec("k"), BIG).await.unwrap();
        for r in [
            wm.update("infantry", &theirs.wid, Patch::default()).await.map(|_| ()),
            wm.focus("infantry", &theirs.wid).await.map(|_| ()),
            wm.close("infantry", &theirs.wid).await,
        ] {
            assert_eq!(r, Err(WinError::NotYours(theirs.wid.clone())));
        }
        assert_eq!(wm.close("infantry", "not-a-wid").await, Err(WinError::NotYours("not-a-wid".into())));
        assert_eq!(wm.list(None).await.unwrap().len(), 1, "their window is untouched");
        assert_eq!(wid_app("w-my-app-12"), Some("my-app"));
        assert!(authorize_wid("my", "w-my-app-12").is_err());
    }

    #[tokio::test]
    async fn removing_the_app_deletes_its_file_an_update_keeps_it() {
        let (_d, p) = home();
        let wm = start(&p, 10_000);
        wm.open("infantry", spec("k"), BIG).await.unwrap();
        wm.open("keeper", spec("k"), BIG).await.unwrap();
        wm.flush().await.unwrap();
        let (gone, kept) = (windows_file(&p, "infantry").unwrap(), windows_file(&p, "keeper").unwrap());
        assert!(gone.exists() && kept.exists());
        wm.app_removed("infantry").await.unwrap();
        wm.flush().await.unwrap();
        assert!(!gone.exists() && kept.exists());
        assert!(wm.list(Some("infantry")).await.unwrap().is_empty());
    }

    #[test]
    fn settings_come_from_os_json() {
        let (_d, p) = home();
        assert_eq!(WindowSettings::load(&p).save_debounce_ms, 500);
        std::fs::write(p.os_json(), r#"{"windows":{"saveDebounceMs":42}}"#).unwrap();
        assert_eq!(WindowSettings::load(&p).save_debounce_ms, 42);
    }

    #[test]
    fn taskbar_groups_windows_unless_os_json_says_false() {
        let (_d, p) = home();
        assert!(taskbar_group_windows(&p));
        std::fs::write(p.os_json(), r#"{"taskbar":{"groupWindows":false}}"#).unwrap();
        assert!(!taskbar_group_windows(&p));
        std::fs::write(p.os_json(), r#"{"windows":{"saveDebounceMs":42}}"#).unwrap();
        assert!(taskbar_group_windows(&p));
    }
}
