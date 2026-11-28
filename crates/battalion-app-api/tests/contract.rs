// SPDX-License-Identifier: Apache-2.0
use battalion_app_api::catalog::{CatalogError, FeedError};
use battalion_app_api::{schemas, Catalog, Feed, Manifest, ManifestError, ReadyError, ReadyFile, CONTRACT};
use serde_json::{json, Value};

const P: &str = "windows-x86_64";

fn good() -> Value {
    json!({
        "manifest": 1, "id": "example", "name": "Example", "version": "1.2.0",
        "contract": {"min": 1, "max": 1},
        "process": { P: {"library": "lib/example.dll"} },
        "permissions": ["window"]
    })
}

fn refuse(edit: impl FnOnce(&mut Value)) -> ManifestError {
    let mut v = good();
    edit(&mut v);
    Manifest::parse(&v.to_string(), P, CONTRACT).expect_err("must be refused")
}

#[test]
fn good_manifest_loads_with_defaults() {
    let m = Manifest::parse(&good().to_string(), P, CONTRACT).unwrap();
    let p = m.process_for(P).unwrap();
    assert_eq!((p.ready_timeout_ms, p.stop_timeout_ms), (60_000, 10_000));
    assert!(m.ui.is_none() && m.init.is_none() && m.cli.is_empty());
}

#[test]
fn unknown_fields_are_ignored_and_optional_parts_stay_optional() {
    let mut v = good();
    v["futureThing"] = json!({"x": 1});
    v["process"][P]["newKnob"] = json!(true);
    assert!(Manifest::parse(&v.to_string(), P, CONTRACT).is_ok());
    // UI-only app: no process part at all
    let ui_only = json!({"manifest":1,"id":"page","name":"Page","version":"0.1.0","contract":{"min":1,"max":2},"ui":{"entry":"ui/index.html"}});
    assert!(Manifest::parse(&ui_only.to_string(), P, CONTRACT).is_ok());
}

#[test]
fn bad_manifests_are_refused_by_reason() {
    assert!(matches!(Manifest::parse("{nope", P, 1), Err(ManifestError::NotJson(_))));
    assert_eq!(refuse(|v| v["manifest"] = json!(2)), ManifestError::UnsupportedManifest(2));
    for bad in ["A", "x", "-a", "a_b", "../x", "a/b", &"a".repeat(64)] {
        assert_eq!(refuse(|v| v["id"] = json!(bad)), ManifestError::BadId(bad.to_string()), "{bad}");
    }
    assert_eq!(refuse(|v| v["name"] = json!("  ")), ManifestError::EmptyName);
    assert_eq!(refuse(|v| v["version"] = json!("1.2")), ManifestError::BadVersion("1.2".into()));
    assert_eq!(refuse(|v| v["contract"] = json!({"min": 2, "max": 1})), ManifestError::BadContractRange { min: 2, max: 1 });
    assert_eq!(refuse(|v| v["contract"] = json!({"min": 0, "max": 1})), ManifestError::BadContractRange { min: 0, max: 1 });
    assert_eq!(refuse(|v| v["contract"] = json!({"min": 2, "max": 3})), ManifestError::ContractNotSupported { os: 1, min: 2, max: 3 });
    assert_eq!(refuse(|v| v["minOs"] = json!("new")), ManifestError::BadMinOs("new".into()));
    // The old exit policy is gone: apps always stop with the OS. A manifest that still says it is not refused.
    let mut v = good();
    v["onOsExit"] = json!("keep");
    assert!(Manifest::parse(&v.to_string(), P, CONTRACT).is_ok());
    assert_eq!(refuse(|v| v["permissions"] = json!(["root"])), ManifestError::UnknownPermission("root".into()));
    assert_eq!(refuse(|v| v["cli"] = json!([{"name": "Bad Name", "exec": "bin/x"}])), ManifestError::BadCliName("Bad Name".into()));
    assert_eq!(refuse(|v| v["process"][P]["library"] = json!(" ")), ManifestError::EmptyExec("process"));
    // no process for this platform, no UI
    assert_eq!(refuse(|v| v["process"] = json!({"linux-x86_64": {"library": "lib/x.so"}})), ManifestError::NothingToRun);
}

