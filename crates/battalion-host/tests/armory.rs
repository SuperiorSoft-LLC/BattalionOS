// SPDX-License-Identifier: MPL-2.0
//! Armory engine tests: real packages (zip + minisign), real fixture processes, file: sources.
//! Crash tests re-run this test binary as a child that dies (exit 99) at one journal step, then
//! recover in the parent. Every test has its own home; the fixture listens on port 0.

use battalion_host::armory::{Armory, ArmoryError, Fetcher, Settings, UpdateOptions};
use battalion_host::armory::remove::RemoveOptions;
use battalion_host::process::{self, ProcId};
use battalion_host::state::{AppState, Current};
use battalion_host::supervisor::{platform_key, Settings as SupSettings, Supervisor};
use battalion_host::{store, Paths};
use blake2::{Blake2b512, Digest};
use ed25519_dalek::{Signer, SigningKey};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod common;

// ---- fixture app --------------------------------------------------------------------------


fn fast() -> SupSettings {
    SupSettings { ready_poll_ms: 20, health_interval_ms: 200, health_timeout_ms: 500, health_fail_limit: 2, backoff_initial_ms: 100, backoff_max_ms: 400, backoff_reset_after_ms: 60_000, max_restarts: 3, kill_wait_ms: 5_000, auto_restart: false }
}

// ---- minisign (prehashed "ED", the format `minisign -S` writes) -----------------------------

const KEY_ID: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

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

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}
fn pub_b64(sk: &SigningKey) -> String {
    let mut b = b"Ed".to_vec();
    b.extend_from_slice(&KEY_ID);
    b.extend_from_slice(sk.verifying_key().as_bytes());
    b64(&b)
}
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
    format!("untrusted comment: test\n{}\ntrusted comment: {trusted}\n{}\n", b64(&bin), b64(&global))
}

// ---- world: catalog, feeds, packages on disk --------------------------------------------------

