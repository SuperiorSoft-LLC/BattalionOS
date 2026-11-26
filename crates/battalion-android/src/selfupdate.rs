// SPDX-License-Identifier: MPL-2.0
//! BOS-AND-SELFUPDATE-1: BattalionOS on Android updates itself. The same commands and Armory row as the desktop (BOS-30:
//! `os_update_state`, `os_update_check`, `os_update_install`), with the phone's own feed and installer:
//! - the feed is `bos-android.json` (its url is data in `os-feed.json`), read at start and every `checkEveryMinutes`;
//! - install = download the APK here, check it against the feed's sha256, stop the apps the normal way (they start again after
//!   the update, from the resume list), then answer the APK's path; the shell hands it to Android's PackageInstaller in this
//!   same process (OsUpdate.kt), which first checks that the APK's signing certificate is the installed app's;
//! - `os_update_abort`: Android did not install it (cancelled, refused, no permission): the apps start again, nothing changed.
//!
//! Only a test build (cargo feature `test-update`) reads another feed, fixed at build time (`BOS_ANDROID_TEST_FEED`, a loopback
//! http url): a release build has no door for it.

use battalion_desktop::host::Host;
use battalion_host::armory::osupdate::ApkFeed;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State, WebviewWindow};

type R<T> = Result<T, String>;

const FEED: &str = include_str!("../os-feed.json");

/// The id of the OS row in the Armory (not an app id).
const OS_ROW: &str = battalion_desktop::osupdate::OS_ROW;

/// What the confirm adds on a phone: Android asks once more itself.
pub const CONFIRM_NOTE: &str = "Android then shows its own Update confirm once; your apps and their data are kept.";

pub struct FeedConfig {
    pub feed: ApkFeed,
    pub every_secs: u64,
}

pub fn feed_from(text: &str) -> Option<FeedConfig> {
    let v: Value = serde_json::from_str(text).ok()?;
    let url = v.get("url")?.as_str()?.to_string();
    let minutes = v.get("checkEveryMinutes").and_then(Value::as_u64).unwrap_or(240).max(1);
    Some(FeedConfig { feed: ApkFeed { url, allow_loopback_http: false }, every_secs: minutes * 60 })
}

