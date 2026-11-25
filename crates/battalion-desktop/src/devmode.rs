// SPDX-License-Identifier: MPL-2.0
//! BOS-31 developer mode of the OS window. Compiled only with the `dev` feature: a release build has none of this.
//!
//! `--dev-app <folder> [--dev-url <loopback url>]` (repeatable) takes an app from its folder: no package, signature, catalog or
//! install. The OS starts it, and when its built library changes it stops that app the normal way and starts the new one.

use crate::{events, host::Host};
use battalion_host::dev::{self, DevEvent};
use battalion_host::Paths;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// One `--dev-app` of the command line: its folder, its dev server url and the environment named for it.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct DevArg {
    pub folder: String,
    pub url: Option<String>,
    pub env: std::collections::BTreeMap<String, String>,
}

/// `--dev-app <folder>` starts an entry; `--dev-url <url>` and `--dev-env KEY=VALUE` (repeatable) belong to the `--dev-app` before them.
pub fn parse_args(args: &[String]) -> Vec<DevArg> {
    let mut out: Vec<DevArg> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dev-app" => {
                if let Some(f) = it.next() {
                    out.push(DevArg { folder: f.clone(), ..Default::default() });
                }
            }
            "--dev-url" => {
                if let (Some(u), Some(last)) = (it.next(), out.last_mut()) {
                    last.url = Some(u.clone());
                }
            }
            "--dev-env" => {
                if let (Some(kv), Some(last)) = (it.next(), out.last_mut()) {
                    if let Some((k, v)) = kv.split_once('=') {
                        last.env.insert(k.to_string(), v.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Register each dev app before the OS starts. A refusal is printed and the OS goes on without that app.
pub fn register_from_args(paths: &Paths, args: &[String]) -> Vec<String> {
    let mut ids = Vec::new();
    for a in parse_args(args) {
        match dev::register(paths, std::path::Path::new(&a.folder), a.url.as_deref(), &a.env) {
            Ok(id) => ids.push(id),
            Err(e) => eprintln!("dev app {}: {e}", a.folder),
        }
    }
    ids
}

/// Start the dev apps and watch their libraries. Progress goes to the shell as `os://dev-app`.
pub fn start(app: &AppHandle, ids: &[String]) {
    let host = app.state::<Host>();
    let handle = app.clone();
    let on_event: Arc<dyn Fn(DevEvent) + Send + Sync> = Arc::new(move |ev| {
        let (id, payload) = match ev {
            DevEvent::Restarted { id, ms } => (id.clone(), serde_json::json!({"id": id, "status": "restarted", "ms": ms})),
            DevEvent::Failed { id, error } => (id.clone(), serde_json::json!({"id": id, "status": "failed", "error": error})),
        };
        eprintln!("dev app: {payload}");
        let state = if payload["status"] == "restarted" { "running" } else { "failed" };
        let _ = handle.emit(events::APP_STATE, serde_json::json!({"id": id, "state": state}));
        let _ = handle.emit(events::DEV_APP, payload);
    });
    let watcher = dev::watch(host.paths.clone(), host.sup.clone(), Duration::from_millis(250), on_event.clone());
    app.manage(Keep(watcher));
    let (h2, ids) = (app.clone(), ids.to_vec());
    std::thread::spawn(move || {
        let host = h2.state::<Host>();
        for id in ids {
            let (state, payload) = match host.sup.start(&id) {
                Ok(_) => ("running", serde_json::json!({"id": id, "status": "started"})),
                Err(e) => ("failed", serde_json::json!({"id": id, "status": "failed", "error": e.to_string()})),
            };
            eprintln!("dev app: {payload}");
            let _ = h2.emit(events::APP_STATE, serde_json::json!({"id": id, "state": state}));
            let _ = h2.emit(events::DEV_APP, payload);
        }
    });
}

struct Keep(#[allow(dead_code)] dev::Watcher);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_dev_url_and_env_belong_to_the_app_before_them() {
        let a: Vec<String> = ["--x", "--dev-app", "C:/a", "--dev-url", "http://127.0.0.1:1", "--dev-env", "P=7", "--dev-app", "C:/b", "--dev-url"].iter().map(|s| s.to_string()).collect();
        let p = parse_args(&a);
        assert_eq!(p[0], DevArg { folder: "C:/a".into(), url: Some("http://127.0.0.1:1".into()), env: [("P".to_string(), "7".to_string())].into() });
        assert_eq!(p[1], DevArg { folder: "C:/b".into(), ..Default::default() });
    }
}