fn file_url(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

#[derive(Clone)]
struct Pkg {
    id: &'static str,
    version: &'static str,
    mode: &'static str,
    schema: u32,
    init: bool,
    contract: (u32, u32),
    min_os: Option<&'static str>,
    manifest_id: Option<&'static str>,
    extra: Vec<(String, Vec<u8>)>,
    signer: u8,
    sha_wrong: bool,
    ready_ms: u64,
    display: Option<&'static str>,
    icon: Option<&'static str>,
    on_exit: Option<&'static str>,
    /// Name of the manifest file at the package root (tests of refusals only).
    root_manifest: &'static str,
    /// A manifest that names a program (`exec`) instead of a library.
    use_exec: bool,
}

impl Pkg {
    fn new(id: &'static str, version: &'static str) -> Pkg {
        Pkg { id, version, mode: "normal", schema: 1, init: false, contract: (1, 1), min_os: None, manifest_id: None, extra: vec![], signer: 7, sha_wrong: false, ready_ms: 5000, display: None, icon: None, on_exit: None, root_manifest: "app.json", use_exec: false }
    }
}

struct World {
    root: PathBuf,
    paths: Paths,
    catalog_key: std::cell::RefCell<u8>,
}

impl World {
    fn new(name: &str) -> World {
        let root = std::env::temp_dir().join(format!("bos-armory-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.join("home"));
        paths.ensure().unwrap();
        std::fs::create_dir_all(root.join("pkgs")).unwrap();
        std::fs::create_dir_all(root.join("feeds")).unwrap();
        World { root, paths, catalog_key: std::cell::RefCell::new(7) }
    }

    fn catalog_path(&self) -> PathBuf {
        self.root.join("catalog.json")
    }

    fn zip_bytes(&self, p: &Pkg) -> Vec<u8> {
        let exec = common::HELPER_IN_PACKAGE;
        let lib = common::LIB_IN_PACKAGE;
        let mut process = serde_json::json!({ platform_key(): { "library": lib, "args": ["--mode", p.mode], "readyTimeoutMs": p.ready_ms, "stopTimeoutMs": 3000 } });
        process[platform_key()]["args"] = serde_json::json!(["--mode", p.mode]);
        if p.use_exec {
            process = serde_json::json!({ platform_key(): { "exec": exec } });
        }
        let mut manifest = serde_json::json!({
            "manifest": 1, "id": p.manifest_id.unwrap_or(p.id), "name": p.display.unwrap_or(p.id), "version": p.version,
            "contract": { "min": p.contract.0, "max": p.contract.1 },
            "process": process,
            "state": { "schema": p.schema }
        });
        if p.init {
            manifest["init"] = serde_json::json!({ "exec": exec, "args": ["--init"] });
        }
        if let Some(i) = p.icon {
            manifest["icon"] = serde_json::json!(i);
        }
        if let Some(e) = p.on_exit {
            manifest["onOsExit"] = serde_json::json!(e);
        }
        if let Some(m) = p.min_os {
            manifest["minOs"] = serde_json::json!(m);
        }
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file(p.root_manifest, opts).unwrap();
        w.write_all(manifest.to_string().as_bytes()).unwrap();
        w.start_file(lib, opts).unwrap();
        w.write_all(&std::fs::read(common::demo_lib()).unwrap()).unwrap();
        w.start_file(exec, opts).unwrap();
        w.write_all(&std::fs::read(common::demo_helper()).unwrap()).unwrap();
        for (name, data) in &p.extra {
            w.start_file(name.as_str(), opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// Write the package, its feed (latest wins) and the catalog.
    fn publish(&self, p: &Pkg) -> Vec<u8> {
        let zip = self.zip_bytes(p);
        let zpath = self.root.join("pkgs").join(format!("{}-{}.zip", p.id, p.version));
        std::fs::write(&zpath, &zip).unwrap();
        let sha = if p.sha_wrong { "0".repeat(64) } else { battalion_host::armory::sha256_hex(&zip) };
        let feed = serde_json::json!({
            "version": p.version, "contract": p.contract.0, "stateSchema": p.schema,
            "platforms": { platform_key(): { "url": file_url(&zpath), "signature": sign(&key(p.signer), &zip), "sha256": sha, "size": zip.len() } }
        });
        std::fs::write(self.root.join("feeds").join(format!("{}-stable.json", p.id)), feed.to_string()).unwrap();
        self.write_catalog(&[p.id], p.contract);
        zip
    }

    fn write_catalog(&self, ids: &[&str], contract: (u32, u32)) {
        let apps: Vec<_> = ids
            .iter()
            .map(|id| {
                serde_json::json!({ "id": id, "name": id, "publicKey": pub_b64(&key(*self.catalog_key.borrow())), "contract": { "min": contract.0, "max": contract.1 },
                    "channels": { "stable": file_url(&self.root.join("feeds").join(format!("{id}-stable.json"))) } })
            })
            .collect();
        std::fs::write(self.catalog_path(), serde_json::json!({ "schema": 2, "apps": apps }).to_string()).unwrap();
    }

    fn settings(&self) -> Settings {
        Settings { sources: vec![file_url(&self.catalog_path())], ..Settings::default() }
    }

    fn armory(&self) -> Armory {
        self.armory_with(self.settings(), None)
    }

    fn armory_with(&self, st: Settings, fetcher: Option<Arc<dyn Fetcher>>) -> Armory {
        let sup = common::sup(self.paths.clone(), fast());
        match fetcher {
            Some(f) => Armory::with_parts(self.paths.clone(), sup, "1.0.0", st, f),
            None => Armory::with_parts(self.paths.clone(), sup, "1.0.0", st, Arc::new(battalion_host::armory::DefaultFetcher { timeout_ms: 5000 })),
        }
    }

    fn state(&self, id: &str) -> Option<AppState> {
        Current::load(&self.paths.current_json(id).unwrap()).unwrap().map(|c| c.state)
    }
    fn current(&self, id: &str) -> Option<Current> {
        Current::load(&self.paths.current_json(id).unwrap()).unwrap()
    }
    fn staging_is_empty(&self) -> bool {
        std::fs::read_dir(self.paths.staging()).map(|d| d.count() == 0).unwrap_or(true)
    }
    fn absent(&self, id: &str) -> bool {
        self.current(id).is_none() && !self.paths.version_dir(id, "1.0.0").unwrap().exists() && !self.paths.version_dir(id, "2.0.0").unwrap().exists()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Ok(rd) = std::fs::read_dir(self.paths.root().join("run")) {
            for d in rd.flatten() {
                if let Ok(Some(v)) = store::read_json::<serde_json::Value>(&d.path().join("proc.json")) {
                    if let Ok(id) = serde_json::from_value::<ProcId>(v["id"].clone()) {
                        process::kill_tree(&id);
                    }
                }
            }
        }
    }
}

fn answers(a: &Armory, id: &str) -> String {
    a.sup().health(id).expect("the app answers").detail.unwrap_or_default()
}

trait SupOf {
    fn sup(&self) -> Supervisor;
}
impl SupOf for Armory {
    fn sup(&self) -> Supervisor {
        self.supervisor()
    }
}

/// The version of the running app: what the OS recorded for its worker (the app is answering).
fn version_answering(w: &World, id: &str) -> String {
    let v = std::fs::read_to_string(w.paths.root().join("run").join(id).join("proc.json")).unwrap();
    let rec: serde_json::Value = serde_json::from_str(&v).unwrap();
    rec["version"].as_str().unwrap().to_string()
}

// ---- install ------------------------------------------------------------------------------

#[test]
fn install_runs_the_app_and_initialises_it() {
    let w = World::new("install");
    w.publish(&Pkg { init: true, ..Pkg::new("demo-one", "1.0.0") });
    let a = w.armory();
    let listing = a.catalog().unwrap();
    assert_eq!(listing.apps.len(), 1);
    assert!(listing.apps[0].compatible);

    let done = a.install("demo-one", "stable").unwrap();
    assert_eq!(done.version, "1.0.0");
    assert_eq!(w.state("demo-one"), Some(AppState::Stopped));
    assert_eq!(w.current("demo-one").unwrap().public_key, pub_b64(&key(7)), "the key is pinned at the first install");
    assert!(w.paths.data_dir("demo-one").unwrap().join("init.done").is_file(), "init ran in the state folder");
    assert!(w.staging_is_empty());
    assert!(a.paths().journal().is_file());
    assert!(battalion_host::Journal::open(w.paths.journal()).unwrap().pending().unwrap().is_empty());
    assert_eq!(a.installed().unwrap().len(), 1);

    let sup = common::sup(w.paths.clone(), fast());
    sup.start("demo-one").unwrap();
    assert_eq!(w.state("demo-one"), Some(AppState::Running));
    sup.stop("demo-one").unwrap();
    assert!(matches!(a.install("demo-one", "stable"), Err(ArmoryError::AlreadyInstalled(_))));
}

#[test]
fn a_bad_package_installs_nothing() {
    // Each case: how the package is wrong -> the error, and the app is absent afterwards.
    let cases: Vec<(&str, Pkg, fn(&ArmoryError) -> bool)> = vec![
        ("bad sha", Pkg { sha_wrong: true, ..Pkg::new("bad-one", "1.0.0") }, |e| matches!(e, ArmoryError::ShaMismatch)),
        ("signed by another key", Pkg { signer: 9, ..Pkg::new("bad-one", "1.0.0") }, |e| matches!(e, ArmoryError::BadSignature(_))),
        ("manifest names another id", Pkg { manifest_id: Some("other-app"), ..Pkg::new("bad-one", "1.0.0") }, |e| matches!(e, ArmoryError::IdMismatch { .. })),
        ("contract outside this OS", Pkg { contract: (2, 3), ..Pkg::new("bad-one", "1.0.0") }, |e| matches!(e, ArmoryError::ContractNotSupported { .. })),
        ("needs a newer OS", Pkg { min_os: Some("9.0.0"), ..Pkg::new("bad-one", "1.0.0") }, |e| matches!(e, ArmoryError::OsTooOld { .. })),
        ("a program instead of a library", Pkg { use_exec: true, ..Pkg::new("bad-one", "1.0.0") }, |e| matches!(e, ArmoryError::Manifest(m) if m.contains("is not allowed") && m.contains("library"))),
        ("zip path leaves the package", Pkg { extra: vec![("../evil.txt".into(), b"x".to_vec())], ..Pkg::new("bad-one", "1.0.0") }, |e| matches!(e, ArmoryError::ZipPath(_))),
        ("absolute zip path", Pkg { extra: vec![("/abs.txt".into(), b"x".to_vec())], ..Pkg::new("bad-one", "1.0.0") }, |e| matches!(e, ArmoryError::ZipPath(_))),
        ("drive zip path", Pkg { extra: vec![("C:/abs.txt".into(), b"x".to_vec())], ..Pkg::new("bad-one", "1.0.0") }, |e| matches!(e, ArmoryError::ZipPath(_))),
    ];
    for (name, pkg, check) in cases {
        let w = World::new("bad");
        w.publish(&pkg);
        let a = w.armory();
        let err = a.install("bad-one", "stable").expect_err(name);
        assert!(check(&err), "{name}: got {err:?}");
        assert!(w.absent("bad-one"), "{name}: left something behind");
        assert!(w.staging_is_empty(), "{name}: staging not cleaned");
        assert!(!w.root.join("evil.txt").exists() && !w.paths.root().join("evil.txt").exists());
        assert!(battalion_host::Journal::open(w.paths.journal()).unwrap().pending().unwrap().is_empty(), "{name}: journal left open");
    }
}

#[test]
fn feed_version_must_match_the_manifest_and_the_catalog_key_must_match() {
    let w = World::new("keys");
    w.publish(&Pkg::new("demo-one", "1.0.0"));
    // The catalog advertises another key than the one that signed the package.
    *w.catalog_key.borrow_mut() = 9;
    w.write_catalog(&["demo-one"], (1, 1));
    assert!(matches!(w.armory().install("demo-one", "stable"), Err(ArmoryError::BadSignature(_))));
    // A catalog key in the old raw-ed25519 format is not a minisign key.
    let bad = std::fs::read_to_string(w.catalog_path()).unwrap().replace(&pub_b64(&key(9)), "AAAA");
    std::fs::write(w.catalog_path(), bad).unwrap();
    assert!(matches!(w.armory().install("demo-one", "stable"), Err(ArmoryError::BadSignature(_))));
}

#[test]
fn zip_limits_stop_a_bomb() {
    let w = World::new("bomb");
    w.publish(&Pkg { extra: vec![("big.bin".into(), vec![0u8; 3 << 20])], ..Pkg::new("bomb-one", "1.0.0") });
    let st = Settings { max_file_bytes: 1 << 20, ..w.settings() };
    let err = w.armory_with(st, None).install("bomb-one", "stable").unwrap_err();
    assert!(matches!(err, ArmoryError::ZipLimit(_)), "{err:?}");
    assert!(w.absent("bomb-one") && w.staging_is_empty());
    let st = Settings { max_entries: 1, ..w.settings() };
    assert!(matches!(w.armory_with(st, None).install("bomb-one", "stable"), Err(ArmoryError::ZipLimit(_))));
    let st = Settings { max_package_bytes: 1000, ..w.settings() };
    assert!(matches!(w.armory_with(st, None).install("bomb-one", "stable"), Err(ArmoryError::TooLarge { .. })));
}

/// Serves real bytes, but the package connection dies (or ends early) after `cut` bytes.
struct Cutting {
    inner: battalion_host::armory::DefaultFetcher,
    cut: usize,
    error: bool,
}
struct CutReader {
    inner: Box<dyn Read + Send>,
    left: usize,
    error: bool,
}
impl Read for CutReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return if self.error { Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "connection reset")) } else { Ok(0) };
        }
        let n = buf.len().min(self.left);
        let got = self.inner.read(&mut buf[..n])?;
        self.left -= got;
        Ok(got)
    }
}
impl Fetcher for Cutting {
    fn open(&self, url: &str) -> std::io::Result<Box<dyn Read + Send>> {
        let r = self.inner.open(url)?;
        if url.ends_with(".zip") {
            Ok(Box::new(CutReader { inner: r, left: self.cut, error: self.error }))
        } else {
            Ok(r)
        }
    }
}

#[test]
fn a_network_cut_mid_download_leaves_nothing_and_a_retry_works() {
    let w = World::new("cut");
    w.publish(&Pkg::new("cut-one", "1.0.0"));
    for error in [true, false] {
        let f = Arc::new(Cutting { inner: battalion_host::armory::DefaultFetcher { timeout_ms: 5000 }, cut: 100_000, error });
        let err = w.armory_with(w.settings(), Some(f)).install("cut-one", "stable").unwrap_err();
        if error {
            assert!(matches!(err, ArmoryError::Fetch { .. }), "{err:?}");
        } else {
            assert!(matches!(err, ArmoryError::SizeMismatch { .. }), "{err:?}");
        }
        assert!(w.absent("cut-one") && w.staging_is_empty(), "a cut download left files");
    }
    w.armory().install("cut-one", "stable").unwrap();
    assert_eq!(w.state("cut-one"), Some(AppState::Stopped));
}

#[test]
fn the_catalog_is_served_from_the_cache_when_the_source_is_gone() {
    let w = World::new("offline");
    w.publish(&Pkg::new("cache-one", "1.0.0"));
    let a = w.armory();
    assert!(a.catalog().unwrap().from_cache.is_empty());
    assert!(a.check_update("cache-one", None).is_err(), "not installed");
    std::fs::remove_file(w.catalog_path()).unwrap();
    let l = a.catalog().unwrap();
    assert_eq!(l.apps.len(), 1, "listed from the cache");
    assert_eq!(l.from_cache.len(), 1);
    // A broken catalog never replaces a good copy.
    std::fs::write(w.catalog_path(), "{ not json").unwrap();
    assert_eq!(a.catalog().unwrap().apps.len(), 1);
    // No cache and no source: offline.
    let w2 = World::new("offline2");
    assert!(matches!(w2.armory_with(w2.settings(), None).catalog(), Err(ArmoryError::Offline(_))));
}

#[test]
fn one_bad_catalog_entry_does_not_hide_the_rest() {
    let w = World::new("lenient");
    w.publish(&Pkg::new("good-one", "1.0.0"));
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(w.catalog_path()).unwrap()).unwrap();
    v["apps"].as_array_mut().unwrap().push(serde_json::json!({ "id": "BAD ID", "name": "x", "publicKey": "k", "contract": {"min":1,"max":1}, "channels": {"stable":"http://insecure"} }));
    std::fs::write(w.catalog_path(), v.to_string()).unwrap();
    let l = w.armory().catalog().unwrap();
    assert_eq!(l.apps.len(), 1);
    assert_eq!(l.rejected.len(), 1);
}

