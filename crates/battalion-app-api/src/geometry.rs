// SPDX-License-Identifier: Apache-2.0
//! OS-WINDOW-1 window geometry: pure, shared by the OS and the SDKs (no GUI, no I/O).
//!
//! `clamp_box`, `drag_box` and `resize_box` are the infantry-ai `popout.ts` functions (same rules, same numbers),
//! generalised: the surface (an app's view, the work area or a monitor) may have an origin. Snap zones are the
//! halves and quarters of a surface. The TypeScript twin is `crates/battalion-desktop/ui/kit/lib/geometry.ts`.

use serde::{Deserialize, Serialize};

/// A window's box, in the coordinate space of its surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Where a window may sit: an app's view, the OS work area or a monitor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Surface {
    pub fn sized(w: f64, h: f64) -> Surface {
        Surface { x: 0.0, y: 0.0, w, h }
    }
}

pub const MIN_W: f64 = 320.0;
pub const MIN_H: f64 = 240.0;
/// Kept on the surface so the title bar can always be grabbed.
pub const GRAB_PX: f64 = 80.0;
/// The bar height kept on the surface at the bottom.
pub const BAR_PX: f64 = 36.0;
/// Gap from the right edge for a "dock right" box (x < 0 in surface space).
pub const DOCK_GAP_PX: f64 = 12.0;

/// Fit `b` into `s` so it stays reachable. `b.x` below the surface's left edge means "dock right".
pub fn clamp_box(b: Rect, s: Surface) -> Rect {
    let w = b.w.max(MIN_W).min(MIN_W.max(s.w));
    let h = b.h.max(MIN_H).min(MIN_H.max(s.h));
    let lx = b.x - s.x;
    let x0 = if lx < 0.0 { s.w - w - DOCK_GAP_PX } else { lx };
    let x = x0.max(GRAB_PX - w).min(0f64.max(s.w - GRAB_PX));
    let y = (b.y - s.y).max(0.0).min(0f64.max(s.h - BAR_PX));
    Rect { x: x + s.x, y: y + s.y, w, h }
}

pub fn drag_box(start: Rect, dx: f64, dy: f64, s: Surface) -> Rect {
    clamp_box(Rect { x: start.x + dx, y: start.y + dy, ..start }, s)
}

/// Resize from an edge or corner (`n s e w` letters). The opposite edge stays put; the size never drops below the minimum.
pub fn resize_box(start: Rect, edge: &str, dx: f64, dy: f64, s: Surface) -> Rect {
    let (sx, sy) = (start.x - s.x, start.y - s.y);
    let Rect { mut x, mut y, mut w, mut h } = Rect { x: sx, y: sy, ..start };
    if edge.contains('e') {
        w = (start.w + dx).max(MIN_W).min(MIN_W.max(s.w - sx));
    }
    if edge.contains('s') {
        h = (start.h + dy).max(MIN_H).min(MIN_H.max(s.h - sy));
    }
    if edge.contains('w') {
        w = (start.w - dx).max(MIN_W).min(sx + start.w);
        x = sx + start.w - w;
    }
    if edge.contains('n') {
        h = (start.h - dy).max(MIN_H).min(sy + start.h);
        y = sy + start.h - h;
    }
    Rect { x: x + s.x, y: y + s.y, w, h }
}

/// A snap zone: a half or a quarter of the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Snap {
    Left,
    Right,
    Tl,
    Tr,
    Bl,
    Br,
}

/// The box a window takes when snapped to `zone`.
pub fn snap_box(zone: Snap, s: Surface) -> Rect {
    let (hw, hh) = (s.w / 2.0, s.h / 2.0);
    let (x, y, w, h) = match zone {
        Snap::Left => (0.0, 0.0, hw, s.h),
        Snap::Right => (hw, 0.0, s.w - hw, s.h),
        Snap::Tl => (0.0, 0.0, hw, hh),
        Snap::Tr => (hw, 0.0, s.w - hw, hh),
        Snap::Bl => (0.0, hh, hw, s.h - hh),
        Snap::Br => (hw, hh, s.w - hw, s.h - hh),
    };
    Rect { x: x + s.x, y: y + s.y, w, h }
}

