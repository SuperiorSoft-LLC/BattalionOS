// SPDX-License-Identifier: MPL-2.0
//! Host state and the per-app window.

use crate::access::{app_label, APP_PREFIX};
use crate::{events, serve::AppServer};
use battalion_app_api::{Manifest, CONTRACT};
use battalion_host::armory::Armory;
use battalion_host::supervisor::platform_key;
use battalion_host::{Paths, Supervisor};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub struct Host {
    pub paths: Paths,
    pub sup: Supervisor,
    pub armory: std::sync::Arc<Armory>,
    /// One loopback origin per open app window.
    pub servers: Mutex<HashMap<String, AppServer>>,
    /// The page server of each app shown inside the OS window: on the app's one kept port, replaced when the app is updated.
    pub pages: std::sync::Arc<Mutex<HashMap<String, Page>>>,
    /// The badge each app has set on itself (`os_badge`).
    pub badges: crate::badge::Badges,
    /// The app a start from outside asked for (`--open <id>`), until the shell takes it (BOS-19).
    pub open_request: Mutex<Option<String>>,
    /// BOS-30: the newer BattalionOS the last check found, and whether an install runs.
    pub os_update: Mutex<crate::osupdate::OsUpdateState>,
    /// The first-run plan carried in the build, for a host with no `first-run.json` beside its executable (Android).
    /// `None` = read the file beside the executable (the Windows installer lays it down when its checkbox is checked).
    pub first_run_plan: Option<&'static str>,
    /// BOS-DEFAULT-INF: the apps the first run just installed, until the shell takes them (`os_take_first_run_notice`) to
    /// name them once. Held here because the run may finish before the shell listens to `os://first-run`.
    pub first_run_notice: Mutex<Vec<String>>,
    /// OS-WINDOW-1 (WM-1): the window manager actor's handle (the window table and its persistence).
    pub windows: crate::windows::Windows,
}

impl Host {
    pub fn new(paths: Paths) -> Host {
        let sup = Supervisor::new(paths.clone());
        let armory = std::sync::Arc::new(Armory::new(paths.clone(), sup.clone(), env!("CARGO_PKG_VERSION")));
        let (windows, wm) = crate::windows::Windows::start(paths.clone(), crate::windows::WindowSettings::load(&paths));
        tauri::async_runtime::spawn(wm);
        Host { windows, paths, sup, armory, servers: Mutex::new(HashMap::new()), pages: Default::default(), badges: Default::default(), open_request: Mutex::new(None), os_update: Default::default(), first_run_plan: None, first_run_notice: Mutex::new(Vec::new()) }
    }

    /// A host whose first run installs the apps of this plan text (`first-run.json`, compiled in).
    pub fn with_first_run_plan(mut self, plan: &'static str) -> Host {
        self.first_run_plan = Some(plan);
        self
    }
}

/// What the shell needs to draw an app's page in a frame.
pub struct Page {
    pub version: String,
    pub origin: String,
    pub url: String,
    /// `None` when the page comes from a dev app's own dev server (BOS-31), or when the slot serves it (AND-3b, `files`).
    _server: Option<AppServer>,
    files: Option<PageFiles>,
}

/// AND-3b: where the page's files are, for a host that serves them itself by request interception (Android: the app's slot answers
/// `http://<id>.bos.localhost/...` from these, no port). The same rules as the page server: only files under `root`, a missing one
/// retried under `page_dir`, every answer with `csp`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageFiles {
    pub root: String,
    /// The entry page's folder, relative to `root` (`""` = none).
    pub page_dir: String,
    pub csp: String,
}

/// AND-3b: pages are served by request interception, not by a loopback page server: on Android any other app can reach a loopback
/// port. The desktop keeps its page server.
pub const PAGES_BY_INTERCEPTION: bool = cfg!(target_os = "android");

