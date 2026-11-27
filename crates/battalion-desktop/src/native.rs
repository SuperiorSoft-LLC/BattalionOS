// SPDX-License-Identifier: MPL-2.0
//! OS-WINDOW-1 (WM-6): the `native` window kind. A native window is an OS window of its own, labelled `win-<app>-<n>` (the `<n>` of
//! its wid `w-<app>-<n>`), that loads the OS host page (`window.html`: the `WindowFrame` bar plus ONE frame of the app at `route`,
//! on the app's origin). The engine is behind [`NativeWindows`] (the engine seam of ENGINE-SEAM-AUDIT-1): the window manager, the
//! taskbar and the `os_win_*` calls never see Tauri. The Tauri/wry implementation is `host::TauriNative` (feature `gui`).
//!
//! Everything here is pure (no engine), so label and identity, the step each call takes, the monitor a window opens on and the
//! host page url are tested without a window.

use crate::windows::{Saved, WinState, Window};
use battalion_app_api::geometry::{Rect, Surface};
use serde_json::Value;
use std::path::PathBuf;

/// The OS label of a native window: `win-<app>-<n>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NativeId(pub String);

/// What the engine needs to make one native window.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeSpec {
    pub id: NativeId,
    pub app: String,
    pub wid: String,
    pub title: String,
    /// The identity script of the host page ([`host_script`]).
    pub script: String,
    /// Screen coordinates, logical pixels.
    pub bounds: Rect,
    pub state: WinState,
    /// The app's own webview data directory (the same one `open_app_window` uses), so the content is not a second login.
    pub data_dir: PathBuf,
}

/// WM-7 sets a hidden native window's memory target through this; an engine without one answers `None`.
pub trait MemoryLevel: Send + Sync {
    /// LOW (`true`) or NORMAL. Fire and forget: done on the engine's own thread.
    fn set_low(&self, id: &NativeId, low: bool) -> Result<(), String>;
    /// What the window's own content costs, in MB, when the runtime tells it (WebView2: the working set of the renderer
    /// processes that hold its frames). `reply` is called once, with `None` when it is not known.
    fn memory_mb(&self, id: &NativeId, reply: Box<dyn FnOnce(Option<u64>) + Send>);
}

/// The engine seam for native windows. Every call is made from a tokio task; an implementation does its window work on the
/// engine's main-thread event loop and never blocks it.
pub trait NativeWindows: Send + Sync {
    /// Make the window, or show and focus it when `spec.id` is already open (focus instead of a duplicate).
    fn create(&self, spec: &NativeSpec) -> Result<NativeId, String>;
    fn close(&self, id: &NativeId);
    /// Normal (show, unminimize, focus), minimized, maximized, or a snap zone of the monitor the window is on.
    fn set_state(&self, id: &NativeId, s: WinState);
    /// Move the window with the pointer (the bar's drag region); Windows snap applies.
    fn start_drag(&self, id: &NativeId);
    /// The window's own taskbar button: the overlay badge (`None` clears it).
    fn set_badge(&self, id: &NativeId, badge: Option<&str>);
    fn memory_level(&self) -> Option<&dyn MemoryLevel>;
    /// The attached monitors, the primary first.
    fn monitors(&self) -> Vec<Monitor>;
}

/// `w-<app>-<n>` → `win-<app>-<n>`.
pub fn label_of(wid: &str) -> Option<NativeId> {
    let app = crate::windows::wid_app(wid)?;
    let n = &wid[2 + app.len() + 1..];
    Some(NativeId(format!("{}{app}-{n}", crate::access::WIN_PREFIX)))
}

/// `win-<app>-<n>` → (`app`, `w-<app>-<n>`): the wid a native window's frame speaks for is known from its label.
pub fn wid_of_label(label: &str) -> Option<(String, String)> {
    let rest = label.strip_prefix(crate::access::WIN_PREFIX)?;
    let wid = format!("w-{rest}");
    let app = crate::windows::wid_app(&wid)?.to_string();
    Some((app, wid))
}

