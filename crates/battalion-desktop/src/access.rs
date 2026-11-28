// SPDX-License-Identifier: MPL-2.0
//! Who may act on whom. A window's label is its identity: `main`, `login`, or `app-<id>`.
//! The label comes from the Tauri runtime (the calling webview), never from the command's arguments,
//! so an app cannot claim to be another one.

use battalion_app_api::manifest::valid_id;

pub const MAIN: &str = "main";
pub const LOGIN: &str = "login";
pub const APP_PREFIX: &str = "app-";
/// OS-WINDOW-1: a native window of an app, `win-<app>-<n>` (WM-6 opens them). Its app is in the label.
pub const WIN_PREFIX: &str = "win-";

/// Closing this window only hides it: apps and agents keep working, and a new start of the OS shows
/// it again. Exit (`os_exit`) is the only way the OS ends. Other windows really close.
pub fn close_hides_window(label: &str) -> bool {
    label == MAIN
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Main,
    Login,
    App(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denied {
    UnknownLabel(String),
    BadAppId(String),
    NotYours { caller: String, target: String },
    ShellOnly(String),
    AppOnly(String),
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Denied::UnknownLabel(l) => write!(f, "unknown window '{l}'"),
            Denied::BadAppId(i) => write!(f, "bad app id '{i}'"),
            Denied::NotYours { caller, target } => write!(f, "window '{caller}' may not act on app '{target}'"),
            Denied::ShellOnly(c) => write!(f, "'{c}' is for the OS shell only"),
            Denied::AppOnly(c) => write!(f, "'{c}' is for an app window only"),
        }
    }
}

pub fn app_label(id: &str) -> Result<String, Denied> {
    // BOS-30: "battalionos" is the Armory row of the OS itself. It is not an app: no app command (remove, start, open) takes it.
    if valid_id(id) && id != crate::osupdate::OS_ROW {
        Ok(format!("{APP_PREFIX}{id}"))
    } else {
        Err(Denied::BadAppId(id.to_string()))
    }
}

pub fn caller_of(label: &str) -> Result<Caller, Denied> {
    match label {
        MAIN => Ok(Caller::Main),
        LOGIN => Ok(Caller::Login),
        l => match l.strip_prefix(APP_PREFIX) {
            Some(id) if valid_id(id) => Ok(Caller::App(id.to_string())),
            _ => Err(Denied::UnknownLabel(l.to_string())),
        },
    }
}

/// May the window `label` act on app `target`? Main: any app. An app window: only itself. Login: none.
pub fn authorize_app(label: &str, target: &str) -> Result<(), Denied> {
    if !valid_id(target) || target == crate::osupdate::OS_ROW {
        return Err(Denied::BadAppId(target.to_string()));
    }
    match caller_of(label)? {
        Caller::Main => Ok(()),
        Caller::App(own) if own == target => Ok(()),
        _ => Err(Denied::NotYours { caller: label.to_string(), target: target.to_string() }),
    }
}

/// OS-wide commands (list, install, remove): the main window only.
pub fn authorize_shell(label: &str, command: &str) -> Result<(), Denied> {
    match caller_of(label)? {
        Caller::Main => Ok(()),
        _ => Err(Denied::ShellOnly(command.to_string())),
    }
}

/// WM-2: the app an `os_win_*` call speaks for. An `app-<id>` window or a native `win-<app>-<n>` window of that app; any other
/// label is refused (the shell uses the `os_frame_win_*` commands, naming the app of the frame).
pub fn authorize_win_caller(label: &str, command: &str) -> Result<String, Denied> {
    if let Some(rest) = label.strip_prefix(WIN_PREFIX) {
        return match rest.rsplit_once('-') {
            Some((app, n)) if valid_id(app) && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => Ok(app.to_string()),
            _ => Err(Denied::UnknownLabel(label.to_string())),
        };
    }
    authorize_self(label, command)
}

/// POPOUT-1: `os_app_status` / `os_app_health` from a native window (`win-<app>-<n>`) for its own app only, so the frame in it
/// finds its daemon (the page needs the address, like the frames in `main`). Every other label: [authorize_app].
pub fn authorize_app_read(label: &str, target: &str) -> Result<(), Denied> {
    if label.starts_with(WIN_PREFIX) {
        let own = authorize_win_caller(label, "os_app_status")?;
        return if valid_id(target) && own == target { Ok(()) } else { Err(Denied::NotYours { caller: label.to_string(), target: target.to_string() }) };
    }
    authorize_app(label, target)
}

