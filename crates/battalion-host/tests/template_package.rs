// SPDX-License-Identifier: MPL-2.0
//! APP-API-1: a package made by the page-app template's Node packer (templates/page-app/scripts/package.mjs, committed in
//! fixtures/template-package) passes the OS's own checks: sha256, minisign signature with the catalog key, unzip, manifest.

use battalion_host::armory::{Armory, Settings};
use battalion_host::state::Current;
use battalion_host::supervisor::{platform_key, Settings as SupSettings};
use battalion_host::Paths;
use std::path::Path;
use std::sync::Arc;

mod common;

fn file_url(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/template-package")
}

/// The fixture laid out as a local repo (what a publish writes), its urls pointing at the copied files.
fn world(name: &str, tamper: bool) -> (std::path::PathBuf, Paths) {
    let root = std::env::temp_dir().join(format!("bos-template-pkg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::new(root.join("home"));
    paths.ensure().unwrap();
    // A local repo is read only with the device's key in place (here: the template's throwaway key).
    std::fs::create_dir_all(paths.root().join("keys")).unwrap();
    std::fs::copy(fixture().join("public.key"), paths.root().join("keys/local-repo.pub")).unwrap();
    let dir = paths.root().join("local-repos").join("template");
    let app = dir.join("apps").join("page-app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(dir.join("repo.json"), r#"{"schema":1,"name":"Template"}"#).unwrap();
    let mut feed: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture().join("feed.json")).unwrap()).unwrap();
    for (plat, p) in feed["platforms"].as_object_mut().unwrap() {
        let file = format!("page-app-1.0.0-{plat}.bosapp");
        let mut bytes = std::fs::read(fixture().join(&file)).unwrap();
        if tamper {
            let n = bytes.len() / 2;
            bytes[n] ^= 1;
            p["sha256"] = battalion_host::armory::sha256_hex(&bytes).into();
        }
        std::fs::write(app.join(&file), &bytes).unwrap();
        p["url"] = file_url(&app.join(&file)).into();
    }
    std::fs::write(app.join("stable.json"), feed.to_string()).unwrap();
    let mut entry: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture().join("catalog-entry.json")).unwrap()).unwrap();
    entry["channels"]["stable"] = file_url(&app.join("stable.json")).into();
    std::fs::write(dir.join("catalog.json"), serde_json::json!({"schema": 2, "apps": [entry]}).to_string()).unwrap();
    (root, paths)
}

fn armory(paths: &Paths) -> Armory {
    let sup = common::sup(paths.clone(), SupSettings { auto_restart: false, ..SupSettings::default() });
    Armory::with_parts(paths.clone(), sup, "1.0.0", Settings::default(), Arc::new(battalion_host::armory::DefaultFetcher { timeout_ms: 5000, allow_loopback_http: false }))
}

#[test]
fn a_template_made_package_installs_through_the_armory() {
    let feed: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture().join("feed.json")).unwrap()).unwrap();
    assert!(feed["platforms"].get(platform_key()).is_some(), "the template packs {}", platform_key());
    let (root, paths) = world("ok", false);
    armory(&paths).install("page-app", "stable").unwrap();
    let cur = Current::load(&paths.current_json("page-app").unwrap()).unwrap().unwrap();
    assert_eq!(cur.version, "1.0.0");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_changed_template_package_is_refused_by_its_signature() {
    let (root, paths) = world("tamper", true);
    let e = armory(&paths).install("page-app", "stable").unwrap_err();
    assert!(e.to_string().to_lowercase().contains("signature"), "{e}");
    assert!(Current::load(&paths.current_json("page-app").unwrap()).unwrap().is_none());
    let _ = std::fs::remove_dir_all(root);
}
