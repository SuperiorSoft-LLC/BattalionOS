// SPDX-License-Identifier: MPL-2.0
//! BOS-30: where the OS looks for a newer BattalionOS, and how it hands over to the installer. The pure parts (config, the
//! installer script, the check state) need no GUI; the commands that use them are in `commands.rs`.
//!
//! The feed address and the key are data in `tauri.conf.json` (`plugins.updater.endpoints`, `plugins.updater.pubkey`). Only a
//! test build (cargo feature `test-update`) lets `BATTALION_OS_UPDATE_FEED` and `BATTALION_OS_UPDATE_PUBKEY` replace them: a
//! release build has no such door, so no environment variable can point the OS at another feed or another key.

use battalion_host::armory::osupdate::{OsFeed, OsUpdate};
use serde_json::Value;

const CONFIG: &str = include_str!("../tauri.conf.json");

/// The platform entry of `latest.json` this build reads.
pub const PLATFORM: &str = "windows-x86_64";
/// The Armory checks once a day (and when it opens).
pub const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;
/// The id the Armory row and its progress report use. Not an app id: it is never passed to an app command.
pub const OS_ROW: &str = "battalionos";

pub fn feed_from(config: &str, platform: &str) -> Option<OsFeed> {
    let v: Value = serde_json::from_str(config).ok()?;
    let u = v.get("plugins")?.get("updater")?;
    let url = u.get("endpoints")?.as_array()?.first()?.as_str()?.to_string();
    let pubkey = u.get("pubkey")?.as_str()?.to_string();
    Some(OsFeed { url, pubkey, platform: platform.into() })
}

/// The feed this build uses.
pub fn feed() -> Option<OsFeed> {
    #[allow(unused_mut)]
    let mut f = feed_from(CONFIG, PLATFORM)?;
    #[cfg(feature = "test-update")]
    {
        if let Ok(u) = std::env::var("BATTALION_OS_UPDATE_FEED") {
            f.url = u;
        }
        if let Ok(k) = std::env::var("BATTALION_OS_UPDATE_PUBKEY") {
            f.pubkey = k;
        }
    }
    Some(f)
}

/// What the Armory shows about the OS.
#[derive(Default)]
pub struct OsUpdateState {
    pub available: Option<OsUpdate>,
    /// An install is running: a second one is refused.
    pub installing: bool,
}

/// How long the OS waits for the installer helper to say it is running before it gives up, starts its apps again and shows the
/// error instead of exiting (BOS-UPDATE-1).
pub const HELPER_START_TIMEOUT_SECS: u64 = 20;

/// The one-line script a hidden PowerShell runs after the OS has exited: say it is running (the `BOS_UPD_READY` file: the OS
/// exits only after it sees it), wait for the OS to end, run the installer (per user, no admin, with its own progress and
/// Cancel), note the installer's exit code in `BOS_UPD_LOG`, then start BattalionOS again whatever the installer did, so a
/// cancelled or failed installer leaves the old version running. Paths come from the environment (`BOS_UPD_*`), never from the
/// command line. The MSI path is quoted for msiexec (a profile path can hold a space).
pub const INSTALLER_SCRIPT: &str = "Set-Content -LiteralPath $env:BOS_UPD_READY -Value $PID; \
Wait-Process -Id ([int]$env:BOS_UPD_PID) -Timeout 120 -ErrorAction SilentlyContinue; \
$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', ('\"' + $env:BOS_UPD_MSI + '\"'), '/passive', '/norestart') -Wait -PassThru; \
Add-Content -LiteralPath $env:BOS_UPD_LOG -Value ('{0:o} msiexec exit {1}' -f (Get-Date), $p.ExitCode) -ErrorAction SilentlyContinue; \
Remove-Item Env:BOS_UPD_PID, Env:BOS_UPD_MSI, Env:BOS_UPD_EXE, Env:BOS_UPD_READY, Env:BOS_UPD_LOG -ErrorAction SilentlyContinue; \
Start-Process -FilePath $exe";

/// BOS-UPDATE-1: how the helper is started. Never DETACHED_PROCESS: `powershell.exe -WindowStyle Hidden` with no console at all
/// ends before it runs a line (0.1.0 to 0.2.x exited with no installer run). CREATE_NO_WINDOW gives it a hidden console;
/// CREATE_NEW_PROCESS_GROUP keeps the OS's console signals off it; CREATE_BREAKAWAY_FROM_JOB lets it outlive the OS when the OS
/// was started inside a kill-on-close job (tried first, then without it when the job does not allow breakaway).
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
pub const DETACHED_PROCESS: u32 = 0x0000_0008;
pub fn helper_flags(breakaway: bool) -> u32 {
    CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | if breakaway { CREATE_BREAKAWAY_FROM_JOB } else { 0 }
}

