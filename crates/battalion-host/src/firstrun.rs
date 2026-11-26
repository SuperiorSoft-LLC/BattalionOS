// SPDX-License-Identifier: MPL-2.0
//! First run: the apps the installer asked for are installed when the OS opens the first time.
//!
//! The installer lays down `first-run.json` beside the OS executable *only* for the apps the person
//! left checked (the checkbox is a WiX component condition, so "not chosen" means no file). The OS
//! reads it on every start and installs what is missing through the Armory, which makes each app
//! all-or-nothing. `first-run.state.json` in the OS home holds the done marker: it is written only
//! after every listed app is installed, so a failure, a cut network or a kill leaves it unwritten
//! and the next start (or a Retry) resumes. Once done, the plan is never run again, so an app the
//! person removes later is not put back. Uninstalling the OS deletes the executable folder, not
//! the home, so the marker and every app's state survive an uninstall.
//!
//! Plan file: `{"sources":["https://.../catalog.json"],"apps":[{"id":"first-app","channel":"stable"}]}`
//! (`channel` defaults to `stable`; `sources` is optional, see [`seed_sources`]).

use crate::armory::{Armory, ArmoryError};
use crate::paths::Paths;
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PLAN_FILE: &str = "first-run.json";
pub const STATE_FILE: &str = "first-run.state.json";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PlanApp {
    pub id: String,
    #[serde(default = "stable")]
    pub channel: String,
}
fn stable() -> String {
    "stable".into()
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Plan {
    #[serde(default)]
    pub apps: Vec<PlanApp>,
    /// Catalog urls to seed into `os.json` (`armory.sources`) when the person has none. Data, so
    /// the installer, not the code, says where the catalog lives.
    #[serde(default)]
    pub sources: Vec<String>,
}

/// What the marker file holds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    pub done: bool,
    /// Apps installed by (or already present at) a run, in plan order.
    pub installed: Vec<String>,
    pub last_error: Option<String>,
    pub attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No plan file (not chosen) or an empty plan: nothing to do, nothing written.
    NothingToDo,
    /// The marker says a run finished earlier.
    AlreadyDone,
    /// Every listed app is installed; the marker is written.
    Done(Vec<String>),
    /// The catalog or a package could not be fetched. Nothing is half installed; retry when online.
    Offline { id: String, reason: String },
    /// An app failed for another reason (bad package, bad plan, recovery). Retry runs it again.
    Failed { id: String, reason: String },
}

/// Progress for the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The run began; these ids will be installed.
    Start(Vec<String>),
    /// One app is now installed (the Armory's own `os://armory-progress` carries the bytes).
    AppInstalled(String),
    Finished(Outcome),
}

/// `first-run.json` beside the running executable (where the installer put it).
pub fn plan_path_beside_exe() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join(PLAN_FILE))
}

pub fn state_path(paths: &Paths) -> PathBuf {
    paths.root().join(STATE_FILE)
}

pub fn load_state(paths: &Paths) -> State {
    store::read_json::<State>(&state_path(paths)).ok().flatten().unwrap_or_default()
}

fn read_plan(path: &Path) -> Result<Option<Plan>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let plan: Plan = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    for a in &plan.apps {
        if !battalion_app_api::manifest::valid_id(&a.id) {
            return Err(format!("{}: app id '{}' is not allowed", path.display(), a.id));
        }
    }
    Ok(Some(plan))
}

fn is_network(e: &ArmoryError) -> bool {
    matches!(e, ArmoryError::Offline(_) | ArmoryError::Fetch { .. })
}

/// Run the first-run install once. Safe to call on every start and from a Retry button.
pub fn run(armory: &Armory, plan_path: &Path, on_event: &mut dyn FnMut(Event)) -> Outcome {
    let out = run_inner(armory, plan_path, on_event);
    on_event(Event::Finished(out.clone()));
    out
}

