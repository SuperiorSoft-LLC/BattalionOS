// SPDX-License-Identifier: MPL-2.0
//! APP-API-1: the per-app key/value store (`os_store_get`, `os_store_set`, `os_store_delete`, `os_store_list`).
//! One JSON file per app, `<home>/data/<app>/store.json` (the app's own state folder): `{"schema": 1, "values": {key: value}}`.
//! The app is always the caller (the frame that asked), never an argument, so an app can never name another app's store.
//! Needs no manifest permission. Bounded by `store.maxBytes` (whole file) and `store.maxKeyChars` in os.json.
//! Written atomically (`store::write_json`); calls are serialized in this process, so two calls never lose a write.

use crate::paths::Paths;
use crate::store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

pub const GET: &str = "os_store_get";
pub const SET: &str = "os_store_set";
pub const DELETE: &str = "os_store_delete";
pub const LIST: &str = "os_store_list";
/// The store calls a page may make (frozen in docs/commands.md, "Added by APP-API-1").
pub const COMMANDS: &[&str] = &[GET, SET, DELETE, LIST];

/// The store's limits (`store` in os.json).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// The most bytes one app's store file may hold (keys, values and JSON punctuation).
    pub max_bytes: u64,
    /// The longest key, in characters.
    pub max_key_chars: usize,
    /// The most keys one `os_store_list` answers.
    pub list_max: usize,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { max_bytes: 1024 * 1024, max_key_chars: 256, list_max: 10_000 }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Settings {
        #[derive(Deserialize, Default)]
        struct Os {
            #[serde(default)]
            store: Settings,
        }
        match store::read_json::<Os>(&paths.os_json()) {
            Ok(Some(o)) => o.store,
            _ => Settings::default(),
        }
    }
}

/// A refused store call. `code()` is the stable name a client maps to a typed error.
#[derive(Debug)]
pub enum StoreError {
    BadArgs(String),
    TooLarge(String),
    BadApp(String),
    Io(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::BadArgs(m) | StoreError::TooLarge(m) | StoreError::BadApp(m) => f.write_str(m),
            StoreError::Io(m) => write!(f, "the store could not be read or written: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl StoreError {
    pub fn code(&self) -> &'static str {
        match self {
            StoreError::BadArgs(_) => "bad_args",
            StoreError::TooLarge(_) => "too_large",
            StoreError::BadApp(_) => "bad_app",
            StoreError::Io(_) => "io",
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    schema: u32,
    #[serde(default)]
    values: Map<String, Value>,
}

static LOCK: Mutex<()> = Mutex::new(());

/// The store of ONE app. Built per call from the caller's id.
pub struct AppStore {
    file: PathBuf,
    pub settings: Settings,
}

impl AppStore {
    pub fn open(paths: &Paths, app: &str) -> Result<AppStore, StoreError> {
        let dir = paths.data_dir(app).map_err(|e| StoreError::BadApp(e.to_string()))?;
        Ok(AppStore { file: dir.join("store.json"), settings: Settings::load(paths) })
    }

    /// The store file of this app (`<home>/data/<app>/store.json`).
    pub fn file(&self) -> &std::path::Path {
        &self.file
    }

    fn read(&self) -> Result<File, StoreError> {
        match store::read_json::<File>(&self.file) {
            Ok(Some(f)) => Ok(f),
            Ok(None) => Ok(File { schema: 1, values: Map::new() }),
            Err(e) => Err(StoreError::Io(e.to_string())),
        }
    }

    fn key<'a>(&self, args: &'a Value) -> Result<&'a str, StoreError> {
        let k = args.get("key").and_then(Value::as_str).ok_or_else(|| StoreError::BadArgs("a store call needs a key (a string)".into()))?;
        if k.is_empty() || k.chars().count() > self.settings.max_key_chars || k.chars().any(char::is_control) {
            return Err(StoreError::BadArgs(format!("a key is 1 to {} characters, no control characters (store.maxKeyChars)", self.settings.max_key_chars)));
        }
        Ok(k)
    }

    pub fn get(&self, key: &str) -> Result<Value, StoreError> {
        Ok(self.read()?.values.get(key).cloned().unwrap_or(Value::Null))
    }

    /// Set `key` to `value` (null deletes it). Refused, with nothing written, when the file would pass `store.maxBytes`.
    pub fn set(&self, key: &str, value: Value) -> Result<(), StoreError> {
        let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut f = self.read()?;
        f.schema = 1;
        if value.is_null() {
            f.values.remove(key);
        } else {
            f.values.insert(key.to_string(), value);
        }
        let bytes = serde_json::to_vec_pretty(&f).map_err(|e| StoreError::Io(e.to_string()))?;
        if bytes.len() as u64 + 1 > self.settings.max_bytes {
            return Err(StoreError::TooLarge(format!("the store would be more than {} bytes (store.maxBytes)", self.settings.max_bytes)));
        }
        store::write_json(&self.file, &f).map_err(|e| StoreError::Io(e.to_string()))
    }

    /// Delete `key`; true when it was there.
    pub fn delete(&self, key: &str) -> Result<bool, StoreError> {
        let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut f = self.read()?;
        let had = f.values.remove(key).is_some();
        if had {
            store::write_json(&self.file, &f).map_err(|e| StoreError::Io(e.to_string()))?;
        }
        Ok(had)
    }

    /// The keys, sorted, that start with `prefix` (all without one), at most `store.listMax`.
    pub fn list(&self, prefix: Option<&str>) -> Result<Vec<String>, StoreError> {
        let f = self.read()?;
        let mut keys: Vec<String> = f.values.keys().filter(|k| prefix.is_none_or(|p| k.starts_with(p))).cloned().collect();
        keys.sort();
        keys.truncate(self.settings.list_max);
        Ok(keys)
    }

    /// One page call by name, with the page's arguments. The result is the call's JSON answer.
    pub fn call(&self, cmd: &str, args: &Value) -> Result<Value, StoreError> {
        match cmd {
            GET => self.get(self.key(args)?),
            SET => {
                let key = self.key(args)?;
                let value = args.get("value").cloned().unwrap_or(Value::Null);
                self.set(key, value)?;
                Ok(Value::Null)
            }
            DELETE => Ok(json!(self.delete(self.key(args)?)?)),
            LIST => {
                let prefix = match args.get("prefix") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(s.as_str()),
                    Some(_) => return Err(StoreError::BadArgs("prefix is a string".into())),
                };
                Ok(json!(self.list(prefix)?))
            }
            other => Err(StoreError::BadArgs(format!("'{other}' is not a store call"))),
        }
    }
}
