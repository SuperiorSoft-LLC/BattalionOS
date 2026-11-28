// SPDX-License-Identifier: MPL-2.0
//! The native calls an app's page may ask the OS for from its frame (BOS-25): pick a folder or a file, open a link in the system
//! browser, open and close a sign-in window. Pure rules, tested without a window: which manifest permission each needs, which
//! addresses may open, which window label the sign-in window gets. The app is always the frame that asked (never an argument).

/// The page-facing command names (frozen in docs/commands.md, "Added by BOS-25").
pub const PICK_FOLDER: &str = "os_pick_folder";
pub const PICK_FILE: &str = "os_pick_file";
pub const OPEN_EXTERNAL: &str = "os_open_external";
pub const LOGIN_WINDOW_OPEN: &str = "os_login_window_open";
pub const LOGIN_WINDOW_CLOSE: &str = "os_login_window_close";
/// AND-4: a notice on the app's own notification channel (`notifications`), and a QR code scanned by the OS (`scan.code`).
pub const NOTIFY: &str = "os_notify";
pub const SCAN_CODE: &str = "os_scan_code";

/// The one manifest permission each call needs; `None` = not a native call (refused by the caller).
pub fn permission_for(cmd: &str) -> Option<&'static str> {
    match cmd {
        PICK_FOLDER | PICK_FILE => Some("files.user"),
        OPEN_EXTERNAL => Some("network"),
        LOGIN_WINDOW_OPEN | LOGIN_WINDOW_CLOSE => Some("login.window"),
        NOTIFY => Some("notifications"),
        SCAN_CODE => Some("scan.code"),
        // WM-2 (OS-WINDOW-1 2.7): every window call needs `window`.
        c if crate::win_calls::COMMANDS.contains(&c) => Some("window"),
        _ => None,
    }
}

/// Is app `id` (its manifest's `permissions`) allowed `cmd`? The refusal names the permission to add.
pub fn require(id: &str, permissions: &[String], cmd: &str) -> Result<(), String> {
    let need = permission_for(cmd).ok_or_else(|| format!("'{cmd}' is not a native call"))?;
    if permissions.iter().any(|p| p == need) {
        Ok(())
    } else {
        Err(format!("app '{id}' has no '{need}' permission in its manifest, so '{cmd}' is refused"))
    }
}

fn clean(s: &str) -> bool {
    !s.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// After `prefix`, a non-empty host (not another slash).
fn web_with_host(lower: &str, prefix: &str) -> bool {
    lower.starts_with(prefix) && lower.len() > prefix.len() && !lower[prefix.len()..].starts_with('/')
}

/// A link the system browser may open: `http`, `https` or `mailto` only, no spaces or control characters, at most 2048 bytes.
pub fn external_link(url: &str) -> Result<String, String> {
    let u = url.trim();
    if u.is_empty() || u.len() > 2048 || !clean(u) {
        return Err("not a link that can be opened".into());
    }
    let lower = u.to_ascii_lowercase();
    if web_with_host(&lower, "http://") || web_with_host(&lower, "https://") || (lower.starts_with("mailto:") && lower.len() > "mailto:".len()) {
        Ok(u.to_string())
    } else {
        Err("only http, https and mailto links open in the browser".into())
    }
}

/// An address the sign-in window may load: `http` or `https` with a host.
pub fn login_url(url: &str) -> Result<String, String> {
    let u = url.trim();
    if u.len() > 2048 || !clean(u) {
        return Err("not a web address".into());
    }
    let lower = u.to_ascii_lowercase();
    if web_with_host(&lower, "http://") || web_with_host(&lower, "https://") {
        Ok(u.to_string())
    } else {
        Err("only web addresses open in the sign-in window".into())
    }
}

/// The label of the sign-in window of app `id`. It is not `main`, `login` or `app-*`, so it has no capability and no command.
pub fn login_label(id: &str) -> String {
    format!("frame-login-{id}")
}

/// The app id of a sign-in window label, if it is one.
pub fn login_label_app(label: &str) -> Option<&str> {
    label.strip_prefix("frame-login-").filter(|i| battalion_app_api::manifest::valid_id(i))
}

/// A dialog title: at most 120 characters, no control characters; empty becomes `fallback`.
pub fn dialog_title(title: Option<&str>, fallback: &str) -> String {
    let t: String = title.unwrap_or("").chars().filter(|c| !c.is_control()).take(120).collect();
    if t.trim().is_empty() {
        fallback.to_string()
    } else {
        t
    }
}

/// The permission a phone never grants, whatever the manifest says: Android does not let an app start a program it downloaded.
pub const PHONE_NEVER: &str = "process.spawn";

/// AND-4: the calls a phone serves natively (the Android side runs them in the app's own slot): the call name the slot knows.
pub fn phone_call_name(cmd: &str) -> Option<&'static str> {
    match cmd {
        PICK_FOLDER => Some("pick_folder"),
        PICK_FILE => Some("pick_file"),
        OPEN_EXTERNAL => Some("open_external"),
        NOTIFY => Some("notify"),
        SCAN_CODE => Some("scan_code"),
        // AND-6: the sign-in window of an app is a sign-in view over the app's own slot.
        LOGIN_WINDOW_OPEN => Some("login_open"),
        LOGIN_WINDOW_CLOSE => Some("login_close"),
        _ => None,
    }
}

