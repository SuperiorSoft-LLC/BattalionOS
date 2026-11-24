// SPDX-License-Identifier: MPL-2.0
use battalion_host::state::{AppState, Current};
use battalion_host::{store, Journal, Lock, LockError, Paths};
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::time::Duration;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bos-host-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn atomic_write_replaces_and_leaves_no_temp() {
    let d = tmp("atomic");
    let f = d.join("a.json");
    store::write_atomic(&f, b"one").unwrap();
    store::write_atomic(&f, b"two").unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), b"two");
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "no temp file left behind");
}

#[test]
fn two_writers_never_produce_a_torn_file() {
    let d = tmp("writers");
    let f = d.join("shared.json");
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for w in 0..2u8 {
        let (f, b) = (f.clone(), barrier.clone());
        handles.push(std::thread::spawn(move || {
            let payload = vec![b'a' + w; 200_000];
            b.wait();
            for _ in 0..50 {
                // On Windows a rename can lose to a reader holding the file; that is a retry, not a tear.
                for _ in 0..200 {
                    if store::write_atomic(&f, &payload).is_ok() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }));
    }
    // read while they write: always one whole payload
    for _ in 0..200 {
        if let Ok(bytes) = std::fs::read(&f) {
            assert_eq!(bytes.len(), 200_000);
            assert!(bytes.iter().all(|c| *c == bytes[0]), "mixed bytes: a torn write");
        }
    }
    handles.into_iter().for_each(|h| h.join().unwrap());
    assert_eq!(std::fs::read(&f).unwrap().len(), 200_000);
    assert_eq!(store::sweep_temp(&d).unwrap(), 0, "no temp left after clean writers");
}

#[test]
fn corrupt_file_is_an_error_not_absent() {
    let d = tmp("corrupt");
    let f = d.join("current.json");
    assert!(store::read_json::<Current>(&f).unwrap().is_none());
    std::fs::write(&f, b"{ half").unwrap();
    assert!(store::read_json::<Current>(&f).is_err());
}

#[test]
fn second_lock_is_busy_until_the_first_drops() {
    let d = tmp("lock");
    let p = Paths::new(&d);
    let path = p.app_lock("some-app").unwrap();
    let first = Lock::try_acquire(&path).unwrap();
    assert!(matches!(Lock::try_acquire(&path), Err(LockError::Busy)));
    // another app is not blocked by it
    assert!(Lock::try_acquire(&p.app_lock("other-app").unwrap()).is_ok());
    drop(first);
    assert!(Lock::try_acquire(&path).is_ok());
}

#[test]
fn waiting_acquire_times_out_as_busy() {
    let d = tmp("lockwait");
    let path = d.join("x.lock");
    let _held = Lock::try_acquire(&path).unwrap();
    assert!(matches!(Lock::acquire_within(&path, Duration::from_millis(60)), Err(LockError::Busy)));
}

#[test]
fn journal_recovery_finds_the_action_that_died() {
    let d = tmp("journal");
    let path = d.join("journal.jsonl");
    {
        let mut j = Journal::open(&path).unwrap();
        let done = j.begin("app-a", "install", "1.0.0").unwrap();
        j.step(done, "download").unwrap();
        j.commit(done).unwrap();
        let undone = j.begin("app-b", "update", "1.0.0->1.1.0").unwrap();
        j.abort(undone, "bad start").unwrap();
        let died = j.begin("app-c", "update", "2.0.0->2.1.0").unwrap();
        j.step(died, "unpack").unwrap();
        j.step(died, "swap").unwrap();
        // the OS dies here: no commit, no abort
    }
    let mut j = Journal::open(&path).unwrap();
    let pending = j.pending().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!((pending[0].app.as_str(), pending[0].op.as_str()), ("app-c", "update"));
    assert_eq!(pending[0].steps, ["unpack", "swap"], "the last step is where it died");
    // recovery finishes it; next start sees nothing, and new ids do not collide with old ones
    j.abort(pending[0].tx, "undone after crash").unwrap();
    assert!(j.pending().unwrap().is_empty());
    let next = j.begin("app-d", "remove", "").unwrap();
    assert!(next > pending[0].tx);
}

#[test]
fn journal_ignores_a_torn_last_line() {
    let d = tmp("torn");
    let path = d.join("journal.jsonl");
    let mut j = Journal::open(&path).unwrap();
    let tx = j.begin("app-a", "install", "").unwrap();
    j.step(tx, "download").unwrap();
    // crash mid-write of the next line
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    std::io::Write::write_all(&mut f, br#"{"kind":"step","tx":1,"ste"#).unwrap();
    drop(f);
    let j = Journal::open(&path).unwrap();
    let p = j.pending().unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].steps, ["download"]);
}

#[test]
fn state_is_persisted_and_illegal_moves_leave_the_file_alone() {
    let d = tmp("state");
    let path = d.join("current.json");
    let mut cur = Current { version: "1.0.0".into(), previous: None, state: AppState::Absent, public_key: "RWQ".into() };
    for s in [AppState::Downloading, AppState::Verifying, AppState::Installed, AppState::Stopped] {
        cur.transition(&path, s).unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    assert!(cur.transition(&path, AppState::Running).is_err(), "stopped must go through starting");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    cur.transition(&path, AppState::Starting).unwrap();
    cur.transition(&path, AppState::Failed("never ready".into())).unwrap();
    let back = Current::load(&path).unwrap().unwrap();
    assert_eq!(back.state, AppState::Failed("never ready".into()));
    assert_eq!(back.public_key, "RWQ");
}

#[test]
fn paths_layout() {
    let p = Paths::new("/home-x");
    assert!(p.version_dir("app-a", "1.0.0").unwrap().ends_with("apps/app-a/1.0.0") || p.version_dir("app-a", "1.0.0").unwrap().ends_with("apps\\app-a\\1.0.0"));
    assert!(p.data_dir("app-a").unwrap().starts_with("/home-x"));
}
