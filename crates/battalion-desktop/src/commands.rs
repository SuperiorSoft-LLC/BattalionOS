// SPDX-License-Identifier: MPL-2.0
//! The os_* commands. Every one is async (a blocking call goes through spawn_blocking), and every one
//! checks the calling window's label (from the runtime, not from the arguments) before it acts.

use crate::access::{authorize_app, authorize_shell, caller_of, Caller};
use crate::host::{open_app_window, Host};
use battalion_host::armory::{info::catalog_icon, RemoveOptions, UpdateOptions};
use battalion_host::supervisor::AppStatus;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

type R<T> = Result<T, String>;

fn status_json(s: &AppStatus) -> Value {
    json!({"id": s.id, "state": s.state, "pid": s.pid, "url": s.url, "paused": s.paused, "restarts": s.restarts, "lastError": s.last_error})
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn os_window_info(window: WebviewWindow) -> R<Value> {
    let label = window.label().to_string();
    let app = match caller_of(&label).map_err(|e| e.to_string())? {
        Caller::App(id) => Value::String(id),
        _ => Value::Null,
    };
    Ok(json!({"label": label, "app": app}))
}

#[tauri::command]
pub async fn os_list_apps(window: WebviewWindow, host: State<'_, Host>) -> R<Value> {
    authorize_shell(window.label(), "os_list_apps").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let list = blocking(move || a.installed()).await?.map_err(|e| e.to_string())?;
    Ok(json!(list
        .iter()
        .map(|a| json!({"id": a.id, "version": a.version, "state": a.state, "channel": a.channel, "name": a.name, "icon": a.icon, "available": a.available, "previous": a.previous}))
        .collect::<Vec<_>>()))
}

#[tauri::command]
pub async fn os_app_status(window: WebviewWindow, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_app(window.label(), &id).map_err(|e| e.to_string())?;
    let sup = host.sup.clone();
    let s = blocking(move || sup.status(&id)).await?.map_err(|e| format!("{e:?}"))?;
    Ok(status_json(&s))
}

#[tauri::command]
pub async fn os_app_health(window: WebviewWindow, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_app(window.label(), &id).map_err(|e| e.to_string())?;
    let sup = host.sup.clone();
    let h = blocking(move || sup.health(&id)).await?.map_err(|e| format!("{e:?}"))?;
    Ok(json!(format!("{h:?}")))
}

#[tauri::command]
pub async fn os_start_app(window: WebviewWindow, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_start_app").map_err(|e| e.to_string())?;
    let sup = host.sup.clone();
    let s = blocking(move || sup.start(&id)).await?.map_err(|e| format!("{e:?}"))?;
    Ok(status_json(&s))
}

#[tauri::command]
pub async fn os_stop_app(window: WebviewWindow, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_stop_app").map_err(|e| e.to_string())?;
    let sup = host.sup.clone();
    let s = blocking(move || sup.stop(&id)).await?.map_err(|e| format!("{e:?}"))?;
    Ok(status_json(&s))
}

#[tauri::command]
pub async fn os_open_app_window(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<()> {
    authorize_shell(window.label(), "os_open_app_window").map_err(|e| e.to_string())?;
    open_app_window(&app, &host, &id)
}

/// Close the window of app `id`. An app may close its own window; main may close any. The process keeps running.
#[tauri::command]
pub async fn os_close_app_window(window: WebviewWindow, app: AppHandle, id: String) -> R<()> {
    authorize_app(window.label(), &id).map_err(|e| e.to_string())?;
    let label = crate::access::app_label(&id).map_err(|e| e.to_string())?;
    if let Some(w) = app.get_webview_window(&label) {
        w.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn os_catalog(window: WebviewWindow, host: State<'_, Host>) -> R<Value> {
    authorize_shell(window.label(), "os_catalog").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    // The offered version needs each app's feed: read here, off the UI thread; a feed that cannot be read is just "no version".
    let (l, offered) = blocking(move || {
        let l = a.catalog()?;
        let offered: Vec<Option<String>> = l.apps.iter().map(|e| a.offered(e, "stable")).collect();
        Ok::<_, battalion_host::armory::ArmoryError>((l, offered))
    })
    .await?
    .map_err(|e| e.to_string())?;
    Ok(json!({
        "apps": l.apps.iter().zip(offered).map(|(e, v)| json!({
            "id": e.app.id, "name": e.app.name, "publisher": e.app.publisher, "summary": e.app.summary,
            "icon": catalog_icon(e.app.icon.as_deref()), "version": v, "source": e.source, "compatible": e.compatible
        })).collect::<Vec<_>>(),
        "rejected": l.rejected, "sourceErrors": l.source_errors, "fromCache": l.from_cache
    }))
}

#[tauri::command]
pub async fn os_install_app(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, channel: Option<String>) -> R<Value> {
    authorize_shell(window.label(), "os_install_app").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let ch = channel.unwrap_or_else(|| "stable".into());
    let id2 = id.clone();
    let r = blocking(move || a.install(&id, &ch)).await?.map_err(|e| e.to_string());
    announce(&app, &host, &id2).await;
    r?;
    Ok(json!({"ok": true}))
}

/// Tell every window that app `id` changed: the shell reloads its list.
async fn announce(app: &AppHandle, host: &Host, id: &str) {
    let a = host.armory.clone();
    let want = id.to_string();
    let state = blocking(move || a.installed().ok().and_then(|l| l.into_iter().find(|x| x.id == want)).map(|x| x.state)).await.ok().flatten();
    let _ = app.emit(crate::events::APP_STATE, json!({"id": id, "state": state}));
}

/// Update app `id` on its channel. Only that app is stopped and started again; the OS and every other app keep running.
#[tauri::command]
pub async fn os_update_app(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_update_app").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let id2 = id.clone();
    let r = blocking(move || a.update(&id, &UpdateOptions::default())).await?.map_err(|e| e.to_string());
    announce(&app, &host, &id2).await;
    let u = r?;
    Ok(json!({"id": u.id, "from": u.from, "to": u.to}))
}

/// Go back to the previous version of app `id` (refused when the state schema changed).
#[tauri::command]
pub async fn os_rollback_app(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_rollback_app").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let id2 = id.clone();
    let r = blocking(move || a.rollback(&id)).await?.map_err(|e| e.to_string());
    announce(&app, &host, &id2).await;
    let u = r?;
    Ok(json!({"id": u.id, "from": u.from, "to": u.to}))
}

/// Remove app `id`: its window closes, its process stops, its versions go. Its state folder stays.
#[tauri::command]
pub async fn os_remove_app(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_remove_app").map_err(|e| e.to_string())?;
    let label = crate::access::app_label(&id).map_err(|e| e.to_string())?;
    if let Some(w) = app.get_webview_window(&label) {
        w.close().map_err(|e| e.to_string())?;
    }
    host.servers.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
    let a = host.armory.clone();
    let id2 = id.clone();
    let r = blocking(move || a.remove(&id, &RemoveOptions::default())).await?.map_err(|e| e.to_string());
    announce(&app, &host, &id2).await;
    r?;
    Ok(json!({"ok": true}))
}

/// End the OS process only. Running apps are let go (or stopped, when their manifest says `onOsExit: stop`)
/// by the exit handler in main.rs; the next start adopts the rest.
#[tauri::command]
pub async fn os_exit(window: WebviewWindow, app: AppHandle) -> R<()> {
    authorize_shell(window.label(), "os_exit").map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn os_open_login(window: WebviewWindow, app: AppHandle) -> R<()> {
    authorize_shell(window.label(), "os_open_login").map_err(|e| e.to_string())?;
    if let Some(w) = app.get_webview_window(crate::access::LOGIN) {
        return w.set_focus().map_err(|e| e.to_string());
    }
    WebviewWindowBuilder::new(&app, crate::access::LOGIN, WebviewUrl::App("login.html".into()))
        .title("Sign in")
        .inner_size(420.0, 560.0)
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The login window says it is finished; the host closes it and tells the shell.
#[tauri::command]
pub async fn os_login_done(window: WebviewWindow, app: AppHandle) -> R<()> {
    if window.label() != crate::access::LOGIN {
        return Err("os_login_done is for the login window only".into());
    }
    let _ = app.emit(crate::events::LOGIN_CHANGED, true);
    window.close().map_err(|e| e.to_string())
}
