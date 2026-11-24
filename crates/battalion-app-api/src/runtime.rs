//! What the OS and a running app say to each other: env names, ready file, health.

use serde::{Deserialize, Serialize};

/// Environment the OS gives a process. Names are part of contract v1.
pub mod env {
    pub const CONTRACT: &str = "BATTALION_CONTRACT";
    pub const APP_ID: &str = "BATTALION_APP_ID";
    pub const APP_VERSION: &str = "BATTALION_APP_VERSION";
    pub const APP_DIR: &str = "BATTALION_APP_DIR";
    pub const STATE_DIR: &str = "BATTALION_STATE_DIR";
    pub const READY_FILE: &str = "BATTALION_READY_FILE";
    pub const TOKEN_FILE: &str = "BATTALION_TOKEN_FILE";
    pub const UI_ORIGIN: &str = "BATTALION_UI_ORIGIN";
    /// Tests and installs only: the OS home. Not given to apps.
    pub const HOME: &str = "BATTALION_HOME";
}

/// `{contract, pid, url, health, stop}` written by the process when it is ready.
/// `health` and `stop` are paths relative to `url`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadyFile {
    pub contract: u32,
    pub pid: u32,
    pub url: String,
    #[serde(default = "default_health")]
    pub health: String,
    #[serde(default = "default_stop")]
    pub stop: String,
}
fn default_health() -> String {
    "/health".into()
}
fn default_stop() -> String {
    "/stop".into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyError {
    NotJson(String),
    /// The process speaks another contract than the OS started it with.
    WrongContract { expected: u32, got: u32 },
    BadUrl(String),
    ZeroPid,
}
impl std::fmt::Display for ReadyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadyError::NotJson(e) => write!(f, "ready file is not valid: {e}"),
            ReadyError::WrongContract { expected, got } => write!(f, "ready file says contract {got}, expected {expected}"),
            ReadyError::BadUrl(u) => write!(f, "ready file url '{u}' is not a loopback http address"),
            ReadyError::ZeroPid => write!(f, "ready file pid is 0"),
        }
    }
}
impl std::error::Error for ReadyError {}

impl ReadyFile {
    /// The OS opens no port; an app only listens on loopback.
    pub fn parse(json: &str, expected_contract: u32) -> Result<ReadyFile, ReadyError> {
        let r: ReadyFile = serde_json::from_str(json).map_err(|e| ReadyError::NotJson(e.to_string()))?;
        if r.contract != expected_contract {
            return Err(ReadyError::WrongContract { expected: expected_contract, got: r.contract });
        }
        if r.pid == 0 {
            return Err(ReadyError::ZeroPid);
        }
        let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"];
        let ok = loopback.iter().any(|p| {
            r.url.strip_prefix(p).is_some_and(|rest| rest.is_empty() || rest.starts_with(':') || rest.starts_with('/'))
        });
        if !ok {
            return Err(ReadyError::BadUrl(r.url));
        }
        Ok(r)
    }
}

/// Answer of `GET health`. `busy` = units of work in flight (the OS may ask before an update).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Health {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
