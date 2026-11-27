// SPDX-License-Identifier: MPL-2.0
//! LOCAL-REPO-1: local repos. A repo is written here in the layout `infantry app publish` writes (repo.json,
//! catalog.json, apps/<id>/<channel>.json, the .bosapp and its minisign signature, keys/local-repo.pub), all in a
//! temp dir. Publish -> list -> install -> update; tampered or foreign-key packages are refused; remote sources
//! are listed as before.

use battalion_host::armory::{Armory, ArmoryError, Settings, UpdateOptions, LOCAL_LABEL};
use battalion_host::state::Current;
use battalion_host::supervisor::{platform_key, Settings as SupSettings};
use battalion_host::Paths;
use blake2::{Blake2b512, Digest};
use ed25519_dalek::{Signer, SigningKey};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod common;

const KEY_ID: [u8; 8] = [9, 8, 7, 6, 5, 4, 3, 2];

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}
fn pub_b64(sk: &SigningKey) -> String {
    let mut b = b"Ed".to_vec();
    b.extend_from_slice(&KEY_ID);
    b.extend_from_slice(sk.verifying_key().as_bytes());
    b64(&b)
}
fn sign(sk: &SigningKey, data: &[u8]) -> String {
    let sig = sk.sign(&Blake2b512::digest(data)).to_bytes();
    let mut bin = b"ED".to_vec();
    bin.extend_from_slice(&KEY_ID);
    bin.extend_from_slice(&sig);
    let mut g = sig.to_vec();
    g.extend_from_slice(b"t");
    format!("untrusted comment: local\n{}\ntrusted comment: t\n{}\n", b64(&bin), b64(&sk.sign(&g).to_bytes()))
}
fn file_url(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

struct World {
    root: PathBuf,
    paths: Paths,
    device: SigningKey,
}

impl World {
    fn new(name: &str) -> World {
        let root = std::env::temp_dir().join(format!("bos-localrepo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.join("home"));
        paths.ensure().unwrap();
        let device = SigningKey::from_bytes(&[42; 32]);
        std::fs::create_dir_all(paths.root().join("keys")).unwrap();
        std::fs::write(paths.root().join("keys/local-repo.pub"), pub_b64(&device) + "\n").unwrap();
        World { root, paths, device }
    }

    fn armory(&self, sources: Vec<String>) -> Armory {
        let sup = common::sup(self.paths.clone(), SupSettings { auto_restart: false, ..SupSettings::default() });
        let st = Settings { sources, ..Settings::default() };
        Armory::with_parts(self.paths.clone(), sup, "1.0.0", st, Arc::new(battalion_host::armory::DefaultFetcher { timeout_ms: 5000, allow_loopback_http: false }))
    }

    fn page_zip(id: &str, version: &str) -> Vec<u8> {
        let manifest = serde_json::json!({"manifest":1,"id":id,"name":"Hello","version":version,"contract":{"min":1,"max":1},"ui":{"entry":"ui/index.html"},"state":{"schema":1}});
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file("app.json", o).unwrap();
        w.write_all(manifest.to_string().as_bytes()).unwrap();
        w.start_file("ui/index.html", o).unwrap();
        w.write_all(format!("<h1>hello {version}</h1>").as_bytes()).unwrap();
        w.finish().unwrap().into_inner()
    }

    /// What `infantry app publish` writes: package + sig + feed + catalog entry, in `<home>/local-repos/<slug>/`.
    fn publish(&self, repo: &str, id: &str, version: &str, signer: &SigningKey, catalog_key: &SigningKey) -> PathBuf {
        let dir = self.paths.root().join("local-repos").join(repo.to_lowercase().replace(' ', "-"));
        let app = dir.join("apps").join(id);
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(dir.join("repo.json"), serde_json::json!({"schema":1,"name":repo}).to_string()).unwrap();
        let zip = Self::page_zip(id, version);
        let pkg = app.join(format!("{id}-{version}-{}.bosapp", platform_key()));
        std::fs::write(&pkg, &zip).unwrap();
        let feed = serde_json::json!({"version":version,"contract":1,"stateSchema":1,"platforms":{platform_key():{
            "url":file_url(&pkg),"signature":sign(signer,&zip),"sha256":battalion_host::armory::sha256_hex(&zip),"size":zip.len()}}});
        std::fs::write(app.join("stable.json"), feed.to_string()).unwrap();
        let cat = serde_json::json!({"schema":2,"apps":[{"id":id,"name":"Hello","publicKey":pub_b64(catalog_key),"contract":{"min":1,"max":1},
            "channels":{"stable":file_url(&app.join("stable.json"))},"platforms":[platform_key()]}]});
        std::fs::write(dir.join("catalog.json"), cat.to_string()).unwrap();
        pkg
    }

    fn version(&self, id: &str) -> Option<String> {
        Current::load(&self.paths.current_json(id).unwrap()).unwrap().map(|c| c.version)
    }
}

#[test]
fn publish_list_install_and_update_from_a_local_repo() {
    let w = World::new("flow");
    w.publish("My apps", "hello", "1.0.0", &w.device, &w.device);
    let a = w.armory(vec![]);
    let l = a.catalog().unwrap();
    let e = l.apps.iter().find(|e| e.app.id == "hello").expect("listed");
    assert!(e.from_local_repo);
    assert_eq!(e.source, "local:My apps");
    assert!(!LOCAL_LABEL.is_empty());
    a.install("hello", "stable").unwrap();
    assert_eq!(w.version("hello").as_deref(), Some("1.0.0"));

    w.publish("My apps", "hello", "1.0.1", &w.device, &w.device);
    let a = w.armory(vec![]);
    a.update("hello", &UpdateOptions::default()).unwrap();
    assert_eq!(w.version("hello").as_deref(), Some("1.0.1"));
}

#[test]
fn a_tampered_local_package_is_refused() {
    let w = World::new("tamper");
    let pkg = w.publish("My apps", "hello", "1.0.0", &w.device, &w.device);
    let mut b = std::fs::read(&pkg).unwrap();
    let n = b.len() / 2;
    b[n] ^= 0xff;
    std::fs::write(&pkg, &b).unwrap();
    let err = w.armory(vec![]).install("hello", "stable").unwrap_err();
    assert!(matches!(err, ArmoryError::ShaMismatch | ArmoryError::BadSignature(_)), "got {err:?}");
    assert!(w.version("hello").is_none());
}

#[test]
fn a_package_signed_by_another_key_is_refused_even_when_the_feed_matches() {
    let w = World::new("foreign-sig");
    let other = SigningKey::from_bytes(&[5; 32]);
    w.publish("My apps", "hello", "1.0.0", &other, &w.device);
    let err = w.armory(vec![]).install("hello", "stable").unwrap_err();
    assert!(matches!(err, ArmoryError::BadSignature(_)), "got {err:?}");
}

#[test]
fn a_local_entry_not_pinned_to_the_device_key_is_not_listed() {
    let w = World::new("foreign-key");
    let other = SigningKey::from_bytes(&[5; 32]);
    w.publish("My apps", "hello", "1.0.0", &other, &other);
    let l = w.armory(vec![]).catalog().unwrap();
    assert!(l.apps.is_empty());
    assert!(l.rejected.iter().any(|(id, why)| id == "hello" && why.contains("device's local key")), "{:?}", l.rejected);
}

#[test]
fn remote_sources_are_listed_as_before_and_win_a_duplicate_id() {
    let w = World::new("remote");
    let remote_key = SigningKey::from_bytes(&[7; 32]);
    let cat = w.root.join("remote-catalog.json");
    let feed = w.root.join("remote-feed.json");
    std::fs::write(&feed, "{}").unwrap();
    std::fs::write(&cat, serde_json::json!({"schema":2,"apps":[{"id":"hello","name":"Remote hello","publicKey":pub_b64(&remote_key),"contract":{"min":1,"max":1},"channels":{"stable":file_url(&feed)}}]}).to_string()).unwrap();
    let src = file_url(&cat);
    // no local repos: exactly the remote listing
    let l = w.armory(vec![src.clone()]).catalog().unwrap();
    assert_eq!(l.apps.len(), 1);
    assert_eq!((l.apps[0].source.as_str(), l.apps[0].from_local_repo), (src.as_str(), false));
    // with a local repo holding the same id: the remote one stays first, the local one is reported
    w.publish("My apps", "hello", "1.0.0", &w.device, &w.device);
    w.publish("Experiments", "other", "1.0.0", &w.device, &w.device);
    let l = w.armory(vec![src.clone()]).catalog().unwrap();
    let ids: Vec<(&str, bool)> = l.apps.iter().map(|e| (e.app.id.as_str(), e.from_local_repo)).collect();
    assert_eq!(ids, vec![("hello", false), ("other", true)]);
    assert!(l.rejected.iter().any(|(id, why)| id == "hello" && why.contains("earlier source")));
    // a remote source that is gone with local apps present is not "offline"
    let l = w.armory(vec![file_url(&w.root.join("missing.json"))]).catalog().unwrap();
    assert_eq!(l.source_errors.len(), 1);
    assert_eq!(l.apps.len(), 2);
}

#[test]
fn repos_can_be_added_renamed_and_removed() {
    let w = World::new("manage");
    let a = w.armory(vec![]);
    let r = a.add_local_repo("Experiments").unwrap();
    assert!(r.dir.join("catalog.json").is_file());
    assert!(a.add_local_repo("experiments").is_err());
    a.add_local_repo("My apps").unwrap();
    let names: Vec<String> = a.local_repos().into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["Experiments", "My apps"]);
    a.rename_local_repo("Experiments", "Playground").unwrap();
    assert!(a.rename_local_repo("Playground", "my apps").is_err());
    a.remove_local_repo("Playground").unwrap();
    assert!(!r.dir.exists());
    let names: Vec<String> = a.local_repos().into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["My apps"]);
    assert!(a.remove_local_repo("nope").is_err());
}

/// Cross-check with the real publisher: `LOCAL_REPO_E2E_HOME` = an --os-home that `infantry app publish` wrote
/// (a temp dir). Skipped when unset.
#[test]
fn installs_what_infantry_app_publish_wrote() {
    let Ok(home) = std::env::var("LOCAL_REPO_E2E_HOME") else { return };
    let paths = Paths::new(PathBuf::from(home));
    paths.ensure().unwrap();
    let sup = common::sup(paths.clone(), SupSettings { auto_restart: false, ..SupSettings::default() });
    let a = Armory::with_parts(paths.clone(), sup, "1.0.0", Settings::default(), Arc::new(battalion_host::armory::DefaultFetcher { timeout_ms: 5000, allow_loopback_http: false }));
    let l = a.catalog().unwrap();
    assert!(l.rejected.iter().all(|(_, why)| why.contains("earlier source")), "{:?}", l.rejected);
    let e = l.apps.iter().find(|e| e.from_local_repo).expect("a local app");
    let (id, ch) = (e.app.id.clone(), e.app.channels.keys().next().unwrap().clone());
    a.install(&id, &ch).unwrap();
    assert!(Current::load(&paths.current_json(&id).unwrap()).unwrap().is_some());
}