#[test]
fn a_package_with_another_kinds_root_manifest_is_refused() {
    let w = World::new("notapp");
    w.publish(&Pkg { root_manifest: "module.json", ..Pkg::new("odd-one", "1.0.0") });
    let err = w.armory().install("odd-one", "stable").expect_err("module.json is not app.json");
    assert!(matches!(err, ArmoryError::NotAnApp(_)), "got {err:?}");
    let msg = err.to_string();
    assert!(msg.contains("not an app") && msg.contains("module.json") && msg.contains("app.json"), "{msg}");
    assert!(w.absent("odd-one") && w.staging_is_empty());
}

#[test]
fn a_catalog_entry_that_is_not_an_app_is_skipped_and_reported() {
    let w = World::new("catkind");
    w.publish(&Pkg::new("good-one", "1.0.0"));
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(w.catalog_path()).unwrap()).unwrap();
    let mut other = v["apps"][0].clone();
    other["id"] = serde_json::json!("other-kind");
    other["kind"] = serde_json::json!("module");
    v["apps"].as_array_mut().unwrap().push(other);
    std::fs::write(w.catalog_path(), v.to_string()).unwrap();
    let l = w.armory().catalog().unwrap();
    assert_eq!(l.apps.len(), 1);
    assert_eq!(l.apps[0].app.id, "good-one");
    assert_eq!(l.rejected.len(), 1);
    assert_eq!(l.rejected[0].0, "other-kind");
    assert!(l.rejected[0].1.contains("not an app") && l.rejected[0].1.contains("'module'"), "{}", l.rejected[0].1);
    assert!(matches!(w.armory().find("other-kind"), Err(ArmoryError::NotInCatalog(_))));
}

