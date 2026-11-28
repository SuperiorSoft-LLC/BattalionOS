// SPDX-License-Identifier: MPL-2.0
//! OPEN-WITH-1: the chooser lists only matching apps, the remembered default is used, a path outside the root is refused,
//! and a one-file grant reaches that file only.
use battalion_app_api::{Manifest, CONTRACT};
use battalion_host::fs::{Fs, Settings};
use battalion_host::openwith::{candidates, decide, extension_of, file_to_open, type_matches, Decision, Defaults, OpenError, Opens};
use battalion_host::Paths;
use serde_json::{json, Value};

fn app(id: &str, name: &str, types: Value, ui: bool) -> Manifest {
    let mut v = json!({"manifest": 1, "id": id, "name": name, "version": "1.0.0", "contract": {"min": 1, "max": CONTRACT}, "fileTypes": types});
    if ui {
        v["ui"] = json!({"entry": "ui/index.html"});
    } else {
        v["process"] = json!({"windows-x86_64": {"library": "a.dll"}});
    }
    Manifest::parse(&v.to_string(), "windows-x86_64", CONTRACT).unwrap()
}

fn apps() -> Vec<Manifest> {
    vec![
        app("notes", "Notes", json!([{"label": "Markdown", "extensions": ["md"]}]), true),
        app("viewer", "Viewer", json!([{"label": "Text", "mime": ["text/*"]}, {"label": "Pictures", "mime": ["image/png"]}]), true),
        app("paint", "Paint", json!([{"label": "Pictures", "extensions": ["png", "jpg"]}]), true),
        app("nopage", "No page", json!([{"label": "Markdown", "extensions": ["md"]}]), false),
        app("plain", "Plain", json!([]), true),
    ]
}

#[test]
fn types_match_by_extension_or_mime() {
    assert_eq!(extension_of("docs/Read.ME.MD").as_deref(), Some("md"));
    assert_eq!(extension_of(".hidden"), None);
    assert_eq!(extension_of("noext"), None);
    let a = apps();
    assert!(type_matches(&a[1].file_types[0], "md"), "text/* covers text/markdown");
    assert!(!type_matches(&a[1].file_types[0], "png"));
    assert!(type_matches(&a[1].file_types[1], "png"));
}

#[test]
fn the_chooser_lists_only_matching_apps_with_a_page() {
    let a = apps();
    let ids = |ext: &str| candidates(&a, ext).into_iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids("md"), vec!["notes", "viewer"]);
    assert_eq!(ids("png"), vec!["viewer", "paint"]);
    assert_eq!(ids("jpg"), vec!["paint"]);
    assert!(ids("exe").is_empty());
    assert_eq!(candidates(&a, "png")[0].label, "Pictures");
    // One candidate still asks the person: the OS never picks the receiver on its own.
    assert_eq!(decide(&candidates(&a, "jpg"), "jpg", None, None).unwrap(), Decision::Choose(candidates(&a, "jpg")));
    assert_eq!(decide(&[], "exe", None, None), Err(OpenError::NoApp("exe".into())));
}

#[test]
fn a_named_app_must_open_the_type() {
    let c = candidates(&apps(), "md");
    assert_eq!(decide(&c, "md", Some("viewer"), Some("notes")).unwrap(), Decision::Open("viewer".into()));
    assert_eq!(decide(&c, "md", Some("paint"), None), Err(OpenError::NoSuchApp("paint".into())));
    assert_eq!(decide(&c, "md", Some("nopage"), None), Err(OpenError::NoSuchApp("nopage".into())));
}

#[test]
fn the_remembered_default_is_used_and_forgotten() {
    let t = tempfile::tempdir().unwrap();
    let paths = Paths::new(t.path().join("home"));
    assert_eq!(Defaults::load(&paths).get("md"), None);
    Defaults::set(&paths, "md", Some("viewer")).unwrap();
    let d = Defaults::load(&paths);
    assert_eq!(d.get("md"), Some("viewer"));
    let c = candidates(&apps(), "md");
    assert_eq!(decide(&c, "md", None, d.get("md")).unwrap(), Decision::Open("viewer".into()));
    // A default that no longer opens the type (removed, or changed its fileTypes) falls back to the chooser.
    assert!(matches!(decide(&c, "md", None, Some("paint")).unwrap(), Decision::Choose(_)));
    Defaults::set(&paths, "md", None).unwrap();
    assert_eq!(Defaults::load(&paths).get("md"), None);
    assert!(Defaults::set(&paths, ".md", Some("x1")).is_err());
    assert!(Defaults::set(&paths, "md", Some("Bad Id")).is_err());
}

