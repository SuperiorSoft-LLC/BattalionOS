// SPDX-License-Identifier: MPL-2.0
//! The app supervisor: one app worker per app, owned by the OS. The OS starts its own program as the
//! app's worker (see [`crate::worker`]), the worker loads the app's library and runs it on its own
//! thread. The worker is the OS's child, in its own job (Windows Job Object; unix process group), and
//! the OS holds its stdin: end of file means stop, so when the OS ends for any reason the app ends too.
//! Stopping is: close stdin (the app's `stop` is called), wait `stopTimeoutMs`, end the job.
//! Health, restart with backoff and the ready file are as in contract v1. A leftover recorded by an
//! older run is stopped by its recorded pid and start time, never by name.
//!
//! Isolation is the worker boundary: apps share nothing but the supervisor's bookkeeping, and each
//! app has its own worker, entry, lock, monitor thread and run folder.
//!
//! ```text
//! <home>/run/<id>/ready.json   written by the app (BATTALION_READY_FILE)
//! <home>/run/<id>/token        per-start secret the app may use (BATTALION_TOKEN_FILE)
//! <home>/run/<id>/proc.json    our record: pid + start time of the worker WE started
//! <home>/logs/<id>.log         its stdout and stderr
//! ```
//! A process is ours only when `proc.json` names it and the OS agrees on pid and start time.
//! A ready file with a live pid and no such record is a foreign process: refused, never killed.
//!
//! Numbers live in `os.json` under `"supervisor"` ([`Settings`]); every one has a default.

use crate::lock::{Lock, LockError};
use crate::paths::{BadAppId, Paths};
use crate::process::{self, ProcId, Worker, WorkerSpec};
use crate::state::{AppState, Current};
use crate::store;
use battalion_app_api::manifest::ProcessSpec;
use battalion_app_api::{env, Health, Manifest, ReadyFile, CONTRACT};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `os.json` -> `"supervisor": { ... }`. Unknown keys are ignored; missing keys take the default.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// How often the ready file is looked at while an app starts.
    pub ready_poll_ms: u64,
    pub health_interval_ms: u64,
    /// One health call that takes longer than this counts as a failed probe.
    pub health_timeout_ms: u64,
    /// Failed probes in a row before a running (but silent) app is killed and restarted.
    pub health_fail_limit: u32,
    pub backoff_initial_ms: u64,
    pub backoff_max_ms: u64,
    /// An app that stayed healthy this long starts its backoff and restart count over.
    pub backoff_reset_after_ms: u64,
    /// Automatic restarts in a row before the supervisor gives up and leaves the app failed.
    pub max_restarts: u32,
    /// Wait after a tree kill for the process to be gone.
    pub kill_wait_ms: u64,
    /// Restart a dead or hung app by itself. Off = it is only reported.
    pub auto_restart: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            ready_poll_ms: 50,
            health_interval_ms: 2_000,
            health_timeout_ms: 2_000,
            health_fail_limit: 3,
            backoff_initial_ms: 1_000,
            backoff_max_ms: 60_000,
            backoff_reset_after_ms: 60_000,
            max_restarts: 5,
            kill_wait_ms: 5_000,
            auto_restart: true,
        }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Settings {
        #[derive(Deserialize, Default)]
        struct Os {
            #[serde(default)]
            supervisor: Settings,
        }
        match store::read_json::<Os>(&paths.os_json()) {
            Ok(Some(o)) => o.supervisor,
            _ => Settings::default(), // no os.json yet, or an unreadable one: the defaults still run apps
        }
    }
    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }
}