/// The origin of app `id`'s page when its host serves it by interception: one host name per app, so each app has its own origin.
/// `http` and not `https`: a `.localhost` origin is a secure context either way, and from `https` the page could not reach a daemon
/// at a plain `ws://` address on the network (mixed content), which it can on the desktop (`http://127.0.0.1:<port>`).
pub fn intercept_origin(id: &str) -> String {
    format!("http://{id}.bos.localhost")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageInfo {
    pub id: String,
    pub version: String,
    /// `http://127.0.0.1:<kept port>`: the origin the frame has, and the only one its app lets in.
    pub origin: String,
    /// The page's entry url on that origin.
    pub url: String,
    /// Set when the host serves the page by interception ([`PAGES_BY_INTERCEPTION`]).
    pub files: Option<PageFiles>,
}

/// The page of app `id` for a frame in the OS window. Starts the page server on the app's one kept port, or reuses it; after an
/// update (another version folder) the server is replaced on the same port, so the origin never changes. Needs no app process:
/// the frame shows what the app says while it is down.
pub fn page_of(paths: &Paths, sup: &Supervisor, pages: &Mutex<HashMap<String, Page>>, id: &str) -> Result<PageInfo, String> {
    page_of_on(paths, sup, pages, id, PAGES_BY_INTERCEPTION)
}

/// [`page_of`], with the way the page is served given (`intercept`: the host serves the files itself, no server is started).
pub fn page_of_on(paths: &Paths, sup: &Supervisor, pages: &Mutex<HashMap<String, Page>>, id: &str, intercept: bool) -> Result<PageInfo, String> {
    let cur = battalion_host::Current::load(&paths.current_json(id).map_err(|e| e.0)?)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("app '{id}' is not installed"))?;
    let mut pages = pages.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(p) = pages.get(id) {
        if p.version == cur.version {
            return Ok(PageInfo { id: id.into(), version: p.version.clone(), origin: p.origin.clone(), url: p.url.clone(), files: p.files.clone() });
        }
    }
    pages.remove(id); // drops the old server: its port is free for the new one
    let vdir = paths.content_dir(id, &cur.version).map_err(|e| e.0)?;
    let text = std::fs::read_to_string(vdir.join("app.json")).map_err(|e| e.to_string())?;
    let m = Manifest::parse(&text, &platform_key(), CONTRACT).map_err(|e| format!("manifest: {e:?}"))?;
    let ui = m.ui.as_ref().ok_or_else(|| format!("app '{id}' has no page"))?;
    let entry = ui.entry.trim_start_matches('/').to_string();
    let page_dir = std::path::Path::new(&entry).parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf());
    // BOS-31: a dev app with a dev server url shows that page (its own hot reload works); the same frame, origin check and
    // permissions as an installed app. Only loopback (checked again here).
    if let Some(u) = battalion_host::dev::load(paths, id).and_then(|d| d.dev_url) {
        let origin = battalion_host::dev::dev_url_origin(&u)?;
        pages.insert(id.to_string(), Page { version: cur.version.clone(), origin: origin.clone(), url: u.clone(), _server: None, files: None });
        return Ok(PageInfo { id: id.into(), version: cur.version, origin, url: u, files: None });
    }
    if intercept {
        // No app process on this host, so no app url in the policy (the page's own origin and the manifest's `connect`).
        let origin = intercept_origin(id);
        let url = format!("{origin}/{entry}");
        let files = PageFiles {
            root: vdir.to_string_lossy().into_owned(),
            page_dir: page_dir.as_ref().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default(),
            csp: crate::serve::csp_for(&ui.connect, None),
        };
        pages.insert(id.to_string(), Page { version: cur.version.clone(), origin: origin.clone(), url: url.clone(), _server: None, files: Some(files.clone()) });
        return Ok(PageInfo { id: id.into(), version: cur.version, origin, url, files: Some(files) });
    }
    let port = battalion_host::uiport::port_for(paths, id).map_err(|e| e.to_string())?;
    let app_url = app_url_of(sup, id);
    let srv = AppServer::start_on(vdir, page_dir, ui.connect.clone(), port, Some(app_url)).map_err(|e| format!("the page of '{id}' could not be served on port {port}: {e}"))?;
    let origin = srv.origin();
    let url = format!("{origin}/{entry}");
    pages.insert(id.to_string(), Page { version: cur.version.clone(), origin: origin.clone(), url: url.clone(), _server: Some(srv), files: None });
    Ok(PageInfo { id: id.into(), version: cur.version, origin, url, files: None })
}