#[test]
fn a_folder_inside_the_package_is_the_apps_own_and_opaque_to_the_os() {
    let w = World::new("opaque");
    // A nested folder with a manifest of the other kind: not read, so it neither refuses nor shows.
    let own = ["plug", "ins"].concat();
    let extra = vec![(format!("{own}/x/module.json"), b"{ this is not even json".to_vec()), (format!("{own}/x/a.bin"), vec![1u8; 10])];
    w.publish(&Pkg { extra, ..Pkg::new("own-one", "1.0.0") });
    let a = w.armory();
    a.install("own-one", "stable").unwrap();
    let all = a.installed().unwrap();
    assert_eq!(all.len(), 1);
    let shown = format!("{all:?}");
    assert!(!shown.contains("module.json") && !shown.to_lowercase().contains(&own), "the OS listed the app's own folder: {shown}");
}

// ---- update -------------------------------------------------------------------------------

fn running_v1(w: &World, id: &'static str) -> Armory {
    w.publish(&Pkg::new(id, "1.0.0"));
    let a = w.armory();
    a.install(id, "stable").unwrap();
    a.sup().start(id).unwrap();
    a
}

#[test]
fn update_to_a_good_version_keeps_the_state_and_the_previous_version() {
    let w = World::new("update");
    let a = running_v1(&w, "upd-one");
    std::fs::write(w.paths.data_dir("upd-one").unwrap().join("user.txt"), "mine").unwrap();
    let old_pid = a.sup().status("upd-one").unwrap().pid;
    w.publish(&Pkg::new("upd-one", "2.0.0"));
    assert!(a.check_update("upd-one", None).unwrap().newer);

    let up = a.update("upd-one", &UpdateOptions::default()).unwrap();
    assert_eq!((up.from.as_str(), up.to.as_str()), ("1.0.0", "2.0.0"));
    let cur = w.current("upd-one").unwrap();
    assert_eq!((cur.version.as_str(), cur.previous.as_deref(), cur.state.clone()), ("2.0.0", Some("1.0.0"), AppState::Running));
    assert_ne!(old_pid, process_pid(&w, "upd-one"));
    assert_eq!(version_answering(&w, "upd-one"), "2.0.0");
    assert_eq!(std::fs::read_to_string(w.paths.data_dir("upd-one").unwrap().join("user.txt")).unwrap(), "mine");
    assert!(w.paths.version_dir("upd-one", "1.0.0").unwrap().join("app.json").is_file(), "previous kept for rollback");
    assert!(matches!(a.update("upd-one", &UpdateOptions::default()), Err(ArmoryError::UpToDate(_))));
    // Same schema: a manual rollback is allowed.
    a.rollback("upd-one").unwrap();
    assert_eq!(w.current("upd-one").unwrap().version, "1.0.0");
    assert_eq!(version_answering(&w, "upd-one"), "1.0.0");
}