#[derive(Debug)]
pub enum SupError {
    BadId(String),
    NotInstalled(String),
    /// The app is not in a state this call can start from or stop (message names it).
    WrongState(String),
    /// Another Armory action or start holds the app lock.
    Busy,
    /// A live process we did not start is on this app's run files. Not touched.
    Foreign { pid: u32 },
    NoProcessPart,
    /// It did not become ready; the message says why (timeout, exit with log tail, bad ready file).
    NotReady(String),
    Io(String),
}
impl std::fmt::Display for SupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupError::BadId(i) => write!(f, "app id '{i}' is not allowed"),
            SupError::NotInstalled(i) => write!(f, "app '{i}' is not installed"),
            SupError::WrongState(s) => write!(f, "{s}"),
            SupError::Busy => write!(f, "the app is busy with another action"),
            SupError::Foreign { pid } => write!(f, "process {pid} is using this app's run files and was not started by the OS; not touched"),
            SupError::NoProcessPart => write!(f, "the app has no process part for this platform"),
            SupError::NotReady(m) => write!(f, "the app did not become ready: {m}"),
            SupError::Io(m) => write!(f, "{m}"),
        }
    }
}
impl std::error::Error for SupError {}
impl From<BadAppId> for SupError {
    fn from(e: BadAppId) -> SupError {
        SupError::BadId(e.0)
    }
}
fn io_err(e: std::io::Error) -> SupError {
    SupError::Io(e.to_string())
}

/// What the supervisor can say about one app.
#[derive(Debug, Clone, PartialEq)]
pub struct AppStatus {
    pub id: String,
    /// `current.json` state name.
    pub state: String,
    pub pid: Option<u32>,
    pub url: Option<String>,
    pub paused: bool,
    /// Automatic restarts since the app was last stable.
    pub restarts: u32,
    pub last_error: Option<String>,
}

/// Our record of a process we started. The only proof that a pid is ours.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    id: ProcId,
    version: String,
    url: String,
    health: String,
    stop_timeout_ms: u64,
}

#[derive(Clone)]
struct Live {
    rec: Record,
    /// The worker, owned by the OS. The last reference dropped ends it and its job.
    worker: Arc<Worker>,
    since: Instant,
}

#[derive(Default)]
struct Entry {
    /// One start/stop/restart at a time per app. Other apps are not affected.
    op: Mutex<()>,
    live: Mutex<Option<Live>>,
    /// Bumped by every start and stop; a monitor thread of an older value ends itself.
    gen: AtomicU64,
    paused: AtomicBool,
    /// AND-1: the app runs as its page only (a `ui` and no process part for this platform): no worker,
    /// no health, nothing to restart. Same states as a process app (starting, running, stopping, stopped).
    page: AtomicBool,
    restarts: AtomicU32,
    last_error: Mutex<Option<String>>,
}

struct Inner {
    paths: Paths,
    settings: Settings,
    /// The OS's own program, started with the worker argument. Tests name their fixture OS here.
    worker_exe: PathBuf,
    apps: Mutex<HashMap<String, Arc<Entry>>>,
}

#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
}