/// Is app `id` installed with a page? (`os_open_app_window` checks it before the shell opens a view.)
pub fn page_exists(paths: &Paths, id: &str) -> Result<(), String> {
    let cur = battalion_host::Current::load(&paths.current_json(id).map_err(|e| e.0)?)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("app '{id}' is not installed"))?;
    let vdir = paths.content_dir(id, &cur.version).map_err(|e| e.0)?;
    let text = std::fs::read_to_string(vdir.join("app.json")).map_err(|e| e.to_string())?;
    let m = Manifest::parse(&text, &platform_key(), CONTRACT).map_err(|e| format!("manifest: {e:?}"))?;
    m.ui.as_ref().map(|_| ()).ok_or_else(|| format!("app '{id}' has no page"))
}

/// The permissions the installed manifest of app `id` declares (BOS-25: the native calls of a frame check them). Read at every call,
/// so an update that changes them is followed.
pub fn permissions_of(paths: &Paths, id: &str) -> Result<Vec<String>, String> {
    let cur = battalion_host::Current::load(&paths.current_json(id).map_err(|e| e.0)?)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("app '{id}' is not installed"))?;
    let vdir = paths.content_dir(id, &cur.version).map_err(|e| e.0)?;
    let text = std::fs::read_to_string(vdir.join("app.json")).map_err(|e| e.to_string())?;
    let m = Manifest::parse(&text, &platform_key(), CONTRACT).map_err(|e| format!("manifest: {e:?}"))?;
    Ok(m.permissions.clone())
}

/// The app's own url while it runs, asked at every request of the page server: an app that restarts listens on a new port, and the
/// page it serves must be allowed to load from where the app is now (a read of the app's small status record, no probe).
fn app_url_of(sup: &Supervisor, id: &str) -> crate::serve::AppUrl {
    let sup = sup.clone();
    let id = id.to_string();
    std::sync::Arc::new(move || sup.status(&id).ok().and_then(|s| s.url))
}

/// Open (or focus) the window of app `id`: label `app-<id>`, its own data_directory, its own origin.
pub fn open_app_window(app: &AppHandle, host: &Host, id: &str) -> Result<(), String> {
    let label = app_label(id).map_err(|e| e.to_string())?;
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.show();
        return w.set_focus().map_err(|e| e.to_string());
    }
    let cur = battalion_host::Current::load(&host.paths.current_json(id).map_err(|e| e.0)?)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("app '{id}' is not installed"))?;
    let vdir = host.paths.content_dir(id, &cur.version).map_err(|e| e.0)?;
    let text = std::fs::read_to_string(vdir.join("app.json")).map_err(|e| e.to_string())?;
    let m = Manifest::parse(&text, &platform_key(), CONTRACT).map_err(|e| format!("manifest: {e:?}"))?;
    let ui = m.ui.as_ref().ok_or_else(|| format!("app '{id}' has no window"))?;

    let page_dir = std::path::Path::new(ui.entry.trim_start_matches('/')).parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf());
    let srv = AppServer::start_with_page_dir(vdir, page_dir, ui.connect.clone()).map_err(|e| e.to_string())?;
    let url = format!("{}/{}", srv.origin(), ui.entry.trim_start_matches('/'));
    let data = host.paths.webview_dir(id).map_err(|e| e.0)?;
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let (w, h) = ui.window.as_ref().map(|w| (w.width as f64, w.height as f64)).unwrap_or((1100.0, 720.0));
    let parsed = url.parse().map_err(|e| format!("{e}"))?;
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed))
        .title(id)
        .inner_size(w, h)
        .data_directory(data)
        .build()
        .map_err(|e| e.to_string())?;
    host.servers.lock().unwrap_or_else(|p| p.into_inner()).insert(id.to_string(), srv);

    // App exit = the window closes; the process keeps running (the supervisor owns it).
    let (handle, app_id) = (app.clone(), id.to_string());
    win.on_window_event(move |ev| {
        if matches!(ev, WindowEvent::Destroyed) {
            if let Some(h) = handle.try_state::<Host>() {
                h.servers.lock().unwrap_or_else(|p| p.into_inner()).remove(&app_id);
            }
            let _ = handle.emit(events::APP_WINDOW_CLOSED, &app_id);
        }
    });
    Ok(())
}

pub fn app_id_of_label(label: &str) -> Option<&str> {
    label.strip_prefix(APP_PREFIX)
}