/// `INSTALLER_SCRIPT` with the exe read before the variables are dropped.
pub fn installer_script() -> String {
    format!("$exe = $env:BOS_UPD_EXE; {INSTALLER_SCRIPT}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_and_the_key_are_data_in_the_config() {
        let f = feed_from(CONFIG, PLATFORM).expect("tauri.conf.json names the feed and the key");
        assert_eq!(f.url, "https://github.com/SuperiorSoft-LLC/BattalionOS/releases/latest/download/latest.json");
        // The pinned key is key id 6F8ADF0B3874237E.
        use base64_text::decode;
        assert!(decode(&f.pubkey).contains("6F8ADF0B3874237E"));
    }

    #[test]
    fn a_release_build_ignores_the_environment() {
        // Without the test feature there is no code that reads these variables.
        #[cfg(not(feature = "test-update"))]
        {
            std::env::set_var("BATTALION_OS_UPDATE_FEED", "file:///evil");
            assert_eq!(feed().unwrap().url, feed_from(CONFIG, PLATFORM).unwrap().url);
            std::env::remove_var("BATTALION_OS_UPDATE_FEED");
        }
    }

    #[test]
    fn the_installer_script_reads_paths_from_the_environment_only() {
        let s = installer_script();
        assert!(s.contains("$env:BOS_UPD_MSI") && s.contains("$env:BOS_UPD_EXE") && s.contains("$env:BOS_UPD_PID"));
        assert!(s.contains("/passive") && !s.contains("/qn"), "the installer shows its progress and a Cancel");
        assert!(s.contains("Start-Process -FilePath $exe"), "the OS starts again whatever the installer did");
        assert!(s.starts_with("$exe = $env:BOS_UPD_EXE; Set-Content -LiteralPath $env:BOS_UPD_READY"), "the helper says it runs before anything else");
        assert!(s.contains("$env:BOS_UPD_LOG") && s.contains("$p.ExitCode"), "the installer's exit code is kept");
    }

    #[test]
    fn the_helper_is_never_detached() {
        for b in [true, false] {
            assert_eq!(helper_flags(b) & DETACHED_PROCESS, 0, "a detached powershell runs nothing (BOS-UPDATE-1)");
            assert_ne!(helper_flags(b) & CREATE_NO_WINDOW, 0);
        }
        assert_ne!(helper_flags(true) & CREATE_BREAKAWAY_FROM_JOB, 0);
    }

    /// BOS-UPDATE-1, the real thing: start the helper exactly as the OS does (same flags, same script) with msiexec swapped for a
    /// harmless `cmd /c exit 0` and the restart dropped, and check it runs: the ready file appears and the exit code is logged.
    #[cfg(windows)]
    #[test]
    fn the_helper_really_runs_on_windows() {
        use std::os::windows::process::CommandExt;
        let dir = std::env::temp_dir().join(format!("bos-upd-helper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (ready, log) = (dir.join("ready"), dir.join("log.txt"));
        let script = installer_script()
            .replace("'msiexec.exe'", "'cmd.exe'")
            .replace("@('/i', ('\"' + $env:BOS_UPD_MSI + '\"'), '/passive', '/norestart')", "@('/c', 'exit 0')")
            .replace("Start-Process -FilePath $exe", "$null = $exe");
        assert!(!script.contains("msiexec.exe") && !script.contains("/passive") && !script.contains("-FilePath $exe"), "nothing real is run");
        let mut c = std::process::Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script])
            .env("BOS_UPD_PID", "0")
            .env("BOS_UPD_MSI", dir.join("x.msi"))
            .env("BOS_UPD_EXE", "unused")
            .env("BOS_UPD_READY", &ready)
            .env("BOS_UPD_LOG", &log);
        let spawned = c.creation_flags(helper_flags(true)).spawn().or_else(|_| c.creation_flags(helper_flags(false)).spawn());
        spawned.expect("powershell starts");
        let t = std::time::Instant::now();
        while !(ready.is_file() && log.is_file()) && t.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let ok = ready.is_file();
        let l = std::fs::read_to_string(&log).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(ok, "the helper wrote its ready file");
        assert!(l.contains("msiexec exit 0"), "the installer's exit code is logged: {l:?}");
    }

    /// A tiny base64 reader for the test (the key is base64 of a text file).
    mod base64_text {
        pub fn decode(s: &str) -> String {
            let t = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
            for c in s.bytes().filter(|c| *c != b'=') {
                acc = acc << 6 | t.iter().position(|x| *x == c).unwrap() as u32;
                bits += 6;
                if bits >= 8 {
                    bits -= 8;
                    out.push((acc >> bits) as u8);
                    acc &= (1 << bits) - 1;
                }
            }
            String::from_utf8_lossy(&out).into_owned()
        }
    }
}
