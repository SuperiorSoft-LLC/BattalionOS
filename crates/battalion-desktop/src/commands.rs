// SPDX-License-Identifier: MPL-2.0
//! The os_* commands. Every one is async (a blocking call goes through spawn_blocking), and every one
//! checks the calling window's label (from the runtime, not from the arguments) before it acts.

use crate::access::{authorize_app, authorize_self, authorize_shell, caller_of, Caller};
use crate::host::Host;
use battalion_host::armory::{info::catalog_icon, ArmoryError, RemoveOptions, UpdateOptions};
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

/// BOS-LAYOUT-1: how far (logical px: top, right, bottom, left) a window reaches past its monitor's work area. An undecorated window
/// on Windows keeps its resize frame, so maximized it covers the work area grown by that frame on every side (and the BOS taskbar at
/// the bottom lands off screen or under the Windows taskbar); the shell pads itself by this. Zero when the window is inside.
pub fn work_area_inset(win: (i32, i32, u32, u32), area: (i32, i32, u32, u32), scale: f64) -> [f64; 4] {
    let (wx, wy, ww, wh) = (win.0 as i64, win.1 as i64, win.2 as i64, win.3 as i64);
    let (ax, ay, aw, ah) = (area.0 as i64, area.1 as i64, area.2 as i64, area.3 as i64);
    let s = if scale > 0.0 { scale } else { 1.0 };
    let px = |d: i64| (d.max(0) as f64) / s;
    [px(ay - wy), px((wx + ww) - (ax + aw)), px((wy + wh) - (ay + ah)), px(ax - wx)]
}

/// BOS-LAYOUT-1: the OS window's overflow past the work area (see `work_area_inset`); all zero while it is not maximized.
#[tauri::command]
pub async fn os_main_inset(window: WebviewWindow) -> R<Value> {
    authorize_shell(window.label(), "os_main_inset").map_err(|e| e.to_string())?;
    let zero = json!({"top": 0, "right": 0, "bottom": 0, "left": 0});
    if !window.is_maximized().unwrap_or(false) {
        return Ok(zero);
    }
    let (Ok(pos), Ok(size), Ok(Some(mon))) = (window.outer_position(), window.outer_size(), window.current_monitor()) else { return Ok(zero) };
    let wa = mon.work_area();
    let [t, r, b, l] = work_area_inset((pos.x, pos.y, size.width, size.height), (wa.position.x, wa.position.y, wa.size.width, wa.size.height), mon.scale_factor());
    Ok(json!({"top": t, "right": r, "bottom": b, "left": l}))
}

#[cfg(test)]
mod work_area_tests {
    use super::work_area_inset;
    #[test]
    fn a_maximized_undecorated_window_overflows_by_its_frame() {
        // 1920x1080 monitor, 48 px Windows taskbar, 8 px frame each side at 100 %.
        assert_eq!(work_area_inset((-8, -8, 1936, 1048), (0, 0, 1920, 1032), 1.0), [8.0, 8.0, 8.0, 8.0]);
        // 150 %: physical 12 px is 8 logical px.
        assert_eq!(work_area_inset((-12, -12, 2904, 1572), (0, 0, 2880, 1548), 1.5), [8.0, 8.0, 8.0, 8.0]);
    }
    #[test]
    fn a_window_inside_the_work_area_needs_none() {
        assert_eq!(work_area_inset((100, 100, 800, 600), (0, 0, 1920, 1032), 1.0), [0.0; 4]);
    }
}

#[tauri::command]
pub async fn os_window_info(window: WebviewWindow, host: State<'_, Host>) -> R<Value> {
    let label = window.label().to_string();
    let app = match caller_of(&label).map_err(|e| e.to_string())? {
        Caller::App(id) => Value::String(id),
        _ => Value::Null,
    };
    // `dev` is true only in a developer build (BOS-31): the shell shows a "dev" mark. `platform` (AND-UI-1) picks the shell's layout:
    // a phone (android, ios) has no window buttons and opens with one tap.
    // `groupWindows` (WM-3): the setting `bos.taskbar.groupWindows` the shell's taskbar follows.
    let group_windows = crate::windows::taskbar_group_windows(&host.paths);
    Ok(json!({"label": label, "app": app, "dev": battalion_host::dev::ENABLED, "platform": std::env::consts::OS, "groupWindows": group_windows}))
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
        .map(|a| json!({"id": a.id, "version": a.version, "state": a.state, "channel": a.channel, "name": a.name, "icon": a.icon, "available": a.available, "previous": a.previous, "pendingUpdate": a.pending_update, "channels": a.channels, "channelVersion": a.channel_version, "badge": badge_of(a)}))
        .collect::<Vec<_>>()))
}

#[tauri::command]
pub async fn os_app_status(window: WebviewWindow, host: State<'_, Host>, id: String) -> R<Value> {
    crate::access::authorize_app_read(window.label(), &id).map_err(|e| e.to_string())?;
    let sup = host.sup.clone();
    let s = blocking(move || sup.status(&id)).await?.map_err(|e| e.to_string())?;
    Ok(status_json(&s))
}

