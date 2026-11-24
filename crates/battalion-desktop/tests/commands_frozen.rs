// SPDX-License-Identifier: MPL-2.0
//! The command surface is frozen in docs/commands.md. These tests read the sources as text, so they
//! run without the GUI feature.

use std::collections::BTreeSet;

fn read(p: &str) -> String {
    std::fs::read_to_string(format!("{}/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Every `#[tauri::command]` item: (is_async, name).
fn commands() -> Vec<(bool, String)> {
    let src = read("src/commands.rs");
    let mut out = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        if l.trim() == "#[tauri::command]" {
            let f = lines[i + 1].trim();
            let is_async = f.starts_with("pub async fn ");
            let name = f.trim_start_matches("pub ").trim_start_matches("async ").trim_start_matches("fn ").split('(').next().unwrap();
            out.push((is_async, name.to_string()));
        }
    }
    out
}

#[test]
fn no_synchronous_command() {
    let c = commands();
    assert!(!c.is_empty());
    for (is_async, name) in c {
        assert!(is_async, "{name} is a synchronous command (it would block the UI thread)");
    }
}

#[test]
fn every_command_is_named_in_the_build_the_handler_and_the_docs() {
    let names: BTreeSet<String> = commands().into_iter().map(|c| c.1).collect();
    let build = read("build.rs");
    let main = read("src/main.rs");
    let docs = std::fs::read_to_string(format!("{}/../../docs/commands.md", env!("CARGO_MANIFEST_DIR"))).unwrap();
    for n in &names {
        assert!(n.starts_with("os_"), "{n} must start with os_");
        assert!(build.contains(&format!("\"{n}\"")), "{n} missing from build.rs COMMANDS");
        assert!(main.contains(n.as_str()), "{n} missing from the invoke handler");
        assert!(docs.contains(&format!("`{n}`")), "{n} missing from docs/commands.md");
    }
}

#[test]
fn every_command_checks_the_calling_window() {
    // The label comes from the runtime window. A command without a check would let any window act.
    let src = read("src/commands.rs");
    for chunk in src.split("#[tauri::command]").skip(1) {
        let name = chunk.split("fn ").nth(1).unwrap().split('(').next().unwrap();
        assert!(chunk.contains("window: WebviewWindow"), "{name} takes no calling window");
        let body_checked = chunk.contains("authorize_app(") || chunk.contains("authorize_shell(") || chunk.contains("authorize_self(") || chunk.contains("caller_of(") || chunk.contains("crate::access::LOGIN");
        assert!(body_checked, "{name} does not check who is calling");
    }
}

#[test]
fn app_capability_grants_only_self_service_commands() {
    let app = read("capabilities/app.json");
    for forbidden in ["os-start-app", "os-stop-app", "os-install-app", "os-list-apps", "os-catalog", "os-open-app-window", "os-open-login", "os-update-app", "os-rollback-app", "os-remove-app", "os-exit"] {
        assert!(!app.contains(&format!("allow-{forbidden}\"")), "app windows must not be granted {forbidden}");
    }
}

#[test]
fn app_windows_get_the_two_self_commands_and_they_take_no_id() {
    let app = read("capabilities/app.json");
    for c in ["os-badge", "os-show-desktop"] {
        assert!(app.contains(&format!("allow-{c}\"")), "app windows must be granted {c}");
    }
    // The app is the calling window: neither command has an id argument.
    let src = read("src/commands.rs");
    for name in ["os_badge", "os_show_desktop"] {
        let sig = src.split(&format!("pub async fn {name}(")).nth(1).unwrap().split(')').next().unwrap();
        assert!(!sig.contains("id:"), "{name} must not take an id");
        assert!(sig.contains("window: WebviewWindow"));
    }
}

#[test]
fn an_app_opens_inside_the_window_not_in_one_of_its_own() {
    let src = read("src/commands.rs");
    let body = src.split("pub async fn os_open_app_window(").nth(1).unwrap().split("#[tauri::command]").next().unwrap();
    assert!(!body.contains("WebviewWindowBuilder") && !body.contains("open_app_window("), "os_open_app_window must not open a window of its own");
    assert!(body.contains("APP_OPEN"), "it says os://app-open so the shell opens the view");
}

#[test]
fn a_frame_gets_no_command_rights_of_the_window_that_holds_it() {
    // The shell window (main) has no remote url: a page in a frame of it is not the shell, so Tauri refuses its IPC.
    let main = read("capabilities/main.json");
    assert!(!main.contains("\"remote\""), "main must be local only");
    // The two shell commands that serve frames are main only and never granted to an app window.
    let app = read("capabilities/app.json");
    for c in ["os-app-page", "os-frame-badge"] {
        assert!(main.contains(&format!("allow-{c}\"")));
        assert!(!app.contains(c), "{c} is not for app windows");
    }
    let src = read("src/commands.rs");
    for name in ["os_app_page", "os_frame_badge"] {
        let body = src.split(&format!("pub async fn {name}(")).nth(1).unwrap().split("#[tauri::command]").next().unwrap();
        assert!(body.contains("authorize_shell("), "{name} is for the shell only");
    }
}