#[test]
fn paths_that_leave_the_package_are_refused() {
    for p in ["../x.dll", "/abs/x.dll", "C:/x.dll", "bin\\..\\..\\x.dll", "bin//x.dll", ""] {
        let e = refuse(|v| v["process"][P]["library"] = json!(p));
        assert!(matches!(e, ManifestError::UnsafePath { .. } | ManifestError::EmptyExec(_)), "{p}: {e:?}");
    }
    assert!(matches!(refuse(|v| v["icon"] = json!("../icon.png")), ManifestError::UnsafePath { field: "icon", .. }));
    assert!(matches!(
        refuse(|v| v["ui"] = json!({"entry": "../../etc/x"})),
        ManifestError::UnsafePath { field: "ui.entry", .. }
    ));
}

#[test]
fn ready_file_refusals() {
    let ok = r#"{"contract":1,"pid":42,"url":"http://127.0.0.1:5000"}"#;
    let r = ReadyFile::parse(ok, 1).unwrap();
    assert_eq!((r.health.as_str(), r.stop.as_str()), ("/health", "/stop"));
    assert_eq!(ReadyFile::parse(r#"{"contract":2,"pid":42,"url":"http://127.0.0.1:1"}"#, 1), Err(ReadyError::WrongContract { expected: 1, got: 2 }));
    assert_eq!(ReadyFile::parse(r#"{"contract":1,"pid":0,"url":"http://127.0.0.1:1"}"#, 1), Err(ReadyError::ZeroPid));
    for u in ["http://example.com", "https://127.0.0.1:1", "http://127.0.0.1.evil.com", "http://0.0.0.0:1"] {
        let j = format!(r#"{{"contract":1,"pid":3,"url":"{u}"}}"#);
        assert!(matches!(ReadyFile::parse(&j, 1), Err(ReadyError::BadUrl(_))), "{u}");
    }
    assert!(matches!(ReadyFile::parse("x", 1), Err(ReadyError::NotJson(_))));
}

fn entry(id: &str) -> Value {
    json!({"id": id, "name": id, "publicKey": "RWQabc", "contract": {"min":1,"max":1}, "channels": {"stable": "https://example.org/app.json"}})
}

#[test]
fn catalog_v2_refusals_and_lenient_reading() {
    let c = json!({"schema": 2, "apps": [entry("one-app"), entry("two-app")]});
    assert_eq!(Catalog::parse(&c.to_string()).unwrap().apps.len(), 2);
    assert_eq!(Catalog::parse(&json!({"schema": 1, "apps": []}).to_string()), Err(CatalogError::UnsupportedSchema(1)));
    let dup = json!({"schema": 2, "apps": [entry("one-app"), entry("one-app")]});
    assert_eq!(Catalog::parse(&dup.to_string()), Err(CatalogError::DuplicateId("one-app".into())));
    let mut nokey = entry("no-key");
    nokey["publicKey"] = json!(" ");
    let mut http = entry("plain-http");
    http["channels"] = json!({"stable": "http://example.org/app.json"});
    let mixed = json!({"schema": 2, "apps": [entry("fine-app"), nokey, http, {"id": "broken"}]});
    let (ok, bad) = Catalog::parse_lenient(&mixed.to_string()).unwrap();
    assert_eq!(ok.len(), 1, "one bad entry must not hide the rest");
    assert!(bad.iter().any(|(id, e)| id == "no-key" && *e == CatalogError::NoKey("no-key".into())));
    assert!(bad.iter().any(|(id, e)| id == "plain-http" && matches!(e, CatalogError::InsecureUrl { .. })));
    assert!(bad.iter().any(|(id, _)| id == "broken"));
}

#[test]
fn feed_v1_refusals() {
    let sha = "a".repeat(64);
    let feed = |sha: &str, url: &str, sig: &str| {
        json!({"version": "1.0.0", "contract": 1, "platforms": {P: {"url": url, "signature": sig, "sha256": sha, "size": 10}}}).to_string()
    };
    assert!(Feed::parse(&feed(&sha, "https://x.org/a.bosapp", "sig")).is_ok());
    assert!(matches!(Feed::parse(&feed("abc", "https://x.org/a", "sig")), Err(FeedError::BadSha256 { .. })));
    assert!(matches!(Feed::parse(&feed(&sha, "http://x.org/a", "sig")), Err(FeedError::InsecureUrl { .. })));
    assert!(matches!(Feed::parse(&feed(&sha, "https://x.org/a", "")), Err(FeedError::EmptySignature { .. })));
    assert!(matches!(Feed::parse(r#"{"version":"1.0.0","contract":1,"platforms":{}}"#), Err(FeedError::NoPlatforms)));
    assert!(matches!(Feed::parse(r#"{"version":"x","contract":1,"platforms":{}}"#), Err(FeedError::BadVersion(_))));
}

#[test]
fn schemas_are_valid_json_and_fixture_manifest_loads() {
    for (name, text) in schemas::ALL {
        let v: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(v["$schema"].as_str().unwrap().contains("2020-12"), "{name}");
    }
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/demo-app/app.json")).unwrap();
    Manifest::parse(&fixture, P, CONTRACT).unwrap();
}

#[test]
fn an_app_is_a_library_exec_is_refused_with_a_clear_message() {
    let e = refuse(|v| v["process"][P] = json!({"exec": "bin/example.exe"}));
    assert_eq!(e, ManifestError::ExecRefused(P.into()));
    let text = e.to_string();
    assert!(text.contains("process.windows-x86_64.exec is not allowed") && text.contains("library"), "{text}");
    // Both given: still refused (the OS never starts a program for an app).
    let e = refuse(|v| v["process"][P]["exec"] = json!("bin/example.exe"));
    assert_eq!(e, ManifestError::ExecRefused(P.into()));
    // The library must be the target's kind.
    assert!(matches!(refuse(|v| v["process"][P]["library"] = json!("lib/example.so")), ManifestError::WrongLibraryKind { .. }));
    let mut v = good();
    v["process"] = json!({"macos-aarch64": {"library": "lib/x.dylib"}, "linux-x86_64": {"library": "lib/x.so"}});
    assert!(Manifest::parse(&v.to_string(), "linux-x86_64", CONTRACT).is_ok());
    // A helper program the app runs itself is not the OS's business: cli entries stay allowed.
    let mut v = good();
    v["cli"] = json!([{"name": "tool", "exec": "bin/tool.exe"}]);
    assert!(Manifest::parse(&v.to_string(), P, CONTRACT).is_ok());
}

/// OPEN-WITH-1: `fileTypes` loads, and each malformed entry is refused with its reason.
#[test]
fn file_types_validate() {
    let mut v = good();
    v["fileTypes"] = json!([{"label": "Text", "extensions": ["txt", "md"], "mime": ["text/plain", "text/*"]}, {"label": "Pictures", "mime": ["image/png"]}]);
    let m = Manifest::parse(&v.to_string(), P, CONTRACT).unwrap();
    assert_eq!(m.file_types.len(), 2);
    assert_eq!(m.file_types[0].extensions, vec!["txt", "md"]);
    assert!(Manifest::parse(&good().to_string(), P, CONTRACT).unwrap().file_types.is_empty());
    // A newer field inside an entry is ignored like any unknown field.
    let mut v = good();
    v["fileTypes"] = json!([{"label": "Text", "extensions": ["txt"], "icon": "x.png"}]);
    assert!(Manifest::parse(&v.to_string(), P, CONTRACT).is_ok());
    for bad in [
        json!([{"label": "", "extensions": ["txt"]}]),
        json!([{"label": "Nothing"}]),
        json!([{"label": "Dot", "extensions": [".txt"]}]),
        json!([{"label": "Upper", "extensions": ["TXT"]}]),
        json!([{"label": "Long", "extensions": ["a".repeat(17)]}]),
        json!([{"label": "Mime", "mime": ["text"]}]),
        json!([{"label": "Mime", "mime": ["Text/Plain"]}]),
        json!([{"label": "Mime", "mime": ["*/*"]}]),
    ] {
        assert!(matches!(refuse(|v| v["fileTypes"] = bad.clone()), ManifestError::BadFileType(_)), "{bad}");
    }
}