fn process_pid(w: &World, id: &str) -> Option<u32> {
    let v = std::fs::read_to_string(w.paths.root().join("run").join(id).join("proc.json")).ok()?;
    let rec: serde_json::Value = serde_json::from_str(&v).ok()?;
    rec["id"]["pid"].as_u64().map(|p| p as u32)
}

#[test]
fn an_update_that_never_gets_ready_rolls_back_and_the_old_version_answers() {
    let w = World::new("rollback");
    let a = running_v1(&w, "rb-one");
    w.publish(&Pkg { mode: "never-ready", ready_ms: 1000, ..Pkg::new("rb-one", "2.0.0") });
    let err = a.update("rb-one", &UpdateOptions::default()).unwrap_err();
    assert!(matches!(err, ArmoryError::UpdateFailed { rolled_back: true, .. }), "{err:?}");
    let cur = w.current("rb-one").unwrap();
    assert_eq!((cur.version.as_str(), cur.state.clone()), ("1.0.0", AppState::Running));
    assert_eq!(version_answering(&w, "rb-one"), "1.0.0", "the old version answers");
    assert!(!w.paths.version_dir("rb-one", "2.0.0").unwrap().exists());
    assert!(battalion_host::Journal::open(w.paths.journal()).unwrap().pending().unwrap().is_empty());
}

#[test]
fn a_schema_bump_refuses_the_automatic_rollback() {
    let w = World::new("schema");
    let a = running_v1(&w, "sc-one");
    w.publish(&Pkg { mode: "never-ready", ready_ms: 1000, schema: 2, ..Pkg::new("sc-one", "2.0.0") });
    let err = a.update("sc-one", &UpdateOptions::default()).unwrap_err();
    assert!(matches!(err, ArmoryError::UpdateFailed { rolled_back: false, .. }), "{err:?}");
    let cur = w.current("sc-one").unwrap();
    assert!(matches!(cur.state, AppState::Failed(_)), "left failed for a person: {:?}", cur.state);
    assert_eq!((cur.version.as_str(), cur.previous.as_deref()), ("2.0.0", Some("1.0.0")));
    assert!(w.paths.version_dir("sc-one", "1.0.0").unwrap().join("app.json").is_file(), "old files stay");
    assert!(matches!(a.rollback("sc-one"), Err(ArmoryError::SchemaBump { from: 1, to: 2 })));
    assert_eq!(w.current("sc-one").unwrap().version, "2.0.0", "the refused rollback changed nothing");
}