/// WM-2: an app may only name its own wids (`w-<app>-<n>`). A wid of another app, or not a wid at all, is refused.
pub fn authorize_wid(app: &str, wid: &str) -> Result<(), Denied> {
    match crate::windows::wid_app(wid) {
        Some(a) if a == app => Ok(()),
        _ => Err(Denied::NotYours { caller: format!("{APP_PREFIX}{app}"), target: wid.to_string() }),
    }
}

/// Commands an app makes about itself (`os_badge`, `os_show_desktop`): only an app window, and the app is the one the label
/// names. The id is never an argument, so no window can act as another app.
pub fn authorize_self(label: &str, command: &str) -> Result<String, Denied> {
    match caller_of(label)? {
        Caller::App(id) => Ok(id),
        _ => Err(Denied::AppOnly(command.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_main_window_hides_it_and_keeps_apps_running() {
        assert!(close_hides_window(MAIN));
        assert!(!close_hides_window(LOGIN));
        assert!(!close_hides_window("app-aa"));
    }

    #[test]
    fn app_a_cannot_act_on_b() {
        assert_eq!(authorize_app("app-aa", "aa"), Ok(()));
        assert!(matches!(authorize_app("app-aa", "bb"), Err(Denied::NotYours { .. })));
        assert!(matches!(authorize_app("app-bb", "aa"), Err(Denied::NotYours { .. })));
    }
    #[test]
    fn main_acts_on_any_login_on_none() {
        assert_eq!(authorize_app("main", "aa"), Ok(()));
        assert!(authorize_app("login", "aa").is_err());
    }
    #[test]
    fn shell_commands_are_main_only() {
        assert_eq!(authorize_shell("main", "os_list_apps"), Ok(()));
        assert!(authorize_shell("app-aa", "os_list_apps").is_err());
        assert!(authorize_shell("login", "os_list_apps").is_err());
    }
    #[test]
    fn self_commands_name_the_app_from_the_label_only() {
        assert_eq!(authorize_self("app-aa", "os_badge"), Ok("aa".to_string()));
        assert!(authorize_self("main", "os_badge").is_err());
        assert!(authorize_self("login", "os_badge").is_err());
        assert!(authorize_self("app-../x", "os_badge").is_err());
    }
    #[test]
    fn the_os_row_is_not_an_app() {
        // BOS-30: no app command can name the BattalionOS row, so it cannot be removed, started, stopped or opened as an app.
        assert!(matches!(app_label("battalionos"), Err(Denied::BadAppId(_))));
        assert!(matches!(authorize_app("main", "battalionos"), Err(Denied::BadAppId(_))));
    }
    #[test]
    fn the_update_commands_are_for_the_shell_only() {
        // BOS-30: an app window, the sign-in window or an unknown one may not read, check or run an OS update.
        for c in ["os_update_state", "os_update_check", "os_update_install"] {
            assert_eq!(authorize_shell("main", c), Ok(()));
            for who in ["app-aa", "login", "evil"] {
                assert!(authorize_shell(who, c).is_err(), "{who} must not call {c}");
            }
        }
    }
    #[test]
    fn a_window_call_speaks_for_the_app_of_its_label_and_only_names_its_own_wids() {
        assert_eq!(authorize_win_caller("app-aa", "os_win_open"), Ok("aa".to_string()));
        assert_eq!(authorize_win_caller("win-my-app-3", "os_win_open"), Ok("my-app".to_string()));
        for bad in ["main", "login", "win-", "win-aa", "win-aa-x", "win-../x-1", "evil"] {
            assert!(authorize_win_caller(bad, "os_win_open").is_err(), "{bad}");
        }
        assert_eq!(authorize_wid("aa", "w-aa-1"), Ok(()));
        assert!(authorize_wid("aa", "w-bb-1").is_err());
        assert!(authorize_wid("aa", "w-aa-bb-1").is_err(), "w-aa-bb-1 is app aa-bb's");
        assert!(authorize_wid("aa", "aa").is_err());
    }
    #[test]
    fn a_native_window_reads_only_its_own_apps_status() {
        assert_eq!(authorize_app_read("win-my-app-3", "my-app"), Ok(()));
        assert!(authorize_app_read("win-my-app-3", "other").is_err());
        assert!(authorize_app_read("win-my-app", "my-app").is_err());
        assert_eq!(authorize_app_read("app-aa", "aa"), Ok(()));
        assert!(authorize_app_read("login", "aa").is_err());
    }
    #[test]
    fn labels_must_be_well_formed() {
        assert!(caller_of("app-../x").is_err());
        assert!(caller_of("app-").is_err());
        assert!(caller_of("evil").is_err());
        assert!(authorize_app("main", "../x").is_err());
        assert_eq!(app_label("aa-b").unwrap(), "app-aa-b");
    }
}
