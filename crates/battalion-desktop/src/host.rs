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
}

impl Host {
    pub fn new(paths: Paths) -> Host {
        let sup = Supervisor::new(paths.clone());
        let armory = std::sync::Arc::new(Armory::new(paths.clone(), sup.clone(), env!("CARGO_PKG_VERSION")));
        Host { paths, sup, armory, servers: Mutex::new(HashMap::new()), pages: Default::default(), badges: Default::default() }
    }
}

/// What the shell needs to draw an app's page in a frame.
pub struct Page {
    pub version: String,
    pub origin: String,
    pub url: String,
    _server: AppServer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageInfo {
    pub id: String,
    pub version: String,
    /// `http://127.0.0.1:<kept port>`: the origin the frame has, and the only one its app lets in.
    pub origin: String,
    /// The page's entry url on that origin.
    pub url: String,
}

/// The page of app `id` for a frame in the OS window. Starts the page server on the app's one kept port, or reuses it; after an
/// update (another version folder) the server is replaced on the same port, so the origin never changes. Needs no app process:
/// the frame shows what the app says while it is down.
pub fn page_of(paths: &Paths, sup: &Supervisor, pages: &Mutex<HashMap<String, Page>>, id: &str) -> Result<PageInfo, String> {
    let cur = battalion_host::Current::load(&paths.current_json(id).map_err(|e| e.0)?)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("app '{id}' is not installed"))?;
    let mut pages = pages.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(p) = pages.get(id) {
        if p.version == cur.version {
            return Ok(PageInfo { id: id.into(), version: p.version.clone(), origin: p.origin.clone(), url: p.url.clone() });
        }
    }
    pages.remove(id); // drops the old server: its port is free for the new one
    let vdir = paths.version_dir(id, &cur.version).map_err(|e| e.0)?;
    let text = std::fs::read_to_string(vdir.join("app.json")).map_err(|e| e.to_string())?;
    let m = Manifest::parse(&text, &platform_key(), CONTRACT).map_err(|e| format!("manifest: {e:?}"))?;
    let ui = m.ui.as_ref().ok_or_else(|| format!("app '{id}' has no page"))?;
    let entry = ui.entry.trim_start_matches('/').to_string();
    let page_dir = std::path::Path::new(&entry).parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf());
    let port = battalion_host::uiport::port_for(paths, id).map_err(|e| e.to_string())?;
    let app_url = app_url_of(sup, id);
    let srv = AppServer::start_on(vdir, page_dir, ui.connect.clone(), port, Some(app_url)).map_err(|e| format!("the page of '{id}' could not be served on port {port}: {e}"))?;
    let origin = srv.origin();
    let url = format!("{origin}/{entry}");
    pages.insert(id.to_string(), Page { version: cur.version.clone(), origin: origin.clone(), url: url.clone(), _server: srv });
    Ok(PageInfo { id: id.into(), version: cur.version, origin, url })
}

/// Is app `id` installed with a page? (`os_open_app_window` checks it before the shell opens a view.)
pub fn page_exists(paths: &Paths, id: &str) -> Result<(), String> {
    let cur = battalion_host::Current::load(&paths.current_json(id).map_err(|e| e.0)?)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("app '{id}' is not installed"))?;
    let vdir = paths.version_dir(id, &cur.version).map_err(|e| e.0)?;
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
    let vdir = paths.version_dir(id, &cur.version).map_err(|e| e.0)?;
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
    let vdir = host.paths.version_dir(id, &cur.version).map_err(|e| e.0)?;
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
    let Some(plan) = fr::plan_path_beside_exe() else { return Outcome::NothingToDo };
    let emit = |p: serde_json::Value| {
        let _ = app.emit(events::FIRST_RUN, p);
    };
    fr::run(&host.armory, &plan, &mut |ev| match ev {
        Event::Start(ids) => emit(serde_json::json!({"status": "start", "ids": ids})),
        Event::AppInstalled(id) => {
            emit(serde_json::json!({"status": "installed", "id": id}));
            let _ = app.emit(events::APP_STATE, serde_json::json!({"id": id, "state": "stopped"}));
        }
        Event::Finished(o) => emit(match o {
            Outcome::NothingToDo => serde_json::json!({"status": "nothing"}),
            Outcome::AlreadyDone => serde_json::json!({"status": "already-done"}),
            Outcome::Done(ids) => serde_json::json!({"status": "done", "ids": ids}),
            Outcome::Offline { id, reason } => serde_json::json!({"status": "offline", "id": id, "error": reason}),
            Outcome::Failed { id, reason } => serde_json::json!({"status": "failed", "id": id, "error": reason}),
        }),
    })
}