#[test]
fn a_good_schema_bump_updates_but_cannot_be_rolled_back_by_hand() {
    let w = World::new("schema-ok");
    let a = running_v1(&w, "sb-one");
    w.publish(&Pkg { schema: 2, ..Pkg::new("sb-one", "2.0.0") });
    a.update("sb-one", &UpdateOptions::default()).unwrap();
    assert!(matches!(a.rollback("sb-one"), Err(ArmoryError::SchemaBump { .. })));
}

#[test]
fn an_update_signed_with_another_key_is_refused_and_the_old_version_keeps_running() {
    let w = World::new("upd-key");
    let a = running_v1(&w, "uk-one");
    w.publish(&Pkg { signer: 9, ..Pkg::new("uk-one", "2.0.0") });
    assert!(matches!(a.update("uk-one", &UpdateOptions::default()), Err(ArmoryError::BadSignature(_))));
    // The catalog swaps its key: the pinned one wins.
    *w.catalog_key.borrow_mut() = 9;
    w.write_catalog(&["uk-one"], (1, 1));
    assert!(matches!(a.update("uk-one", &UpdateOptions::default()), Err(ArmoryError::KeyChanged)));
    assert_eq!(w.current("uk-one").unwrap().version, "1.0.0");
    assert_eq!(version_answering(&w, "uk-one"), "1.0.0");
}

// ---- remove -------------------------------------------------------------------------------

#[test]
fn remove_deletes_the_app_and_keeps_its_state() {
    let w = World::new("remove");
    let a = running_v1(&w, "rm-one");
    std::fs::write(w.paths.data_dir("rm-one").unwrap().join("keep.txt"), "state").unwrap();
    a.remove("rm-one", &RemoveOptions::default()).unwrap();
    assert!(w.current("rm-one").is_none());
    assert!(!w.paths.version_dir("rm-one", "1.0.0").unwrap().exists());
    assert!(w.paths.data_dir("rm-one").unwrap().join("keep.txt").is_file(), "state survives a remove");
    assert!(matches!(a.remove("rm-one", &RemoveOptions::default()), Err(ArmoryError::NotInstalled(_))));
    // Installed again, the app finds its state.
    a.install("rm-one", "stable").unwrap();
    assert!(w.paths.data_dir("rm-one").unwrap().join("keep.txt").is_file());
    a.remove("rm-one", &RemoveOptions { purge_state: true }).unwrap();
    assert!(!w.paths.data_dir("rm-one").unwrap().exists(), "purge was asked for");
}

// ---- killed at each journal step ----------------------------------------------------------

/// The child half: runs one Armory action and is killed by `BATTALION_KILL_AT`. A no-op in a normal run.
#[test]
fn crash_child() {
    let (Ok(root), Ok(op), Ok(id)) = (std::env::var("BOS_CHILD_ROOT"), std::env::var("BOS_CHILD_OP"), std::env::var("BOS_CHILD_ID")) else { return };
    let paths = Paths::new(PathBuf::from(&root).join("home"));
    let st = Settings { sources: vec![file_url(&PathBuf::from(&root).join("catalog.json"))], ..Settings::default() };
    let a = Armory::with_parts(paths.clone(), common::sup(paths, fast()), "1.0.0", st, Arc::new(battalion_host::armory::DefaultFetcher { timeout_ms: 5000 }));
    let r = match op.as_str() {
        "install" => a.install(&id, "stable").map(|_| ()),
        "update" => a.update(&id, &UpdateOptions::default()).map(|_| ()),
        "remove" => a.remove(&id, &RemoveOptions::default()),
        _ => Ok(()),
    };
    // Reached only when the kill point was never hit.
    println!("child finished without being killed: {r:?}");
    std::process::exit(0);
}

fn kill_child(w: &World, op: &str, id: &str, at: &str) -> i32 {
    // Files, not pipes: an app the child started outlives it and would hold a pipe open forever.
    let log = std::fs::File::create(w.root.join(format!("child-{op}-{at}.log"))).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["crash_child", "--exact", "--nocapture", "--test-threads=1"])
        .env("BOS_CHILD_ROOT", &w.root)
        .env("BOS_CHILD_OP", op)
        .env("BOS_CHILD_ID", id)
        .env("BATTALION_KILL_AT", at)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .status()
        .unwrap();
    status.code().unwrap_or(-1)
}

fn pending(w: &World) -> usize {
    battalion_host::Journal::open(w.paths.journal()).unwrap().pending().unwrap().len()
}

