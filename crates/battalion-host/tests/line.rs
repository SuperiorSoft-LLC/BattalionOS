// SPDX-License-Identifier: MPL-2.0
//! Guard: the line between BattalionOS apps and the other kind of extension the products built on it
//! may have. The OS manages apps only. No OS crate may depend on a host, SDK or WIT crate of that
//! other kind, and no OS source may carry that kind's word (a type, field, command, route, event or
//! UI text). `docs/apps.md` is the only place the word may appear. Words are built from pieces so
//! this file does not trip itself.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            // `gen` holds files the Tauri build generates from its own crates.
            if !matches!(name.as_str(), "target" | ".git" | "node_modules" | "gen" | "dist") {
                walk(&p, out);
            }
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("rs" | "toml" | "json" | "ts" | "tsx" | "js" | "css" | "html" | "md")) {
            out.push(p);
        }
    }
}

fn files(dirs: &[&str]) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for d in dirs {
        walk(&root().join(d), &mut v);
    }
    v.push(root().join("Cargo.toml"));
    v.retain(|f| f.file_name().is_none_or(|n| n != "line.rs" && n != "package-lock.json"));
    v
}

fn word() -> String {
    ["plug", "in"].concat()
}

/// Names of the third-party build tools that carry the word in their own names. They are not ours.
fn third_party() -> Vec<String> {
    let w = word();
    vec![format!("tauri-{w}"), format!("tauri_{w}"), format!("@vitejs/{w}-react"), format!(".{w}("), format!("\"{w}s\""), format!("{w}s: [")]
}

#[test]
fn no_os_crate_depends_on_a_plugin_host_sdk_or_wit_crate() {
    let w = word();
    let bad = [format!("{w}-host"), format!("{w}-sdk"), format!("{w}-wit")];
    let mut hits = Vec::new();
    let mut n = 0;
    for f in files(&["crates"]) {
        if f.file_name().is_some_and(|n| n == "Cargo.toml") {
            n += 1;
            let text = std::fs::read_to_string(&f).unwrap_or_default().to_lowercase();
            for b in &bad {
                if text.contains(b.as_str()) {
                    hits.push(format!("{} names {b}", f.display()));
                }
            }
        }
    }
    // The lock file lists every crate the build resolves, ours or not.
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).unwrap_or_default().to_lowercase();
    for b in &bad {
        if lock.contains(&format!("name = \"{b}")) || lock.lines().any(|l| l.trim_start().starts_with('"') && l.contains(b.as_str()) && !l.contains("tauri")) {
            hits.push(format!("Cargo.lock resolves a crate named like {b}"));
        }
    }
    assert!(n >= 3, "guard found too few manifests: {n}");
    assert!(hits.is_empty(), "the OS must not depend on a plugin host, sdk or wit crate: {hits:?}");
}

#[test]
fn the_os_source_has_no_plugin_type_field_command_route_event_or_text() {
    let w = word();
    let allowed = third_party();
    let all = files(&["crates", "fixtures", "scripts", "packaging", ".cargo"]);
    assert!(all.len() > 10, "guard found too few files: {}", all.len());
    let mut hits = Vec::new();
    for f in all {
        // docs/ is not scanned; docs/apps.md is where the word may appear.
        let text = std::fs::read_to_string(&f).unwrap_or_default().to_lowercase();
        for (i, line) in text.lines().enumerate() {
            let mut l = line.to_string();
            for a in &allowed {
                l = l.replace(a.as_str(), "");
            }
            if l.contains(&w) {
                hits.push(format!("{}:{}", f.display(), i + 1));
            }
        }
    }
    assert!(hits.is_empty(), "the OS names no plugin; found at: {hits:?}");
}
