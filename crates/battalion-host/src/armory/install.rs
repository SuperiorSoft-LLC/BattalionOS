// SPDX-License-Identifier: MPL-2.0
//! Install: journaled, and undone completely when any step fails.
//!
//! ```text
//! state:  absent -> downloading -> verifying -> installed -> (initialising ->) stopped
//! steps:  download, verify, unpack, place, (init)          then commit
//! ```
//! An app that fails to install is removed again (absent), never left half there. A crash is
//! undone by [`Armory::recover`](super::recover). The state folder is not touched by a failed
//! install: the OS never deletes a folder it is not sure it created.

use super::*;
use crate::journal::Journal;
use crate::state::{AppState, Current};
use crate::supervisor::{platform_key, state_dir_of};
use battalion_app_api::{env, Manifest, CONTRACT};
use std::sync::Mutex;

static JOURNAL_GATE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    pub id: String,
    pub version: String,
    pub state: String,
    pub channel: Option<String>,
    /// From the installed version's manifest.
    pub name: Option<String>,
    /// The manifest's icon file as a `data:` url; `None` when there is none or it cannot be read.
    pub icon: Option<String>,
    /// The version the last feed read (cached, no network) offers, when it is newer.
    pub available: Option<String>,
    /// The version a rollback would go back to, when one is kept.
    pub previous: Option<String>,
}

impl Armory {
    /// Open the journal and begin one action. The gate keeps two actions from taking one number.
    pub(crate) fn begin_tx(&self, app: &str, op: &str, detail: &str) -> Result<(Journal, u64)> {
        let _g = JOURNAL_GATE.lock().unwrap_or_else(|p| p.into_inner());
        let mut j = Journal::open(self.paths.journal())?;
        let tx = j.begin(app, op, detail)?;
        Ok((j, tx))
    }

    pub(crate) fn load_current(&self, id: &str) -> Result<Option<Current>> {
        Ok(Current::load(&self.paths.current_json(id)?)?)
    }

    pub(crate) fn load_meta(&self, id: &str) -> Option<Meta> {
        store::read_json(&meta_path(&self.paths, id).ok()?).ok().flatten()
    }