/// The engine step one `os_win_*` call takes for a native window, from the call and its answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// `os_win_open` answered `kind: "native"`: make it (or focus the open one).
    Create { wid: String },
    Close(NativeId),
    State(NativeId, WinState),
    Badge(NativeId, Option<String>),
}

/// What to do on screen after `cmd` ran with `args` and answered `result`. `native` is the window as the table had it (for the
/// calls that name a wid), `None` when the wid is not a native window: then nothing happens on screen.
pub fn step(cmd: &str, args: &Value, result: &Value, native: Option<&Window>) -> Option<Step> {
    use crate::win_calls as c;
    if cmd == c::OPEN {
        return (result.get("kind").and_then(Value::as_str) == Some("native")).then(|| Step::Create { wid: result["wid"].as_str().unwrap_or_default().to_string() });
    }
    let w = native.filter(|w| w.kind == crate::windows::Kind::Native)?;
    let id = label_of(&w.wid)?;
    match cmd {
        c::CLOSE => Some(Step::Close(id)),
        c::MINIMIZE => Some(Step::State(id, WinState::Minimized)),
        c::MAXIMIZE => Some(Step::State(id, WinState::Maximized)),
        c::RESTORE | c::FOCUS => Some(Step::State(id, WinState::Normal)),
        c::SNAP => serde_json::from_value(args.get("zone")?.clone()).ok().map(|z| Step::State(id, WinState::Snap(z))),
        c::SET => {
            let b = args.get("badge")?;
            let text = if b.is_null() { None } else { crate::badge::parse(b.get("count").and_then(Value::as_u64), b.get("text").and_then(Value::as_str)).ok()? };
            Some(Step::Badge(id, text))
        }
        _ => None,
    }
}

/// One monitor: its OS name and its work area (logical pixels, screen coordinates).
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub work: Surface,
}

/// The monitor a native window opens on: the saved one while it is still attached, else the first (the primary). Its work area is
/// the surface the saved bounds are clamped to, so a window whose monitor is gone comes back on screen.
pub fn pick_monitor<'a>(saved: Option<&Saved>, monitors: &'a [Monitor]) -> Option<&'a Monitor> {
    let want = saved.and_then(|s| s.monitor.as_deref());
    want.and_then(|n| monitors.iter().find(|m| m.name == n)).or_else(|| monitors.first())
}

/// The bounds a new native window is offered when nothing is saved: centred on the monitor's work area.
pub fn default_bounds(m: &Surface) -> Rect {
    let (w, h) = (900f64.min(m.w), 700f64.min(m.h));
    Rect { x: m.x + (m.w - w) / 2.0, y: m.y + (m.h - h) / 2.0, w, h }
}

/// The OS host page every native window loads (bundled with the shell, `ui/window.html`).
pub const HOST_PAGE: &str = "window.html";