#[test]
fn a_path_outside_the_root_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let fs = Fs::with_root(&t.path().join("root"), None, Settings::default()).unwrap();
    std::fs::write(t.path().join("root").join("a.md"), "# hi").unwrap();
    std::fs::create_dir(t.path().join("root").join("dir.md")).unwrap();
    std::fs::write(t.path().join("secret.md"), "no").unwrap();
    assert_eq!(file_to_open(&fs, &json!({"path": "a.md"})).unwrap(), ("a.md".into(), "md".into()));
    for bad in ["../secret.md", "/etc/passwd", "C:/x.md", "a/../../secret.md"] {
        let e = file_to_open(&fs, &json!({"path": bad})).unwrap_err();
        assert_eq!(e.code(), "bad_path", "{bad}");
    }
    assert_eq!(file_to_open(&fs, &json!({"path": "dir.md"})).unwrap_err().code(), "not_a_file");
    assert_eq!(file_to_open(&fs, &json!({"path": ""})).unwrap_err().code(), "not_a_file");
    assert_eq!(file_to_open(&fs, &json!({"path": "gone.md"})).unwrap_err().code(), "not_found");
    assert_eq!(file_to_open(&fs, &json!({})).unwrap_err().code(), "bad_args");
    // A link under the root that points outside it is refused after resolving.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(t.path().join("secret.md"), t.path().join("root").join("link.md")).unwrap();
        assert_eq!(file_to_open(&fs, &json!({"path": "link.md"})).unwrap_err().code(), "leaves_root");
    }
}

#[test]
fn the_receiver_gets_the_file_and_the_grant_reaches_that_file_only() {
    let mut o = Opens::default();
    o.deliver("notes", "docs/a.md", false);
    o.deliver("notes", "docs/a.md", false);
    o.deliver("full", "docs/a.md", true);
    assert_eq!(o.take("notes"), vec!["docs/a.md"]);
    assert!(o.take("notes").is_empty(), "taken once");
    assert_eq!(o.take("full"), vec!["docs/a.md"]);
    assert!(o.grant_allows("notes", "os_fs_read", &json!({"path": "docs/a.md"})));
    assert!(o.grant_allows("notes", "os_fs_stat", &json!({"path": "docs/a.md"})));
    for (cmd, path) in [("os_fs_read", "docs/b.md"), ("os_fs_read", "docs"), ("os_fs_list", "docs"), ("os_fs_stat", ""), ("os_fs_write", "docs/a.md"), ("os_fs_delete", "docs/a.md"), ("os_fs_watch", "docs/a.md")] {
        assert!(!o.grant_allows("notes", cmd, &json!({"path": path})), "{cmd} {path}");
    }
    assert!(!o.grant_allows("other", "os_fs_read", &json!({"path": "docs/a.md"})), "another app has no grant");
    assert!(!o.grant_allows("full", "os_fs_read", &json!({"path": "docs/a.md"})), "an app with files.read gets no grant");
    o.close_app("notes");
    assert!(!o.grant_allows("notes", "os_fs_read", &json!({"path": "docs/a.md"})), "the grant ends with the frame");
}

/// The schema (the source of the @battalionos/app-api types) names exactly the calls the host serves, and every code it gives.
#[test]
fn the_schema_names_the_calls_and_codes() {
    let v: Value = serde_json::from_str(battalion_app_api::schemas::OPEN_JSON).unwrap();
    let enum_of = |def: &str| -> Vec<String> { v["$defs"][def]["enum"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect() };
    assert_eq!(enum_of("OpenCall"), battalion_host::openwith::COMMANDS);
    let codes = enum_of("OpenErrorCode");
    let t = tempfile::tempdir().unwrap();
    let fs = Fs::with_root(&t.path().join("root"), None, Settings::default()).unwrap();
    for e in [OpenError::BadArgs("x".into()), OpenError::NoSuchApp("x".into()), OpenError::NoApp("x".into()), OpenError::Io("x".into())] {
        assert!(codes.contains(&e.code().to_string()), "{}", e.code());
    }
    for p in ["../x.md", "gone.md", ""] {
        let e = file_to_open(&fs, &json!({"path": p})).unwrap_err();
        assert!(codes.contains(&e.code().to_string()), "{}", e.code());
    }
}
