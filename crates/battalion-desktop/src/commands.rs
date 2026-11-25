// SPDX-License-Identifier: MPL-2.0
//! The os_* commands. Every one is async (a blocking call goes through spawn_blocking), and every one
//! checks the calling window's label (from the runtime, not from the arguments) before it acts.

use crate::access::{authorize_app, authorize_self, authorize_shell, caller_of, Caller};
use crate::host::Host;
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
    // `dev` is true only in a developer build (BOS-31): the shell shows a "dev" mark.
    Ok(json!({"label": label, "app": app, "dev": battalion_host::dev::ENABLED}))
}

#[tauri::command]
pub async fn os_list_apps(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    authorize_shell(window.label(), "os_list_apps").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let list = blocking(move || a.installed()).await?.map_err(|e| e.to_string())?;
    // A badge belongs to a running app: one whose app is no longer running is dropped here.
    let badge_of = |a: &battalion_host::armory::InstalledApp| {
        if a.state == "running" {
            host.badges.get(&a.id)
        } else {
            if host.badges.set(&a.id, None) {
                paint_badge(&app, &a.id, None);
            }
            None
        }
    };
    Ok(json!(list
        .iter()
        .map(|a| json!({"id": a.id, "version": a.version, "state": a.state, "channel": a.channel, "name": a.name, "icon": a.icon, "available": a.available, "previous": a.previous, "badge": badge_of(a)}))
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
pub async fn os_stop_app(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_stop_app").map_err(|e| e.to_string())?;
    let sup = host.sup.clone();
    let id2 = id.clone();
    let s = blocking(move || sup.stop(&id)).await?.map_err(|e| format!("{e:?}"))?;
    if host.badges.set(&id2, None) {
        paint_badge(&app, &id2, None);
    }
    Ok(status_json(&s))
}

/// Open app `id` inside the OS window (it fills the app area above the taskbar). The window of an app of its own is not opened
/// now; the shell opens the view when it hears `os://app-open`. The app must be installed and have a page. Nothing is started here:
/// the shell starts a stopped app first and shows its state while the frame waits.
#[tauri::command]
pub async fn os_open_app_window(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<()> {
    authorize_shell(window.label(), "os_open_app_window").map_err(|e| e.to_string())?;
    crate::access::app_label(&id).map_err(|e| e.to_string())?;
    let paths = host.paths.clone();
    let id2 = id.clone();
    blocking(move || crate::host::page_exists(&paths, &id2)).await??;
    if let Some(main) = app.get_webview_window(crate::access::MAIN) {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
    app.emit(crate::events::APP_OPEN, &id).map_err(|e| e.to_string())
}

/// The page of app `id` for a frame in the OS window: `{id, version, origin, url}`. The origin is the app's one kept page origin.
#[tauri::command]
pub async fn os_app_page(window: WebviewWindow, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_app_page").map_err(|e| e.to_string())?;
    crate::access::app_label(&id).map_err(|e| e.to_string())?;
    let (paths, sup, pages) = (host.paths.clone(), host.sup.clone(), host.pages.clone());
    let r = blocking(move || crate::host::page_of(&paths, &sup, &pages, &id)).await??;
    Ok(json!({"id": r.id, "version": r.version, "origin": r.origin, "url": r.url}))
}

/// The badge of the app whose frame sent the message. The shell names the app from the frame (never from the message); an app
/// window sets its own badge with `os_badge`. Main only.
#[tauri::command]
pub async fn os_frame_badge(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, count: Option<u64>, text: Option<String>) -> R<()> {
    authorize_shell(window.label(), "os_frame_badge").map_err(|e| e.to_string())?;
    crate::access::app_label(&id).map_err(|e| e.to_string())?;
    let badge = crate::badge::parse(count, text.as_deref())?;
    if host.badges.set(&id, badge.clone()) {
        paint_badge(&app, &id, badge.as_deref());
    }
    Ok(())
}

/// BOS-25: the native calls of an app's page in a frame. All are main only (the shell asks for the app of the frame that sent the
/// message, never from the message), and each checks the one manifest permission of its kind in the app's installed manifest.
/// Labels of sign-in windows that a second `os_frame_login_open` is replacing: their end is not reported as a close.
static REPLACING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

async fn allowed(host: &State<'_, Host>, id: &str, cmd: &'static str) -> R<()> {
    crate::access::app_label(id).map_err(|e| e.to_string())?;
    let (paths, id2) = (host.paths.clone(), id.to_string());
    let perms = blocking(move || crate::host::permissions_of(&paths, &id2)).await??;
    crate::frame_calls::require(id, &perms, cmd)
}

/// Choose a folder for the app of the frame (`os_pick_folder`): the path, or null when cancelled. Needs `files.user`.
#[tauri::command]
pub async fn os_frame_pick_folder(window: WebviewWindow, host: State<'_, Host>, id: String, title: Option<String>) -> R<Option<String>> {
    authorize_shell(window.label(), "os_frame_pick_folder").map_err(|e| e.to_string())?;
    allowed(&host, &id, crate::frame_calls::PICK_FOLDER).await?;
    let title = crate::frame_calls::dialog_title(title.as_deref(), "Choose a folder");
    let w = window.clone();
    let picked = blocking(move || rfd::FileDialog::new().set_title(title).set_parent(&w).pick_folder()).await?;
    Ok(picked.map(|p| p.to_string_lossy().into_owned()))
}

/// Choose one file for the app of the frame (`os_pick_file`): the path, or null when cancelled. Needs `files.user`.
#[tauri::command]
pub async fn os_frame_pick_file(window: WebviewWindow, host: State<'_, Host>, id: String, title: Option<String>) -> R<Option<String>> {
    authorize_shell(window.label(), "os_frame_pick_file").map_err(|e| e.to_string())?;
    allowed(&host, &id, crate::frame_calls::PICK_FILE).await?;
    let title = crate::frame_calls::dialog_title(title.as_deref(), "Choose a file");
    let w = window.clone();
    let picked = blocking(move || rfd::FileDialog::new().set_title(title).set_parent(&w).pick_file()).await?;
    Ok(picked.map(|p| p.to_string_lossy().into_owned()))
}

/// Open a link in the system browser for the app of the frame (`os_open_external`). http, https and mailto only. Needs `network`.
#[tauri::command]
pub async fn os_frame_open_external(window: WebviewWindow, host: State<'_, Host>, id: String, url: String) -> R<()> {
    authorize_shell(window.label(), "os_frame_open_external").map_err(|e| e.to_string())?;
    allowed(&host, &id, crate::frame_calls::OPEN_EXTERNAL).await?;
    let url = crate::frame_calls::external_link(&url)?;
    blocking(move || open::that_detached(url).map_err(|e| e.to_string())).await?
}

/// Open the sign-in window of the app of the frame (`os_login_window_open`): a window of the OS with no capability and its own
/// data folder, showing `url`. Every web address it reaches is sent to the shell (`os://frame-login-url`), and its end
/// (`os://frame-login-closed`). One per app: a second call replaces the first. Needs `login.window`.
#[tauri::command]
pub async fn os_frame_login_open(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, url: String) -> R<()> {
    authorize_shell(window.label(), "os_frame_login_open").map_err(|e| e.to_string())?;
    allowed(&host, &id, crate::frame_calls::LOGIN_WINDOW_OPEN).await?;
    let target: tauri::Url = crate::frame_calls::login_url(&url)?.parse().map_err(|e| format!("not a web address: {e}"))?;
    let label = crate::frame_calls::login_label(&id);
    if let Some(old) = app.get_webview_window(&label) {
        // A second call replaces the first: the old window ends without telling the page it was closed, and its label must be gone
        // before the new one is built (a close is asynchronous).
        REPLACING.lock().unwrap_or_else(|p| p.into_inner()).push(label.clone());
        let _ = old.close();
        let (a2, l2) = (app.clone(), label.clone());
        blocking(move || {
            for _ in 0..80 {
                if a2.get_webview_window(&l2).is_none() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        })
        .await?;
        REPLACING.lock().unwrap_or_else(|p| p.into_inner()).retain(|l| l != &label);
    }
    let data = host.paths.webview_dir(&id).map_err(|e| e.0)?;
    let data = data.with_file_name(format!("{}-login", data.file_name().and_then(|n| n.to_str()).unwrap_or("app")));
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let (seen, app_id) = (app.clone(), id.clone());
    let win = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(target))
        .title("Sign in")
        .inner_size(520.0, 760.0)
        .center()
        .data_directory(data)
        .on_navigation(move |u| {
            if matches!(u.scheme(), "http" | "https") {
                let _ = seen.emit(crate::events::FRAME_LOGIN_URL, json!({"id": app_id, "url": u.to_string()}));
            }
            true
        })
        .build()
        .map_err(|e| e.to_string())?;
    let (closed, app_id) = (app.clone(), id);
    win.on_window_event(move |e| {
        if matches!(e, tauri::WindowEvent::Destroyed) {
            if REPLACING.lock().unwrap_or_else(|p| p.into_inner()).contains(&crate::frame_calls::login_label(&app_id)) {
                return;
            }
            let _ = closed.emit(crate::events::FRAME_LOGIN_CLOSED, json!({"id": app_id}));
        }
    });
    Ok(())
}

/// Close the sign-in window of the app of the frame (`os_login_window_close`). Needs `login.window`; nothing open is not an error.
#[tauri::command]
pub async fn os_frame_login_close(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<()> {
    authorize_shell(window.label(), "os_frame_login_close").map_err(|e| e.to_string())?;
    allowed(&host, &id, crate::frame_calls::LOGIN_WINDOW_CLOSE).await?;
    if let Some(w) = app.get_webview_window(&crate::frame_calls::login_label(&id)) {
        w.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Show app `id`'s badge on its window: the taskbar button (overlay dot on Windows, the badge label where the OS has one) and the
/// title. `None` removes it. The desktop icon is the shell's: it reads the badge from `os_list_apps`.
fn paint_badge(app: &AppHandle, id: &str, badge: Option<&str>) {
    let _ = app.emit(crate::events::BADGE, json!({"id": id, "badge": badge}));
    let Ok(label) = crate::access::app_label(id) else { return };
    let Some(w) = app.get_webview_window(&label) else { return };
    let _ = w.set_title(&match badge {
        Some(b) => format!("({b}) {id}"),
        None => id.to_string(),
    });
    #[cfg(windows)]
    {
        // Windows has no badge label: a red dot overlay on the taskbar button; the count is in the title.
        let dot = badge.map(|_| {
            let mut rgba = Vec::with_capacity(16 * 16 * 4);
            for y in 0..16i32 {
                for x in 0..16i32 {
                    let inside = (x - 8) * (x - 8) + (y - 8) * (y - 8) <= 49;
                    rgba.extend_from_slice(if inside { &[0xE5, 0x39, 0x35, 0xFF] } else { &[0, 0, 0, 0] });
                }
            }
            tauri::image::Image::new_owned(rgba, 16, 16)
        });
        let _ = w.set_overlay_icon(dot);
    }
    #[cfg(not(windows))]
    {
        let _ = w.set_badge_label(badge.map(str::to_string));
    }
}

/// The app's own badge: a count, a short text, or null / 0 / "" to clear it. The app is the window making the call
/// (its label), never an argument. The badge goes away when the app stops, is updated or is removed.
#[tauri::command]
pub async fn os_badge(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, count: Option<u64>, text: Option<String>) -> R<()> {
    let id = authorize_self(window.label(), "os_badge").map_err(|e| e.to_string())?;
    let badge = crate::badge::parse(count, text.as_deref())?;
    if host.badges.set(&id, badge.clone()) {
        paint_badge(&app, &id, badge.as_deref());
    }
    Ok(())
}

/// Bring the desktop (the `main` window) to the front and leave the calling app's window open, minimized.
#[tauri::command]
pub async fn os_show_desktop(window: WebviewWindow, app: AppHandle) -> R<()> {
    authorize_self(window.label(), "os_show_desktop").map_err(|e| e.to_string())?;
    let main = app.get_webview_window(crate::access::MAIN).ok_or("the desktop window is not open")?;
    main.unminimize().map_err(|e| e.to_string())?;
    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    // Minimize is the one hide every window manager does the same way (the window stays open, one click on the taskbar returns it).
    window.minimize().map_err(|e| e.to_string())
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

/// (BOS-19) The app a start from outside asked for (`--open <id>`), once: the shell calls this when it has loaded and when it
/// hears `os://open-request`. `null` when there is none.
#[tauri::command]
pub async fn os_take_open_request(window: WebviewWindow, host: State<'_, Host>) -> R<Option<String>> {
    authorize_shell(window.label(), "os_take_open_request").map_err(|e| e.to_string())?;
    Ok(host.open_request.lock().unwrap_or_else(|p| p.into_inner()).take())
}

/// Keep the Start menu in step with the installed apps (one shortcut each). A failure is logged, never an install failure.
pub async fn sync_shortcuts(host: &Host) {
    let (p, a) = (host.paths.clone(), host.armory.clone());
    if let Ok(Err(e)) = blocking(move || crate::shortcuts::sync(&p, &a)).await {
        eprintln!("start menu shortcuts: {e}");
    }
}

/// Tell every window that app `id` changed: the shell reloads its list.
async fn announce(app: &AppHandle, host: &Host, id: &str) {
    sync_shortcuts(host).await;
    // The app was stopped or replaced by this action: its badge goes; a running app sets it again.
    if host.badges.set(id, None) {
        paint_badge(app, id, None);
    }
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
    host.pages.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
    let a = host.armory.clone();
    let id2 = id.clone();
    let r = blocking(move || a.remove(&id, &RemoveOptions::default())).await?.map_err(|e| e.to_string());
    announce(&app, &host, &id2).await;
    r?;
    Ok(json!({"ok": true}))
}

/// Exit: the exit handler in main.rs stops every running app cleanly, then the OS ends.
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

/// Run the first-run install again (Retry after a failure or while offline). Returns the outcome as text; progress is
/// the `os://first-run` event. A second call while one runs is refused by the Armory's own app lock, not queued.
#[tauri::command]
pub async fn os_first_run_retry(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    authorize_shell(window.label(), "os_first_run_retry").map_err(|e| e.to_string())?;
    let app2 = app.clone();
    let out = blocking(move || crate::host::run_first_run(&app2, &app2.state::<Host>()))
    .await?;
    Ok(json!({"outcome": format!("{out:?}")}))
}

// ---- BOS-30: BattalionOS updates itself ------------------------------------------------------------------------------------

fn os_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// What the Armory shows about the OS: the version, a newer one if the last check found it, and what the confirm needs to name
/// (apps that report work in flight, apps that will be stopped). Never reads the network.
async fn os_update_json(app: &AppHandle, host: &Host) -> Value {
    let (available, installing) = {
        let s = host.os_update.lock().unwrap_or_else(|p| p.into_inner());
        (s.available.clone(), s.installing)
    };
    let a = host.armory.clone();
    let (busy, running) = blocking(move || (a.os_busy_apps(), a.os_running_apps())).await.unwrap_or_default();
    json!({"current": os_version(app), "available": available.map(|u| json!({"version": u.version, "notes": u.notes})), "installing": installing, "busy": busy, "running": running})
}

/// What the OS knows about its own update now. No network.
#[tauri::command]
pub async fn os_update_state(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    authorize_shell(window.label(), "os_update_state").map_err(|e| e.to_string())?;
    Ok(os_update_json(&app, &host).await)
}

/// Read the OS's own feed. A feed that is missing, 404, offline or broken is not an error here: the Armory keeps working and
/// the answer is what was known before.
#[tauri::command]
pub async fn os_update_check(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    authorize_shell(window.label(), "os_update_check").map_err(|e| e.to_string())?;
    check_os_update(&app, &host).await;
    Ok(os_update_json(&app, &host).await)
}

/// One check of the feed; the result is kept in the host and told to the shell. Used by the command and by the daily timer.
pub async fn check_os_update(app: &AppHandle, host: &Host) {
    let Some(feed) = crate::osupdate::feed() else { return };
    let (a, cur) = (host.armory.clone(), os_version(app));
    // Only a real read changes what is known: "nothing newer" clears it, an error (no feed, 404, offline) keeps it.
    match blocking(move || a.os_check(&feed, &cur)).await {
        Ok(Ok(found)) => host.os_update.lock().unwrap_or_else(|p| p.into_inner()).available = found,
        Ok(Err(e)) => eprintln!("os update check: {e}"),
        Err(e) => eprintln!("os update check: {e}"),
    }
    let state = os_update_json(app, host).await;
    let _ = app.emit(crate::events::OS_UPDATE, state);
}

/// Update BattalionOS: download and verify the signature BEFORE anything stops, then stop every app the normal way, hand over to the
/// installer and end. A bad signature, a broken download or an app that will not stop is an error and the running version, and its
/// apps, are as they were. The installer (per user, no admin) starts the OS again; the apps that ran start again at that start.
#[tauri::command]
pub async fn os_update_install(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    authorize_shell(window.label(), "os_update_install").map_err(|e| e.to_string())?;
    let (upd, feed) = {
        let mut s = host.os_update.lock().unwrap_or_else(|p| p.into_inner());
        if s.installing {
            return Err("an update is already running".into());
        }
        let upd = s.available.clone().ok_or("there is no newer BattalionOS to install")?;
        s.installing = true;
        (upd, crate::osupdate::feed().ok_or("this build has no update feed")?)
    };
    let a = host.armory.clone();
    let app2 = app.clone();
    let upd2 = upd.clone();
    let r = blocking(move || {
        let mut last = 0u8;
        let r = a.os_prepare(&feed, &upd2, &mut |got, total| {
            let pct = if total > 0 { Some((got.min(total) * 100 / total) as u8) } else { None };
            if pct.is_some() && pct == Some(last) {
                return;
            }
            last = pct.unwrap_or(0);
            let _ = app2.emit(crate::events::ARMORY_PROGRESS, json!({"id": crate::osupdate::OS_ROW, "op": "update", "stage": "download", "bytes": got, "total": if total > 0 { json!(total) } else { Value::Null }, "percent": pct}));
        });
        if r.is_ok() {
            let _ = app2.emit(crate::events::ARMORY_PROGRESS, json!({"id": crate::osupdate::OS_ROW, "op": "update", "stage": "install"}));
        }
        r
    })
    .await?;
    let finish_err = |host: &Host| host.os_update.lock().unwrap_or_else(|p| p.into_inner()).installing = false;
    let (msi, stopped) = match r {
        Ok(x) => x,
        Err(e) => {
            finish_err(&host);
            return Err(e.to_string());
        }
    };
    // Every app is stopped. Hand over: a hidden helper outlives this process, waits for it to end, runs the installer and
    // starts the OS again. When it cannot even start, the apps start again here and the OS goes on as it was.
    if let Err(e) = spawn_installer(&msi) {
        let a = host.armory.clone();
        let _ = blocking(move || a.os_resume_apps()).await;
        finish_err(&host);
        return Err(format!("could not start the installer: {e}"));
    }
    let _ = stopped;
    app.exit(0);
    Ok(json!({"ok": true}))
}

#[cfg(windows)]
fn spawn_installer(msi: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe()?;
    // CREATE_NO_WINDOW | DETACHED_PROCESS: no console, and it outlives this process.
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &crate::osupdate::installer_script()])
        .env("BOS_UPD_PID", std::process::id().to_string())
        .env("BOS_UPD_MSI", msi)
        .env("BOS_UPD_EXE", exe)
        .creation_flags(0x0800_0000 | 0x0000_0008)
        .spawn()
        .map(|_| ())
}

#[cfg(not(windows))]
fn spawn_installer(_msi: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::other("updating BattalionOS in place is only done on Windows for now"))
}