/// The zone a drag pointer at (`px`, `py`) asks for: within `edge` px of the left or right edge is a half, and of a
/// corner (both edges) a quarter. `None` elsewhere (no snap).
pub fn snap_zone_at(px: f64, py: f64, s: Surface, edge: f64) -> Option<Snap> {
    let (lx, ly) = (px - s.x, py - s.y);
    let left = lx <= edge;
    let right = lx >= s.w - edge;
    let top = ly <= edge;
    let bottom = ly >= s.h - edge;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(Snap::Tl),
        (true, _, _, true) => Some(Snap::Bl),
        (_, true, true, _) => Some(Snap::Tr),
        (_, true, _, true) => Some(Snap::Br),
        (true, _, _, _) => Some(Snap::Left),
        (_, true, _, _) => Some(Snap::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn clamp_matches_popout_ts() {
        let s = Surface::sized(1000.0, 800.0);
        assert_eq!(clamp_box(r(10.0, 10.0, 400.0, 300.0), s), r(10.0, 10.0, 400.0, 300.0));
        assert_eq!(clamp_box(r(10.0, 10.0, 100.0, 100.0), s), r(10.0, 10.0, MIN_W, MIN_H), "minimum size");
        assert_eq!(clamp_box(r(-1.0, 10.0, 400.0, 300.0), s).x, 1000.0 - 400.0 - DOCK_GAP_PX, "dock right");
        assert_eq!(clamp_box(r(5000.0, 5000.0, 400.0, 300.0), s), r(1000.0 - GRAB_PX, 800.0 - BAR_PX, 400.0, 300.0));
    }

    #[test]
    fn clamp_onto_a_smaller_surface_keeps_the_window_reachable() {
        let big = r(1800.0, 1000.0, 1200.0, 900.0); // saved on a 3000x2000 monitor
        let small = Surface::sized(800.0, 600.0);
        let c = clamp_box(big, small);
        assert!(c.w <= 800.0 && c.h <= 600.0);
        assert!(c.x <= 800.0 - GRAB_PX && c.y <= 600.0 - BAR_PX && c.x + c.w >= GRAB_PX && c.y >= 0.0);
        // A surface with an origin (a second monitor).
        let m2 = Surface { x: 1920.0, y: 0.0, w: 1280.0, h: 720.0 };
        let c = clamp_box(r(100.0, 100.0, 400.0, 300.0), m2);
        assert!(c.x >= m2.x - 400.0 + GRAB_PX && c.x <= m2.x + m2.w - GRAB_PX);
    }

    #[test]
    fn drag_and_resize() {
        let s = Surface::sized(1000.0, 800.0);
        assert_eq!(drag_box(r(10.0, 10.0, 400.0, 300.0), 50.0, 20.0, s), r(60.0, 30.0, 400.0, 300.0));
        assert_eq!(resize_box(r(100.0, 100.0, 400.0, 300.0), "se", 50.0, 50.0, s), r(100.0, 100.0, 450.0, 350.0));
        assert_eq!(resize_box(r(100.0, 100.0, 400.0, 300.0), "nw", 50.0, 20.0, s), r(150.0, 120.0, 350.0, 280.0));
        assert_eq!(resize_box(r(100.0, 100.0, 400.0, 300.0), "w", 300.0, 0.0, s), r(180.0, 100.0, MIN_W, 300.0), "minimum; right edge stays");
        assert_eq!(resize_box(r(100.0, 100.0, 400.0, 300.0), "e", 5000.0, 0.0, s).w, 900.0, "stops at the surface edge");
    }

    #[test]
    fn snap_zones_are_halves_and_quarters() {
        let s = Surface { x: 0.0, y: 40.0, w: 1000.0, h: 800.0 };
        assert_eq!(snap_box(Snap::Left, s), r(0.0, 40.0, 500.0, 800.0));
        assert_eq!(snap_box(Snap::Right, s), r(500.0, 40.0, 500.0, 800.0));
        assert_eq!(snap_box(Snap::Tl, s), r(0.0, 40.0, 500.0, 400.0));
        assert_eq!(snap_box(Snap::Tr, s), r(500.0, 40.0, 500.0, 400.0));
        assert_eq!(snap_box(Snap::Bl, s), r(0.0, 440.0, 500.0, 400.0));
        assert_eq!(snap_box(Snap::Br, s), r(500.0, 440.0, 500.0, 400.0));
        assert_eq!(snap_zone_at(2.0, 400.0, s, 8.0), Some(Snap::Left));
        assert_eq!(snap_zone_at(998.0, 400.0, s, 8.0), Some(Snap::Right));
        assert_eq!(snap_zone_at(2.0, 42.0, s, 8.0), Some(Snap::Tl));
        assert_eq!(snap_zone_at(998.0, 839.0, s, 8.0), Some(Snap::Br));
        assert_eq!(snap_zone_at(500.0, 400.0, s, 8.0), None);
        assert_eq!(serde_json::to_string(&Snap::Tl).unwrap(), "\"tl\"");
    }
}