pub fn platform_key() -> String {
    // AND-1: an Android OS runs pages only (no native app content), so every Android device (the x86_64
    // emulator too) takes the feed's one `android-aarch64` package.
    if cfg!(target_os = "android") {
        return "android-aarch64".into();
    }
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

impl Supervisor {
    /// Settings are read from `os.json` once, here.
    pub fn new(paths: Paths) -> Supervisor {
        let settings = Settings::load(&paths);
        Supervisor::with_settings(paths, settings)
    }
    pub fn with_settings(paths: Paths, settings: Settings) -> Supervisor {
        let exe = std::env::current_exe().unwrap_or_default();
        Supervisor::with_worker_exe(paths, settings, exe)
    }
    /// `worker_exe` is the program that answers to the worker argument (the OS itself).
    pub fn with_worker_exe(paths: Paths, settings: Settings, worker_exe: PathBuf) -> Supervisor {
        Supervisor { inner: Arc::new(Inner { paths, settings, worker_exe, apps: Mutex::new(HashMap::new()) }) }
    }
    pub fn settings(&self) -> &Settings {
        &self.inner.settings
    }

    fn entry(&self, id: &str) -> Arc<Entry> {
        self.inner.apps.lock().unwrap().entry(id.to_string()).or_default().clone()
    }
    fn run_dir(&self, id: &str) -> Result<PathBuf, SupError> {
        self.inner.paths.app_dir(id)?; // validates the id
        Ok(self.inner.paths.root().join("run").join(id))
    }

    // ---- public API -------------------------------------------------------------------------

    /// Start an installed, stopped app and return once it is ready and healthy.
    pub fn start(&self, id: &str) -> Result<AppStatus, SupError> {
        let e = self.entry(id);
        let _op = e.op.lock().unwrap();
        e.restarts.store(0, Ordering::SeqCst);
        self.start_locked(id, &e)?;
        self.status(id)
    }

    /// Ask the app to stop (its worker sees end of file on stdin and calls the app's `stop`), then end the worker's job if it does not go in time.
    /// An app that is not running is `Ok`. A foreign process is refused.
    pub fn stop(&self, id: &str) -> Result<AppStatus, SupError> {
        let e = self.entry(id);
        e.gen.fetch_add(1, Ordering::SeqCst); // the monitor ends; no restart races the stop
        let _op = e.op.lock().unwrap();
        self.stop_locked(id, &e)?;
        self.status(id)
    }

    /// Stop supervising: no health kills, no restarts. The process keeps running untouched.
    pub fn pause(&self, id: &str) {
        self.entry(id).paused.store(true, Ordering::SeqCst);
    }
    pub fn resume(&self, id: &str) {
        let e = self.entry(id);
        e.paused.store(false, Ordering::SeqCst);
    }

    pub fn status(&self, id: &str) -> Result<AppStatus, SupError> {
        let cur_path = self.inner.paths.current_json(id)?;
        let state = Current::load(&cur_path).map_err(io_err)?.ok_or_else(|| SupError::NotInstalled(id.into()))?.state;
        let e = self.entry(id);
        let live = e.live.lock().unwrap().clone();
        let last_error = e.last_error.lock().unwrap().clone();
        Ok(AppStatus {
            id: id.into(),
            state: state.name().into(),
            pid: live.as_ref().map(|l| l.rec.id.pid),
            url: live.as_ref().map(|l| l.rec.url.clone()),
            paused: e.paused.load(Ordering::SeqCst),
            restarts: e.restarts.load(Ordering::SeqCst),
            last_error,
        })
    }

    /// One health call. `Err` = no answer; `Ok(health)` = whatever the app said.
    pub fn health(&self, id: &str) -> Result<Health, SupError> {
        let live = self.entry(id).live.lock().unwrap().clone().ok_or_else(|| SupError::WrongState(format!("app '{id}' is not running")))?;
        probe(&live.rec, &self.inner.settings).map_err(SupError::NotReady)
    }

    /// When the OS starts: settle what an older run left behind. A worker or process recorded in
    /// `proc.json` that still runs is stopped by its recorded pid and start time (never by name),
    /// and an app whose state says up with nothing behind it is set to stopped. Apps are not
    /// adopted: an app belongs to the OS run that started it.
    pub fn recover_all(&self) -> Vec<(String, Result<bool, SupError>)> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(self.inner.paths.apps()) else { return out };
        let mut ids: Vec<String> = rd.flatten().filter(|d| d.path().is_dir()).map(|d| d.file_name().to_string_lossy().to_string()).collect();
        ids.sort();
        for id in ids {
            if battalion_app_api::manifest::valid_id(&id) {
                let r = self.recover(&id);
                out.push((id, r));
            }
        }
        out
    }

    /// `Ok(true)` = a leftover was stopped or a state was settled.
    pub fn recover(&self, id: &str) -> Result<bool, SupError> {
        let e = self.entry(id);
        let _op = e.op.lock().unwrap();
        let cur_path = self.inner.paths.current_json(id)?;
        let Some(mut cur) = Current::load(&cur_path).map_err(io_err)? else { return Ok(false) };
        if e.live.lock().unwrap().is_some() || e.page.load(Ordering::SeqCst) {
            return Ok(false); // ours, this run
        }
        let run = self.run_dir(id)?;
        let rec: Option<Record> = store::read_json(&run.join("proc.json")).map_err(io_err)?;
        let was_up = matches!(cur.state, AppState::Starting | AppState::Running | AppState::Stopping);
        let mut did = false;
        if let Some(rec) = rec {
            if process::is_alive(&rec.id) {
                // Recorded by us (pid and start time match): ours to stop, by pid.
                process::kill_tree(&rec.id);
                process::wait_gone(&rec.id, Settings::ms(self.inner.settings.kill_wait_ms));
            }
            self.cleanup(&run);
            did = true;
        }
        if was_up {
            self.mark_dead(&mut cur, &cur_path, "the app ended when the OS ended");
            did = true;
        }
        Ok(did)
    }

    /// The OS exits: stop every running app cleanly (the app's `stop`, the timeout, then the job).
    /// Returns (id, error) of an app that could not be stopped; the exit goes on regardless.
    pub fn stop_all(&self) -> Vec<(String, String)> {
        let ids: Vec<String> = self.inner.apps.lock().unwrap().iter().filter(|(_, e)| e.live.lock().unwrap().is_some() || e.page.load(Ordering::SeqCst)).map(|(k, _)| k.clone()).collect();
        let mut failed = Vec::new();
        for id in ids {
            if let Err(e) = self.stop(&id) {
                failed.push((id, e.to_string()));
            }
        }
        failed
    }

    // ---- start ------------------------------------------------------------------------------

    fn start_locked(&self, id: &str, e: &Arc<Entry>) -> Result<(), SupError> {
        let paths = &self.inner.paths;
        let st = &self.inner.settings;
        let cur_path = paths.current_json(id)?;
        let mut cur = Current::load(&cur_path).map_err(io_err)?.ok_or_else(|| SupError::NotInstalled(id.into()))?;
        if e.live.lock().unwrap().is_some() || e.page.load(Ordering::SeqCst) {
            return Err(SupError::WrongState(format!("app '{id}' is already running")));
        }
        let app_lock = match Lock::try_acquire(&paths.app_lock(id)?) {
            Ok(l) => l,
            Err(LockError::Busy) => return Err(SupError::Busy),
            Err(LockError::Io(x)) => return Err(io_err(x)),
        };
        let run = self.run_dir(id)?;
        std::fs::create_dir_all(&run).map_err(io_err)?;

        // Before anything is written: is something already there that is not ours?
        let ready_path = run.join("ready.json");
        let rec: Option<Record> = store::read_json(&run.join("proc.json")).map_err(io_err)?;
        if let Some(r) = &rec {
            if process::is_alive(&r.id) {
                // Ours and running although we hold no handle (e.g. adopt was not called): refuse a
                // second copy; the caller adopts or stops it.
                return Err(SupError::WrongState(format!("app '{id}' already has a running process (pid {}); adopt or stop it", r.id.pid)));
            }
        } else if let Ok(text) = std::fs::read_to_string(&ready_path) {
            if let Ok(rf) = serde_json::from_str::<ReadyFile>(&text) {
                if process::pid_running(rf.pid) {
                    return Err(SupError::Foreign { pid: rf.pid });
                }
            }
        }
        self.cleanup(&run); // stale files of a dead process

        match cur.state {
            AppState::Stopped => {}
            AppState::Failed(_) => cur.transition(&cur_path, AppState::Stopped).map_err(io_err)?,
            AppState::Starting | AppState::Running | AppState::Stopping => {
                // The state says up but nothing of ours is: the OS died in between. Settle it.
                self.mark_dead(&mut cur, &cur_path, "no running process behind the recorded state");
            }
            ref s => return Err(SupError::WrongState(format!("app '{id}' is {}, not stopped", s.name()))),
        }

        let version = cur.version.clone();
        let vdir = paths.content_dir(id, &version)?;
        let manifest_text = std::fs::read_to_string(vdir.join("app.json")).map_err(io_err)?;
        let manifest = Manifest::parse(&manifest_text, &platform_key(), CONTRACT).map_err(|x| SupError::NotReady(format!("manifest: {x}")))?;
        let Some(spec) = manifest.process_for(&platform_key()) else {
            // AND-1 page runner: an app with a page and no process part for this platform opens as its page.
            if manifest.ui.is_none() {
                return Err(SupError::NoProcessPart);
            }
            std::fs::create_dir_all(state_dir_of(&manifest, paths, id)?).map_err(io_err)?;
            cur.transition(&cur_path, AppState::Starting).map_err(io_err)?;
            cur.transition(&cur_path, AppState::Running).map_err(io_err)?;
            *e.last_error.lock().unwrap() = None;
            e.page.store(true, Ordering::SeqCst);
            drop(app_lock);
            return Ok(());
        };
        let spec: &ProcessSpec = spec;

        let state_dir = state_dir_of(&manifest, paths, id)?;
        std::fs::create_dir_all(&state_dir).map_err(io_err)?;
        // An app with a page gets the page's origin before it starts: one port that stays the same between runs, so the app
        // can let exactly that origin reach its socket and the page keeps its saved settings.
        let ui_origin = match manifest.ui {
            // A dev app with its own dev server: that server's origin is the page's origin (loopback only, checked at registration).
            Some(_) => match crate::dev::load(paths, id).and_then(|d| d.dev_url) {
                Some(u) => Some(crate::dev::dev_url_origin(&u).map_err(SupError::Io)?),
                None => Some(crate::uiport::origin_for(paths, id).map_err(io_err)?),
            },
            None => None,
        };
        let library = crate::dev::library_to_load(paths, id, &vdir.join(&spec.library)).map_err(SupError::Io)?;
        let token = process::random_token();
        store::write_atomic(&run.join("token"), token.as_bytes()).map_err(io_err)?;
        let log = paths.logs().join(format!("{id}.log"));

        cur.transition(&cur_path, AppState::Starting).map_err(io_err)?;
        // The OS's own BATTALION_* and whatever the manifest scrubs (`envScrub` prefixes) are not the
        // app's; the contract values are set, the manifest's own values next, and the rest (the
        // user's tools) passes through.
        let env_remove: Vec<String> = std::env::vars_os()
            .filter_map(|(k, _)| k.into_string().ok())
            .filter(|k| {
                let u = k.to_ascii_uppercase();
                u.starts_with("BATTALION_") || spec.env_scrub.iter().any(|p| u.starts_with(&p.to_ascii_uppercase()))
            })
            .collect();
        let mut envs: Vec<(String, String)> = spec.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        // BOS-31: what a developer registered for this dev app (a free port for its server, say) goes on top of its manifest's own.
        if let Some(d) = crate::dev::load(paths, id) {
            envs.extend(d.env);
        }
        envs.extend([
            (env::CONTRACT.into(), CONTRACT.to_string()),
            (env::APP_ID.into(), id.into()),
            (env::APP_VERSION.into(), version.clone()),
            (env::APP_DIR.into(), vdir.display().to_string()),
            (env::STATE_DIR.into(), state_dir.display().to_string()),
            (env::READY_FILE.into(), ready_path.display().to_string()),
            (env::TOKEN_FILE.into(), run.join("token").display().to_string()),
        ]);
        if let Some(o) = &ui_origin {
            envs.push((env::UI_ORIGIN.into(), o.clone()));
        }
        let context = battalion_app_api::abi::Context {
            contract: CONTRACT,
            abi: battalion_app_api::abi::ABI_VERSION,
            app_id: id.into(),
            app_version: version.clone(),
            app_dir: vdir.display().to_string(),
            state_dir: state_dir.display().to_string(),
            ready_file: ready_path.display().to_string(),
            token_file: run.join("token").display().to_string(),
            args: spec.args.clone(),
            ui_origin: ui_origin.clone(),
        };
        let spawn = process::spawn_worker(&WorkerSpec {
            exe: self.inner.worker_exe.clone(),
            library,
            env: envs,
            env_remove,
            cwd: vdir.clone(),
            log: log.clone(),
            context: serde_json::to_string(&context).unwrap_or_default(),
        });
        let worker = match spawn {
            Ok(w) => Arc::new(w),
            Err(x) => return self.fail_start(&mut cur, &cur_path, &run, None, e, format!("could not start the worker for {}: {x}", spec.library)),
        };
        let pid = worker.id.clone();

        // Wait for the ready file; a process that dies first ends the wait at once.
        let deadline = Instant::now() + Duration::from_millis(spec.ready_timeout_ms);
        let ready = loop {
            if let Ok(text) = std::fs::read_to_string(&ready_path) {
                match ReadyFile::parse(&text, CONTRACT) {
                    Ok(r) => break r,
                    Err(x) => return self.fail_start(&mut cur, &cur_path, &run, Some(&worker), e, x.to_string()),
                }
            }
            if !process::is_alive(&worker.id) {
                let why = process::exit_reason(&log);
                return self.fail_start(&mut cur, &cur_path, &run, Some(&worker), e, why);
            }
            if Instant::now() >= deadline {
                return self.fail_start(&mut cur, &cur_path, &run, Some(&worker), e, format!("no ready file after {} ms", spec.ready_timeout_ms));
            }
            std::thread::sleep(Settings::ms(st.ready_poll_ms));
        };
        let rec = Record {
            id: pid.clone(),
            version,
            url: ready.url.clone(),
            health: ready.health.clone(),
            stop_timeout_ms: spec.stop_timeout_ms,
        };
        // First health call must pass inside what is left of the ready window.
        loop {
            match probe(&rec, st) {
                Ok(_) => break,
                Err(m) if Instant::now() >= deadline => return self.fail_start(&mut cur, &cur_path, &run, Some(&worker), e, format!("ready but not healthy: {m}")),
                Err(_) => std::thread::sleep(Settings::ms(st.ready_poll_ms)),
            }
        }
        store::write_json(&run.join("proc.json"), &rec).map_err(io_err)?;
        cur.transition(&cur_path, AppState::Running).map_err(io_err)?;
        *e.last_error.lock().unwrap() = None;
        *e.live.lock().unwrap() = Some(Live { rec, worker, since: Instant::now() });
        let gen = e.gen.fetch_add(1, Ordering::SeqCst) + 1;
        drop(app_lock);
        self.spawn_monitor(id, e, gen);
        Ok(())
    }

    fn fail_start(&self, cur: &mut Current, cur_path: &std::path::Path, run: &std::path::Path, worker: Option<&Arc<Worker>>, e: &Entry, why: String) -> Result<(), SupError> {
        if let Some(w) = worker {
            w.kill();
            process::wait_gone(&w.id, Settings::ms(self.inner.settings.kill_wait_ms));
            w.try_exit();
        }
        self.cleanup(run);
        let _ = cur.transition(cur_path, AppState::Failed(why.clone()));
        let _ = cur.transition(cur_path, AppState::Stopped);
        *e.last_error.lock().unwrap() = Some(why.clone());
        Err(SupError::NotReady(why))
    }

    // ---- stop -------------------------------------------------------------------------------

    fn stop_locked(&self, id: &str, e: &Arc<Entry>) -> Result<(), SupError> {
        let cur_path = self.inner.paths.current_json(id)?;
        let mut cur = Current::load(&cur_path).map_err(io_err)?.ok_or_else(|| SupError::NotInstalled(id.into()))?;
        let run = self.run_dir(id)?;
        if e.page.swap(false, Ordering::SeqCst) {
            // A page-only app: nothing to end but its state.
            if cur.state == AppState::Running {
                cur.transition(&cur_path, AppState::Stopping).map_err(io_err)?;
            }
            if cur.state == AppState::Stopping {
                cur.transition(&cur_path, AppState::Stopped).map_err(io_err)?;
            }
            return Ok(());
        }
        let live = e.live.lock().unwrap().clone();
        // `live` = a worker of this run. Without it only a leftover of an older run can be ours: the
        // record and the OS must agree on pid and start time.
        let (rec, worker) = match live {
            Some(l) => (Some(l.rec), Some(l.worker)),
            None => {
                let rec: Option<Record> = store::read_json(&run.join("proc.json")).map_err(io_err)?;
                (rec.filter(|r| process::is_alive(&r.id)), None)
            }
        };
        let Some(rec) = rec else {
            // Nothing of ours is running. A live ready-file pid here is somebody else's: say so, touch nothing.
            if let Ok(text) = std::fs::read_to_string(run.join("ready.json")) {
                if let Ok(rf) = serde_json::from_str::<ReadyFile>(&text) {
                    if process::pid_running(rf.pid) && !matches!(cur.state, AppState::Stopped | AppState::Installed) {
                        return Err(SupError::Foreign { pid: rf.pid });
                    }
                }
            }
            self.cleanup(&run);
            if matches!(cur.state, AppState::Starting | AppState::Running | AppState::Stopping) {
                self.mark_dead(&mut cur, &cur_path, "stop: no running process");
            }
            return Ok(());
        };
        if cur.state == AppState::Running {
            cur.transition(&cur_path, AppState::Stopping).map_err(io_err)?;
        }
        match &worker {
            Some(w) => {
                // Polite first: the worker sees end of file and calls the app's stop. A hung app does
                // not end; the timeout passes and the job is ended.
                w.close_stdin();
                if !process::wait_gone(&rec.id, Duration::from_millis(rec.stop_timeout_ms)) {
                    w.kill();
                    if !process::wait_gone(&rec.id, Settings::ms(self.inner.settings.kill_wait_ms)) {
                        return Err(SupError::Io(format!("worker {} did not end after its job was ended", rec.id.pid)));
                    }
                } else {
                    w.kill(); // the worker ended on its own; whatever the app started in the job goes with it
                }
                w.try_exit();
            }
            None => {
                process::kill_tree(&rec.id);
                if !process::wait_gone(&rec.id, Settings::ms(self.inner.settings.kill_wait_ms)) {
                    return Err(SupError::Io(format!("process {} did not end after a tree kill", rec.id.pid)));
                }
            }
        }
        *e.live.lock().unwrap() = None;
        self.cleanup(&run);
        if cur.state == AppState::Stopping {
            cur.transition(&cur_path, AppState::Stopped).map_err(io_err)?;
        }
        Ok(())
    }

    // ---- monitor ----------------------------------------------------------------------------

    fn spawn_monitor(&self, id: &str, e: &Arc<Entry>, gen: u64) {
        let (sup, id, e) = (self.clone(), id.to_string(), e.clone());
        let _ = std::thread::Builder::new().name(format!("bos-sup-{id}")).spawn(move || sup.monitor(&id, &e, gen));
    }

    /// Sleep in small slices; false = this monitor was replaced or the OS released the app.
    fn nap(&self, e: &Entry, gen: u64, total: Duration) -> bool {
        let end = Instant::now() + total;
        while Instant::now() < end {
            if e.gen.load(Ordering::SeqCst) != gen {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20).min(total));
        }
        e.gen.load(Ordering::SeqCst) == gen
    }

    fn monitor(&self, id: &str, e: &Arc<Entry>, gen: u64) {
        let st = &self.inner.settings;
        let mut fails = 0u32;
        loop {
            if !self.nap(e, gen, Settings::ms(st.health_interval_ms)) {
                return;
            }
            if e.paused.load(Ordering::SeqCst) {
                continue;
            }
            let Some(live) = e.live.lock().unwrap().clone() else { return };
            let alive = process::is_alive(&live.rec.id);
            if alive && probe(&live.rec, st).is_ok() {
                fails = 0;
                if live.since.elapsed() >= Settings::ms(st.backoff_reset_after_ms) {
                    e.restarts.store(0, Ordering::SeqCst);
                }
                continue;
            }
            if alive {
                fails += 1;
                if fails < st.health_fail_limit {
                    continue;
                }
            }
            let reason = if alive { format!("no healthy answer in {fails} probes") } else { "the process ended".to_string() };
            self.on_failure(id, e, gen, reason);
            return; // a restart starts its own monitor
        }
    }

    fn on_failure(&self, id: &str, e: &Arc<Entry>, gen: u64, reason: String) {
        {
            let _op = e.op.lock().unwrap();
            if e.gen.load(Ordering::SeqCst) != gen || e.paused.load(Ordering::SeqCst) {
                return; // stopped, replaced or paused while we waited for the lock
            }
            let Some(live) = e.live.lock().unwrap().take() else { return };
            // Ours (our worker, our job): end it. Another app is never in reach.
            live.worker.kill();
            process::wait_gone(&live.rec.id, Settings::ms(self.inner.settings.kill_wait_ms));
            live.worker.try_exit();
            if let (Ok(run), Ok(cp)) = (self.run_dir(id), self.inner.paths.current_json(id)) {
                self.cleanup(&run);
                if let Ok(Some(mut cur)) = Current::load(&cp) {
                    self.mark_dead(&mut cur, &cp, &reason);
                }
            }
            *e.last_error.lock().unwrap() = Some(reason);
        }
        if self.inner.settings.auto_restart {
            self.restart_loop(id, e, gen);
        }
    }

    /// Wait (backoff), start again; a start that fails counts and is retried, up to `max_restarts`.
    fn restart_loop(&self, id: &str, e: &Arc<Entry>, gen: u64) {
        let st = &self.inner.settings;
        loop {
            let n = e.restarts.load(Ordering::SeqCst);
            if n >= st.max_restarts {
                let msg = format!("{}; gave up after {n} restarts", e.last_error.lock().unwrap().clone().unwrap_or_default());
                *e.last_error.lock().unwrap() = Some(msg.clone());
                if let Ok(cp) = self.inner.paths.current_json(id) {
                    if let Ok(Some(mut cur)) = Current::load(&cp) {
                        let _ = cur.transition(&cp, AppState::Failed(msg));
                    }
                }
                return;
            }
            let delay = st.backoff_initial_ms.saturating_mul(1u64 << n.min(20)).min(st.backoff_max_ms);
            // Back off without holding the app's lock: a stop must get in.
            if !self.nap(e, gen, Settings::ms(delay)) {
                return;
            }
            let _op = e.op.lock().unwrap();
            if e.gen.load(Ordering::SeqCst) != gen || e.paused.load(Ordering::SeqCst) {
                return;
            }
            e.restarts.fetch_add(1, Ordering::SeqCst);
            match self.start_locked(id, e) {
                Ok(()) => return, // running again, with its own monitor
                Err(err) => *e.last_error.lock().unwrap() = Some(format!("restart failed: {err}")),
            }
        }
    }
    // ---- small helpers ----------------------------------------------------------------------

    /// Remove the run files of a process that is gone (or being replaced). Never the state dir.
    fn cleanup(&self, run: &std::path::Path) {
        for f in ["ready.json", "proc.json", "token"] {
            let _ = std::fs::remove_file(run.join(f));
        }
    }

    /// The state said up, nothing is: record it as failed, then stopped, so a start can follow.
    fn mark_dead(&self, cur: &mut Current, path: &std::path::Path, why: &str) {
        let _ = cur.transition(path, AppState::Failed(why.into()));
        let _ = cur.transition(path, AppState::Stopped);
    }
}

