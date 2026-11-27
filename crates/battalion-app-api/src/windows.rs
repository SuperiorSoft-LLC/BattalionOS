// SPDX-License-Identifier: Apache-2.0
//! OS-WINDOW-1 §2.4 (WM-5): the SDK window types, `battalion-sdk::windows` (SDK-RS-1 re-exports this module; until it
//! exists, this crate is the shared contract crate). The JSON schema `schemas/window.schema.json` describes the same
//! shapes, and the TypeScript types (`crates/battalion-desktop/ui/kit/lib/windows/types.gen.ts`) are generated from it.
//!
//! Wire shapes are the `os_win_*` frame commands and the `os_win` event (docs/commands.md, "Window calls (WM-2)").

use serde::{Deserialize, Serialize};

pub use crate::geometry::{Rect, Snap as SnapZone};

/// Where and how a window is drawn (design 2.2). An OS without `float`/`native` answers `inline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowKind {
    #[default]
    Inline,
    Float,
    Native,
}

/// `"normal" | "minimized" | "maximized" | {"snap": zone}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowState {
    #[default]
    Normal,
    Minimized,
    Maximized,
    Snap(SnapZone),
}

/// A window's badge: a count, a short text, or both.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WindowBadge {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// What an app asks for in `os_win_open`. The app is the caller's identity, never a field.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WindowSpec {
    /// The app's own stable name for the window (`session.<id>`); state is saved per `(app, key)`.
    pub key: String,
    #[serde(default)]
    pub kind: WindowKind,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A path on the app's own origin; used for `float`/`native` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    /// Used when nothing is saved for `(app, key)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Rect>,
    /// Used when nothing is saved for `(app, key)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<WindowState>,
}

/// The answer of `os_win_open`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opened {
    pub wid: String,
    /// The kind actually used (a phone or the local backend answers `inline`).
    pub kind: WindowKind,
    pub state: WindowState,
    pub bounds: Rect,
    /// True when the OS used saved state for `(app, key)`.
    pub restored: bool,
}

/// One row of `os_win_list` (the caller's own windows, z order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub wid: String,
    pub key: String,
    pub kind: WindowKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<WindowBadge>,
    #[serde(default)]
    pub attention: bool,
    pub state: WindowState,
    pub bounds: Rect,
    pub z: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
}

/// The `os_win_set` arguments besides `wid`. Absent fields stay; `badge: null` clears the badge.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WindowPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    pub badge: Option<Option<WindowBadge>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<bool>,
    /// Only for `inline` (the app moved it; the OS saves it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Rect>,
}

/// The `os_win` event as an app's frame receives it (`{bos: 1, event: "os_win", wid, change, ...}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "lowercase")]
pub enum WinEvent {
    /// The OS (a taskbar click, a snap) changed the window's state or bounds.
    State { wid: String, state: WindowState, bounds: Rect },
    /// The window is gone (taskbar close, the app stopped, a native window closed).
    Closed { wid: String },
    /// The window went to the top of its surface.
    Focus {
        wid: String,
        z: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<WindowState>,
    },
}

impl WinEvent {
    pub fn wid(&self) -> &str {
        match self {
            WinEvent::State { wid, .. } | WinEvent::Closed { wid } | WinEvent::Focus { wid, .. } => wid,
        }
    }
}

/// `Option<Option<T>>` with `null` kept apart from "absent".
mod double_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(v: &Option<Option<T>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(inner) => inner.serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn state_wire_shapes() {
        assert_eq!(serde_json::to_value(WindowState::Maximized).unwrap(), json!("maximized"));
        assert_eq!(serde_json::to_value(WindowState::Snap(SnapZone::Tl)).unwrap(), json!({"snap": "tl"}));
        let s: WindowState = serde_json::from_value(json!({"snap": "right"})).unwrap();
        assert_eq!(s, WindowState::Snap(SnapZone::Right));
    }

    #[test]
    fn spec_defaults_to_inline() {
        let s: WindowSpec = serde_json::from_value(json!({"key": "session.1"})).unwrap();
        assert_eq!(s.kind, WindowKind::Inline);
        assert_eq!(s.title, "");
    }

    #[test]
    fn events_are_tagged_by_change() {
        let e: WinEvent = serde_json::from_value(json!({"change": "state", "wid": "w-a-1", "state": "minimized", "bounds": {"x": 0, "y": 0, "w": 400, "h": 300}})).unwrap();
        assert_eq!(e.wid(), "w-a-1");
        let c = serde_json::to_value(WinEvent::Closed { wid: "w-a-2".into() }).unwrap();
        assert_eq!(c, json!({"change": "closed", "wid": "w-a-2"}));
        let f: WinEvent = serde_json::from_value(json!({"change": "focus", "wid": "w", "z": 3, "bos": 1, "event": "os_win"})).unwrap();
        assert!(matches!(f, WinEvent::Focus { z: 3, .. }));
    }

    /// The schema (the source of the TS types) names exactly the values these types put on the wire.
    #[test]
    fn schema_matches_the_rust_types() {
        let schema: serde_json::Value = serde_json::from_str(crate::schemas::WINDOW_JSON).unwrap();
        let defs = &schema["$defs"];
        let names = |v: &serde_json::Value| -> Vec<String> { v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect() };
        let wire = |v: serde_json::Value| v.as_str().unwrap().to_string();
        let kinds: Vec<String> = [WindowKind::Inline, WindowKind::Float, WindowKind::Native].map(|k| wire(serde_json::to_value(k).unwrap())).into();
        assert_eq!(names(&defs["WindowKind"]["enum"]), kinds);
        let zones: Vec<String> = [SnapZone::Left, SnapZone::Right, SnapZone::Tl, SnapZone::Tr, SnapZone::Bl, SnapZone::Br].map(|z| wire(serde_json::to_value(z).unwrap())).into();
        assert_eq!(names(&defs["SnapZone"]["enum"]), zones);
        let states: Vec<String> = [WindowState::Normal, WindowState::Minimized, WindowState::Maximized].map(|s| wire(serde_json::to_value(s).unwrap())).into();
        assert_eq!(names(&defs["WindowState"]["oneOf"][0]["enum"]), states);
        let changes: Vec<&str> = defs["WinEvent"]["oneOf"].as_array().unwrap().iter().map(|o| o["properties"]["change"]["const"].as_str().unwrap()).collect();
        assert_eq!(changes, ["state", "closed", "focus"]);
        for (def, required) in [("WindowSpec", vec!["key"]), ("Opened", vec!["wid", "kind", "state", "bounds", "restored"])] {
            assert_eq!(names(&defs[def]["required"]), required, "{def}");
        }
    }

    #[test]
    fn patch_keeps_null_badge_apart_from_absent() {
        let p: WindowPatch = serde_json::from_value(json!({"badge": null})).unwrap();
        assert_eq!(p.badge, Some(None));
        let p: WindowPatch = serde_json::from_value(json!({})).unwrap();
        assert_eq!(p.badge, None);
        assert_eq!(serde_json::to_value(WindowPatch { badge: Some(None), ..Default::default() }).unwrap(), json!({"badge": null}));
    }
}
