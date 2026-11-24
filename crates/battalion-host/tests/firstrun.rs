// SPDX-License-Identifier: MPL-2.0
//! First-run tests: chosen, not chosen, offline, failed, already done, killed mid-install.
//! The fixture world (zip + minisign + file: catalog) is the one tests/armory.rs uses.

use battalion_host::armory::{Armory, Fetcher, Settings};
use battalion_host::firstrun::{self, Event, Outcome};
use battalion_host::process::{self, ProcId};
use battalion_host::state::{AppState, Current};
use battalion_host::supervisor::{platform_key, Settings as SupSettings, Supervisor};
use battalion_host::{store, Paths};
use blake2::{Blake2b512, Digest};
use ed25519_dalek::{Signer, SigningKey};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod common;


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
}

impl Pkg {
    fn new(id: &'static str, version: &'static str) -> Pkg {
        Pkg { id, version, mode: "normal", schema: 1, init: false, contract: (1, 1), min_os: None, manifest_id: None, extra: vec![], signer: 7, sha_wrong: false, ready_ms: 5000, display: None, icon: None, on_exit: None }
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
        w.start_file("app.json", opts).unwrap();
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

// ---- first run --------------------------------------------------------------------------------

fn plan(w: &World, ids: &[&str]) -> PathBuf {
    let apps: Vec<_> = ids.iter().map(|i| serde_json::json!({ "id": i })).collect();
    let p = w.root.join("first-run.json");
    std::fs::write(&p, serde_json::json!({ "apps": apps }).to_string()).unwrap();
    p
}

fn go(a: &Armory, plan: &Path) -> (Outcome, Vec<Event>) {
    let mut ev = Vec::new();
    let o = firstrun::run(a, plan, &mut |e| ev.push(e));
    (o, ev)
}

#[test]
fn chosen_installs_the_app_and_writes_the_done_marker() {
    let w = World::new("fr-chosen");
    w.publish(&Pkg::new("first-app", "1.0.0"));
    let a = w.armory();
    let (o, ev) = go(&a, &plan(&w, &["first-app"]));
    assert_eq!(o, Outcome::Done(vec!["first-app".into()]));
    assert_eq!(w.state("first-app"), Some(AppState::Stopped));
    assert!(firstrun::load_state(&w.paths).done);
    assert!(w.staging_is_empty());
    assert!(matches!(ev.first(), Some(Event::Start(ids)) if ids == &["first-app".to_string()]));
    assert!(ev.contains(&Event::AppInstalled("first-app".into())));
    assert!(matches!(ev.last(), Some(Event::Finished(Outcome::Done(_)))));
}

#[test]
fn not_chosen_means_no_plan_file_and_nothing_happens() {
    let w = World::new("fr-notchosen");
    w.publish(&Pkg::new("first-app", "1.0.0"));
    let a = w.armory();
    let (o, ev) = go(&a, &w.root.join("first-run.json"));
    assert_eq!(o, Outcome::NothingToDo);
    assert_eq!(ev, vec![Event::Finished(Outcome::NothingToDo)]);
    assert!(w.absent("first-app"));
    assert!(!firstrun::state_path(&w.paths).exists(), "no marker is written when nothing was chosen");
    // An empty plan is the same.
    assert_eq!(go(&a, &plan(&w, &[])).0, Outcome::NothingToDo);
}

#[test]
fn offline_leaves_nothing_then_a_retry_online_finishes() {
    let w = World::new("fr-offline");
    w.publish(&Pkg::new("first-app", "1.0.0"));
    let p = plan(&w, &["first-app"]);
    // The catalog source is gone and nothing is cached.
    let st = Settings { sources: vec![file_url(&w.root.join("nowhere.json"))], ..Settings::default() };
    let off = w.armory_with(st, None);
    let (o, _) = go(&off, &p);
    assert!(matches!(o, Outcome::Offline { ref id, .. } if id == "first-app"), "{o:?}");
    assert!(w.absent("first-app"));
    assert!(w.staging_is_empty());
    let s = firstrun::load_state(&w.paths);
    assert!(!s.done && s.last_error.is_some() && s.attempts == 1);
    // Back online: the same plan runs again and finishes.
    let (o, _) = go(&w.armory(), &p);
    assert_eq!(o, Outcome::Done(vec!["first-app".into()]));
    assert!(firstrun::load_state(&w.paths).done);
}

#[test]
fn a_failed_app_is_not_done_and_does_not_stop_the_others() {
    let w = World::new("fr-failed");
    w.publish(&Pkg { sha_wrong: true, ..Pkg::new("bad-one", "1.0.0") });
    w.publish(&Pkg::new("good-one", "1.0.0"));
    w.write_catalog(&["bad-one", "good-one"], (1, 1));
    let p = plan(&w, &["bad-one", "good-one"]);
    let (o, _) = go(&w.armory(), &p);
    assert!(matches!(o, Outcome::Failed { ref id, .. } if id == "bad-one"), "{o:?}");
    assert!(w.absent("bad-one"), "the failed app left nothing");
    assert_eq!(w.state("good-one"), Some(AppState::Stopped), "the good app is whole");
    assert!(!firstrun::load_state(&w.paths).done);
    // The package is fixed: the retry installs the missing one, skips the present one, and finishes.
    w.publish(&Pkg::new("bad-one", "1.0.0"));
    w.write_catalog(&["bad-one", "good-one"], (1, 1));
    let (o, _) = go(&w.armory(), &p);
    assert_eq!(o, Outcome::Done(vec!["bad-one".into(), "good-one".into()]));
    assert!(firstrun::load_state(&w.paths).done);
}

#[test]
fn a_bad_plan_fails_loudly_and_installs_nothing() {
    let w = World::new("fr-badplan");
    w.publish(&Pkg::new("first-app", "1.0.0"));
    let p = w.root.join("first-run.json");
    for text in ["{not json", r#"{"apps":[{"id":"../evil"}]}"#] {
        std::fs::write(&p, text).unwrap();
        assert!(matches!(go(&w.armory(), &p).0, Outcome::Failed { .. }), "{text}");
    }
    assert!(w.absent("first-app"));
}

#[test]
fn already_done_never_runs_again_even_after_the_app_is_removed() {
    let w = World::new("fr-done");
    w.publish(&Pkg::new("first-app", "1.0.0"));
    let a = w.armory();
    let p = plan(&w, &["first-app"]);
    assert!(matches!(go(&a, &p).0, Outcome::Done(_)));
    a.remove("first-app", &Default::default()).unwrap();
    let (o, ev) = go(&a, &p);
    assert_eq!(o, Outcome::AlreadyDone);
    assert_eq!(ev, vec![Event::Finished(Outcome::AlreadyDone)]);
    assert!(w.absent("first-app"), "a removed app is not put back");
}

#[test]
fn the_app_already_there_counts_as_installed() {
    let w = World::new("fr-present");
    w.publish(&Pkg::new("first-app", "1.0.0"));
    let a = w.armory();
    a.install("first-app", "stable").unwrap();
    assert_eq!(go(&a, &plan(&w, &["first-app"])).0, Outcome::Done(vec!["first-app".into()]));
}

#[test]
fn uninstalling_the_os_keeps_the_state_and_the_marker() {
    // The MSI removes the executable folder only; the home (marker, apps, data) is outside it.
    let w = World::new("fr-uninstall");
    w.publish(&Pkg { init: true, ..Pkg::new("first-app", "1.0.0") });
    let a = w.armory();
    let p = plan(&w, &["first-app"]);
    assert!(matches!(go(&a, &p).0, Outcome::Done(_)));
    std::fs::remove_file(&p).unwrap(); // the installer's file goes with the install folder
    assert!(w.paths.data_dir("first-app").unwrap().join("init.done").is_file());
    assert!(firstrun::load_state(&w.paths).done);
    // A reinstall of the OS does not repeat first run: the marker stands.
    assert_eq!(go(&a, &plan(&w, &["first-app"])).0, Outcome::AlreadyDone);
}

// ---- killed mid-install ---------------------------------------------------------------------------

#[test]
fn crash_child() {
    let (Ok(root), Ok(id)) = (std::env::var("BOS_CHILD_ROOT"), std::env::var("BOS_CHILD_ID")) else { return };
    let root = PathBuf::from(root);
    let paths = Paths::new(root.join("home"));
    let st = Settings { sources: vec![file_url(&root.join("catalog.json"))], ..Settings::default() };
    let a = Armory::with_parts(paths.clone(), common::sup(paths, fast()), "1.0.0", st, Arc::new(battalion_host::armory::DefaultFetcher { timeout_ms: 5000 }));
    let plan = root.join("first-run.json");
    let o = firstrun::run(&a, &plan, &mut |_| {});
    println!("child finished without being killed: {o:?} ({id})");
    std::process::exit(0);
}

fn kill_child(w: &World, id: &str, at: &str) -> i32 {
    let log = std::fs::File::create(w.root.join(format!("child-{at}.log"))).unwrap();
    std::process::Command::new(std::env::current_exe().unwrap())
        .args(["crash_child", "--exact", "--nocapture", "--test-threads=1"])
        .env("BOS_CHILD_ROOT", &w.root)
        .env("BOS_CHILD_ID", id)
        .env("BATTALION_KILL_AT", at)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .status()
        .unwrap()
        .code()
        .unwrap_or(-1)
}

#[test]
fn killed_mid_install_the_next_open_finishes_with_nothing_half_installed() {
    for at in ["download", "verify", "unpack", "place", "init", "commit"] {
        let w = World::new(&format!("fr-kill-{at}"));
        w.publish(&Pkg { init: true, ..Pkg::new("first-app", "1.0.0") });
        let p = plan(&w, &["first-app"]);
        assert_eq!(kill_child(&w, "first-app", at), 99, "{at}: the child was not killed at the step");
        assert!(!firstrun::load_state(&w.paths).done, "{at}: the marker must not exist before the install is whole");
        let (o, _) = go(&w.armory(), &p);
        assert_eq!(o, Outcome::Done(vec!["first-app".into()]), "{at}");
        assert_eq!(w.state("first-app"), Some(AppState::Stopped), "{at}");
        assert!(w.staging_is_empty(), "{at}: staging left behind");
        assert!(battalion_host::Journal::open(w.paths.journal()).unwrap().pending().unwrap().is_empty(), "{at}: journal left open");
        assert!(firstrun::load_state(&w.paths).done, "{at}");
    }
}

#[test]
fn the_plan_seeds_the_catalog_source_once_and_never_over_the_persons_own() {
    let w = World::new("fr-seed");
    let p = w.root.join("first-run.json");
    std::fs::write(&p, r#"{"sources":["https://example.org/catalog.json"],"apps":[{"id":"first-app"}]}"#).unwrap();
    std::fs::write(w.paths.os_json(), r#"{"supervisor":{"maxRestarts":9}}"#).unwrap();
    assert!(firstrun::seed_sources(&w.paths, &p).unwrap());
    let os: serde_json::Value = store::read_json(&w.paths.os_json()).unwrap().unwrap();
    assert_eq!(os["armory"]["sources"][0], "https://example.org/catalog.json");
    assert_eq!(os["supervisor"]["maxRestarts"], 9, "other keys stay");
    assert_eq!(Settings::load(&w.paths).sources, vec!["https://example.org/catalog.json".to_string()]);
    // The person edits it: the next start leaves it alone.
    std::fs::write(w.paths.os_json(), r#"{"armory":{"sources":["file:///mine.json"]}}"#).unwrap();
    assert!(!firstrun::seed_sources(&w.paths, &p).unwrap());
    assert_eq!(Settings::load(&w.paths).sources, vec!["file:///mine.json".to_string()]);
    // No plan file (not chosen): nothing is written.
    let w2 = World::new("fr-seed2");
    assert!(!firstrun::seed_sources(&w2.paths, &w2.root.join("first-run.json")).unwrap());
    assert!(!w2.paths.os_json().exists());
}