pub(crate) fn state_dir_of(m: &Manifest, paths: &Paths, id: &str) -> Result<PathBuf, SupError> {
    // A manifest that names its own folder wins (`state.dir.<os>`); the OS never deletes it.
    // BOS-31: a dev app never uses the folder its manifest names (that is the installed app's real data): it has its own under the
    // OS's own data folder.
    let own = m.state.as_ref().and_then(|s| s.dir.get(std::env::consts::OS)).filter(|d| !d.is_empty()).filter(|_| crate::dev::load(paths, id).is_none());
    match own {
        Some(d) => Ok(PathBuf::from(expand_env(d))),
        None => Ok(paths.data_dir(id)?),
    }
}

/// `%NAME%` (and `$NAME`-free) expansion for a manifest's own state folder.
fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        match rest[i + 1..].find('%') {
            Some(j) => {
                let name = &rest[i + 1..i + 1 + j];
                out.push_str(&std::env::var(name).unwrap_or_default());
                rest = &rest[i + 2 + j..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// One health call against a recorded process. `Err(message)` = unhealthy.
fn probe(rec: &Record, st: &Settings) -> Result<Health, String> {
    let (code, body) = process::http("GET", &rec.url, &rec.health, None, Settings::ms(st.health_timeout_ms)).map_err(|e| e.to_string())?;
    if code != 200 {
        return Err(format!("health answered {code}"));
    }
    let h: Health = serde_json::from_str(&body).map_err(|e| format!("health answer is not valid: {e}"))?;
    if h.ok {
        Ok(h)
    } else {
        Err(h.detail.unwrap_or_else(|| "app says it is not ok".into()))
    }
}
