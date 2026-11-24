// SPDX-License-Identifier: MPL-2.0
//! Starting an app from outside (BOS-19): `BattalionOS --open <app-id>` and the Start menu shortcut that runs exactly that.
//! Pure logic here (command line, shortcut file names, the reconcile plan); the Windows shortcut writer is in `shortcuts`.

use battalion_app_api::manifest::valid_id;

/// The app id of `--open <id>` (or `--open=<id>`) in a command line (`args` without the program name). Everything else on the
/// line is ignored; an id that is not a valid app id is ignored too (the request is dropped, never half-run).
pub fn open_request(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let id = if a == "--open" { it.next().map(String::as_str) } else { a.strip_prefix("--open=") };
        if let Some(id) = id {
            return valid_id(id).then(|| id.to_string());
        }
    }
    None
}

/// A file name for a shortcut named `name` (one path segment: no separators, no characters Windows refuses).
pub fn shortcut_file(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = clean.trim_matches('.').trim();
    format!("{}.lnk", if clean.is_empty() { "App" } else { clean })
}

/// One shortcut the OS wants: the app, the name it shows, and an `.ico` for it (the OS's own icon when the app has none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Want {
    pub id: String,
    pub name: String,
    pub icon: Option<std::path::PathBuf>,
}

/// What to do to make the shortcuts of `have` (id -> file name written earlier) match `want`: the files to write
/// (id, file) and the files to delete. A renamed app gets its new file and loses the old one.
pub fn plan(want: &[Want], have: &std::collections::BTreeMap<String, String>) -> (Vec<(String, String)>, Vec<String>) {
    let mut write = Vec::new();
    let mut del: Vec<String> = Vec::new();
    let mut used = std::collections::BTreeSet::new();
    for w in want {
        let mut file = shortcut_file(&w.name);
        if !used.insert(file.to_lowercase()) {
            file = shortcut_file(&format!("{} ({})", w.name, w.id));
            used.insert(file.to_lowercase());
        }
        if let Some(old) = have.get(&w.id) {
            if old != &file {
                del.push(old.clone());
            }
        }
        write.push((w.id.clone(), file));
    }
    for (id, file) in have {
        if !want.iter().any(|w| &w.id == id) {
            del.push(file.clone());
        }
    }
    (write, del)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }
    #[test]
    fn open_takes_the_id_and_ignores_the_rest() {
        assert_eq!(open_request(&s(&["--open", "infantry-ai"])), Some("infantry-ai".into()));
        assert_eq!(open_request(&s(&["--x", "--open=infantry-ai", "--y"])), Some("infantry-ai".into()));
        assert_eq!(open_request(&s(&["--popout", "--open", "a-b", "junk"])), Some("a-b".into()));
        assert_eq!(open_request(&s(&["--open"])), None);
        assert_eq!(open_request(&s(&[])), None);
    }
    #[test]
    fn a_bad_id_is_dropped() {
        for bad in ["../x", "a/b", "", "A b", "x;y"] {
            assert_eq!(open_request(&s(&["--open", bad])), None, "{bad}");
        }
    }
    #[test]
    fn file_names_are_one_safe_segment() {
        assert_eq!(shortcut_file("Infantry AI"), "Infantry AI.lnk");
        assert_eq!(shortcut_file(r"a/b\c:d"), "a b c d.lnk");
        assert_eq!(shortcut_file("  ..  "), "App.lnk");
    }
    #[test]
    fn plan_writes_all_and_deletes_the_gone_and_renamed() {
        let w = |id: &str, n: &str| Want { id: id.into(), name: n.into(), icon: None };
        let have = [("a".to_string(), "Old.lnk".to_string()), ("gone".to_string(), "Gone.lnk".to_string())].into();
        let (write, del) = plan(&[w("a", "New"), w("b", "New")], &have);
        assert_eq!(write, vec![("a".into(), "New.lnk".into()), ("b".into(), "New (b).lnk".into())]);
        assert_eq!(del, vec!["Old.lnk".to_string(), "Gone.lnk".to_string()]);
    }
}