    /// Every app that has a `current.json`.
    pub fn installed(&self) -> Result<Vec<InstalledApp>> {
        let mut out = Vec::new();
        let rd = match std::fs::read_dir(self.paths.apps()) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            if let Ok(Some(cur)) = self.load_current(&id) {
                let channel = self.load_meta(&id).map(|m| m.channel);
                let (name, icon) = self.card_of(&id, &cur.version);
                let previous = cur.previous.clone().filter(|v| self.paths.version_dir(&id, v).is_ok_and(|d| d.join("app.json").is_file()));
                let available = self.cached_newer(&id, &cur.version, channel.as_deref());
                out.push(InstalledApp { id, version: cur.version, state: cur.state.name().into(), channel, name, icon, available, previous });
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// Install `id` from the catalog, on `channel` (`stable` when you have no other).
    pub fn install(&self, id: &str, channel: &str) -> Result<Installed> {
        self.paths.app_dir(id)?;
        let lock = self.lock_app(id)?;
        let r = self.install_locked(id, channel);
        drop(lock);
        if r.is_err() && matches!(self.load_current(id), Ok(None)) {
            // Nothing of the app is left: tidy the folder the lock created.
            if let Ok(d) = self.paths.app_dir(id) {
                let _ = std::fs::remove_file(d.join("armory.lock"));
                let _ = std::fs::remove_dir(d);
            }
        }
        r
    }

    fn install_locked(&self, id: &str, channel: &str) -> Result<Installed> {
        if self.load_current(id)?.is_some() {
            return Err(ArmoryError::AlreadyInstalled(id.into()));
        }
        let entry = self.find(id)?;
        let (feed, _stale) = self.feed(&entry.app, channel)?;
        let plat = self.plan(&entry.app, &feed)?;
        let (j, tx) = self.begin_tx(id, "install", &feed.version)?;
        match self.install_steps(&j, tx, &entry.app, &feed, &plat, channel) {
            Ok(v) => {
                self.point("commit");
                j.commit(tx)?;
                Ok(v)
            }
            Err(e) => {
                self.undo_install(id, &feed.version);
                let _ = j.abort(tx, &e.to_string());
                Err(e)
            }
        }
    }

    fn install_steps(&self, j: &Journal, tx: u64, app: &battalion_app_api::CatalogApp, feed: &battalion_app_api::Feed, plat: &battalion_app_api::FeedPlatform, channel: &str) -> Result<Installed> {
        let id = app.id.as_str();
        let cur_path = self.paths.current_json(id)?;
        let key = app.public_key.trim().to_string();
        let mut cur = Current { version: feed.version.clone(), previous: None, state: AppState::Absent, public_key: key.clone() };
        let (dir, manifest) = self.fetch_and_check(app, feed, plat, &key, "install", &mut |step| {
            self.step(j, tx, step)?;
            match step {
                "download" => cur.transition(&cur_path, AppState::Downloading)?,
                "verify" => cur.transition(&cur_path, AppState::Verifying)?,
                _ => {}
            }
            Ok(())
        })?;

        self.step(j, tx, "place")?;
        let vdir = self.paths.version_dir(id, &feed.version)?;
        remove_tree(&vdir)?;
        std::fs::rename(&dir, &vdir)?;
        let custom = manifest.state.as_ref().and_then(|s| s.dir.get(std::env::consts::OS)).is_some_and(|d| !d.is_empty());
        let meta = Meta { channel: channel.into(), state_schema: schema_of(&manifest), previous_state_schema: None, custom_state_dir: custom };
        store::write_json(&meta_path(&self.paths, id)?, &meta)?;
        cur.transition(&cur_path, AppState::Installed)?;

        if let Some(init) = &manifest.init {
            self.step(j, tx, "init")?;
            cur.transition(&cur_path, AppState::Initialising)?;
            self.run_init(id, &manifest, &vdir, &init.exec, &init.args)?;
        }
        cur.transition(&cur_path, AppState::Stopped)?;
        Ok(Installed { id: id.into(), version: feed.version.clone() })
    }

    /// Put an install back to "absent": the version folder, the sidecar, `current.json`.
    pub(crate) fn undo_install(&self, id: &str, version: &str) {
        if let Ok(v) = self.paths.version_dir(id, version) {
            let _ = remove_tree(&v);
        }
        if let Ok(p) = meta_path(&self.paths, id) {
            let _ = std::fs::remove_file(p);
        }
        if let Ok(p) = self.paths.current_json(id) {
            let _ = std::fs::remove_file(p);
        }
        let _ = remove_tree(&self.paths.staging().join(format!("{id}-{version}.dir")));
        let _ = std::fs::remove_file(self.paths.staging().join(format!("{id}-{version}.zip.part")));
    }

    /// Run the manifest's init part to its end, in the app's state folder. A failing or hanging
    /// init fails the install.
    fn run_init(&self, id: &str, m: &Manifest, vdir: &Path, exec: &str, args: &[String]) -> Result<()> {
        let state_dir = state_dir_of(m, &self.paths, id).map_err(|e| ArmoryError::Init(e.to_string()))?;
        std::fs::create_dir_all(&state_dir)?;
        let log_path = self.paths.logs().join(format!("{id}-init.log"));
        std::fs::create_dir_all(self.paths.logs())?;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
        let mut cmd = std::process::Command::new(vdir.join(exec));
        cmd.args(args)
            .current_dir(vdir)
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .env(env::CONTRACT, CONTRACT.to_string())
            .env(env::APP_ID, id)
            .env(env::APP_VERSION, &m.version)
            .env(env::APP_DIR, vdir)
            .env(env::STATE_DIR, &state_dir);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let _ = platform_key();
        let mut child = cmd.spawn().map_err(|e| ArmoryError::Init(format!("could not run {exec}: {e}")))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(self.settings.init_timeout_ms);
        loop {
            match child.try_wait()? {
                Some(s) if s.success() => return Ok(()),
                Some(s) => return Err(ArmoryError::Init(format!("{exec} exited with {s}; see {}", log_path.display()))),
                None if std::time::Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ArmoryError::Init(format!("{exec} did not end within {} ms", self.settings.init_timeout_ms)));
                }
                None => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
    }
}

pub(crate) fn schema_of(m: &Manifest) -> u32 {
    m.state.as_ref().map_or(1, |s| s.schema)
}
