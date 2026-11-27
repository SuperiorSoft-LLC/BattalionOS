// SPDX-License-Identifier: MPL-2.0
//! OS-WINDOW-1 (WM-2): the `os_win_*` calls, as pure rules over the `WindowManager` (no Tauri, tested without a window).
//! The caller has already been named from its window label or its frame (never from the arguments); here each call checks the
//! `window` permission of the app's installed manifest (read by the caller at every call), the wid rule (an app names only its own
//! wids, `access::authorize_wid`) and the arguments, then sends to the actor. It returns the result and the `os_win` event the
//! owning app's frames get (`{bos:1, event:"os_win", wid, change, ...}`; the host adds the envelope).
//! There is no cap on the number of windows (`bos.windows.max` stays unset).

use crate::windows::{Badge, Kind, OpenSpec, Patch, WinState, Window, Windows};
use battalion_app_api::geometry::{snap_box, Rect, Snap, Surface};
use serde::Deserialize;
use serde_json::{json, Value};

pub const OPEN: &str = "os_win_open";
pub const CLOSE: &str = "os_win_close";
pub const MINIMIZE: &str = "os_win_minimize";
pub const RESTORE: &str = "os_win_restore";
pub const MAXIMIZE: &str = "os_win_maximize";
pub const FOCUS: &str = "os_win_focus";
pub const SNAP: &str = "os_win_snap";
pub const SET: &str = "os_win_set";
pub const LIST: &str = "os_win_list";
/// Every page-facing window call (frozen in docs/commands.md). The shell serves each as `os_frame_win_<x>`.
pub const COMMANDS: &[&str] = &[OPEN, CLOSE, MINIMIZE, RESTORE, MAXIMIZE, FOCUS, SNAP, SET, LIST];

/// The answer of one call and the event it causes (to the owning app's frames only).
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub result: Value,
    pub event: Option<Value>,
}