#[test]
fn killed_at_each_install_step_the_next_start_finishes_or_undoes_it() {
    for at in ["download", "verify", "unpack", "place", "init", "commit"] {
        let w = World::new(&format!("ki-{at}"));
        w.publish(&Pkg { init: true, ..Pkg::new("ki-one", "1.0.0") });
        assert_eq!(kill_child(&w, "install", "ki-one", at), 99, "{at}: the child was not killed at the step");
        assert_eq!(pending(&w), 1, "{at}: the journal shows the action in flight");
        let rec = w.armory().recover().unwrap();
        assert_eq!(rec.len(), 1, "{at}");
        assert_eq!(pending(&w), 0, "{at}: recovery closed the journal");
        if at == "commit" {
            assert_eq!(rec[0].outcome, "committed");
            assert_eq!(w.state("ki-one"), Some(AppState::Stopped));
        } else {
            assert_eq!(rec[0].outcome, "undone", "{at}");
            assert!(w.absent("ki-one"), "{at}: files left behind");
            assert!(w.staging_is_empty(), "{at}: staging left behind");
            w.armory().install("ki-one", "stable").unwrap_or_else(|e| panic!("{at}: the retry failed: {e}"));
        }
        assert_eq!(w.state("ki-one"), Some(AppState::Stopped), "{at}");
        assert!(w.armory().recover().unwrap().is_empty(), "{at}: a second recovery has nothing to do");
    }
}

#[test]
fn killed_at_each_update_step_the_app_ends_on_one_good_version() {
    // (step, version the app has after recovery, outcome)
    let cases = [
        ("download", "1.0.0", "undone"),
        ("verify", "1.0.0", "undone"),
        ("unpack", "1.0.0", "undone"),
        ("place", "1.0.0", "undone"),
        ("stop", "1.0.0", "undone"),
        ("switch", "1.0.0", "undone"),
        ("start", "1.0.0", "rolled back"),
        ("done", "2.0.0", "committed"),
        ("commit", "2.0.0", "committed"),
    ];
    for (at, want, outcome) in cases {
        let w = World::new(&format!("ku-{at}"));
        w.publish(&Pkg::new("ku-one", "1.0.0"));
        let a = w.armory();
        a.install("ku-one", "stable").unwrap();
        w.publish(&Pkg::new("ku-one", "2.0.0"));
        assert_eq!(kill_child(&w, "update", "ku-one", at), 99, "{at}: the child was not killed at the step");
        assert_eq!(pending(&w), 1, "{at}");
        let rec = a.recover().unwrap();
        assert_eq!(rec[0].outcome, outcome, "{at}");
        assert_eq!(pending(&w), 0, "{at}");
        let cur = w.current("ku-one").unwrap();
        assert_eq!(cur.version, want, "{at}");
        assert!(w.staging_is_empty(), "{at}: staging left behind");
        // Whatever it ended on, that version really starts and answers.
        let sup = common::sup(w.paths.clone(), fast());
        let _ = sup.recover("ku-one");
        if w.state("ku-one") != Some(AppState::Running) {
            sup.start("ku-one").unwrap_or_else(|e| panic!("{at}: version {want} does not start: {e}"));
        }
        assert_eq!(version_answering(&w, "ku-one"), want, "{at}");
    }
}

#[test]
fn killed_at_each_remove_step_the_removal_is_finished_and_state_stays() {
    for at in ["stop", "mark", "delete", "forget", "commit"] {
        let w = World::new(&format!("kr-{at}"));
        w.publish(&Pkg::new("kr-one", "1.0.0"));
        let a = w.armory();
        a.install("kr-one", "stable").unwrap();
        std::fs::create_dir_all(w.paths.data_dir("kr-one").unwrap()).unwrap();
        std::fs::write(w.paths.data_dir("kr-one").unwrap().join("keep.txt"), "state").unwrap();
        assert_eq!(kill_child(&w, "remove", "kr-one", at), 99, "{at}");
        assert_eq!(pending(&w), 1, "{at}");
        let rec = a.recover().unwrap();
        assert_eq!(rec[0].outcome, "rolled forward", "{at}");
        assert!(w.current("kr-one").is_none(), "{at}: still installed");
        assert!(!w.paths.version_dir("kr-one", "1.0.0").unwrap().exists(), "{at}");
        assert!(w.paths.data_dir("kr-one").unwrap().join("keep.txt").is_file(), "{at}: state lost");
        assert_eq!(pending(&w), 0, "{at}");
    }
}

// ---- BOS-10b: names and icons, progress, exit ------------------------------------------------

use battalion_host::armory::Progress;
use std::sync::Mutex;

fn listen(a: &Armory) -> Arc<Mutex<Vec<Progress>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    a.set_progress_sink(Some(Arc::new(move |p: &Progress| s2.lock().unwrap().push(p.clone()))));
    seen
}

#[test]
fn a_list_carries_name_icon_version_and_the_version_on_offer() {
    let w = World::new("cards");
    let png = b"not-really-a-png".to_vec();
    w.publish(&Pkg { display: Some("Demo Notes"), icon: Some("assets/icon.png"), extra: vec![("assets/icon.png".into(), png.clone())], ..Pkg::new("card-one", "1.0.0") });
    let a = w.armory();
    a.install("card-one", "stable").unwrap();
    let i = &a.installed().unwrap()[0];
    assert_eq!((i.name.as_deref(), i.version.as_str(), i.available.clone()), (Some("Demo Notes"), "1.0.0", None));
    let want = format!("data:image/png;base64,{}", battalion_host::armory::info::base64(&png));
    assert_eq!(i.icon.as_deref(), Some(want.as_str()));

    // Nothing newer is known until a feed is read; then the cached feed names it, with no network.
    w.publish(&Pkg { display: Some("Demo Notes"), ..Pkg::new("card-one", "1.1.0") });
    assert_eq!(a.installed().unwrap()[0].available, None);
    let listing = a.catalog().unwrap();
    assert_eq!(a.offered(&listing.apps[0], "stable").as_deref(), Some("1.1.0"));
    std::fs::remove_file(w.catalog_path()).unwrap();
    assert_eq!(a.installed().unwrap()[0].available.as_deref(), Some("1.1.0"));
}

