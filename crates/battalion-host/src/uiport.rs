// SPDX-License-Identifier: MPL-2.0
//! The one page port of each app. The OS serves an app's page from `http://127.0.0.1:<port>` (its own origin) and tells
//! the app that origin before the app starts (`BATTALION_UI_ORIGIN`). The port is chosen once, kept in a file, and stays the
//! same between runs and updates, so the page's saved settings (its origin's storage) stay and the app's allow list is stable.

use crate::paths::Paths;
use std::io;
use std::net::TcpListener;

/// The page port of app `id`: the kept one, or a free loopback port chosen now and kept. The page server and the app's start both ask,
/// possibly at the same moment: the choice is made once (one lock in this process, and a file that is created only if it is not there).
pub fn port_for(paths: &Paths, id: &str) -> io::Result<u16> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let file = paths.ui_port_file(id).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    let kept = |f: &std::path::Path| std::fs::read_to_string(f).ok().and_then(|t| t.trim().parse::<u16>().ok()).filter(|p| *p != 0);
    if let Some(p) = kept(&file) {
        return Ok(p);
    }
    let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    use std::io::Write;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&file) {
        Ok(mut f) => {
            f.write_all(port.to_string().as_bytes())?;
            f.sync_all()?;
            Ok(port)
        }
        // Another process chose first: its port is the one.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => kept(&file).ok_or(e),
        Err(e) => Err(e),
    }
}

/// `http://127.0.0.1:<port>`, no trailing slash.
pub fn origin_for(paths: &Paths, id: &str) -> io::Result<String> {
    Ok(format!("http://127.0.0.1:{}", port_for(paths, id)?))
}

/// Forget the kept port (the next `port_for` chooses a new one). For a port something else holds.
pub fn forget(paths: &Paths, id: &str) {
    if let Ok(f) = paths.ui_port_file(id) {
        let _ = std::fs::remove_file(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_stays_the_same_between_runs_and_differs_per_app() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path());
        let a = port_for(&paths, "app-a").unwrap();
        assert_eq!(a, port_for(&paths, "app-a").unwrap(), "same port on the second call");
        // a new Paths over the same folder = a restart of the OS
        assert_eq!(a, port_for(&Paths::new(d.path()), "app-a").unwrap(), "same port after a restart");
        let b = port_for(&paths, "app-b").unwrap();
        assert_ne!(a, b);
        assert_eq!(origin_for(&paths, "app-a").unwrap(), format!("http://127.0.0.1:{a}"));
        forget(&paths, "app-a");
        assert!(paths.ui_port_file("app-a").unwrap().exists() == false);
    }

    #[test]
    fn two_askers_at_once_get_the_same_port() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path());
        for round in 0..20 {
            let id = format!("app-{round}");
            let ports: Vec<u16> = (0..8)
                .map(|_| {
                    let (p, id) = (paths.clone(), id.clone());
                    std::thread::spawn(move || port_for(&p, &id).unwrap())
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect();
            assert!(ports.iter().all(|p| *p == ports[0]), "round {round}: {ports:?}");
        }
    }

    #[test]
    fn a_bad_id_is_refused() {
        let d = tempfile::tempdir().unwrap();
        assert!(port_for(&Paths::new(d.path()), "../x").is_err());
    }
}
