// SPDX-License-Identifier: MPL-2.0
//! BOS-AND-SELFUPDATE-1: BattalionOS on Android reads its own feed (bos-android.json): newer / same / older, a broken sha256 in
//! the feed, and an APK whose bytes do not match the feed are refused and leave no file.

use battalion_host::armory::osupdate::ApkFeed;
use battalion_host::armory::{sha256_hex, Armory, ArmoryError, Settings};
use battalion_host::supervisor::Settings as SupSettings;
use battalion_host::Paths;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod common;

fn file_url(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

fn world(name: &str) -> (PathBuf, Armory) {
    let root = std::env::temp_dir().join(format!("bos-apkupd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::new(root.join("home"));
    paths.ensure().unwrap();
    let sup = common::sup(paths.clone(), SupSettings { kill_wait_ms: 5_000, auto_restart: false, ..Default::default() });
    let a = Armory::with_parts(paths, sup, "0.1.5", Settings::default(), Arc::new(battalion_host::armory::DefaultFetcher { timeout_ms: 5000, allow_loopback_http: false }));
    (root, a)
}

fn feed(root: &Path, version: &str, apk: &[u8], sha: Option<&str>) -> ApkFeed {
    let apk_path = root.join("b.apk");
    std::fs::write(&apk_path, apk).unwrap();
    let sha = sha.map(str::to_string).unwrap_or_else(|| sha256_hex(apk));
    let f = root.join("bos-android.json");
    std::fs::write(&f, format!(r#"{{"version":"{version}","versionCode":106,"url":"{}","sha256":"{sha}","commit":"x"}}"#, file_url(&apk_path))).unwrap();
    ApkFeed { url: file_url(&f), allow_loopback_http: false }
}

#[test]
fn a_newer_apk_is_offered_and_staged_when_its_sha256_matches() {
    let (root, a) = world("ok");
    let f = feed(&root, "0.1.6", b"apk bytes", None);
    let u = a.os_check_apk(&f, "0.1.5").unwrap().expect("0.1.6 > 0.1.5");
    assert_eq!(u.version, "0.1.6");
    let p = a.os_stage_apk(&f, &u, &mut |_, _| {}).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"apk bytes");
    assert!(p.extension().is_some_and(|e| e == "apk"));
}

#[test]
fn same_or_older_is_not_offered() {
    let (root, a) = world("same");
    assert!(a.os_check_apk(&feed(&root, "0.1.5", b"x", None), "0.1.5").unwrap().is_none());
    assert!(a.os_check_apk(&feed(&root, "0.1.4", b"x", None), "0.1.5").unwrap().is_none());
}

#[test]
fn an_apk_that_does_not_match_the_feed_is_refused_and_removed() {
    let (root, a) = world("bad");
    let f = feed(&root, "0.1.6", b"tampered", Some(&sha256_hex(b"original")));
    let u = a.os_check_apk(&f, "0.1.5").unwrap().unwrap();
    let e = a.os_stage_apk(&f, &u, &mut |_, _| {}).unwrap_err();
    assert!(matches!(e, ArmoryError::BadSignature(ref t) if t.contains("does not match")), "{e:?}");
    let left: Vec<_> = std::fs::read_dir(a.paths().staging()).unwrap().flatten().collect();
    assert!(left.is_empty(), "no APK is left behind");
}

#[test]
fn a_feed_without_a_real_sha256_or_offline_is_an_error() {
    let (root, a) = world("broken");
    assert!(matches!(a.os_check_apk(&feed(&root, "0.1.6", b"x", Some("abc")), "0.1.5"), Err(ArmoryError::Feed(_))));
    let gone = ApkFeed { url: file_url(&root.join("missing.json")), allow_loopback_http: false };
    assert!(a.os_check_apk(&gone, "0.1.5").is_err());
    let http = ApkFeed { url: "http://example.com/bos-android.json".into(), allow_loopback_http: false };
    assert!(a.os_check_apk(&http, "0.1.5").is_err(), "a release build never reads plain http");
}