pub fn feed() -> Option<FeedConfig> {
    #[allow(unused_mut)]
    let mut f = feed_from(FEED)?;
    #[cfg(feature = "test-update")]
    if let Some(u) = option_env!("BOS_ANDROID_TEST_FEED") {
        f.feed = ApkFeed { url: u.to_string(), allow_loopback_http: true };
    }
    Some(f)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

fn shell_only(window: &WebviewWindow, cmd: &str) -> R<()> {
    battalion_desktop::access::authorize_shell(window.label(), cmd).map_err(|e| e.to_string())
}

async fn state_json(app: &AppHandle, host: &Host) -> Value {
    let (available, installing) = {
        let s = host.os_update.lock().unwrap_or_else(|p| p.into_inner());
        (s.available.clone(), s.installing)
    };
    let a = host.armory.clone();
    let (busy, running) = blocking(move || (a.os_busy_apps(), a.os_running_apps())).await.unwrap_or_default();
    json!({
        "current": app.package_info().version.to_string(),
        "available": available.map(|u| json!({"version": u.version, "notes": u.notes})),
        "installing": installing, "busy": busy, "running": running,
        "confirmNote": CONFIRM_NOTE,
    })
}

#[tauri::command]
pub async fn os_update_state(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    shell_only(&window, "os_update_state")?;
    Ok(state_json(&app, &host).await)
}

#[tauri::command]
pub async fn os_update_check(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    shell_only(&window, "os_update_check")?;
    check(&app, &host).await;
    Ok(state_json(&app, &host).await)
}

/// One read of the feed. "Nothing newer" clears what was known; an error (offline, 404, broken) keeps it and is not shown.
pub async fn check(app: &AppHandle, host: &Host) {
    let Some(cfg) = feed() else { return };
    let (a, cur) = (host.armory.clone(), app.package_info().version.to_string());
    match blocking(move || a.os_check_apk(&cfg.feed, &cur)).await {
        Ok(Ok(found)) => host.os_update.lock().unwrap_or_else(|p| p.into_inner()).available = found,
        Ok(Err(e)) => eprintln!("os update check: {e}"),
        Err(e) => eprintln!("os update check: {e}"),
    }
    let s = state_json(app, host).await;
    let _ = app.emit(battalion_desktop::events::OS_UPDATE, s);
}

/// Download and check the APK, then stop the apps. Answers `{apk, sha256, version}`: the shell hands it to Android's installer
/// (`bosSlots` op `osInstall`). Any error before that leaves the apps running.
#[tauri::command]
pub async fn os_update_install(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    shell_only(&window, "os_update_install")?;
    let (upd, cfg) = {
        let mut s = host.os_update.lock().unwrap_or_else(|p| p.into_inner());
        if s.installing {
            return Err("an update is already running".into());
        }
        let upd = s.available.clone().ok_or("there is no newer BattalionOS to install")?;
        let cfg = feed().ok_or("this build has no update feed")?;
        s.installing = true;
        (upd, cfg)
    };
    let (a, app2, upd2) = (host.armory.clone(), app.clone(), upd.clone());
    let r = blocking(move || {
        let mut last = 0u8;
        let apk = a.os_stage_apk(&cfg.feed, &upd2, &mut |got, total| {
            let pct = if total > 0 { Some((got.min(total) * 100 / total) as u8) } else { None };
            if pct.is_some() && pct == Some(last) {
                return;
            }
            last = pct.unwrap_or(0);
            let _ = app2.emit(battalion_desktop::events::ARMORY_PROGRESS, json!({"id": OS_ROW, "op": "update", "stage": "download", "bytes": got, "total": if total > 0 { json!(total) } else { Value::Null }, "percent": pct}));
        })?;
        // The apps stop the normal way and are named in the resume list; they start again after the update (or the abort).
        if let Err(e) = a.os_stop_for_update() {
            let _ = std::fs::remove_file(&apk);
            return Err(e);
        }
        let _ = app2.emit(battalion_desktop::events::ARMORY_PROGRESS, json!({"id": OS_ROW, "op": "update", "stage": "install"}));
        Ok(apk)
    })
    .await?;
    match r {
        Ok(apk) => Ok(json!({"apk": apk.to_string_lossy(), "sha256": upd.sha256, "version": upd.version})),
        Err(e) => {
            host.os_update.lock().unwrap_or_else(|p| p.into_inner()).installing = false;
            Err(friendly(&e.to_string()))
        }
    }
}

/// Android did not install it (cancelled, wrong signature, no permission): start the apps again, drop the APK, keep the offer.
#[tauri::command]
pub async fn os_update_abort(window: WebviewWindow, app: AppHandle, host: State<'_, Host>) -> R<Value> {
    shell_only(&window, "os_update_abort")?;
    let a = host.armory.clone();
    let failed = blocking(move || {
        if let Ok(rd) = std::fs::read_dir(a.paths().staging()) {
            for e in rd.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".apk")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
        a.os_resume_apps()
    })
    .await?;
    for (id, e) in &failed {
        eprintln!("start {id} after a cancelled update: {e}");
    }
    host.os_update.lock().unwrap_or_else(|p| p.into_inner()).installing = false;
    Ok(state_json(&app, &host).await)
}

/// A download that could not reach the server, in plain words.
pub fn friendly(e: &str) -> String {
    let l = e.to_ascii_lowercase();
    if ["dns", "connect", "resolve", "network", "timed out", "connection lost"].iter().any(|k| l.contains(k)) {
        format!("No connection to the update server. Check the phone's internet and press Retry. ({e})")
    } else {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_is_data_and_points_at_this_repos_android_feed() {
        let f = feed_from(FEED).expect("os-feed.json");
        assert_eq!(f.feed.url, "https://github.com/SuperiorSoft-LLC/BattalionOS/releases/download/bos-android-stable/bos-android.json");
        assert!(!f.feed.allow_loopback_http);
        assert_eq!(f.every_secs, 4 * 60 * 60);
    }

    #[cfg(not(feature = "test-update"))]
    #[test]
    fn a_release_build_reads_only_the_configured_feed() {
        assert_eq!(feed().unwrap().feed, feed_from(FEED).unwrap().feed);
    }

    #[test]
    fn offline_reads_in_plain_words() {
        assert!(friendly("fetch https://x: Connection Failed: Connect error: dns").starts_with("No connection"));
        assert_eq!(friendly("an update is already running"), "an update is already running");
    }
}
