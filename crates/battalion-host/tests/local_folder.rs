// SPDX-License-Identifier: MPL-2.0
//! ARMORY-LOCAL-2: install from a folder or package file, and a linked local repo (a checkout shaped like
//! battalionOS-apps, relative paths). Temp homes only.

use battalion_host::armory::{Armory, ArmoryError, FolderOptions, Settings, Signing, UpdateOptions};
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

struct World {
    root: PathBuf,
    paths: Paths,
    device: SigningKey,
}


impl World {
    fn new(name: &str) -> World {
        let root = std::env::temp_dir().join(format!("bos-localfolder-{name}-{}", std::process::id()));
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

    fn manifest(id: &str, version: &str) -> serde_json::Value {
        serde_json::json!({"manifest":1,"id":id,"name":"Hello","version":version,"contract":{"min":1,"max":1},"ui":{"entry":"ui/index.html"},"state":{"schema":1}})
    }

    fn zip_of(manifest: &str, version: &str) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file("app.json", o).unwrap();
        w.write_all(manifest.as_bytes()).unwrap();
        w.start_file("ui/index.html", o).unwrap();
        w.write_all(format!("<h1>hello {version}</h1>").as_bytes()).unwrap();
        w.finish().unwrap().into_inner()
    }

    /// What the template's package script writes: out/<id>-<v>-<plat>.bosapp(.minisig) and public.key.
    fn dist(&self, name: &str, id: &str, version: &str, signer: Option<&SigningKey>) -> PathBuf {
        let dir = self.root.join(name);
        let dist = dir.join("out");
        std::fs::create_dir_all(&dist).unwrap();
        let zip = Self::zip_of(&Self::manifest(id, version).to_string(), version);
        let pkg = dist.join(format!("{id}-{version}-{}.bosapp", platform_key()));
        std::fs::write(&pkg, &zip).unwrap();
        if let Some(k) = signer {
            std::fs::write(dist.join(format!("{id}-{version}-{}.bosapp.minisig", platform_key())), sign(k, &zip)).unwrap();
            std::fs::write(dist.join("public.key"), format!("untrusted comment: test\n{}\n", pub_b64(k))).unwrap();
        }
        dir
    }

    fn version(&self, id: &str) -> Option<String> {
        Current::load(&self.paths.current_json(id).unwrap()).unwrap().map(|c| c.version)
    }
}

fn yes() -> FolderOptions {
    FolderOptions { allow_unsigned: true, replace_catalog: true }
}

#[test]
fn a_package_signed_with_the_device_key_installs_without_a_confirm() {
    let w = World::new("signed");
    let dir = w.dist("proj", "hello", "1.0.0", Some(&w.device));
    let r = w.armory(vec![]).install_from_path(&dir, &FolderOptions::default()).unwrap();
    assert_eq!((r.id.as_str(), r.version.as_str(), r.signing), ("hello", "1.0.0", Signing::Trusted));
    assert_eq!(w.version("hello").as_deref(), Some("1.0.0"));
    let inst = w.armory(vec![]).installed().unwrap();
    let local = inst[0].local.as_ref().expect("marked local");
    assert_eq!(local.signing, Signing::Trusted);
    assert_eq!(inst[0].channel.as_deref(), Some("local"));
}

#[test]
fn unsigned_and_throwaway_key_packages_need_the_confirm() {
    let w = World::new("unsigned");
    let dir = w.dist("proj", "hello", "1.0.0", None);
    let e = w.armory(vec![]).install_from_path(&dir, &FolderOptions::default()).unwrap_err();
    assert!(matches!(&e, ArmoryError::NeedsConfirm(m) if m.to_string().contains("unsigned local app")), "{e:?}");
    assert!(w.version("hello").is_none());
    assert!(!w.paths.app_dir("hello").unwrap().exists(), "nothing left behind");
    let r = w.armory(vec![]).install_from_path(&dir, &FolderOptions { allow_unsigned: true, ..Default::default() }).unwrap();
    assert_eq!(r.signing, Signing::Unsigned);

    let throwaway = SigningKey::from_bytes(&[3; 32]);
    let dir = w.dist("proj2", "other", "1.0.0", Some(&throwaway));
    let e = w.armory(vec![]).install_from_path(&dir, &FolderOptions::default()).unwrap_err();
    assert!(matches!(e, ArmoryError::NeedsConfirm(_)), "{e:?}");
    assert_eq!(w.armory(vec![]).install_from_path(&dir, &yes()).unwrap().signing, Signing::OwnKey);
}

