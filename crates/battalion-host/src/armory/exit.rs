// SPDX-License-Identifier: MPL-2.0
//! The OS ends. Only the OS process goes: apps are let go and keep running, and the next start
//! adopts them. An app whose manifest says `"onOsExit": "stop"` is stopped first.

use super::*;

impl Armory {
    /// Ids of the running apps whose installed manifest asks to be stopped when the OS exits.
    pub fn stop_on_exit_ids(&self) -> Vec<String> {
        #[derive(Deserialize)]
        struct Policy {
            #[serde(rename = "onOsExit")]
            on_os_exit: Option<String>,
        }
        let mut out = Vec::new();
        for a in self.installed().unwrap_or_default() {
            if !matches!(a.state.as_str(), "running" | "starting") {
                continue;
            }
            let p = self.paths.version_dir(&a.id, &a.version).ok().and_then(|d| std::fs::read_to_string(d.join("app.json")).ok()).and_then(|t| serde_json::from_str::<Policy>(&t).ok());
            if p.and_then(|p| p.on_os_exit).is_some_and(|v| v == "stop") {
                out.push(a.id);
            }
        }
        out
    }

    /// Stop the apps that asked for it, then let go of the rest. Returns (id, error) of an app
    /// that could not be stopped; the exit goes on regardless.
    pub fn exit_os(&self) -> Vec<(String, String)> {
        let mut failed = Vec::new();
        for id in self.stop_on_exit_ids() {
            if let Err(e) = self.sup.stop(&id) {
                failed.push((id, format!("{e:?}")));
            }
        }
        self.sup.release_all();
        failed
    }
}

