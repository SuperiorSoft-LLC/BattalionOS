// SPDX-License-Identifier: MPL-2.0
//! APP-API-1: the per-app store. Two apps never see each other's keys; limits refuse without writing.

use battalion_host::appstore::{AppStore, COMMANDS};
use battalion_host::Paths;
use serde_json::json;

fn paths() -> (tempfile::TempDir, Paths) {
    let t = tempfile::tempdir().unwrap();
    let p = Paths::new(t.path());
    (t, p)
}

#[test]
fn set_get_list_delete_round_trip() {
    let (_t, p) = paths();
    let s = AppStore::open(&p, "notes").unwrap();
    assert_eq!(s.call("os_store_get", &json!({"key": "a"})).unwrap(), json!(null));
    s.call("os_store_set", &json!({"key": "b", "value": {"x": 1}})).unwrap();
    s.call("os_store_set", &json!({"key": "a", "value": "hi"})).unwrap();
    assert_eq!(s.call("os_store_get", &json!({"key": "b"})).unwrap(), json!({"x": 1}));
    assert_eq!(s.call("os_store_list", &json!({})).unwrap(), json!(["a", "b"]));
    assert_eq!(s.call("os_store_list", &json!({"prefix": "b"})).unwrap(), json!(["b"]));
    assert_eq!(s.call("os_store_delete", &json!({"key": "a"})).unwrap(), json!(true));
    assert_eq!(s.call("os_store_delete", &json!({"key": "a"})).unwrap(), json!(false));
    assert!(s.file().ends_with(std::path::Path::new("data").join("notes").join("store.json")));
    // Survives a reopen (it is a file, not memory).
    let again = AppStore::open(&p, "notes").unwrap();
    assert_eq!(again.get("b").unwrap(), json!({"x": 1}));
}

#[test]
fn two_apps_never_share_a_store() {
    let (_t, p) = paths();
    let a = AppStore::open(&p, "app-a").unwrap();
    let b = AppStore::open(&p, "app-b").unwrap();
    a.set("secret", json!("a's")).unwrap();
    b.set("secret", json!("b's")).unwrap();
    assert_eq!(a.get("secret").unwrap(), json!("a's"));
    assert_eq!(b.get("secret").unwrap(), json!("b's"));
    b.delete("secret").unwrap();
    assert_eq!(a.get("secret").unwrap(), json!("a's"));
    assert_eq!(b.list(None).unwrap(), Vec::<String>::new());
    // The key is only a key: a path-like key stays inside the caller's own file.
    a.set("../app-b/store.json", json!(1)).unwrap();
    assert_eq!(b.list(None).unwrap(), Vec::<String>::new());
    // An id that is not an app id cannot name a folder.
    for bad in ["../app-b", "A", "", "x/y"] {
        assert!(AppStore::open(&p, bad).is_err(), "{bad}");
    }
}

#[test]
fn limits_refuse_without_writing() {
    let (_t, p) = paths();
    std::fs::write(p.os_json(), r#"{"store": {"maxBytes": 200, "maxKeyChars": 8}}"#).unwrap();
    let s = AppStore::open(&p, "notes").unwrap();
    s.set("k", json!("small")).unwrap();
    let e = s.set("big", json!("x".repeat(500))).unwrap_err();
    assert_eq!(e.code(), "too_large");
    assert!(e.to_string().contains("store.maxBytes"));
    assert_eq!(s.list(None).unwrap(), vec!["k".to_string()]);
    let e = s.call("os_store_get", &json!({"key": "123456789"})).unwrap_err();
    assert_eq!(e.code(), "bad_args");
    assert_eq!(s.call("os_store_get", &json!({})).unwrap_err().code(), "bad_args");
    assert_eq!(s.call("os_store_get", &json!({"key": "a\nb"})).unwrap_err().code(), "bad_args");
    assert_eq!(s.call("os_store_list", &json!({"prefix": 3})).unwrap_err().code(), "bad_args");
}

#[test]
fn null_value_deletes_and_commands_are_four() {
    let (_t, p) = paths();
    let s = AppStore::open(&p, "notes").unwrap();
    s.set("k", json!(1)).unwrap();
    s.call("os_store_set", &json!({"key": "k", "value": null})).unwrap();
    assert_eq!(s.list(None).unwrap(), Vec::<String>::new());
    assert_eq!(COMMANDS, &["os_store_get", "os_store_set", "os_store_delete", "os_store_list"]);
}

/// The schemas (the source of the @battalionos/app-api types) name exactly the calls and error codes the host serves.
#[test]
fn schemas_name_the_host_calls_and_codes() {
    let enum_of = |text: &str, def: &str| -> Vec<String> {
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        v["$defs"][def]["enum"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
    };
    use battalion_app_api::schemas::{FS_JSON, STORE_JSON};
    assert_eq!(enum_of(STORE_JSON, "StoreCall"), COMMANDS);
    let fs: Vec<&str> = battalion_host::fs::COMMANDS.iter().chain(battalion_host::fs::WRITE_COMMANDS).copied().collect();
    assert_eq!(enum_of(FS_JSON, "FsCall"), fs);
    for code in enum_of(STORE_JSON, "StoreErrorCode") {
        assert!(["bad_args", "too_large", "bad_app", "io"].contains(&code.as_str()));
    }
    let e = battalion_host::appstore::StoreError::Io("x".into());
    assert!(enum_of(STORE_JSON, "StoreErrorCode").contains(&e.code().to_string()));
}
