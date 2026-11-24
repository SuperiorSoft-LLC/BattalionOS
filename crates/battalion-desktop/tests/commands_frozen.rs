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
        let body_checked = chunk.contains("authorize_app(") || chunk.contains("authorize_shell(") || chunk.contains("caller_of(") || chunk.contains("crate::access::LOGIN");
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