#[tauri::command]
pub async fn os_app_health(window: WebviewWindow, host: State<'_, Host>, id: String) -> R<Value> {
    crate::access::authorize_app_read(window.label(), &id).map_err(|e| e.to_string())?;
    let sup = host.sup.clone();
    let h = blocking(move || sup.health(&id)).await?.map_err(|e| e.to_string())?;
    Ok(json!(format!("{h:?}")))
}

#[tauri::command]
pub async fn os_start_app(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_start_app").map_err(|e| e.to_string())?;
    // UPDATE-LIVE-1: an update "Later" was chosen for is applied before the app starts (the app is not running: nothing waits).
    let (a, pid) = (host.armory.clone(), id.clone());
    if let Ok(Ok(Some(u))) = blocking(move || a.apply_pending(&pid)).await {
        updated(&app, &host, &u).await;
    }
    let sup = host.sup.clone();
    let id2 = id.clone();
    let s = blocking(move || sup.start(&id)).await?.map_err(|e| e.to_string())?;
    restore_native_of(&app, &host, &id2).await;
    Ok(status_json(&s))
}

#[tauri::command]
pub async fn os_stop_app(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_stop_app").map_err(|e| e.to_string())?;
    let sup = host.sup.clone();
    let id2 = id.clone();
    let s = blocking(move || sup.stop(&id)).await?.map_err(|e| e.to_string())?;
    if host.badges.set(&id2, None) {
        paint_badge(&app, &id2, None);
    }
    close_native_of(&app, &host, &id2).await;
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
        #[cfg(desktop)]
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
    let mut v = json!({"id": r.id, "version": r.version, "origin": r.origin, "url": r.url});
    if let Some(f) = r.files {
        v["files"] = json!({"root": f.root, "pageDir": f.page_dir, "csp": f.csp});
    }
    Ok(v)
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
    // AND-1: no native file dialog on Android yet (a later step uses the system picker).
    #[cfg(target_os = "android")]
    {
        let _ = (title, w);
        Err("choosing a file is not available on this device yet".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let picked = blocking(move || rfd::FileDialog::new().set_title(title).set_parent(&w).pick_folder()).await?;
        Ok(picked.map(|p| p.to_string_lossy().into_owned()))
    }
}

/// Choose one file for the app of the frame (`os_pick_file`): the path, or null when cancelled. Needs `files.user`.
#[tauri::command]
pub async fn os_frame_pick_file(window: WebviewWindow, host: State<'_, Host>, id: String, title: Option<String>) -> R<Option<String>> {
    authorize_shell(window.label(), "os_frame_pick_file").map_err(|e| e.to_string())?;
    allowed(&host, &id, crate::frame_calls::PICK_FILE).await?;
    let title = crate::frame_calls::dialog_title(title.as_deref(), "Choose a file");
    let w = window.clone();
    // AND-1: no native file dialog on Android yet (a later step uses the system picker).
    #[cfg(target_os = "android")]
    {
        let _ = (title, w);
        Err("choosing a file is not available on this device yet".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let picked = blocking(move || rfd::FileDialog::new().set_title(title).set_parent(&w).pick_file()).await?;
        Ok(picked.map(|p| p.to_string_lossy().into_owned()))
    }
}

/// Open a link in the system browser for the app of the frame (`os_open_external`). http, https and mailto only. Needs `network`.
#[tauri::command]
pub async fn os_frame_open_external(window: WebviewWindow, host: State<'_, Host>, id: String, url: String) -> R<()> {
    authorize_shell(window.label(), "os_frame_open_external").map_err(|e| e.to_string())?;
    allowed(&host, &id, crate::frame_calls::OPEN_EXTERNAL).await?;
    let url = crate::frame_calls::external_link(&url)?;
    blocking(move || open::that_detached(url).map_err(|e| e.to_string())).await?
}

/// BOS-NOTIFY-1: the per-app rate of desktop notices.
#[cfg(not(target_os = "android"))]
static NOTICE_RATE: std::sync::Mutex<Option<crate::frame_calls::NoticeRate>> = std::sync::Mutex::new(None);

/// Show a desktop notice for the app of the frame (`os_notify` on the desktop): a Windows toast naming the app, shown without
/// taking the focus. `args` is the page's (`title`, `text?`, `tag?`, `route?`; a repeat with the same tag replaces the shown one).
/// A click restores BattalionOS, opens the app (`os://app-open`) and emits `os://frame-notify-click` `{id, tag, route}`. Needs
/// `notifications`; at most `NOTIFY_PER_MINUTE` a minute per app.
#[tauri::command]
pub async fn os_frame_notify(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<()> {
    authorize_shell(window.label(), "os_frame_notify").map_err(|e| e.to_string())?;
    crate::access::app_label(&id).map_err(|e| e.to_string())?;
    let (paths, id2) = (host.paths.clone(), id.clone());
    let (perms, name) = blocking(move || crate::host::permissions_and_name(&paths, &id2)).await??;
    crate::frame_calls::require(&id, &perms, crate::frame_calls::NOTIFY)?;
    let n = crate::frame_calls::notice(&args.unwrap_or(Value::Null))?;
    #[cfg(target_os = "android")]
    {
        let _ = (app, name, n);
        Err("a phone notice is the app slot's own (os_frame_phone_call)".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        NOTICE_RATE.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(Default::default).take(&id, std::time::Instant::now())?;
        let app_id = if tauri::is_dev() { crate::toast::POWERSHELL_APP_ID.to_string() } else { app.config().identifier.clone() };
        let (handle, click) = (app.clone(), json!({"id": id, "tag": n.tag, "route": n.route}));
        let group = format!("app-{id}");
        blocking(move || {
            crate::toast::show(&app_id, &group, &name, &n.title, &n.text, n.tag.as_deref(), move || {
                crate::front::bring_main_to_front(&handle);
                let _ = handle.emit(crate::events::APP_OPEN, click["id"].as_str().unwrap_or_default());
                let _ = handle.emit(crate::events::FRAME_NOTIFY_CLICK, click.clone());
            })
        })
        .await?
    }
}

/// AND-4: a native call of an app's page on a phone (`os_pick_folder`, `os_pick_file`, `os_open_external`, `os_notify`,
/// `os_scan_code`). The host checks the app's installed manifest and the arguments; the shell then hands `{call, args}` to the
/// app's own slot, which runs it with Android (document picker, intent, notification channel, code scanner). Main only.
#[tauri::command]
pub async fn os_frame_phone_call(window: WebviewWindow, host: State<'_, Host>, id: String, cmd: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_phone_call").map_err(|e| e.to_string())?;
    crate::access::app_label(&id).map_err(|e| e.to_string())?;
    let (paths, id2) = (host.paths.clone(), id.clone());
    let perms = blocking(move || crate::host::permissions_of(&paths, &id2)).await??;
    crate::frame_calls::phone_call(&id, &perms, &cmd, &args.unwrap_or(Value::Null))
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
    let win = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(target));
    // AND-1: title, size and placement are desktop calls; on a phone the window fills the screen.
    #[cfg(desktop)]
    let win = win.title("Sign in").inner_size(520.0, 760.0).center();
    let win = win
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
    paint_window_badge(&w, badge);
}

/// The badge on one window's own taskbar button (an app window, or a native window of WM-6). `None` removes it.
pub(crate) fn paint_window_badge(w: &WebviewWindow, badge: Option<&str>) {
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
    #[cfg(all(desktop, not(windows)))]
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
    #[cfg(desktop)]
    main.unminimize().map_err(|e| e.to_string())?;
    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    // Minimize is the one hide every window manager does the same way (the window stays open, one click on the taskbar returns it).
    #[cfg(desktop)]
    return window.minimize().map_err(|e| e.to_string());
    #[cfg(mobile)]
    Ok(())
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
        // AND-2: and whether this device can install it (the catalog's `platforms`, else the feed's packages), with the reason.
        // LOCAL-REPO-1: an app a local repo publishes on another channel only is offered on that channel.
        let offered: Vec<battalion_host::armory::Offer> = l.apps.iter().map(|e| a.offer(e, default_channel(&e.app))).collect();
        Ok::<_, battalion_host::armory::ArmoryError>((l, offered))
    })
    .await?
    .map_err(|e| e.to_string())?;
    Ok(json!({
        "apps": l.apps.iter().zip(offered).map(|(e, v)| json!({
            "id": e.app.id, "name": e.app.name, "publisher": e.app.publisher, "summary": e.app.summary,
            "icon": catalog_icon(e.app.icon.as_deref()), "version": v.version, "unavailable": v.unavailable, "source": e.source, "compatible": e.compatible,
            "fromLocalRepo": e.from_local_repo, "label": e.from_local_repo.then_some(battalion_host::armory::LOCAL_LABEL)
        })).collect::<Vec<_>>(),
        "rejected": l.rejected, "sourceErrors": l.source_errors, "fromCache": l.from_cache
    }))
}

#[tauri::command]
pub async fn os_install_app(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, channel: Option<String>) -> R<Value> {
    authorize_shell(window.label(), "os_install_app").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let id2 = id.clone();
    let r = blocking(move || {
        // LOCAL-REPO-1: no channel given and the app has no stable one (a local prerelease-only app): its first channel.
        let ch = match channel {
            Some(c) => c,
            None => a.find(&id).map(|e| default_channel(&e.app).to_string()).unwrap_or_else(|_| "stable".into()),
        };
        a.install(&id, &ch)
    })
    .await?
    .map_err(|e| e.to_string());
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
/// UPDATE-LIVE-1: the new version is staged first, then a running app is asked to get ready and the update applies by itself
/// once it is. `now` = "Update now" (the app checkpoints at once). An app that does not get ready in time is not an error: the
/// answer is `{notReady: true, version, reason, etaMs}` and the shell offers "Update now" and "Later".
#[tauri::command]
pub async fn os_update_app(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, now: Option<bool>) -> R<Value> {
    authorize_shell(window.label(), "os_update_app").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let id2 = id.clone();
    let opts = UpdateOptions { now: now.unwrap_or(false), ..Default::default() };
    let r = blocking(move || a.update(&id, &opts)).await?;
    announce(&app, &host, &id2).await;
    match r {
        Ok(u) => {
            updated(&app, &host, &u).await;
            Ok(json!({"id": u.id, "from": u.from, "to": u.to}))
        }
        Err(ArmoryError::NotReady { version, reason, eta_ms }) => Ok(json!({"id": id2, "notReady": true, "version": version, "reason": reason, "etaMs": eta_ms})),
        Err(e) => Err(e.to_string()),
    }
}

/// ARMORY-CHANNEL-SWITCH: move app `id` to another channel its catalog entry lists. Nothing is installed: only the channel is
/// saved, the channel's feed is read (so the list shows its version), and every window hears the change. An update from there
/// still refuses a version that is not newer than the installed one (never a silent downgrade).
#[tauri::command]
pub async fn os_set_app_channel(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, channel: String) -> R<()> {
    authorize_shell(window.label(), "os_set_app_channel").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let id2 = id.clone();
    blocking(move || -> battalion_host::armory::Result<()> {
        a.set_channel(&id, &channel)?;
        // A channel with no feed yet (404, offline, nothing cached) is no offer, not an error.
        let entry = a.find(&id)?;
        let _ = a.feed(&entry.app, &channel);
        Ok(())
    })
    .await?
    .map_err(|e| e.to_string())?;
    announce(&app, &host, &id2).await;
    Ok(())
}

/// UPDATE-LIVE-1 "Later": app `id` updates by itself at its next start or the next moment it is idle. Answers the version.
#[tauri::command]
pub async fn os_update_later(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String) -> R<Value> {
    authorize_shell(window.label(), "os_update_later").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let id2 = id.clone();
    let v = blocking(move || a.defer_update(&id)).await?.map_err(|e| e.to_string())?;
    announce(&app, &host, &id2).await;
    Ok(json!({"id": id2, "pending": v}))
}

/// After an update of app `id`: every window hears `os://app-updated` (the shell closes the app's view and opens it again on the
/// new version), and a window of its own (`app-<id>`) is closed and opened again by the host.
pub async fn updated(app: &AppHandle, host: &Host, u: &battalion_host::armory::Updated) {
    let _ = app.emit(crate::events::APP_UPDATED, json!({"id": u.id, "from": u.from, "to": u.to}));
    let Ok(label) = crate::access::app_label(&u.id) else { return };
    let Some(w) = app.get_webview_window(&label) else { return };
    let _ = w.close();
    for _ in 0..50 {
        if app.get_webview_window(&label).is_none() {
            break;
        }
        let _ = blocking(|| std::thread::sleep(std::time::Duration::from_millis(100))).await;
    }
    if let Err(e) = crate::host::open_app_window(app, host, &u.id) {
        eprintln!("reopen {} after its update: {e}", u.id);
    }
}

/// UPDATE-LIVE-1: every app with a pending update ("Later") that is idle now is updated (a busy one stays pending). Called on the
/// desktop's timer.
pub async fn apply_pending_updates(app: &AppHandle, host: &Host) {
    let a = host.armory.clone();
    let ids: Vec<String> = blocking(move || a.installed().map(|l| l.into_iter().map(|x| x.id).filter(|id| a.pending_update(id).is_some()).collect()).unwrap_or_default()).await.unwrap_or_default();
    for id in ids {
        let a = host.armory.clone();
        let id2 = id.clone();
        match blocking(move || a.apply_pending(&id2)).await {
            Ok(Ok(Some(u))) => {
                announce(app, host, &id).await;
                updated(app, host, &u).await;
            }
            Ok(Err(e)) => eprintln!("pending update of {id}: {e}"),
            _ => {}
        }
    }
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
    if r.is_ok() {
        let _ = host.windows.app_removed(&id2).await; // OS-WINDOW-1: removing the app deletes its window file (an update keeps it)
    }
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

/// BOS-DEFAULT-INF: the apps the first run installed, once (then empty), for the shell's one-line notice.
#[tauri::command]
pub async fn os_take_first_run_notice(window: WebviewWindow, host: State<'_, Host>) -> R<Vec<String>> {
    authorize_shell(window.label(), "os_take_first_run_notice").map_err(|e| e.to_string())?;
    Ok(std::mem::take(&mut *host.first_run_notice.lock().unwrap_or_else(|p| p.into_inner())))
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
    // BOS-UPDATE-1: the OS ends only once the helper has said it runs; otherwise the apps start again and the error is shown.
    let logs = host.armory.paths().logs();
    let started = match blocking(move || spawn_installer(&msi, &logs)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(e) => Err(e),
    };
    if let Err(e) = started {
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
fn spawn_installer(msi: &std::path::Path, logs: &std::path::Path) -> std::io::Result<()> {
    use crate::osupdate::{helper_flags, HELPER_START_TIMEOUT_SECS};
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe()?;
    let _ = std::fs::create_dir_all(logs);
    let ready = logs.join("os-update-helper.ready");
    let _ = std::fs::remove_file(&ready);
    let mut c = std::process::Command::new("powershell.exe");
    c.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &crate::osupdate::installer_script()])
        .env("BOS_UPD_PID", std::process::id().to_string())
        .env("BOS_UPD_MSI", msi)
        .env("BOS_UPD_EXE", exe)
        .env("BOS_UPD_READY", &ready)
        .env("BOS_UPD_LOG", logs.join("os-update.log"));
    // A hidden console, its own group, out of any job; a job that forbids breakaway refuses the first try.
    let mut child = match c.creation_flags(helper_flags(true)).spawn() {
        Ok(ch) => ch,
        Err(_) => c.creation_flags(helper_flags(false)).spawn()?,
    };
    let t = std::time::Instant::now();
    while t.elapsed().as_secs() < HELPER_START_TIMEOUT_SECS {
        if ready.is_file() {
            let _ = std::fs::remove_file(&ready);
            return Ok(());
        }
        if let Ok(Some(st)) = child.try_wait() {
            return Err(std::io::Error::other(format!("the installer helper ended before it ran ({st})")));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = child.kill();
    Err(std::io::Error::other(format!("the installer helper did not start within {HELPER_START_TIMEOUT_SECS} s")))
}

#[cfg(not(windows))]
fn spawn_installer(_msi: &std::path::Path, _logs: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::other("updating BattalionOS in place is only done on Windows for now"))
}

// ---- OS-WINDOW-1 (WM-2): the window calls ----
// `os_win_*` are for an app's own window (`app-<id>`, or a native `win-<app>-<n>`): the app is the label's. `os_frame_win_*` are
// main only and serve a page in a frame (the shell names the app of the frame, never the message). Every call checks the `window`
// permission of the app's installed manifest, read at each call; the rules are in `win_calls.rs`.

/// The surface an inline window lives in: the OS window's client area, in logical pixels.
fn win_surface(app: &AppHandle) -> battalion_app_api::geometry::Surface {
    let size = app.get_webview_window(crate::access::MAIN).and_then(|w| {
        let s = w.inner_size().ok()?;
        let f = w.scale_factor().ok()?;
        Some((s.width as f64 / f, s.height as f64 / f))
    });
    let (w, h) = size.filter(|(w, h)| *w > 0.0 && *h > 0.0).unwrap_or((1200.0, 800.0));
    battalion_app_api::geometry::Surface::sized(w, h)
}

/// `os://win` to the owning app's windows only: the shell (it forwards to that app's frames, `{bos:1, event:"os_win", ...}`),
/// the app's own window and its native windows. Never to another app's window.
pub(crate) fn emit_win(app: &AppHandle, id: &str, ev: Value) {
    let mut payload = json!({"bos": 1, "event": "os_win", "id": id});
    if let (Some(p), Value::Object(e)) = (payload.as_object_mut(), ev) {
        p.extend(e);
    }
    let own_app = format!("{}{id}", crate::access::APP_PREFIX);
    let own_win = format!("{}{id}-", crate::access::WIN_PREFIX);
    for label in app.webview_windows().into_keys() {
        let native_of_id = label.strip_prefix(&own_win).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if label == crate::access::MAIN || label == own_app || native_of_id {
            let _ = app.emit_to(label.as_str(), crate::events::WIN, payload.clone());
        }
    }
}

/// WM-6: does this OS open windows of its own (`native`)? A phone opens them as a slot view.
const OS_WINDOWS: bool = cfg!(not(any(target_os = "android", target_os = "ios")));

async fn win_run(app: &AppHandle, host: &Host, id: &str, cmd: &'static str, args: Value) -> R<Value> {
    use crate::native::{self, NativeWindows, Step};
    crate::access::app_label(id).map_err(|e| e.to_string())?;
    let (paths, id2) = (host.paths.clone(), id.to_string());
    let perms = blocking(move || crate::host::permissions_of(&paths, &id2)).await??;
    let engine = crate::native_tauri::TauriNative(app.clone());
    // A native window lives on a monitor, not in the OS window: its surface is that monitor's work area (the saved one while it is
    // attached, else the primary), so a window whose monitor is gone is clamped back on screen.
    let mut surface = win_surface(app);
    let mut monitor = None;
    let mut args = args;
    if cmd == crate::win_calls::OPEN && OS_WINDOWS {
        let key = args.get("key").and_then(Value::as_str).unwrap_or_default().to_string();
        let saved = host.windows.saved(id).await.ok().and_then(|m| m.get(&key).cloned());
        let asked = args.get("kind").and_then(Value::as_str);
        if asked == Some("native") || (asked.is_none() && saved.as_ref().is_some_and(|s| s.kind == crate::windows::Kind::Native)) {
            let monitors = engine.monitors();
            if let Some(m) = native::pick_monitor(saved.as_ref(), &monitors) {
                surface = m.work;
                if args.get("bounds").is_none_or(Value::is_null) {
                    args["bounds"] = serde_json::to_value(native::default_bounds(&m.work)).map_err(|e| e.to_string())?;
                }
                monitor = Some(m.name.clone());
            }
        }
    }
    let before = match args.get("wid").and_then(Value::as_str) {
        Some(wid) => host.windows.list(Some(id)).await.map_err(|e| e.to_string())?.into_iter().find(|w| w.wid == wid),
        None => None,
    };
    let done = crate::win_calls::call_on(&host.windows, id, &perms, cmd, &args, surface, OS_WINDOWS).await?;
    match native::step(cmd, &args, &done.result, before.as_ref()) {
        Some(Step::Create { wid }) => open_native(&engine, host, id, &wid, monitor).await?,
        Some(Step::Close(n)) => engine.close(&n),
        Some(Step::State(n, s)) => engine.set_state(&n, s),
        Some(Step::Badge(n, b)) => engine.set_badge(&n, b.as_deref()),
        None => {}
    }
    if let Some(ev) = done.event {
        emit_win(app, id, ev);
    }
    let mut result = done.result;
    if cmd == crate::win_calls::LIST {
        add_memory(&engine, &mut result).await;
    }
    Ok(result)
}

/// WM-7 (POPOUT-1: users see what each window costs): `memoryMb` on each native window of an `os_win_list` answer, where the
/// engine tells it. Asked in parallel, each with a short wait, so a slow runtime never holds the list.
async fn add_memory(engine: &crate::native_tauri::TauriNative, result: &mut Value) {
    use crate::native::NativeWindows;
    let Some(m) = engine.memory_level() else { return };
    let Some(list) = result.as_array_mut() else { return };
    let mut asks = Vec::new();
    for (i, w) in list.iter().enumerate() {
        if w.get("kind").and_then(Value::as_str) != Some("native") {
            continue;
        }
        let Some(n) = w.get("wid").and_then(Value::as_str).and_then(crate::native::label_of) else { continue };
        let (tx, rx) = tokio::sync::oneshot::channel();
        m.memory_mb(&n, Box::new(move |mb| {
            let _ = tx.send(mb);
        }));
        asks.push((i, rx));
    }
    for (i, rx) in asks {
        if let Ok(Ok(Some(mb))) = tokio::time::timeout(std::time::Duration::from_millis(1500), rx).await {
            list[i]["memoryMb"] = json!(mb);
        }
    }
}

/// WM-6: put native window `wid` (already in the table) on screen: the OS host page with one frame of the app at its route, in the
/// app's own webview data directory (the same one `open_app_window` uses). A window that cannot be made leaves the table again.
async fn open_native(engine: &crate::native_tauri::TauriNative, host: &Host, id: &str, wid: &str, monitor: Option<String>) -> R<()> {
    use crate::native::{self, NativeWindows};
    let w = host.windows.list(Some(id)).await.map_err(|e| e.to_string())?.into_iter().find(|w| w.wid == wid).ok_or_else(|| format!("no window {wid}"))?;
    let label = native::label_of(wid).ok_or_else(|| format!("no window {wid}"))?;
    let made: R<native::NativeId> = async {
        let (paths, sup, pages, id2) = (host.paths.clone(), host.sup.clone(), host.pages.clone(), id.to_string());
        let page = blocking(move || crate::host::page_of(&paths, &sup, &pages, &id2)).await??;
        let data_dir = host.paths.webview_dir(id).map_err(|e| e.0)?;
        let route = w.route.clone().unwrap_or_else(|| "/".into());
        let script = native::host_script(id, wid, &w.title, &page.origin, &route);
        let title = if w.title.is_empty() { id.to_string() } else { w.title.clone() };
        let spec = native::NativeSpec { id: label, app: id.into(), wid: wid.into(), title, script, bounds: w.bounds, state: w.state, data_dir };
        engine.create(&spec)
    }
    .await;
    match made {
        Ok(_) => {
            if monitor.is_some() {
                let _ = host.windows.update(id, wid, crate::windows::Patch { monitor, ..Default::default() }).await;
            }
            Ok(())
        }
        Err(e) => {
            let _ = host.windows.close(id, wid).await;
            Err(format!("the window could not be opened: {e}"))
        }
    }
}

/// WM-6: app `id` stopped: its native windows close (their geometry stays saved) and its frames hear `closed` for each.
async fn close_native_of(app: &AppHandle, host: &Host, id: &str) {
    use crate::native::NativeWindows;
    let engine = crate::native_tauri::TauriNative(app.clone());
    for w in host.windows.app_stopped(id).await.unwrap_or_default() {
        // WM-8: a float lives in the shell, which drops it on `closed`; a native one is closed on screen here.
        if w.kind == crate::windows::Kind::Native {
            if let Some(n) = crate::native::label_of(&w.wid) {
                engine.close(&n);
            }
        }
        emit_win(app, id, json!({"wid": w.wid, "change": "closed"}));
    }
}

/// WM-6 `bos.windows.restoreOnStart`: app `id` started; reopen the native windows it had (route and geometry are saved).
async fn restore_native_of(app: &AppHandle, host: &Host, id: &str) {
    if !OS_WINDOWS || !crate::windows::WindowSettings::load(&host.paths).restore_on_start {
        return;
    }
    let saved = host.windows.saved(id).await.unwrap_or_default();
    for (key, s) in saved.into_iter().filter(|(_, s)| s.kind == crate::windows::Kind::Native && s.route.is_some()) {
        let args = json!({"key": key, "kind": "native", "title": s.title, "route": s.route});
        if let Err(e) = Box::pin(win_run(app, host, id, crate::win_calls::OPEN, args)).await {
            eprintln!("restore window {key} of {id}: {e}");
        }
    }
}

/// Open a window of the calling app (`key` already open: focus it). `{wid, kind, state, bounds, restored}`.
#[tauri::command]
pub async fn os_win_open(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, key: String, kind: Option<Value>, title: Option<String>, icon: Option<String>, route: Option<String>, bounds: Option<Value>, state: Option<Value>) -> R<Value> {
    let id = crate::access::authorize_win_caller(window.label(), "os_win_open").map_err(|e| e.to_string())?;
    let args = json!({"key": key, "kind": kind, "title": title.unwrap_or_default(), "icon": icon, "route": route, "bounds": bounds, "state": state});
    win_run(&app, &host, &id, crate::win_calls::OPEN, args).await
}

#[tauri::command]
pub async fn os_win_close(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, wid: String) -> R<Value> {
    let id = crate::access::authorize_win_caller(window.label(), "os_win_close").map_err(|e| e.to_string())?;
    win_run(&app, &host, &id, crate::win_calls::CLOSE, json!({"wid": wid})).await
}

#[tauri::command]
pub async fn os_win_minimize(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, wid: String) -> R<Value> {
    let id = crate::access::authorize_win_caller(window.label(), "os_win_minimize").map_err(|e| e.to_string())?;
    win_run(&app, &host, &id, crate::win_calls::MINIMIZE, json!({"wid": wid})).await
}

#[tauri::command]
pub async fn os_win_restore(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, wid: String) -> R<Value> {
    let id = crate::access::authorize_win_caller(window.label(), "os_win_restore").map_err(|e| e.to_string())?;
    win_run(&app, &host, &id, crate::win_calls::RESTORE, json!({"wid": wid})).await
}

#[tauri::command]
pub async fn os_win_maximize(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, wid: String) -> R<Value> {
    let id = crate::access::authorize_win_caller(window.label(), "os_win_maximize").map_err(|e| e.to_string())?;
    win_run(&app, &host, &id, crate::win_calls::MAXIMIZE, json!({"wid": wid})).await
}

#[tauri::command]
pub async fn os_win_focus(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, wid: String) -> R<Value> {
    let id = crate::access::authorize_win_caller(window.label(), "os_win_focus").map_err(|e| e.to_string())?;
    win_run(&app, &host, &id, crate::win_calls::FOCUS, json!({"wid": wid})).await
}

#[tauri::command]
pub async fn os_win_snap(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, wid: String, zone: String) -> R<Value> {
    let id = crate::access::authorize_win_caller(window.label(), "os_win_snap").map_err(|e| e.to_string())?;
    win_run(&app, &host, &id, crate::win_calls::SNAP, json!({"wid": wid, "zone": zone})).await
}

/// Title, icon, badge (`null` clears it), attention, and bounds (inline only). Absent fields stay as they are.
#[tauri::command]
pub async fn os_win_set(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, wid: String, title: Option<String>, icon: Option<String>, badge: Option<Value>, attention: Option<bool>, bounds: Option<Value>) -> R<Value> {
    let id = crate::access::authorize_win_caller(window.label(), "os_win_set").map_err(|e| e.to_string())?;
    let mut args = json!({"wid": wid});
    for (k, v) in [("title", title.map(Value::from)), ("icon", icon.map(Value::from)), ("badge", badge), ("attention", attention.map(Value::from)), ("bounds", bounds)] {
        if let Some(v) = v {
            args[k] = v;
        }
    }
    win_run(&app, &host, &id, crate::win_calls::SET, args).await
}

/// The calling app's own windows, in z order.
#[tauri::command]
pub async fn os_win_list(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    let id = crate::access::authorize_win_caller(window.label(), "os_win_list").map_err(|e| e.to_string())?;
    win_run(&app, &host, &id, crate::win_calls::LIST, json!({})).await
}

/// The shell's half for a page in a frame: `args` are the page's, the app `id` is the frame's. Main only.
async fn frame_win(app: &AppHandle, host: &State<'_, Host>, cmd: &'static str, id: &str, args: Option<Value>) -> R<Value> {
    let mut args = args.filter(|a| a.is_object()).unwrap_or_else(|| json!({}));
    if let Some(o) = args.as_object_mut() {
        o.remove("id"); // the app is the frame's, never the message's
    }
    win_run(app, host, id, cmd, args).await
}

#[tauri::command]
pub async fn os_frame_win_open(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_win_open").map_err(|e| e.to_string())?;
    frame_win(&app, &host, crate::win_calls::OPEN, &id, args).await
}

#[tauri::command]
pub async fn os_frame_win_close(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_win_close").map_err(|e| e.to_string())?;
    frame_win(&app, &host, crate::win_calls::CLOSE, &id, args).await
}

#[tauri::command]
pub async fn os_frame_win_minimize(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_win_minimize").map_err(|e| e.to_string())?;
    frame_win(&app, &host, crate::win_calls::MINIMIZE, &id, args).await
}

#[tauri::command]
pub async fn os_frame_win_restore(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_win_restore").map_err(|e| e.to_string())?;
    frame_win(&app, &host, crate::win_calls::RESTORE, &id, args).await
}

#[tauri::command]
pub async fn os_frame_win_maximize(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_win_maximize").map_err(|e| e.to_string())?;
    frame_win(&app, &host, crate::win_calls::MAXIMIZE, &id, args).await
}

#[tauri::command]
pub async fn os_frame_win_focus(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_win_focus").map_err(|e| e.to_string())?;
    frame_win(&app, &host, crate::win_calls::FOCUS, &id, args).await
}

#[tauri::command]
pub async fn os_frame_win_snap(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_win_snap").map_err(|e| e.to_string())?;
    frame_win(&app, &host, crate::win_calls::SNAP, &id, args).await
}

#[tauri::command]
pub async fn os_frame_win_set(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_win_set").map_err(|e| e.to_string())?;
    frame_win(&app, &host, crate::win_calls::SET, &id, args).await
}

#[tauri::command]
pub async fn os_frame_win_list(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, args: Option<Value>) -> R<Value> {
    authorize_shell(window.label(), "os_frame_win_list").map_err(|e| e.to_string())?;
    frame_win(&app, &host, crate::win_calls::LIST, &id, args).await
}

/// WM-8: the shell moved or resized float window `wid` of app `id` (drag, resize, snap-off). Main only: the OS draws floats, an app
/// never moves one itself. `{state, bounds}`; the app's frames hear `os_win` `state`.
#[tauri::command]
pub async fn os_shell_win_bounds(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, wid: String, bounds: battalion_app_api::geometry::Rect) -> R<Value> {
    authorize_shell(window.label(), "os_shell_win_bounds").map_err(|e| e.to_string())?;
    crate::access::app_label(&id).map_err(|e| e.to_string())?;
    let done = crate::win_calls::shell_bounds(&host.windows, &id, &wid, bounds, win_surface(&app)).await?;
    if let Some(ev) = done.event {
        emit_win(&app, &id, ev);
    }
    Ok(done.result)
}

/// WM-8 (owner 11-28): "Pop out to its own window" on a float's title bar: the float closes and a native window of the same key,
/// route and size opens (the WM-6 kind). Main only. Answers the native window's `os_win_open` result.
#[tauri::command]
pub async fn os_shell_win_pop_out(window: WebviewWindow, app: AppHandle, host: State<'_, Host>, id: String, wid: String) -> R<Value> {
    authorize_shell(window.label(), "os_shell_win_pop_out").map_err(|e| e.to_string())?;
    crate::access::app_label(&id).map_err(|e| e.to_string())?;
    if !OS_WINDOWS {
        return Err("this device has no windows of its own".into());
    }
    let (args, done) = crate::win_calls::pop_out(&host.windows, &id, &wid).await?;
    if let Some(ev) = done.event {
        emit_win(&app, &id, ev);
    }
    win_run(&app, &host, &id, crate::win_calls::OPEN, args).await
}

/// `stable` when the app has it, else its first channel.
fn default_channel(app: &battalion_app_api::CatalogApp) -> &str {
    if app.channels.contains_key("stable") { "stable" } else { app.channels.keys().next().map(String::as_str).unwrap_or("stable") }
}

// ---- LOCAL-REPO-1: local repos -----------------------------------------------------------------

fn repo_json(r: &battalion_host::armory::LocalRepo) -> Value {
    json!({"name": r.name, "path": r.dir.display().to_string(), "registered": r.registered})
}

#[tauri::command]
pub async fn os_local_repos(window: WebviewWindow, host: State<'_, Host>) -> R<Value> {
    authorize_shell(window.label(), "os_local_repos").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let repos = blocking(move || a.local_repos()).await?;
    Ok(json!({"repos": repos.iter().map(repo_json).collect::<Vec<_>>(), "label": battalion_host::armory::LOCAL_LABEL}))
}

#[tauri::command]
pub async fn os_local_repo_add(window: WebviewWindow, host: State<'_, Host>, name: String) -> R<Value> {
    authorize_shell(window.label(), "os_local_repo_add").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let r = blocking(move || a.add_local_repo(&name)).await?.map_err(|e| e.to_string())?;
    Ok(repo_json(&r))
}

#[tauri::command]
pub async fn os_local_repo_rename(window: WebviewWindow, host: State<'_, Host>, name: String, new_name: String) -> R<Value> {
    authorize_shell(window.label(), "os_local_repo_rename").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    let r = blocking(move || a.rename_local_repo(&name, &new_name)).await?.map_err(|e| e.to_string())?;
    Ok(repo_json(&r))
}

#[tauri::command]
pub async fn os_local_repo_remove(window: WebviewWindow, host: State<'_, Host>, name: String) -> R<Value> {
    authorize_shell(window.label(), "os_local_repo_remove").map_err(|e| e.to_string())?;
    let a = host.armory.clone();
    blocking(move || a.remove_local_repo(&name)).await?.map_err(|e| e.to_string())?;
    Ok(json!({"ok": true}))
}
