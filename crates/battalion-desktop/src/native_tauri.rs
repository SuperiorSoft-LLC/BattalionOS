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
                WindowEvent::Resized(_) | WindowEvent::Focused(true) => {
                    // WM-7: minimized or hidden starts the low-memory timer; shown again (restore, focus) cancels it or sets NORMAL.
                    let hidden = win.is_minimized().unwrap_or(false) || !win.is_visible().unwrap_or(true);
                    low_memory(&handle, &host.paths, win.label(), hidden);
                    if let Some((bounds, monitor)) = placed(&win) {
                        tauri::async_runtime::spawn(async move {
                            let _ = wm.update(&app, &wid, Patch { bounds: Some(bounds), monitor, ..Default::default() }).await;
                        });
                    }
                }
                WindowEvent::Moved(_) => {
                    if let Some((bounds, monitor)) = placed(&win) {
                        tauri::async_runtime::spawn(async move {
                            let _ = wm.update(&app, &wid, Patch { bounds: Some(bounds), monitor, ..Default::default() }).await;
                        });
                    }
                }
                WindowEvent::Destroyed => {
                    lowmem().lock().unwrap_or_else(|e| e.into_inner()).forget(win.label());
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
        #[cfg(windows)]
        return Some(self);
        #[cfg(not(windows))]
        None // WebKitGTK / WKWebView: no memory target
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

/// WM-7: the low-memory timer of every native window of this OS (one OS per process).
#[cfg(desktop)]
fn lowmem() -> &'static std::sync::Mutex<crate::lowmem::LowMem> {
    static M: std::sync::OnceLock<std::sync::Mutex<crate::lowmem::LowMem>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// WM-7: native window `label` was hidden (minimized) or shown. Hidden for `bos.windows.lowMemoryAfterSec` -> LOW; shown -> NORMAL.
#[cfg(desktop)]
fn low_memory(app: &AppHandle, paths: &battalion_host::Paths, label: &str, hidden: bool) {
    let id = NativeId(label.to_string());
    if !hidden {
        if lowmem().lock().unwrap_or_else(|e| e.into_inner()).show(label) {
            if let Some(m) = TauriNative(app.clone()).memory_level() {
                let _ = m.set_low(&id, false);
            }
        }
        return;
    }
    let after = crate::windows::WindowSettings::load(paths).low_memory_after_sec;
    let Some(wait) = lowmem().lock().unwrap_or_else(|e| e.into_inner()).hide(label, std::time::Instant::now(), after) else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(wait).await;
        if lowmem().lock().unwrap_or_else(|e| e.into_inner()).due(&id.0, std::time::Instant::now(), after) {
            let engine = TauriNative(app);
            if let Some(m) = engine.memory_level() {
                if let Err(e) = m.set_low(&id, true) {
                    eprintln!("low memory {}: {e}", id.0);
                }
            }
        }
    });
}

/// WM-7: WebView2's memory target and the window's renderer memory, on the main thread through `with_webview`.
#[cfg(all(desktop, windows))]
impl MemoryLevel for TauriNative {
    fn set_low(&self, id: &NativeId, low: bool) -> Result<(), String> {
        let w = self.get(id).ok_or_else(|| format!("no window {}", id.0))?;
        let label = id.0.clone();
        w.with_webview(move |wv| {
            use webview2_com::Microsoft::Web::WebView2::Win32::*;
            use windows_core::Interface;
            let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
            // SAFETY: COM calls on the webview's own (main) thread, which `with_webview` runs us on.
            let r = unsafe { wv.controller().CoreWebView2().and_then(|c| c.cast::<ICoreWebView2_19>()).and_then(|c| c.SetMemoryUsageTargetLevel(level)) };
            if let Err(e) = r {
                // An older WebView2 runtime has no ICoreWebView2_19: nothing to do.
                eprintln!("memory target {label}: {e}");
            }
        })
        .map_err(|e| e.to_string())
    }

    fn memory_mb(&self, id: &NativeId, reply: Box<dyn FnOnce(Option<u64>) + Send>) {
        let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(reply)));
        let answer = move |mb: Option<u64>| {
            if let Some(r) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
                r(mb)
            }
        };
        let Some(w) = self.get(id) else { return answer(None) };
        let a2 = answer.clone();
        let sent = w.with_webview(move |wv| {
            let a3 = a2.clone();
            if renderer_memory(&wv, Box::new(a3)).is_err() {
                a2(None);
            }
        });
        if sent.is_err() {
            answer(None);
        }
    }
}

/// The working set (MB) of the renderer processes whose frames belong to this webview (its main frame or a frame under it).
#[cfg(all(desktop, windows))]
fn renderer_memory(wv: &tauri::webview::PlatformWebview, reply: Box<dyn FnOnce(Option<u64>)>) -> windows_core::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows_core::Interface;
    // SAFETY: COM calls on the webview's main thread (`with_webview`); the completion handler runs there too.
    unsafe {
        let mut main = 0u32;
        wv.controller().CoreWebView2()?.cast::<ICoreWebView2_20>()?.FrameId(&mut main)?;
        let env = wv.environment().cast::<ICoreWebView2Environment13>()?;
        let handler = webview2_com::GetProcessExtendedInfosCompletedHandler::create(Box::new(move |hr, list| {
            let mut pids: Vec<i32> = Vec::new();
            if let (true, Some(list)) = (hr.is_ok(), list) {
                let mut n = 0u32;
                list.Count(&mut n)?;
                for i in 0..n {
                    let ext = list.GetValueAtIndex(i)?;
                    let info = ext.ProcessInfo()?;
                    let (mut kind, mut pid) = (COREWEBVIEW2_PROCESS_KIND::default(), 0i32);
                    info.Kind(&mut kind)?;
                    info.ProcessId(&mut pid)?;
                    if kind != COREWEBVIEW2_PROCESS_KIND_RENDERER {
                        continue;
                    }
                    let it = ext.AssociatedFrameInfos()?.GetIterator()?;
                    let mut more = windows_core::BOOL::default();
                    it.HasCurrent(&mut more)?;
                    while more.as_bool() {
                        if root_frame(it.GetCurrent()?) == Some(main) && !pids.contains(&pid) {
                            pids.push(pid);
                        }
                        it.MoveNext(&mut more)?;
                    }
                }
            }
            let bytes: u64 = pids.iter().filter_map(|p| working_set(*p as u32)).sum();
            reply((!pids.is_empty()).then_some(bytes / (1024 * 1024)));
            Ok(())
        }));
        env.GetProcessExtendedInfos(&handler)
    }
}

/// The id of the top frame a frame sits under (its own id when it is a main frame).
#[cfg(all(desktop, windows))]
fn root_frame(f: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2FrameInfo) -> Option<u32> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows_core::Interface;
    // SAFETY: COM calls on the webview's main thread.
    unsafe {
        let mut f = f.cast::<ICoreWebView2FrameInfo2>().ok()?;
        while let Ok(p) = f.ParentFrameInfo() {
            f = p.cast::<ICoreWebView2FrameInfo2>().ok()?;
        }
        let mut id = 0u32;
        f.FrameId(&mut id).ok()?;
        Some(id)
    }
}

/// A process's working set in bytes.
#[cfg(all(desktop, windows))]
fn working_set(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: a handle opened and closed here; the counters struct is sized for the call.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = K32GetProcessMemoryInfo(h, &mut c, c.cb) != 0;
        CloseHandle(h);
        ok.then_some(c.WorkingSetSize as u64)
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
