// SPDX-License-Identifier: MPL-2.0
//! OS-WINDOW-1 §4 (WM-7): low-memory mode for hidden native windows. A native window that has been minimized or hidden for
//! `bos.windows.lowMemoryAfterSec` seconds is asked to use less memory (WebView2 `MemoryUsageTargetLevel` LOW, through
//! `native::MemoryLevel`); restore or focus puts it back to NORMAL. `TrySuspend` is NOT used: it stops timers and sockets.
//!
//! This is the pure timer, keyed by the native window label; the engine side only calls it on window events and sets the level.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct LowMem {
    /// Label → when it was hidden (the first hide; a repeated hide keeps it).
    hidden: HashMap<String, Instant>,
    /// Labels set to LOW.
    low: HashSet<String>,
}

impl LowMem {
    /// The window was minimized or hidden at `now`. Answers how long to wait before asking [`LowMem::due`], or `None` when the
    /// setting is 0 (off) or the window is already LOW.
    pub fn hide(&mut self, id: &str, now: Instant, after_sec: u64) -> Option<Duration> {
        if after_sec == 0 || self.low.contains(id) {
            return None;
        }
        let since = *self.hidden.entry(id.to_string()).or_insert(now);
        Some((since + Duration::from_secs(after_sec)).saturating_duration_since(now))
    }

    /// True once, when the window has stayed hidden for `after_sec` seconds: the caller sets LOW now.
    pub fn due(&mut self, id: &str, now: Instant, after_sec: u64) -> bool {
        if after_sec == 0 || self.low.contains(id) {
            return false;
        }
        match self.hidden.get(id) {
            Some(since) if now.saturating_duration_since(*since) >= Duration::from_secs(after_sec) => {
                self.low.insert(id.to_string());
                true
            }
            _ => false,
        }
    }

    /// The window was restored or focused. True when it was LOW: the caller sets NORMAL again.
    pub fn show(&mut self, id: &str) -> bool {
        self.hidden.remove(id);
        self.low.remove(id)
    }

    /// The window closed.
    pub fn forget(&mut self, id: &str) {
        self.hidden.remove(id);
        self.low.remove(id);
    }

    pub fn is_low(&self, id: &str) -> bool {
        self.low.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = "win-demo-app-1";

    #[test]
    fn it_moves_to_low_after_n_seconds() {
        let (mut m, t0) = (LowMem::default(), Instant::now());
        assert_eq!(m.hide(W, t0, 30), Some(Duration::from_secs(30)));
        assert!(!m.due(W, t0 + Duration::from_secs(29), 30));
        // Hiding again (another resize while minimized) keeps the first time.
        assert_eq!(m.hide(W, t0 + Duration::from_secs(10), 30), Some(Duration::from_secs(20)));
        assert!(m.due(W, t0 + Duration::from_secs(30), 30));
        assert!(m.is_low(W));
        // Only once.
        assert!(!m.due(W, t0 + Duration::from_secs(60), 30));
        assert_eq!(m.hide(W, t0 + Duration::from_secs(61), 30), None);
    }

    #[test]
    fn restore_sets_normal_and_cancels_a_pending_low() {
        let (mut m, t0) = (LowMem::default(), Instant::now());
        m.hide(W, t0, 30);
        assert!(m.due(W, t0 + Duration::from_secs(31), 30));
        assert!(m.show(W), "it was LOW: back to NORMAL");
        assert!(!m.is_low(W));
        // Restored before the time: no LOW, and nothing to set back.
        m.hide(W, t0 + Duration::from_secs(40), 30);
        assert!(!m.show(W));
        assert!(!m.due(W, t0 + Duration::from_secs(80), 30));
        // Hidden again later: the timer starts over from the new hide.
        m.hide(W, t0 + Duration::from_secs(100), 30);
        assert!(!m.due(W, t0 + Duration::from_secs(110), 30));
        assert!(m.due(W, t0 + Duration::from_secs(130), 30));
    }

    #[test]
    fn nothing_happens_when_the_setting_is_0() {
        let (mut m, t0) = (LowMem::default(), Instant::now());
        assert_eq!(m.hide(W, t0, 0), None);
        assert!(!m.due(W, t0 + Duration::from_secs(3600), 0));
        assert!(!m.is_low(W));
        assert!(!m.show(W));
    }

    #[test]
    fn a_closed_window_is_forgotten() {
        let (mut m, t0) = (LowMem::default(), Instant::now());
        m.hide(W, t0, 5);
        assert!(m.due(W, t0 + Duration::from_secs(5), 5));
        m.forget(W);
        assert!(!m.is_low(W));
        assert!(!m.due(W, t0 + Duration::from_secs(9), 5));
    }
}
