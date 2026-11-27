// SPDX-License-Identifier: MPL-2.0
//! OS-WINDOW-1 (WM-6): the Tauri/wry engine behind `native::NativeWindows`, the only one today. Called from async commands (tokio
//! workers): every window call here is dispatched by Tauri to the main-thread event loop, and nothing blocking runs there. Never
//! call it from a sync command (building a window from one deadlocks on Windows).

use crate::native::{MemoryLevel, Monitor, NativeId, NativeSpec, NativeWindows};
use crate::windows::{Patch, WinState};
use battalion_app_api::geometry::{snap_box, Rect, Surface};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

pub struct TauriNative(pub AppHandle);

#[cfg(desktop)]
fn logical(m: &tauri::Monitor) -> Monitor {
    let f = m.scale_factor();
    let a = m.work_area();
    Monitor {
        name: m.name().cloned().unwrap_or_default(),
        work: Surface { x: a.position.x as f64 / f, y: a.position.y as f64 / f, w: a.size.width as f64 / f, h: a.size.height as f64 / f },
    }
}

/// The window's bounds on screen (logical pixels) and its monitor, when it is in its normal state.
#[cfg(desktop)]
fn placed(w: &WebviewWindow) -> Option<(Rect, Option<String>)> {
    if w.is_maximized().unwrap_or(false) || w.is_minimized().unwrap_or(false) {
        return None;
    }
    let f = w.scale_factor().ok()?;
    let p = w.outer_position().ok()?;
    let s = w.outer_size().ok()?;
    let name = w.current_monitor().ok().flatten().and_then(|m| m.name().cloned());
    Some((Rect { x: p.x as f64 / f, y: p.y as f64 / f, w: s.width as f64 / f, h: s.height as f64 / f }, name))
}

#[cfg(desktop)]
impl TauriNative {
    fn get(&self, id: &NativeId) -> Option<WebviewWindow> {
        self.0.get_webview_window(&id.0)
    }

    /// Follow the window the person moves: its bounds and monitor go to the window table (debounced there); when it is closed from
    /// the OS (Alt+F4, the taskbar), it leaves the table and its app hears `closed`.
    fn follow(&self, w: &WebviewWindow, app: String, wid: String) {
        let (handle, win) = (self.0.clone(), w.clone());
        w.on_window_event(move |ev| {
            let Some(host) = handle.try_state::<crate::host::Host>() else { return };
            let wm = host.windows.clone();
            let (app, wid) = (app.clone(), wid.clone());
            match ev {
                WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                    if let Some((bounds, monitor)) = placed(&win) {
                        tauri::async_runtime::spawn(async move {
                            let _ = wm.update(&app, &wid, Patch { bounds: Some(bounds), monitor, ..Default::default() }).await;
                        });
                    }
                }
                WindowEvent::Destroyed => {
                    let h = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        // Already gone from the table when the OS closed it (os_win_close, the app stopped): nothing to tell.
                        if wm.close(&app, &wid).await.is_ok() {
                            crate::commands::emit_win(&h, &app, serde_json::json!({"wid": wid, "change": "closed"}));
                        }
                    });
                }
                _ => {}
            }
        });
    }
}

#[cfg(desktop)]
impl NativeWindows for TauriNative {
    fn create(&self, s: &NativeSpec) -> Result<NativeId, String> {
        if let Some(w) = self.get(&s.id) {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            return Ok(s.id.clone());
        }
        std::fs::create_dir_all(&s.data_dir).map_err(|e| e.to_string())?;
        let w = WebviewWindowBuilder::new(&self.0, &s.id.0, WebviewUrl::App(crate::native::HOST_PAGE.into()))
            .title(&s.title)
            .decorations(false)
            .inner_size(s.bounds.w, s.bounds.h)
            .position(s.bounds.x, s.bounds.y)
            .data_directory(s.data_dir.clone())
            .initialization_script(&s.script)
            .build()
            .map_err(|e| e.to_string())?;
        if s.state != WinState::Normal {
            self.set_state(&s.id, s.state);
        }
        self.follow(&w, s.app.clone(), s.wid.clone());
        Ok(s.id.clone())
    }

    fn close(&self, id: &NativeId) {
        if let Some(w) = self.get(id) {
            let _ = w.destroy();
        }
    }

    fn set_state(&self, id: &NativeId, s: WinState) {
        let Some(w) = self.get(id) else { return };
        match s {
            WinState::Minimized => {
                let _ = w.minimize();
            }
            WinState::Maximized => {
                let _ = w.maximize();
            }
            WinState::Normal => {
                let _ = w.unmaximize();
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
            WinState::Snap(z) => {
                let Some(m) = w.current_monitor().ok().flatten() else { return };
                let b = snap_box(z, logical(&m).work);
                let _ = w.unmaximize();
                let _ = w.set_position(LogicalPosition::new(b.x, b.y));
                let _ = w.set_size(LogicalSize::new(b.w, b.h));
            }
        }
    }

    fn start_drag(&self, id: &NativeId) {
        if let Some(w) = self.get(id) {
            let _ = w.start_dragging();
        }
    }

    fn set_badge(&self, id: &NativeId, badge: Option<&str>) {
        let Some(w) = self.get(id) else { return };
        crate::commands::paint_window_badge(&w, badge);
    }

    fn memory_level(&self) -> Option<&dyn MemoryLevel> {
        None // WM-7
    }

    fn monitors(&self) -> Vec<Monitor> {
        let primary = self.0.primary_monitor().ok().flatten();
        let mut v: Vec<Monitor> = primary.iter().map(logical).collect();
        for m in self.0.available_monitors().unwrap_or_default() {
            let m = logical(&m);
            if !v.iter().any(|x| x.name == m.name) {
                v.push(m);
            }
        }
        v
    }
}

/// A phone has no windows of its own: `native` opens as a slot view (`win_calls::kind_used`), so nothing reaches the engine.
#[cfg(mobile)]
impl NativeWindows for TauriNative {
    fn create(&self, _: &NativeSpec) -> Result<NativeId, String> {
        Err("this OS has no windows of its own".into())
    }
    fn close(&self, _: &NativeId) {}
    fn set_state(&self, _: &NativeId, _: WinState) {}
    fn start_drag(&self, _: &NativeId) {}
    fn set_badge(&self, _: &NativeId, _: Option<&str>) {}
    fn memory_level(&self) -> Option<&dyn MemoryLevel> {
        None
    }
    fn monitors(&self) -> Vec<Monitor> {
        Vec::new()
    }
}
