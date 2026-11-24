//! Guard: the OS names no app. No source file of the OS may contain the name of the first app.
//! Packaging data (lock files, installers, docs) is out of scope; this reads code and manifests.

use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                walk(&p, out);
            }
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("rs" | "toml" | "json" | "ts" | "tsx" | "js")) {
            out.push(p);
        }
    }
}

#[test]
fn no_source_file_of_the_os_names_an_app() {
    // Built from pieces so this file does not trip itself.
    let banned = ["infan", "try"].concat();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for d in ["crates", "fixtures", ".cargo"] {
        walk(&root.join(d), &mut files);
    }
    files.push(root.join("Cargo.toml"));
    assert!(files.len() > 10, "guard found too few files: {}", files.len());
    let mut hits = Vec::new();
    for f in files {
        if f.file_name().is_some_and(|n| n == "guard.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap_or_default().to_lowercase();
        if text.contains(&banned) {
            hits.push(f.display().to_string());
        }
    }
    assert!(hits.is_empty(), "the OS must not name an app; found in: {hits:?}");
}