fn run_inner(armory: &Armory, plan_path: &Path, on_event: &mut dyn FnMut(Event)) -> Outcome {
    let paths = armory.paths();
    let plan = match read_plan(plan_path) {
        Ok(None) => return Outcome::NothingToDo,
        Ok(Some(p)) if p.apps.is_empty() => return Outcome::NothingToDo,
        Ok(Some(p)) => p,
        Err(reason) => return Outcome::Failed { id: String::new(), reason },
    };
    let mut state = load_state(paths);
    if state.done {
        return Outcome::AlreadyDone;
    }
    // A kill during an earlier run left a journal entry: finish or undo it before anything else.
    if let Err(e) = armory.recover() {
        return fail(paths, state, String::new(), e.to_string(), false);
    }
    on_event(Event::Start(plan.apps.iter().map(|a| a.id.clone()).collect()));
    state.attempts += 1;
    let mut installed = Vec::new();
    let mut first_failure: Option<Outcome> = None;
    for app in &plan.apps {
        match armory.install(&app.id, &app.channel) {
            Ok(_) | Err(ArmoryError::AlreadyInstalled(_)) => {
                installed.push(app.id.clone());
                on_event(Event::AppInstalled(app.id.clone()));
            }
            Err(e) => {
                let net = is_network(&e);
                let reason = e.to_string();
                first_failure.get_or_insert(if net { Outcome::Offline { id: app.id.clone(), reason } } else { Outcome::Failed { id: app.id.clone(), reason } });
                // Offline: every other app would fail the same way, so stop. Otherwise try the rest.
                if net {
                    break;
                }
            }
        }
    }
    state.installed = installed.clone();
    match first_failure {
        None => {
            state.done = true;
            state.last_error = None;
            let _ = store::write_json(&state_path(paths), &state);
            Outcome::Done(installed)
        }
        Some(out) => {
            let (Outcome::Offline { reason, .. } | Outcome::Failed { reason, .. }) = &out else { unreachable!() };
            state.last_error = Some(reason.clone());
            let _ = store::write_json(&state_path(paths), &state);
            out
        }
    }
}

fn fail(paths: &Paths, mut state: State, id: String, reason: String, net: bool) -> Outcome {
    state.last_error = Some(reason.clone());
    let _ = store::write_json(&state_path(paths), &state);
    if net { Outcome::Offline { id, reason } } else { Outcome::Failed { id, reason } }
}

/// Call before the Armory is built (it reads `os.json` once): when the plan names catalog sources and
/// `os.json` has none, write them into `armory.sources`, keeping every other key. Returns whether it wrote.
/// A source the person set themselves is never replaced.
pub fn seed_sources(paths: &Paths, plan_path: &Path) -> std::io::Result<bool> {
    let Ok(Some(plan)) = read_plan(plan_path) else { return Ok(false) };
    seed_source_list(paths, &plan.sources)
}

/// The catalog urls of a plan's text (`{"sources":[...]}`), for a build that carries its plan inside
/// (AND-2: the Android app has no folder beside an exe). Bad text = none.
pub fn plan_sources(text: &str) -> Vec<String> {
    serde_json::from_str::<Plan>(text).map(|p| p.sources).unwrap_or_default()
}

/// [`seed_sources`] with the urls in hand: written only when `os.json` names no source yet.
pub fn seed_source_list(paths: &Paths, sources: &[String]) -> std::io::Result<bool> {
    let plan = Plan { sources: sources.to_vec(), ..Plan::default() };
    if plan.sources.is_empty() {
        return Ok(false);
    }
    let mut os: serde_json::Value = store::read_json(&paths.os_json()).ok().flatten().unwrap_or_else(|| serde_json::json!({}));
    let Some(obj) = os.as_object_mut() else { return Ok(false) };
    let armory = obj.entry("armory").or_insert_with(|| serde_json::json!({}));
    let Some(armory) = armory.as_object_mut() else { return Ok(false) };
    if armory.get("sources").and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty()) {
        return Ok(false);
    }
    armory.insert("sources".into(), serde_json::json!(plan.sources));
    paths.ensure()?;
    store::write_json(&paths.os_json(), &os)?;
    Ok(true)
}
