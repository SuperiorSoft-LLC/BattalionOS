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
        let body_checked = chunk.contains("authorize_app(") || chunk.contains("authorize_shell(") || chunk.contains("authorize_self(") || chunk.contains("caller_of(") || chunk.contains("authorize_win_caller(") || chunk.contains("crate::access::LOGIN");
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

#[test]
fn the_window_has_no_windows_title_bar_and_only_main_may_drive_it() {
    // BOS-27: decorations off on `main`; the shell draws the bar, so main (and only main) may minimize, maximize, drag and resize it.
    assert!(read("src/main.rs").contains(".decorations(false)"));
    let main = read("capabilities/main.json");
    let app = read("capabilities/app.json");
    for c in ["minimize", "toggle-maximize", "start-dragging", "start-resize-dragging", "close"] {
        assert!(main.contains(&format!("core:window:allow-{c}\"")), "main needs core:window:allow-{c}");
        assert!(!app.contains(&format!("allow-{c}\"")) || c == "close", "{c} is not for app windows");
    }
}

#[test]
fn the_os_update_commands_are_shell_only_and_never_granted_to_an_app_window() {
    // BOS-30: only the shell window may read, check or run an update of the OS itself.
    let app = read("capabilities/app.json");
    let main = read("capabilities/main.json");
    let src = read("src/commands.rs");
    for (name, perm) in [("os_update_state", "os-update-state"), ("os_update_check", "os-update-check"), ("os_update_install", "os-update-install")] {
        assert!(main.contains(&format!("allow-{perm}\"")), "main is granted {name}");
        assert!(!app.contains(perm), "{name} is not for app windows");
        let body = src.split(&format!("pub async fn {name}(")).nth(1).unwrap().split("#[tauri::command]").next().unwrap();
        assert!(body.contains("authorize_shell("), "{name} is for the shell only");
    }
}

#[test]
fn a_release_build_has_no_door_to_another_feed_or_key() {
    // BOS-30: the feed and key overrides exist only behind the `test-update` feature, and the release workflow never names it.
    let src = read("src/osupdate.rs");
    let code: String = src.split("#[cfg(test)]").next().unwrap().lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    for var in ["BATTALION_OS_UPDATE_FEED", "BATTALION_OS_UPDATE_PUBKEY"] {
        let at = code.find(var).expect("the override is there");
        assert!(code[..at].rfind("#[cfg(feature = \"test-update\")]").is_some_and(|f| at - f < 200), "{var} must sit behind the test-update feature");
    }
    assert!(!read("Cargo.toml").contains("default = [\"gui\", \"test-update\"]"), "test-update is not a default feature");
    assert!(read("Cargo.toml").contains("default = [\"gui\"]"));
}

#[test]
fn the_window_model_and_its_calls_are_documented() {
    // OS-WINDOW-1 (WM-1): the model is in docs/commands.md. WM-2: the os_win_* calls and the os_win event.
    let docs = std::fs::read_to_string(format!("{}/../../docs/commands.md", env!("CARGO_MANIFEST_DIR"))).unwrap();
    assert!(docs.contains("## Windows (OS-WINDOW-1)"));
    assert!(docs.contains("bos.windows.saveDebounceMs"));
    assert!(docs.contains("`os://win`") && docs.contains("event: \"os_win\""));
}

const WIN_CALLS: &[&str] = &["open", "close", "minimize", "restore", "maximize", "focus", "snap", "set", "list"];

#[test]
fn an_app_window_gets_the_window_calls_for_itself_and_the_shell_serves_frames() {
    // WM-2: os_win_* for an app's own window (app-* and native win-*), the app taken from the label; os_frame_win_* main only.
    let app = read("capabilities/app.json");
    let main = read("capabilities/main.json");
    let src = read("src/commands.rs");
    assert!(app.contains("\"win-*\""), "native windows of an app get the app capability");
    let names: BTreeSet<String> = commands().into_iter().map(|c| c.1).collect();
    for c in WIN_CALLS {
        let (own, frame) = (format!("os_win_{c}"), format!("os_frame_win_{c}"));
        assert!(names.contains(&own) && names.contains(&frame), "{own} and {frame} are commands");
        assert!(app.contains(&format!("allow-os-win-{c}\"")), "app windows are granted {own}");
        assert!(!main.contains(&format!("allow-os-win-{c}\"")), "the shell uses {frame}, not {own}");
        assert!(main.contains(&format!("allow-os-frame-win-{c}\"")) && !app.contains(&format!("os-frame-win-{c}")), "{frame} is main only");
        let body = |n: &str| src.split(&format!("pub async fn {n}(")).nth(1).unwrap().split("#[tauri::command]").next().unwrap().to_string();
        let own_body = body(&own);
        assert!(own_body.contains("authorize_win_caller("), "{own} speaks for the app of its label");
        let sig = own_body.split(')').next().unwrap();
        assert!(!sig.contains(" id:"), "{own} must not take an app id");
        assert!(body(&frame).contains("authorize_shell("), "{frame} is for the shell only");
    }
    // Every call checks the `window` permission, and there is no cap on the number of windows.
    let rules = read("src/win_calls.rs");
    assert!(rules.contains("frame_calls::require(app, permissions, cmd)"));
    assert!(read("src/frame_calls.rs").contains("win_calls::COMMANDS.contains(&c) => Some(\"window\")"));
    let settings = read("src/windows.rs").split("pub struct WindowSettings").nth(1).unwrap().split('}').next().unwrap().to_string();
    assert!(!settings.to_lowercase().contains("max"), "bos.windows.max stays unset: no cap");
}
