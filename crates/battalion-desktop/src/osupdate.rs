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

/// The one-line script a hidden PowerShell runs after the OS has exited: wait for the OS to end, run the installer (per user, no
/// admin, with its own progress and Cancel), then start BattalionOS again whatever the installer did, so a cancelled or failed
/// installer leaves the old version running. Paths come from the environment (`BOS_UPD_*`), never from the command line.
pub const INSTALLER_SCRIPT: &str = "Wait-Process -Id ([int]$env:BOS_UPD_PID) -Timeout 120 -ErrorAction SilentlyContinue; \
$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', $env:BOS_UPD_MSI, '/passive', '/norestart') -Wait -PassThru; \
Remove-Item Env:BOS_UPD_PID, Env:BOS_UPD_MSI, Env:BOS_UPD_EXE -ErrorAction SilentlyContinue; \
Start-Process -FilePath $exe";

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
