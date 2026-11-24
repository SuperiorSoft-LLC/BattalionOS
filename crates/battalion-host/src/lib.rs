// SPDX-License-Identifier: MPL-2.0
//! Host base of BattalionOS (MPL-2.0): where things live, how they are written safely, who may
//! write them, what was in flight when the OS died, and the one state machine per app.
//! Knows no app by name.

pub mod armory;
pub mod firstrun;
pub mod job;
pub mod journal;
pub mod lock;
pub mod paths;
pub mod process;
pub mod state;
pub mod store;
pub mod supervisor;
pub mod worker;

pub use journal::{Journal, JournalEntry, Pending};
pub use lock::{Lock, LockError};
pub use paths::Paths;
pub use state::{AppState, Current};
pub use supervisor::{AppStatus, Settings as SupervisorSettings, SupError, Supervisor};
