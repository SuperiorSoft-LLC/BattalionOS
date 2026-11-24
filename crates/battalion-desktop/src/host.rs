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
}

impl Host {
    pub fn new(paths: Paths) -> Host {
        let sup = Supervisor::new(paths.clone());
        let armory = std::sync::Arc::new(Armory::new(paths.clone(), sup.clone(), env!("CARGO_PKG_VERSION")));
        Host { paths, sup, armory, servers: Mutex::new(HashMap::new()) }
    }
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

    let srv = AppServer::start(vdir, ui.connect.clone()).map_err(|e| e.to_string())?;
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
