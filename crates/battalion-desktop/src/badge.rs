// SPDX-License-Identifier: MPL-2.0
//! App badges: the short mark an app puts on its own desktop icon and taskbar button (`os_badge`).
//! The OS keeps one per app id; the id comes from the calling window's label (see `access`), never from an argument.

use std::collections::HashMap;
use std::sync::Mutex;

/// Longest badge text, in characters.
pub const MAX_TEXT: usize = 8;
/// A count above this shows as `99+`.
pub const MAX_COUNT: u64 = 99;

/// What the badge shows: `Some(text)`, or `None` for no badge. `count` 0 and an empty text clear the badge.
/// Both given is refused (the app must say one thing).
pub fn parse(count: Option<u64>, text: Option<&str>) -> Result<Option<String>, String> {
    match (count, text) {
        (Some(_), Some(_)) => Err("os_badge takes a count or a text, not both".into()),
        (Some(0), None) | (None, None) => Ok(None),
        (Some(n), None) => Ok(Some(if n > MAX_COUNT { format!("{MAX_COUNT}+") } else { n.to_string() })),
        (None, Some(t)) => {
            let t = t.trim();
            if t.is_empty() {
                Ok(None)
            } else if t.chars().any(char::is_control) {
                Err("badge text has a control character".into())
            } else if t.chars().count() > MAX_TEXT {
                Err(format!("badge text is longer than {MAX_TEXT} characters"))
            } else {
                Ok(Some(t.to_string()))
            }
        }
    }
}

#[derive(Default)]
pub struct Badges(Mutex<HashMap<String, String>>);

impl Badges {
    /// Set (`Some`) or clear (`None`) app `id`'s badge. Returns true when it changed.
    pub fn set(&self, id: &str, badge: Option<String>) -> bool {
        let mut m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        match badge {
            Some(b) => m.insert(id.to_string(), b.clone()).as_deref() != Some(b.as_str()),
            None => m.remove(id).is_some(),
        }
    }
    pub fn get(&self, id: &str) -> Option<String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_text_and_clear() {
        assert_eq!(parse(Some(3), None), Ok(Some("3".into())));
        assert_eq!(parse(Some(250), None), Ok(Some("99+".into())));
        assert_eq!(parse(Some(0), None), Ok(None));
        assert_eq!(parse(None, None), Ok(None));
        assert_eq!(parse(None, Some(" new ")), Ok(Some("new".into())));
        assert_eq!(parse(None, Some("  ")), Ok(None));
    }
    #[test]
    fn refused() {
        assert!(parse(Some(1), Some("x")).is_err());
        assert!(parse(None, Some("123456789")).is_err());
        assert!(parse(None, Some("a\nb")).is_err());
    }
    #[test]
    fn set_get_clear() {
        let b = Badges::default();
        assert!(b.set("aa", Some("2".into())));
        assert!(!b.set("aa", Some("2".into())));
        assert_eq!(b.get("aa").as_deref(), Some("2"));
        assert_eq!(b.get("bb"), None);
        assert!(b.set("aa", None));
        assert!(!b.set("aa", None));
    }
}