#[test]
fn an_icon_over_the_size_cap_is_not_inlined() {
    let w = World::new("icons");
    w.publish(&Pkg { icon: Some("big.png"), extra: vec![("big.png".into(), vec![0u8; 300 * 1024])], ..Pkg::new("icon-big", "1.0.0") });
    let a = w.armory();
    a.install("icon-big", "stable").unwrap();
    let i = &a.installed().unwrap()[0];
    assert!(i.icon.is_none(), "over the size cap");
    assert_eq!(i.name.as_deref(), Some("icon-big"));
}

#[test]
fn install_reports_bytes_and_stages_in_order() {
    let w = World::new("progress-install");
    let zip = w.publish(&Pkg { init: true, ..Pkg::new("prog-one", "1.0.0") });
    let a = w.armory();
    let seen = listen(&a);
    a.install("prog-one", "stable").unwrap();
    let ev = seen.lock().unwrap().clone();
    assert!(ev.iter().all(|p| p.id == "prog-one" && p.op == "install"));
    let stages: Vec<&str> = ev.iter().map(|p| p.stage.as_str()).collect();
    let order = ["download", "verify", "unpack", "place", "init"];
    let mut at = 0;
    for s in &stages {
        let k = order.iter().position(|o| o == s).unwrap();
        assert!(k >= at, "stage {s} came out of order: {stages:?}");
        at = k;
    }
    assert_eq!(*stages.last().unwrap(), "init");
    let last_bytes = ev.iter().rfind(|p| p.bytes.is_some()).expect("a byte report");
    assert_eq!((last_bytes.bytes, last_bytes.total, last_bytes.percent), (Some(zip.len() as u64), Some(zip.len() as u64), Some(100)));
}

#[test]
fn update_remove_and_rollback_report_their_stages() {
    let w = World::new("progress-ops");
    let a = running_v1(&w, "prog-two");
    let seen = listen(&a);
    w.publish(&Pkg::new("prog-two", "2.0.0"));
    a.update("prog-two", &UpdateOptions::default()).unwrap();
    a.rollback("prog-two").unwrap();
    a.remove("prog-two", &RemoveOptions::default()).unwrap();
    let ev = seen.lock().unwrap().clone();
    let ops = |op: &str| ev.iter().filter(|p| p.op == op).map(|p| p.stage.clone()).collect::<Vec<_>>();
    assert_eq!(ops("update").last().map(String::as_str), Some("done"));
    assert!(ops("update").contains(&"download".to_string()) && ops("update").contains(&"switch".to_string()));
    assert_eq!(ops("rollback"), ["stop", "switch", "start", "done"]);
    assert_eq!(ops("remove"), ["stop", "mark", "delete", "forget"]);
}

#[test]
fn updating_one_app_never_touches_another_app() {
    let w = World::new("isolation");
    let a = running_v1(&w, "iso-one");
    w.publish(&Pkg::new("iso-two", "1.0.0"));
    a.install("iso-two", "stable").unwrap();
    a.sup().start("iso-two").unwrap();
    let before = process_pid(&w, "iso-two");
    w.publish(&Pkg::new("iso-one", "2.0.0"));
    a.update("iso-one", &UpdateOptions::default()).unwrap();
    a.remove("iso-one", &RemoveOptions::default()).unwrap();
    assert_eq!(process_pid(&w, "iso-two"), before, "the other app kept its process");
    assert_eq!(w.state("iso-two"), Some(AppState::Running));
}

#[test]
fn the_os_exit_stops_every_app_cleanly() {
    let w = World::new("exit");
    w.publish(&Pkg::new("exit-one", "1.0.0"));
    let sup = common::sup(w.paths.clone(), fast());
    let os = Armory::with_parts(w.paths.clone(), sup.clone(), "1.0.0", w.settings(), Arc::new(battalion_host::armory::DefaultFetcher { timeout_ms: 5000 }));
    os.install("exit-one", "stable").unwrap();
    w.publish(&Pkg::new("exit-two", "1.0.0"));
    os.install("exit-two", "stable").unwrap();
    sup.start("exit-one").unwrap();
    sup.start("exit-two").unwrap();
    let pids = [sup.status("exit-one").unwrap().pid.unwrap(), sup.status("exit-two").unwrap().pid.unwrap()];
    assert!(os.exit_os().is_empty());
    for (id, pid) in ["exit-one", "exit-two"].iter().zip(pids) {
        assert_eq!(w.state(id), Some(AppState::Stopped));
        assert!(!process::pid_running(pid), "{id} ended with the OS");
    }
}
