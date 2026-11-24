//! Host base of BattalionOS (MPL-2.0): where things live, how they are written safely, who may
//! write them, what was in flight when the OS died, and the one state machine per app.
//! Knows no app by name.

pub mod journal;
pub mod lock;
pub mod paths;
pub mod state;
pub mod store;

pub use journal::{Journal, JournalEntry, Pending};
pub use lock::{Lock, LockError};
pub use paths::Paths;
pub use state::{AppState, Current};