/// The script the host page finds before it runs (`window.__BOS_WIN`): the app, the wid and the frame it shows. `src` is the app's
/// origin joined with the window's route (a path on that origin, checked by `win_calls::route`). Run in the top frame only.
pub fn host_script(app: &str, wid: &str, title: &str, origin: &str, route: &str) -> String {
    let src = format!("{}{route}", origin.trim_end_matches('/'));
    let origin = origin.trim_end_matches('/');
    format!("window.__BOS_WIN={};", serde_json::json!({"app": app, "wid": wid, "title": title, "origin": origin, "src": src}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::{Kind, Windows, WindowSettings, OpenSpec};
    use battalion_app_api::geometry::Snap;
    use battalion_host::paths::Paths;
    use serde_json::json;
    use std::sync::Mutex;

    fn native(wid: &str) -> Window {
        Window { wid: wid.into(), app: "infantry".into(), key: "k".into(), kind: Kind::Native, title: "T".into(), icon: None, badge: None, attention: false, state: WinState::Normal, bounds: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }, z: 1, route: Some("/s".into()) }
    }

    #[test]
    fn label_and_identity_round_trip() {
        assert_eq!(label_of("w-infantry-3"), Some(NativeId("win-infantry-3".into())));
        assert_eq!(label_of("w-my-app-12"), Some(NativeId("win-my-app-12".into())));
        assert_eq!(wid_of_label("win-my-app-12"), Some(("my-app".into(), "w-my-app-12".into())));
        // The label the window carries names the same app the WM-2 access rule reads from it.
        assert_eq!(crate::access::authorize_win_caller("win-my-app-12", "os_win_list").unwrap(), "my-app");
        for bad in ["x-1", "w-", "w-app", "w-app-", "w-app-1a"] {
            assert_eq!(label_of(bad), None, "{bad}");
        }
        assert_eq!(wid_of_label("app-infantry"), None);
        assert_eq!(wid_of_label("win-infantry"), None);
    }

    #[test]
    fn each_call_takes_its_step_only_for_a_native_window() {
        let n = native("w-infantry-2");
        let id = NativeId("win-infantry-2".into());
        assert_eq!(step("os_win_open", &json!({}), &json!({"wid": "w-infantry-2", "kind": "native"}), None), Some(Step::Create { wid: "w-infantry-2".into() }));
        assert_eq!(step("os_win_open", &json!({}), &json!({"wid": "w-infantry-2", "kind": "inline"}), None), None);
        assert_eq!(step("os_win_close", &json!({}), &json!(null), Some(&n)), Some(Step::Close(id.clone())));
        assert_eq!(step("os_win_minimize", &json!({}), &json!({}), Some(&n)), Some(Step::State(id.clone(), WinState::Minimized)));
        assert_eq!(step("os_win_focus", &json!({}), &json!({}), Some(&n)), Some(Step::State(id.clone(), WinState::Normal)));
        assert_eq!(step("os_win_snap", &json!({"zone": "left"}), &json!({}), Some(&n)), Some(Step::State(id.clone(), WinState::Snap(Snap::Left))));
        assert_eq!(step("os_win_set", &json!({"badge": {"count": 120}}), &json!(null), Some(&n)), Some(Step::Badge(id.clone(), Some("99+".into()))));
        assert_eq!(step("os_win_set", &json!({"badge": null}), &json!(null), Some(&n)), Some(Step::Badge(id, None)));
        assert_eq!(step("os_win_set", &json!({"title": "x"}), &json!(null), Some(&n)), None);
        let inline = Window { kind: Kind::Inline, ..n };
        assert_eq!(step("os_win_close", &json!({}), &json!(null), Some(&inline)), None, "an inline window has no OS window");
    }

    #[test]
    fn clamping_when_the_saved_monitor_is_gone() {
        let primary = Monitor { name: "DISPLAY1".into(), work: Surface { x: 0.0, y: 0.0, w: 1920.0, h: 1040.0 } };
        let second = Monitor { name: "DISPLAY2".into(), work: Surface { x: 1920.0, y: 0.0, w: 2560.0, h: 1400.0 } };
        let saved = Saved { kind: Kind::Native, bounds: Rect { x: 3000.0, y: 200.0, w: 1200.0, h: 900.0 }, state: WinState::Normal, monitor: Some("DISPLAY2".into()), route: Some("/s".into()), title: String::new() };
        let both = [primary.clone(), second.clone()];
        assert_eq!(pick_monitor(Some(&saved), &both).unwrap().name, "DISPLAY2", "the saved monitor while it is attached");
        // The second monitor is unplugged: the primary is used and the saved bounds are clamped onto it.
        let only = [primary.clone()];
        let m = pick_monitor(Some(&saved), &only).unwrap();
        assert_eq!(m.name, "DISPLAY1");
        let b = battalion_app_api::geometry::clamp_box(saved.bounds, m.work);
        assert!(b.x + 80.0 <= 1920.0 && b.w <= 1920.0 && b.h <= 1040.0, "{b:?}");
        assert!(pick_monitor(None, &[]).is_none());
        let d = default_bounds(&primary.work);
        assert_eq!((d.w, d.h, d.x, d.y), (900.0, 700.0, 510.0, 170.0));
    }

    #[test]
    fn the_host_page_carries_the_frame_identity() {
        let p = host_script("infantry", "w-infantry-1", "Chat \"</script>", "http://127.0.0.1:4100/", "/session/e7?x=2");
        let v: serde_json::Value = serde_json::from_str(p.strip_prefix("window.__BOS_WIN=").unwrap().trim_end_matches(';')).unwrap();
        assert_eq!(v, json!({"app": "infantry", "wid": "w-infantry-1", "title": "Chat \"</script>", "origin": "http://127.0.0.1:4100", "src": "http://127.0.0.1:4100/session/e7?x=2"}));
    }

    /// A fake engine: records what the OS asked of it.
    #[derive(Default)]
    struct Fake(Mutex<Vec<String>>);
    impl NativeWindows for Fake {
        fn create(&self, s: &NativeSpec) -> Result<NativeId, String> {
            self.0.lock().unwrap().push(format!("create {}", s.id.0));
            Ok(s.id.clone())
        }
        fn close(&self, id: &NativeId) {
            self.0.lock().unwrap().push(format!("close {}", id.0));
        }
        fn set_state(&self, id: &NativeId, s: WinState) {
            self.0.lock().unwrap().push(format!("state {} {s:?}", id.0));
        }
        fn start_drag(&self, _: &NativeId) {}
        fn set_badge(&self, _: &NativeId, _: Option<&str>) {}
        fn memory_level(&self) -> Option<&dyn MemoryLevel> {
            None
        }
        fn monitors(&self) -> Vec<Monitor> {
            vec![]
        }
    }

    fn start(p: &Paths) -> Windows {
        let (w, fut) = Windows::start(p.clone(), WindowSettings { save_debounce_ms: 10_000, ..Default::default() });
        tokio::spawn(fut);
        w
    }
    fn spec(key: &str) -> OpenSpec {
        OpenSpec { key: key.into(), kind: Some(Kind::Native), route: Some("/s".into()), title: "S".into(), ..Default::default() }
    }
    const BIG: Surface = Surface { x: 0.0, y: 0.0, w: 2000.0, h: 1200.0 };

    #[tokio::test]
    async fn focus_instead_of_a_duplicate_by_key() {
        let d = tempfile::tempdir().unwrap();
        let wm = start(&Paths::new(d.path()));
        let a = wm.open("infantry", spec("session.1"), BIG).await.unwrap();
        let b = wm.open("infantry", spec("session.1"), BIG).await.unwrap();
        // The same wid, so the same label: the engine's create shows the open window instead of a second one.
        assert_eq!(label_of(&a.wid), label_of(&b.wid));
        assert_eq!(wm.list(Some("infantry")).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn native_windows_close_when_their_app_stops() {
        let d = tempfile::tempdir().unwrap();
        let wm = start(&Paths::new(d.path()));
        let n = wm.open("infantry", spec("session.1"), BIG).await.unwrap();
        wm.open("infantry", OpenSpec { key: "inline".into(), ..Default::default() }, BIG).await.unwrap();
        let other = wm.open("other", spec("x"), BIG).await.unwrap();
        let engine = Fake::default();
        for w in wm.app_stopped("infantry").await.unwrap() {
            engine.close(&label_of(&w.wid).unwrap());
        }
        assert_eq!(*engine.0.lock().unwrap(), vec![format!("close win-infantry-{}", &n.wid["w-infantry-".len()..])]);
        let left: Vec<String> = wm.list(None).await.unwrap().into_iter().map(|w| w.wid).collect();
        assert!(left.contains(&other.wid) && !left.contains(&n.wid) && left.len() == 2, "{left:?}");
        // Its geometry and route stay saved for the next start.
        wm.flush().await.unwrap();
        let saved = wm.saved("infantry").await.unwrap();
        assert_eq!(saved["session.1"].route.as_deref(), Some("/s"));
    }
}
