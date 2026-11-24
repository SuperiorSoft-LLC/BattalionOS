// SPDX-License-Identifier: MPL-2.0
//! Who may act on whom. A window's label is its identity: `main`, `login`, or `app-<id>`.
//! The label comes from the Tauri runtime (the calling webview), never from the command's arguments,
//! so an app cannot claim to be another one.

use battalion_app_api::manifest::valid_id;

pub const MAIN: &str = "main";
pub const LOGIN: &str = "login";
pub const APP_PREFIX: &str = "app-";

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
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Denied::UnknownLabel(l) => write!(f, "unknown window '{l}'"),
            Denied::BadAppId(i) => write!(f, "bad app id '{i}'"),
            Denied::NotYours { caller, target } => write!(f, "window '{caller}' may not act on app '{target}'"),
            Denied::ShellOnly(c) => write!(f, "'{c}' is for the OS shell only"),
        }
    }
}

pub fn app_label(id: &str) -> Result<String, Denied> {
    if valid_id(id) {
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
    if !valid_id(target) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn labels_must_be_well_formed() {
        assert!(caller_of("app-../x").is_err());
        assert!(caller_of("app-").is_err());
        assert!(caller_of("evil").is_err());
        assert!(authorize_app("main", "../x").is_err());
        assert_eq!(app_label("aa-b").unwrap(), "app-aa-b");
    }
}