#[test]
fn a_tampered_package_is_refused_even_when_confirmed() {
    let w = World::new("tamper");
    let dir = w.dist("proj", "hello", "1.0.0", Some(&w.device));
    let pkg = dir.join("out").join(format!("hello-1.0.0-{}.bosapp", platform_key()));
    let mut b = std::fs::read(&pkg).unwrap();
    let n = b.len() / 2;
    b[n] ^= 0xff;
    std::fs::write(&pkg, &b).unwrap();
    // picked as the file itself this time
    let e = w.armory(vec![]).install_from_path(&pkg, &yes()).unwrap_err();
    assert!(matches!(e, ArmoryError::BadSignature(_)), "{e:?}");
    assert!(w.version("hello").is_none());
}

#[test]
fn a_bad_manifest_is_refused() {
    let w = World::new("badmanifest");
    let dir = w.root.join("unpacked");
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(dir.join("app.json"), r#"{"manifest":1,"id":"hello","version":"1.0.0"}"#).unwrap();
    std::fs::write(dir.join("ui/index.html"), "x").unwrap();
    let e = w.armory(vec![]).install_from_path(&dir, &yes()).unwrap_err();
    assert!(matches!(e, ArmoryError::Manifest(_) | ArmoryError::ContractNotSupported { .. }), "{e:?}");
    assert!(w.version("hello").is_none());
    let empty = w.root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let e = w.armory(vec![]).install_from_path(&empty, &yes()).unwrap_err();
    assert!(e.to_string().contains("no app found"), "{e}");
}

#[test]
fn an_unpacked_folder_installs_and_picking_it_again_updates_and_rolls_back() {
    let w = World::new("reinstall");
    let dir = w.root.join("unpacked");
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(dir.join("ui/index.html"), "v1").unwrap();
    std::fs::write(dir.join("app.json"), World::manifest("hello", "1.0.0").to_string()).unwrap();
    w.armory(vec![]).install_from_path(&dir, &yes()).unwrap();
    // same version again: replaced in place
    std::fs::write(dir.join("ui/index.html"), "v1b").unwrap();
    let r = w.armory(vec![]).install_from_path(&dir, &yes()).unwrap();
    assert_eq!(r.from.as_deref(), Some("1.0.0"));
    let vdir = w.paths.version_dir("hello", "1.0.0").unwrap();
    assert_eq!(std::fs::read_to_string(vdir.join("ui/index.html")).unwrap(), "v1b");
    // a newer version: through the update switch, and rollback works
    std::fs::write(dir.join("app.json"), World::manifest("hello", "1.1.0").to_string()).unwrap();
    let r = w.armory(vec![]).install_from_path(&dir, &yes()).unwrap();
    assert_eq!((r.from.as_deref(), r.version.as_str()), (Some("1.0.0"), "1.1.0"));
    assert_eq!(w.version("hello").as_deref(), Some("1.1.0"));
    w.armory(vec![]).rollback("hello").unwrap();
    assert_eq!(w.version("hello").as_deref(), Some("1.0.0"));
    // a catalog update never picks up a local app (its channel is "local")
    assert!(w.armory(vec![]).update("hello", &UpdateOptions::default()).is_err());
}

/// A battalionOS-apps-shaped checkout: catalog.json, feeds/<id>/stable.json and packages/, with relative paths.
fn checkout(w: &World, publisher: &SigningKey, versions: &[&str]) -> PathBuf {
    let dir = w.root.join("battalionOS-apps");
    std::fs::create_dir_all(dir.join("feeds/hello")).unwrap();
    std::fs::create_dir_all(dir.join("packages")).unwrap();
    for v in versions {
        let zip = World::zip_of(&World::manifest("hello", v).to_string(), v);
        std::fs::write(dir.join(format!("packages/hello-{v}.bosapp")), &zip).unwrap();
    }
    let v = versions.last().unwrap();
    let zip = std::fs::read(dir.join(format!("packages/hello-{v}.bosapp"))).unwrap();
    let feed = serde_json::json!({"version":v,"contract":1,"stateSchema":1,"platforms":{platform_key():{
        "url":format!("../../packages/hello-{v}.bosapp"),"signature":sign(publisher,&zip),"sha256":battalion_host::armory::sha256_hex(&zip),"size":zip.len()}}});
    std::fs::write(dir.join("feeds/hello/stable.json"), feed.to_string()).unwrap();
    let cat = serde_json::json!({"schema":2,"apps":[{"id":"hello","name":"Hello","publicKey":pub_b64(publisher),"contract":{"min":1,"max":1},
        "channels":{"stable":"feeds/hello/stable.json"},"platforms":[platform_key()]}]});
    std::fs::write(dir.join("catalog.json"), cat.to_string()).unwrap();
    dir
}

#[test]
fn a_linked_checkout_lists_installs_updates_and_rolls_back() {
    let w = World::new("linked");
    let publisher = SigningKey::from_bytes(&[11; 32]);
    let dir = checkout(&w, &publisher, &["1.0.0"]);
    let a = w.armory(vec![]);
    let r = a.link_local_repo("Apps checkout", &dir).unwrap();
    assert!(r.link.is_some());
    assert!(a.link_local_repo("Again", &dir).is_err());
    let l = a.catalog().unwrap();
    let e = l.apps.iter().find(|e| e.app.id == "hello").unwrap_or_else(|| panic!("listed: {:?}", l.rejected));
    assert_eq!((e.source.as_str(), e.from_local_repo), ("local:Apps checkout", true));
    a.install("hello", "stable").unwrap();
    assert_eq!(w.version("hello").as_deref(), Some("1.0.0"));
    checkout(&w, &publisher, &["1.0.0", "1.1.0"]);
    w.armory(vec![]).update("hello", &UpdateOptions::default()).unwrap();
    assert_eq!(w.version("hello").as_deref(), Some("1.1.0"));
    w.armory(vec![]).rollback("hello").unwrap();
    assert_eq!(w.version("hello").as_deref(), Some("1.0.0"));
    // removing the repo removes the link only
    w.armory(vec![]).remove_local_repo("Apps checkout").unwrap();
    assert!(dir.join("catalog.json").is_file());
    assert!(w.armory(vec![]).local_repos().is_empty());
}

fn file_url_of(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

#[test]
fn a_catalog_app_is_not_replaced_from_a_folder_without_asking() {
    let w = World::new("replace");
    let publisher = SigningKey::from_bytes(&[11; 32]);
    let dir = checkout(&w, &publisher, &["1.0.0"]);
    // the "public" catalog: a file source with absolute urls, read exactly as before
    let feed_path = dir.join("feeds/hello/stable.json");
    let pkg = file_url_of(&dir.join("packages/hello-1.0.0.bosapp"));
    let feed = std::fs::read_to_string(&feed_path).unwrap().replace("../../packages/hello-1.0.0.bosapp", &pkg);
    std::fs::write(&feed_path, feed).unwrap();
    let cat = std::fs::read_to_string(dir.join("catalog.json")).unwrap().replace("\"feeds/hello/stable.json\"", &format!("\"{}\"", file_url_of(&feed_path)));
    std::fs::write(dir.join("catalog.json"), cat).unwrap();
    let src = file_url_of(&dir.join("catalog.json"));
    let a = w.armory(vec![src.clone()]);
    let l = a.catalog().unwrap();
    assert_eq!((l.apps[0].source.as_str(), l.apps[0].from_local_repo), (src.as_str(), false));
    a.install("hello", "stable").unwrap();
    assert!(a.local_install("hello").is_none());
    let local = w.dist("proj", "hello", "2.0.0", Some(&w.device));
    let e = w.armory(vec![src.clone()]).install_from_path(&local, &FolderOptions::default()).unwrap_err();
    assert!(matches!(&e, ArmoryError::NeedsConfirm(m) if m.to_string().contains("from the catalog")), "{e:?}");
    assert_eq!(w.version("hello").as_deref(), Some("1.0.0"));
    w.armory(vec![src]).install_from_path(&local, &FolderOptions { replace_catalog: true, ..Default::default() }).unwrap();
    assert_eq!(w.version("hello").as_deref(), Some("2.0.0"));
}