/// A route a `float`/`native` window may load: a path on the app's own origin (`/...`), no scheme, host, backslash, space or
/// control character, at most 2048 bytes.
pub fn route(r: &str) -> Result<String, String> {
    let ok = r.starts_with('/') && !r.starts_with("//") && r.len() <= 2048 && !r.contains('\\') && !r.chars().any(|c| c.is_control() || c.is_whitespace());
    if ok {
        Ok(r.to_string())
    } else {
        Err(format!("route '{r}' is not a path on the app's own origin"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenArgs {
    key: String,
    kind: Option<Kind>,
    #[serde(default)]
    title: String,
    icon: Option<String>,
    route: Option<String>,
    bounds: Option<Rect>,
    state: Option<WinState>,
}

#[derive(Deserialize)]
struct SetArgs {
    title: Option<String>,
    icon: Option<String>,
    /// `badge: null` clears the badge; an absent badge leaves it.
    #[serde(default, deserialize_with = "present")]
    badge: Option<Option<Badge>>,
    attention: Option<bool>,
    bounds: Option<Rect>,
}
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<Badge>>, D::Error> {
    Option::<Badge>::deserialize(d).map(Some)
}

fn arg<T: for<'de> Deserialize<'de>>(cmd: &str, args: &Value) -> Result<T, String> {
    serde_json::from_value(args.clone()).map_err(|e| format!("{cmd}: {e}"))
}

fn wid_of(cmd: &str, app: &str, args: &Value) -> Result<String, String> {
    let wid = args.get("wid").and_then(|w| w.as_str()).ok_or_else(|| format!("{cmd} needs a wid"))?;
    crate::access::authorize_wid(app, wid).map_err(|e| e.to_string())?;
    Ok(wid.to_string())
}

fn state_event(w: &Window) -> Value {
    json!({"wid": w.wid, "change": "state", "state": w.state, "bounds": w.bounds})
}

async fn find(wm: &Windows, app: &str, wid: &str) -> Result<Option<Window>, String> {
    Ok(wm.list(Some(app)).await.map_err(|e| e.to_string())?.into_iter().find(|w| w.wid == wid))
}

/// Run window call `cmd` for app `app` (whose manifest `permissions` the caller read just now). `surface` is the area the window
/// lives in (it clamps a restore and gives a snap its halves and quarters).
pub async fn call(wm: &Windows, app: &str, permissions: &[String], cmd: &str, args: &Value, surface: Surface) -> Result<Done, String> {
    if !COMMANDS.contains(&cmd) {
        return Err(format!("'{cmd}' is not a window call"));
    }
    crate::frame_calls::require(app, permissions, cmd)?;
    let e = |e: crate::windows::WinError| e.to_string();
    let empty = json!({});
    let args = if args.is_null() { &empty } else { args };
    match cmd {
        OPEN => {
            let a: OpenArgs = arg(cmd, args)?;
            if a.key.trim().is_empty() {
                return Err("os_win_open needs a key".into());
            }
            let route = a.route.as_deref().map(route).transpose()?;
            let spec = OpenSpec { key: a.key, kind: a.kind, title: a.title, icon: a.icon, route, bounds: a.bounds, state: a.state, monitor: None };
            let (w, restored) = wm.open_restored(app, spec, surface).await.map_err(e)?;
            Ok(Done { result: json!({"wid": w.wid, "kind": w.kind, "state": w.state, "bounds": w.bounds, "restored": restored}), event: None })
        }
        CLOSE => {
            let wid = wid_of(cmd, app, args)?;
            wm.close(app, &wid).await.map_err(e)?;
            Ok(Done { result: Value::Null, event: Some(json!({"wid": wid, "change": "closed"})) })
        }
        MINIMIZE | RESTORE | MAXIMIZE => {
            let wid = wid_of(cmd, app, args)?;
            let state = match cmd {
                MINIMIZE => WinState::Minimized,
                MAXIMIZE => WinState::Maximized,
                _ => WinState::Normal,
            };
            let w = wm.update(app, &wid, Patch { state: Some(state), ..Default::default() }).await.map_err(e)?;
            Ok(Done { result: json!({"state": w.state}), event: Some(state_event(&w)) })
        }
        FOCUS => {
            let wid = wid_of(cmd, app, args)?;
            // Focus restores a minimized window, then raises it to the top of its surface.
            if find(wm, app, &wid).await?.is_some_and(|w| w.state == WinState::Minimized) {
                wm.update(app, &wid, Patch { state: Some(WinState::Normal), ..Default::default() }).await.map_err(e)?;
            }
            let w = wm.focus(app, &wid).await.map_err(e)?;
            Ok(Done { result: json!({"state": w.state}), event: Some(json!({"wid": w.wid, "change": "focus", "z": w.z, "state": w.state})) })
        }
        SNAP => {
            let wid = wid_of(cmd, app, args)?;
            let zone: Snap = args
                .get("zone")
                .and_then(|z| serde_json::from_value(z.clone()).ok())
                .ok_or("os_win_snap needs a zone: left, right, tl, tr, bl or br")?;
            let b = snap_box(zone, surface);
            let w = wm.update(app, &wid, Patch { state: Some(WinState::Snap(zone)), bounds: Some(b), ..Default::default() }).await.map_err(e)?;
            Ok(Done { result: json!({"state": w.state, "bounds": w.bounds}), event: Some(state_event(&w)) })
        }
        SET => {
            let wid = wid_of(cmd, app, args)?;
            let a: SetArgs = arg(cmd, args)?;
            if a.bounds.is_some() && find(wm, app, &wid).await?.is_some_and(|w| w.kind != Kind::Inline) {
                return Err("os_win_set: bounds are only for an inline window (the OS moves the others)".into());
            }
            let moved = a.bounds.is_some();
            let w = wm.update(app, &wid, Patch { title: a.title, icon: a.icon, badge: a.badge, attention: a.attention, bounds: a.bounds, ..Default::default() }).await.map_err(e)?;
            Ok(Done { result: Value::Null, event: moved.then(|| state_event(&w)) })
        }
        LIST => {
            let v = wm.list(Some(app)).await.map_err(e)?;
            Ok(Done { result: serde_json::to_value(v).map_err(|e| e.to_string())?, event: None })
        }
        _ => unreachable!("checked above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::WindowSettings;
    use battalion_host::paths::Paths;

    const BIG: Surface = Surface { x: 0.0, y: 0.0, w: 2000.0, h: 1200.0 };
    fn win() -> Vec<String> {
        vec!["window".into()]
    }
    fn start(p: &Paths) -> Windows {
        let (w, fut) = Windows::start(p.clone(), WindowSettings { save_debounce_ms: 10_000 });
        tokio::spawn(fut);
        w
    }
    async fn open(wm: &Windows, app: &str, key: &str) -> Value {
        call(wm, app, &win(), OPEN, &json!({"key": key, "title": "Chat", "bounds": {"x": 50, "y": 60, "w": 700, "h": 500}}), BIG).await.unwrap().result
    }

    #[tokio::test]
    async fn app_a_cannot_touch_bs_wid() {
        let d = tempfile::tempdir().unwrap();
        let wm = start(&Paths::new(d.path()));
        let theirs = open(&wm, "bb", "k").await["wid"].as_str().unwrap().to_string();
        for cmd in [CLOSE, MINIMIZE, RESTORE, MAXIMIZE, FOCUS, SET] {
            let err = call(&wm, "aa", &win(), cmd, &json!({"wid": theirs, "title": "x"}), BIG).await.unwrap_err();
            assert!(err.contains("may not act"), "{cmd}: {err}");
        }
        assert!(call(&wm, "aa", &win(), SNAP, &json!({"wid": theirs, "zone": "left"}), BIG).await.is_err());
        // A's list shows none of B's windows; B's window is untouched.
        assert_eq!(call(&wm, "aa", &win(), LIST, &json!({}), BIG).await.unwrap().result, json!([]));
        let b = wm.list(Some("bb")).await.unwrap();
        assert_eq!((b.len(), b[0].state, b[0].title.as_str()), (1, WinState::Normal, "Chat"));
    }

    #[tokio::test]
    async fn every_call_is_refused_without_the_window_permission() {
        let d = tempfile::tempdir().unwrap();
        let wm = start(&Paths::new(d.path()));
        for cmd in COMMANDS {
            let err = call(&wm, "aa", &["network".to_string()], cmd, &json!({"key": "k", "wid": "w-aa-1"}), BIG).await.unwrap_err();
            assert!(err.contains("'window' permission"), "{cmd}: {err}");
        }
        assert!(wm.list(None).await.unwrap().is_empty(), "nothing was opened");
        assert!(call(&wm, "aa", &win(), "os_win_exit", &json!({}), BIG).await.is_err());
    }

    #[tokio::test]
    async fn the_same_key_focuses_the_open_window_instead_of_a_duplicate() {
        let d = tempfile::tempdir().unwrap();
        let wm = start(&Paths::new(d.path()));
        let a = open(&wm, "aa", "session.1").await;
        open(&wm, "aa", "session.2").await;
        let again = open(&wm, "aa", "session.1").await;
        assert_eq!(again["wid"], a["wid"]);
        assert_eq!(again["restored"], false);
        let list = call(&wm, "aa", &win(), LIST, &Value::Null, BIG).await.unwrap().result;
        assert_eq!(list.as_array().unwrap().len(), 2);
        assert_eq!(list[1]["wid"], a["wid"], "the focused one is on top");
    }

    #[tokio::test]
    async fn saved_state_wins_over_the_given_bounds() {
        let d = tempfile::tempdir().unwrap();
        let p = Paths::new(d.path());
        let wm = start(&p);
        let w = open(&wm, "aa", "k").await;
        assert_eq!(w["restored"], false);
        let wid = w["wid"].as_str().unwrap();
        let moved = json!({"x": 300, "y": 200, "w": 640, "h": 480});
        let done = call(&wm, "aa", &win(), SET, &json!({"wid": wid, "bounds": moved}), BIG).await.unwrap();
        assert_eq!(done.event.unwrap()["change"], "state");
        call(&wm, "aa", &win(), MAXIMIZE, &json!({"wid": wid}), BIG).await.unwrap();
        wm.flush().await.unwrap();
        // A fresh OS run: the saved bounds and state come back, not the ones the call gives.
        let wm2 = start(&p);
        let r = open(&wm2, "aa", "k").await;
        assert_eq!(r["restored"], true);
        assert_eq!(r["bounds"], json!({"x": 300.0, "y": 200.0, "w": 640.0, "h": 480.0}));
        assert_eq!(r["state"], "maximized");
    }

    #[tokio::test]
    async fn state_calls_answer_and_tell_the_owning_app() {
        let d = tempfile::tempdir().unwrap();
        let wm = start(&Paths::new(d.path()));
        let wid = open(&wm, "aa", "k").await["wid"].as_str().unwrap().to_string();
        let m = call(&wm, "aa", &win(), MINIMIZE, &json!({"wid": wid}), BIG).await.unwrap();
        assert_eq!(m.result, json!({"state": "minimized"}));
        assert_eq!(m.event.as_ref().unwrap()["change"], "state");
        // Focus restores a minimized window.
        let f = call(&wm, "aa", &win(), FOCUS, &json!({"wid": wid}), BIG).await.unwrap();
        assert_eq!(f.result, json!({"state": "normal"}));
        assert_eq!(f.event.as_ref().unwrap()["change"], "focus");
        let s = call(&wm, "aa", &win(), SNAP, &json!({"wid": wid, "zone": "tr"}), BIG).await.unwrap();
        assert_eq!(s.result, json!({"state": {"snap": "tr"}, "bounds": {"x": 1000.0, "y": 0.0, "w": 1000.0, "h": 600.0}}));
        assert!(call(&wm, "aa", &win(), SNAP, &json!({"wid": wid, "zone": "middle"}), BIG).await.is_err());
        let c = call(&wm, "aa", &win(), CLOSE, &json!({"wid": wid}), BIG).await.unwrap();
        assert_eq!(c.event, Some(json!({"wid": wid, "change": "closed"})));
        assert!(wm.list(Some("aa")).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn routes_stay_on_the_apps_origin_and_bounds_are_only_for_inline() {
        let d = tempfile::tempdir().unwrap();
        let wm = start(&Paths::new(d.path()));
        for bad in ["https://evil.example/x", "//evil.example", "session/1", "/a b", "/a\\b", "javascript:alert(1)"] {
            assert!(call(&wm, "aa", &win(), OPEN, &json!({"key": "n", "kind": "native", "route": bad}), BIG).await.is_err(), "{bad}");
        }
        let n = call(&wm, "aa", &win(), OPEN, &json!({"key": "n", "kind": "native", "route": "/session/1?x=2"}), BIG).await.unwrap().result;
        let err = call(&wm, "aa", &win(), SET, &json!({"wid": n["wid"], "bounds": {"x": 0, "y": 0, "w": 400, "h": 300}}), BIG).await.unwrap_err();
        assert!(err.contains("only for an inline"), "{err}");
        assert!(call(&wm, "aa", &win(), SET, &json!({"wid": n["wid"], "title": "T", "badge": {"count": 2}}), BIG).await.unwrap().event.is_none());
        assert!(call(&wm, "aa", &win(), OPEN, &json!({"key": " "}), BIG).await.is_err());
    }
}