/// First run: install the apps the installer chose (`first-run.json` beside the executable), once.
/// Blocking; call it from a thread. Emits `os://first-run` and, per app, `os://app-state`.
pub fn run_first_run(app: &AppHandle, host: &Host) -> battalion_host::firstrun::Outcome {
    use battalion_host::firstrun::{self as fr, Event, Outcome};
    let emit = |p: serde_json::Value| {
        let _ = app.emit(events::FIRST_RUN, p);
    };
    let mut on_event = |ev| match ev {
        Event::Start(ids) => emit(serde_json::json!({"status": "start", "ids": ids})),
        Event::AppInstalled(id) => {
            emit(serde_json::json!({"status": "installed", "id": id}));
            let _ = app.emit(events::APP_STATE, serde_json::json!({"id": id, "state": "stopped"}));
        }
        Event::Finished(o) => emit(match o {
            Outcome::NothingToDo => serde_json::json!({"status": "nothing"}),
            Outcome::AlreadyDone => serde_json::json!({"status": "already-done"}),
            Outcome::Done(ids) => {
                *host.first_run_notice.lock().unwrap_or_else(|p| p.into_inner()) = ids.clone();
                serde_json::json!({"status": "done", "ids": ids})
            }
            Outcome::Offline { id, reason } => serde_json::json!({"status": "offline", "id": id, "error": reason}),
            Outcome::Failed { id, reason } => serde_json::json!({"status": "failed", "id": id, "error": reason}),
        }),
    };
    match host.first_run_plan {
        Some(text) => fr::run_text(&host.armory, text, &mut on_event),
        None => match fr::plan_path_beside_exe() {
            Some(plan) => fr::run(&host.armory, &plan, &mut on_event),
            None => Outcome::NothingToDo,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An installed page-only app (the fixture `page-app`) in a fresh OS home.
    fn installed() -> (tempfile::TempDir, Paths, Supervisor) {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path().join("os"));
        paths.ensure().unwrap();
        let fx = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/page-app");
        let v = paths.content_dir("page-app", "1.0.0").unwrap();
        std::fs::create_dir_all(v.join("ui")).unwrap();
        std::fs::copy(fx.join("app.json"), v.join("app.json")).unwrap();
        std::fs::copy(fx.join("ui/index.html"), v.join("ui/index.html")).unwrap();
        let cur = battalion_host::Current { version: "1.0.0".into(), previous: None, state: battalion_host::AppState::Stopped, public_key: String::new() };
        let cj = paths.current_json("page-app").unwrap();
        std::fs::create_dir_all(cj.parent().unwrap()).unwrap();
        std::fs::write(&cj, serde_json::to_vec(&cur).unwrap()).unwrap();
        let sup = Supervisor::new(paths.clone());
        (d, paths, sup)
    }

    /// AND-3b: served by interception, the page has its own `<id>.bos.localhost` origin and its files, and no server is started.
    #[test]
    fn interception_gives_the_app_its_own_origin_and_no_port() {
        let (_d, paths, sup) = installed();
        let pages = Mutex::new(HashMap::new());
        let p = page_of_on(&paths, &sup, &pages, "page-app", true).unwrap();
        assert_eq!(p.origin, "http://page-app.bos.localhost");
        assert_eq!(p.url, "http://page-app.bos.localhost/ui/index.html");
        let f = p.files.expect("files for the slot");
        assert!(std::path::Path::new(&f.root).join("ui/index.html").is_file());
        assert_eq!(f.page_dir, "ui");
        assert_eq!(f.csp, crate::serve::csp_for(&[], None));
        assert!(pages.lock().unwrap()["page-app"]._server.is_none(), "no page server on this host");
        assert!(!paths.root().join("ui-ports").join("page-app").exists(), "no page port was kept");
        // Asked again, the same page (no server either).
        assert_eq!(page_of_on(&paths, &sup, &pages, "page-app", true).unwrap().origin, p.origin);
    }

    /// The desktop keeps its loopback page server.
    #[test]
    fn the_desktop_keeps_its_page_server() {
        let (_d, paths, sup) = installed();
        let pages = Mutex::new(HashMap::new());
        let p = page_of_on(&paths, &sup, &pages, "page-app", false).unwrap();
        assert!(p.origin.starts_with("http://127.0.0.1:"));
        assert!(p.files.is_none());
        assert!(!PAGES_BY_INTERCEPTION, "this test runs on a desktop");
    }
}
