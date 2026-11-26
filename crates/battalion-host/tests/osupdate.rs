// SPDX-License-Identifier: MPL-2.0
//! BOS-30: the OS updates itself. Feed newer / same / older, a bad signature is refused before any app stops, a missing feed
//! is an error the caller keeps quiet about, and the apps that ran are started again from the resume list.

use battalion_host::armory::osupdate::{OsFeed, OsUpdate};
use battalion_host::armory::{Armory, ArmoryError, Settings};
use battalion_host::state::{AppState, Current};
use battalion_host::supervisor::{platform_key, Settings as SupSettings};
use battalion_host::{store, Paths};
use blake2::{Blake2b512, Digest};
use ed25519_dalek::{Signer, SigningKey};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod common;

const KEY_ID: [u8; 8] = [9, 8, 7, 6, 5, 4, 3, 2];

fn b64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}
fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}
/// The pubkey as `tauri.conf.json` holds it: base64 of the `.pub` file.
fn pubkey(sk: &SigningKey) -> String {
    let mut b = b"Ed".to_vec();
    b.extend_from_slice(&KEY_ID);
    b.extend_from_slice(sk.verifying_key().as_bytes());
    b64(format!("untrusted comment: test\n{}\n", b64(&b)).as_bytes())
}
/// The signature as `latest.json` holds it: base64 of the `.sig` file.
fn sign(sk: &SigningKey, data: &[u8]) -> String {
    let hash = Blake2b512::digest(data);
    let sig = sk.sign(&hash).to_bytes();
    let mut bin = b"ED".to_vec();
    bin.extend_from_slice(&KEY_ID);
    bin.extend_from_slice(&sig);
    let trusted = "t";
    let mut g = sig.to_vec();
    g.extend_from_slice(trusted.as_bytes());
    let global = sk.sign(&g).to_bytes();
    b64(format!("untrusted comment: test\n{}\ntrusted comment: {trusted}\n{}\n", b64(&bin), b64(&global)).as_bytes())
}
fn file_url(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

struct World {
    root: PathBuf,
    paths: Paths,
}
impl World {
    fn new(name: &str) -> World {
        let root = std::env::temp_dir().join(format!("bos-osupd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.join("home"));
        paths.ensure().unwrap();
        World { root, paths }
    }
    fn armory(&self) -> Armory {
        let sup = common::sup(self.paths.clone(), SupSettings { kill_wait_ms: 5_000, auto_restart: false, ..Default::default() });
        Armory::with_parts(self.paths.clone(), sup, "1.0.0", Settings::default(), Arc::new(battalion_host::armory::DefaultFetcher { timeout_ms: 5000, allow_loopback_http: false }))
    }
    /// Publish an installer signed by key `signer` (the OS pins key 7), a feed that names version `ver`.
    fn publish(&self, ver: &str, signer: u8, tamper: bool) -> OsFeed {
        let mut msi = format!("pretend installer {ver}").into_bytes();
        let sig = sign(&key(signer), &msi);
        if tamper {
            msi.push(b'!');
        }
        let msi_path = self.root.join("BattalionOS.msi");
        std::fs::write(&msi_path, &msi).unwrap();
        let latest = serde_json::json!({
            "version": ver, "notes": "n", "pub_date": "2026-11-24T00:00:00Z",
            "platforms": { "windows-x86_64": { "signature": sig, "url": file_url(&msi_path) } }
        });
        let feed = self.root.join("latest.json");
        std::fs::write(&feed, latest.to_string()).unwrap();
        OsFeed { url: file_url(&feed), pubkey: pubkey(&key(7)), platform: "windows-x86_64".into() }
    }
    /// A demo app, installed and running.
    fn running_app(&self, id: &str, sup: &battalion_host::Supervisor) {
        let vdir = self.paths.version_dir(id, "1.0.0").unwrap();
        common::lay_demo(&vdir);
        let manifest = serde_json::json!({
            "manifest": 1, "id": id, "name": id, "version": "1.0.0", "contract": { "min": 1, "max": 1 },
            "process": { platform_key(): { "library": common::LIB_IN_PACKAGE, "args": [], "readyTimeoutMs": 5000, "stopTimeoutMs": 3000 } }
        });
        std::fs::write(vdir.join("app.json"), manifest.to_string()).unwrap();
        let cur = Current { version: "1.0.0".into(), previous: None, state: AppState::Stopped, public_key: String::new() };
        store::write_json(&self.paths.current_json(id).unwrap(), &cur).unwrap();
        sup.start(id).unwrap();
    }
}

#[test]
fn a_newer_feed_is_offered_a_same_or_older_one_is_not() {
    let w = World::new("versions");
    let a = w.armory();
    let feed = w.publish("0.2.0", 7, false);
    let u = a.os_check(&feed, "0.1.0").unwrap().expect("newer is offered");
    assert_eq!(u.version, "0.2.0");
    assert_eq!(a.os_check(&feed, "0.2.0").unwrap(), None, "same");
    assert_eq!(a.os_check(&feed, "0.3.0").unwrap(), None, "older");
    assert_eq!(a.os_check(&feed, "0.1.9").unwrap().map(|u| u.version), Some("0.2.0".into()), "compared as numbers");
}

#[test]
fn a_missing_feed_is_an_error_the_caller_keeps_quiet_about() {
    let w = World::new("missing");
    let a = w.armory();
    let feed = OsFeed { url: file_url(&w.root.join("nope.json")), pubkey: pubkey(&key(7)), platform: "windows-x86_64".into() };
    assert!(matches!(a.os_check(&feed, "0.1.0"), Err(ArmoryError::Fetch { .. })));
    std::fs::write(w.root.join("bad.json"), "<html>404</html>").unwrap();
    let feed = OsFeed { url: file_url(&w.root.join("bad.json")), ..feed };
    assert!(matches!(a.os_check(&feed, "0.1.0"), Err(ArmoryError::Feed(_))));
}

#[test]
fn a_good_signature_stages_the_installer_then_stops_the_apps_and_resume_starts_them_again() {
    let w = World::new("good");
    let a = w.armory();
    w.running_app("alpha", &a.supervisor());
    let feed = w.publish("0.2.0", 7, false);
    let upd = a.os_check(&feed, "0.1.0").unwrap().unwrap();
    let (msi, stopped) = a.os_prepare(&feed, &upd, &mut |_, _| {}).unwrap();
    assert!(msi.is_file());
    assert_eq!(stopped, vec!["alpha".to_string()]);
    assert!(a.supervisor().status("alpha").unwrap().pid.is_none(), "stopped");
    assert!(a.os_resume_pending());
    // The next start (the new OS, or the old one after a cancelled installer) starts them again, once.
    let a2 = w.armory();
    assert!(a2.os_resume_apps().is_empty());
    assert!(a2.supervisor().status("alpha").unwrap().pid.is_some(), "running again");
    assert!(!a2.os_resume_pending());
    assert!(a2.os_resume_apps().is_empty(), "the list is used once");
    a2.exit_os();
}

#[test]
fn a_bad_signature_is_refused_before_any_app_stops_and_leaves_no_file() {
    let w = World::new("bad");
    let a = w.armory();
    w.running_app("alpha", &a.supervisor());
    let pid = a.supervisor().status("alpha").unwrap().pid.unwrap();
    for (what, feed) in [("tampered installer", w.publish("0.2.0", 7, true)), ("wrong signer", w.publish("0.2.0", 8, false))] {
        let upd = a.os_check(&feed, "0.1.0").unwrap().unwrap();
        let e = a.os_prepare(&feed, &upd, &mut |_, _| {}).unwrap_err();
        assert!(matches!(e, ArmoryError::BadSignature(_)), "{what}: {e}");
        assert_eq!(a.supervisor().status("alpha").unwrap().pid, Some(pid), "{what}: the app was not touched");
        assert!(!a.os_resume_pending(), "{what}");
        assert_eq!(std::fs::read_dir(w.paths.staging()).map(|d| d.count()).unwrap_or(0), 0, "{what}: no installer left");
    }
    // A key that is not a key at all is refused the same way.
    let mut feed = w.publish("0.2.0", 7, false);
    feed.pubkey = "not a key".into();
    let upd: OsUpdate = a.os_check(&feed, "0.1.0").unwrap().unwrap();
    assert!(a.os_prepare(&feed, &upd, &mut |_, _| {}).is_err());
    assert_eq!(a.supervisor().status("alpha").unwrap().pid, Some(pid));
    a.exit_os();
}