/// Short single-line text: control characters dropped, at most `max` characters.
fn short(v: Option<&serde_json::Value>, max: usize) -> String {
    v.and_then(|v| v.as_str()).unwrap_or("").chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// AND-4: the check of a phone call of app `id` (its manifest `permissions`): the permission, then the arguments, made safe for
/// the slot. Returns `{call, args}` for the Android side. `process.spawn` (and any call that would need it) is always refused.
pub fn phone_call(id: &str, permissions: &[String], cmd: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    if cmd == PHONE_NEVER || cmd.contains("spawn") {
        return Err("starting a program is never allowed on a phone (Android does not let an app run a program it downloaded)".into());
    }
    let call = phone_call_name(cmd).ok_or_else(|| format!("'{cmd}' is not available on this device"))?;
    require(id, permissions, cmd)?;
    let args = match cmd {
        PICK_FOLDER => json!({"title": dialog_title(args.get("title").and_then(|t| t.as_str()), "Choose a folder")}),
        PICK_FILE => json!({"title": dialog_title(args.get("title").and_then(|t| t.as_str()), "Choose a file")}),
        LOGIN_WINDOW_OPEN => json!({"url": login_url(args.get("url").and_then(|u| u.as_str()).ok_or("os_login_window_open needs a url")?)?}),
        OPEN_EXTERNAL => json!({"url": external_link(args.get("url").and_then(|u| u.as_str()).ok_or("os_open_external needs a url")?)?}),
        NOTIFY => {
            let title = short(args.get("title"), 80);
            if title.is_empty() {
                return Err("os_notify needs a title".into());
            }
            json!({"title": title, "text": short(args.get("text"), 400)})
        }
        _ => json!({}),
    };
    Ok(json!({"call": call, "args": args}))
}

/// BOS-NOTIFY-1: the notices one app may show on the desktop in a minute; more are refused (a page cannot flood the person).
pub const NOTIFY_PER_MINUTE: usize = 10;

/// BOS-NOTIFY-1: a desktop notice of an app, made safe: `title` (up to 80), `text` (up to 400), `tag` (a repeat with the same tag
/// replaces the shown one; up to 64 of `A-Za-z0-9_.:-`) and `route` (handed back to the page on a click; up to 512).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub text: String,
    pub tag: Option<String>,
    pub route: Option<String>,
}

/// The checked notice of `os_notify` (`title`, `text?` or `body?`, `tag?`, `route?`).
pub fn notice(args: &serde_json::Value) -> Result<Notice, String> {
    let title = short(args.get("title"), 80);
    if title.is_empty() {
        return Err("os_notify needs a title".into());
    }
    let text = short(args.get("text").or_else(|| args.get("body")), 400);
    let tag = match args.get("tag").and_then(|t| t.as_str()) {
        None | Some("") => None,
        Some(t) if t.len() <= 64 && t.chars().all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c)) => Some(t.to_string()),
        Some(_) => return Err("os_notify tag: up to 64 letters, digits or _.:-".into()),
    };
    let route = Some(short(args.get("route"), 512)).filter(|r| !r.is_empty());
    Ok(Notice { title, text, tag, route })
}

