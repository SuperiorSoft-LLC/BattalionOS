// SPDX-License-Identifier: MPL-2.0
//! BOS-34: opening BattalionOS ends with its window in front, and a notification area icon (Open, Exit) while it runs.
//! X still only hides the window (access::close_hides_window); Exit is the same `app.exit(0)` as Start > Exit (`os_exit`).

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

pub const TRAY_OPEN: &str = "bos-open";
pub const TRAY_EXIT: &str = "bos-exit";

/// In a new start, before the single-instance hand-off: any process may take the foreground from this one.
/// Windows gives the foreground right to the process the user just started, not to the running OS it signals.
pub fn allow_any_to_take_foreground() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows_sys::Win32::UI::WindowsAndMessaging::ASFW_ANY);
    }
}

/// The main window shown, restored (not minimized) and in front. The shell keeps the app that was in front.
pub fn bring_main_to_front<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(crate::access::MAIN) {
        let _ = w.show();
        // A window hidden while minimized comes back minimized: restore after show.
        if w.is_minimized().unwrap_or(false) {
            let _ = w.unminimize();
        }
        let _ = w.set_focus();
    }
}

pub fn tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, TRAY_OPEN, "Open BattalionOS", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, TRAY_EXIT, "Exit BattalionOS", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &exit])?;
    let mut b = TrayIconBuilder::with_id("bos-tray")
        .tooltip(if battalion_host::dev::ENABLED { "BattalionOS (dev)" } else { "BattalionOS" })
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            TRAY_OPEN => bring_main_to_front(app),
            TRAY_EXIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|t, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                bring_main_to_front(t.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    b.build(app)?;
    Ok(())
}
