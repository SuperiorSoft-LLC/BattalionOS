//! One loopback file server per app window (the origin fallback, see docs/commands.md "Origin").
//! Each app gets its own port, so its web origin (`http://127.0.0.1:<port>`) is its own; the window's
//! data_directory separates the rest. It serves only files under the app's version folder, GET/HEAD only.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    BadEncoding,
    Traversal,
    Missing,
    Outside,
}

fn pct_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Map a request path to a file under `root`, or refuse. Refuses `..`, backslashes, drive letters,
/// NUL, double-encoding and anything whose real path leaves `root` (symlinks, junctions).
pub fn resolve(root: &Path, request_path: &str) -> Result<PathBuf, Refused> {
    let path = request_path.split(['?', '#']).next().unwrap_or("");
    let dec = pct_decode(path).ok_or(Refused::BadEncoding)?;
    // Decoding again must change nothing: no %252e%252e tricks.
    if dec.contains('%') {
        return Err(Refused::BadEncoding);
    }
    if dec.contains('\\') || dec.contains('\0') || dec.contains(':') {
        return Err(Refused::Traversal);
    }
    let rel = dec.trim_start_matches('/');
    let mut p = root.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(n) => p.push(n),
            Component::CurDir => {}
            _ => return Err(Refused::Traversal),
        }
    }
    let real = p.canonicalize().map_err(|_| Refused::Missing)?;
    let real_root = root.canonicalize().map_err(|_| Refused::Missing)?;
    if !real.starts_with(&real_root) {
        return Err(Refused::Outside);
    }
    if real.is_dir() {
        return Err(Refused::Missing);
    }
    Ok(real)
}

fn mime(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// A running per-app server. Dropping it stops it.
pub struct AppServer {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl AppServer {
    /// Serve `root` on a free loopback port. `connect` becomes the CSP `connect-src` list.
    pub fn start(root: PathBuf, connect: Vec<String>) -> std::io::Result<AppServer> {
        let l = TcpListener::bind(("127.0.0.1", 0))?;
        let port = l.local_addr()?.port();
        l.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let mut csp = String::from("default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'");
        for u in &connect {
            if !u.contains([';', '\r', '\n', ',']) {
                csp.push(' ');
                csp.push_str(u);
            }
        }
        std::thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                match l.accept() {
                    Ok((s, _)) => {
                        let (root, csp) = (root.clone(), csp.clone());
                        std::thread::spawn(move || {
                            let _ = handle(s, &root, port, &csp);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(20)),
                }
            }
        });
        Ok(AppServer { port, stop })
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn reply(s: &mut TcpStream, code: u16, text: &str, ctype: &str, csp: &str, body: &[u8], head: bool) -> std::io::Result<()> {
    let h = format!(
        "HTTP/1.1 {code} {text}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nContent-Security-Policy: {csp}\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(h.as_bytes())?;
    if !head {
        s.write_all(body)?;
    }
    Ok(())
}

fn handle(mut s: TcpStream, root: &Path, port: u16, csp: &str) -> std::io::Result<()> {
    s.set_nonblocking(false)?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; 8192];
    let mut n = 0;
    while n < buf.len() && !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
        let k = s.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    let req = String::from_utf8_lossy(&buf[..n]).to_string();
    let mut lines = req.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut it = first.split(' ');
    let (method, target) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
    let want = format!("127.0.0.1:{port}");
    let host_ok = lines.filter_map(|l| l.split_once(':')).any(|(k, v)| k.eq_ignore_ascii_case("host") && v.trim() == want);
    let head = method == "HEAD";
    let r = if !host_ok {
        reply(&mut s, 421, "Misdirected Request", "text/plain", csp, b"bad host", head)
    } else if method != "GET" && !head {
        reply(&mut s, 405, "Method Not Allowed", "text/plain", csp, b"GET only", false)
    } else {
        match resolve(root, target) {
            Ok(p) => match std::fs::read(&p) {
                Ok(b) => reply(&mut s, 200, "OK", mime(&p), csp, &b, head),
                Err(_) => reply(&mut s, 404, "Not Found", "text/plain", csp, b"not found", head),
            },
            Err(Refused::Missing) => reply(&mut s, 404, "Not Found", "text/plain", csp, b"not found", head),
            Err(_) => reply(&mut s, 403, "Forbidden", "text/plain", csp, b"refused", head),
        }
    };
    let _ = s.shutdown(Shutdown::Both);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("app/ui")).unwrap();
        std::fs::write(d.path().join("app/ui/index.html"), "<p>hi</p>").unwrap();
        std::fs::write(d.path().join("secret.txt"), "no").unwrap();
        d
    }

    #[test]
    fn serves_inside_root() {
        let d = tree();
        assert!(resolve(&d.path().join("app"), "/ui/index.html?x=1").is_ok());
    }

    #[test]
    fn traversal_is_refused() {
        let d = tree();
        let root = d.path().join("app");
        for bad in [
            "/../secret.txt",
            "/ui/../../secret.txt",
            "/%2e%2e/secret.txt",
            "/%2E%2E%2Fsecret.txt",
            "/ui/..%5c..%5csecret.txt",
            "/%252e%252e/secret.txt",
            "/C:/Windows/win.ini",
            "/ui\\..\\..\\secret.txt",
            "/ui/%00.html",
            "//secret.txt/..",
        ] {
            assert!(resolve(&root, bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn http_roundtrip_and_refusals() {
        let d = tree();
        let srv = AppServer::start(d.path().join("app"), vec![]).unwrap();
        let get = |path: &str, host: &str| {
            let mut c = TcpStream::connect(("127.0.0.1", srv.port())).unwrap();
            write!(c, "GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
            let mut out = String::new();
            c.read_to_string(&mut out).unwrap();
            out
        };
        let h = format!("127.0.0.1:{}", srv.port());
        assert!(get("/ui/index.html", &h).starts_with("HTTP/1.1 200"));
        assert!(get("/../secret.txt", &h).starts_with("HTTP/1.1 403"));
        assert!(get("/ui/index.html", "evil.example").starts_with("HTTP/1.1 421"));
        assert!(get("/nope", &h).starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn two_apps_two_origins() {
        let d = tree();
        let a = AppServer::start(d.path().join("app"), vec![]).unwrap();
        let b = AppServer::start(d.path().join("app"), vec![]).unwrap();
        assert_ne!(a.origin(), b.origin());
    }
}