/// BOS-NOTIFY-1: the per-app rate of desktop notices (`NOTIFY_PER_MINUTE` in any 60 s).
#[derive(Default)]
pub struct NoticeRate(std::collections::HashMap<String, std::collections::VecDeque<std::time::Instant>>);

impl NoticeRate {
    /// Counts one notice of app `id` at `now`, or refuses it when the app already showed its minute's share.
    pub fn take(&mut self, id: &str, now: std::time::Instant) -> Result<(), String> {
        let q = self.0.entry(id.to_string()).or_default();
        while q.front().is_some_and(|t| now.duration_since(*t) >= std::time::Duration::from_secs(60)) {
            q.pop_front();
        }
        if q.len() >= NOTIFY_PER_MINUTE {
            return Err(format!("app '{id}' showed {NOTIFY_PER_MINUTE} notices in the last minute; try again later"));
        }
        q.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn perms(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn each_call_needs_its_one_permission() {
        assert!(require("aa", &perms(&["files.user"]), PICK_FOLDER).is_ok());
        assert!(require("aa", &perms(&["files.user"]), PICK_FILE).is_ok());
        assert!(require("aa", &perms(&["network"]), OPEN_EXTERNAL).is_ok());
        assert!(require("aa", &perms(&["login.window"]), LOGIN_WINDOW_OPEN).is_ok());
        assert!(require("aa", &perms(&["login.window"]), LOGIN_WINDOW_CLOSE).is_ok());
        let e = require("aa", &perms(&["network", "window"]), PICK_FOLDER).unwrap_err();
        assert!(e.contains("files.user"), "{e}");
        assert!(require("aa", &perms(&["files.user", "network", "login.window"]), "os_exit").is_err());
        assert!(require("aa", &[], OPEN_EXTERNAL).is_err());
    }
    #[test]
    fn a_desktop_notice_is_cleaned_and_tagged() {
        use serde_json::json;
        let n = notice(&json!({"title": "A
B", "body": "x".repeat(900), "tag": "ticket:7", "route": "board"})).unwrap();
        assert_eq!(n.title, "AB");
        assert_eq!(n.text.len(), 400);
        assert_eq!(n.tag.as_deref(), Some("ticket:7"));
        assert_eq!(n.route.as_deref(), Some("board"));
        let n = notice(&json!({"title": "t", "text": "hi"})).unwrap();
        assert_eq!((n.text.as_str(), n.tag, n.route), ("hi", None, None));
        assert!(notice(&json!({"title": " "})).is_err());
        assert!(notice(&json!({"title": "t", "tag": "a b"})).is_err());
        assert!(notice(&json!({"title": "t", "tag": "x".repeat(65)})).is_err());
    }
    #[test]
    fn notices_are_rate_limited_per_app() {
        let mut r = NoticeRate::default();
        let t0 = std::time::Instant::now();
        for _ in 0..NOTIFY_PER_MINUTE {
            r.take("aa", t0).unwrap();
        }
        assert!(r.take("aa", t0).unwrap_err().contains("last minute"));
        assert!(r.take("bb", t0).is_ok(), "another app has its own share");
        assert!(r.take("aa", t0 + std::time::Duration::from_secs(60)).is_ok());
    }
    #[test]
    fn links_open_only_as_web_or_mail() {
        assert!(external_link("https://example.com/a?b=1").is_ok());
        assert!(external_link("http://127.0.0.1:9/x").is_ok());
        assert!(external_link("mailto:a@b.c").is_ok());
        for bad in [
            "file:///c:/windows/system32/calc.exe",
            "javascript:alert(1)",
            "ms-msdt:/id x",
            "calc.exe",
            "",
            "https://",
            "https:///x",
            "//evil.example",
            "https://a b",
            "https://a\u{0}b",
            "data:text/html,x",
            "\\\\host\\share",
        ] {
            assert!(external_link(bad).is_err(), "{bad:?}");
        }
        assert!(external_link(&format!("https://a/{}", "x".repeat(2100))).is_err());
    }
    #[test]
    fn sign_in_opens_only_web_addresses() {
        assert!(login_url("https://claude.ai/login").is_ok());
        assert!(login_url("http://127.0.0.1:5/p").is_ok());
        assert!(login_url("mailto:a@b.c").is_err());
        assert!(login_url("file:///x").is_err());
        assert!(login_url("https:///x").is_err());
    }
    #[test]
    fn the_sign_in_window_has_a_label_of_its_own() {
        let l = login_label("aa-b");
        assert_eq!(login_label_app(&l), Some("aa-b"));
        assert!(crate::access::caller_of(&l).is_err(), "no capability, no command");
        assert_eq!(login_label_app("frame-login-../x"), None);
        assert_eq!(login_label_app("app-aa"), None);
    }
    #[test]
    fn titles_are_short_and_clean() {
        assert_eq!(dialog_title(None, "Choose a folder"), "Choose a folder");
        assert_eq!(dialog_title(Some("  "), "F"), "F");
        assert_eq!(dialog_title(Some("Project\nfolder"), "F"), "Projectfolder");
        assert_eq!(dialog_title(Some(&"x".repeat(500)), "F").len(), 120);
    }
    #[test]
    fn phone_calls_are_checked_against_the_manifest() {
        use serde_json::json;
        let all = perms(&["files.user", "network", "notifications", "scan.code", "process.spawn"]);
        let ok = phone_call("aa", &all, PICK_FILE, &json!({"title": "A\nB"})).unwrap();
        assert_eq!(ok, json!({"call": "pick_file", "args": {"title": "AB"}}));
        assert_eq!(phone_call("aa", &all, PICK_FOLDER, &json!({})).unwrap()["args"]["title"], "Choose a folder");
        assert_eq!(phone_call("aa", &all, OPEN_EXTERNAL, &json!({"url": "mailto:a@b.c"})).unwrap()["call"], "open_external");
        assert!(phone_call("aa", &all, OPEN_EXTERNAL, &json!({"url": "intent://x#Intent;end"})).is_err());
        assert!(phone_call("aa", &all, OPEN_EXTERNAL, &json!({})).is_err());
        let n = phone_call("aa", &all, NOTIFY, &json!({"title": "x".repeat(200), "text": "hi"})).unwrap();
        assert_eq!(n["args"]["title"].as_str().unwrap().len(), 80);
        assert!(phone_call("aa", &all, NOTIFY, &json!({"title": " "})).is_err());
        assert_eq!(phone_call("aa", &all, SCAN_CODE, &json!({})).unwrap(), json!({"call": "scan_code", "args": {}}));
        // Undeclared: a plain refusal naming the permission.
        for (cmd, need) in [(PICK_FILE, "files.user"), (OPEN_EXTERNAL, "network"), (NOTIFY, "notifications"), (SCAN_CODE, "scan.code")] {
            let e = phone_call("aa", &[], cmd, &json!({"url": "https://a", "title": "t"})).unwrap_err();
            assert!(e.contains(need), "{e}");
        }
        // Never on a phone, even when declared.
        for c in ["process.spawn", "os_process_spawn", "os_spawn"] {
            let e = phone_call("aa", &all, c, &json!({})).unwrap_err();
            assert!(e.contains("never allowed on a phone"), "{e}");
        }
        assert!(phone_call("aa", &all, LOGIN_WINDOW_OPEN, &json!({})).is_err());
        // AND-6: the sign-in window is served on a phone, under login.window, for a web address only.
        let ok = phone_call("aa", &perms(&["login.window"]), LOGIN_WINDOW_OPEN, &json!({"url": "https://example.com/login"})).unwrap();
        assert_eq!(ok["call"], "login_open");
        assert!(ok["args"]["url"].as_str().unwrap().starts_with("https://example.com/login"));
        assert!(phone_call("aa", &perms(&["login.window"]), LOGIN_WINDOW_OPEN, &json!({"url": "file:///etc/passwd"})).is_err());
        assert!(phone_call("aa", &[], LOGIN_WINDOW_OPEN, &json!({"url": "https://example.com"})).unwrap_err().contains("login.window"));
        assert_eq!(phone_call("aa", &perms(&["login.window"]), LOGIN_WINDOW_CLOSE, &json!({})).unwrap()["call"], "login_close");
        assert!(phone_call("aa", &all, "os_exit", &json!({})).is_err());
    }
}
