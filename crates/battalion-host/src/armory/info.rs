// SPDX-License-Identifier: MPL-2.0
//! What a list shows of an app: its name, its icon and the version on offer. Reads only local
//! files (the installed manifest, the cached feed); `offered` is the one call that may touch the
//! network, and only for a catalog entry the caller is already listing.

use super::*;
use battalion_app_api::manifest::safe_rel_path;
use battalion_app_api::{Feed, Version};

/// An icon larger than this is not inlined (a list of data urls must stay small).
pub const MAX_ICON_BYTES: u64 = 256 * 1024;

fn mime_of(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "svg" => Some("image/svg+xml"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "ico" => Some("image/x-icon"),
        _ => None,
    }
}

pub fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// A catalog icon the shell may show as it is: an inline image or an https url. Anything else
/// (a relative path, `file:`, `javascript:`) is dropped.
pub fn catalog_icon(icon: Option<&str>) -> Option<String> {
    let i = icon?.trim();
    (i.starts_with("data:image/") || i.starts_with("https://")).then(|| i.to_string())
}

impl Armory {
    /// (name, icon data url) of the installed version, from its `app.json`.
    pub(crate) fn card_of(&self, id: &str, version: &str) -> (Option<String>, Option<String>) {
        #[derive(Deserialize)]
        struct Card {
            name: Option<String>,
            icon: Option<String>,
        }
        let Ok(vdir) = self.paths.version_dir(id, version) else { return (None, None) };
        let Some(card) = std::fs::read_to_string(vdir.join("app.json")).ok().and_then(|t| serde_json::from_str::<Card>(&t).ok()) else { return (None, None) };
        let icon = card.icon.as_deref().filter(|p| safe_rel_path(p)).and_then(|rel| {
            let mime = mime_of(rel)?;
            let file = vdir.join(rel);
            // The file must really live under the version folder (no link out of it).
            let real = file.canonicalize().ok()?;
            if !real.starts_with(vdir.canonicalize().ok()?) || std::fs::metadata(&real).ok()?.len() > MAX_ICON_BYTES {
                return None;
            }
            Some(format!("data:{mime};base64,{}", base64(&std::fs::read(real).ok()?)))
        });
        (card.name.filter(|n| !n.trim().is_empty()), icon)
    }

    /// BOS-27: whether the installed manifest says the app's page draws its own title bar (`ui.ownTitleBar`).
    pub(crate) fn own_title_bar_of(&self, id: &str, version: &str) -> bool {
        #[derive(Deserialize)]
        struct Ui {
            #[serde(default, rename = "ownTitleBar")]
            own: bool,
        }
        #[derive(Deserialize)]
        struct M {
            ui: Option<Ui>,
        }
        let Ok(vdir) = self.paths.version_dir(id, version) else { return false };
        std::fs::read_to_string(vdir.join("app.json")).ok().and_then(|t| serde_json::from_str::<M>(&t).ok()).and_then(|m| m.ui).is_some_and(|u| u.own)
    }

    /// The version the cached feed of the app's channel offers, when newer than `installed`.
    pub(crate) fn cached_newer(&self, id: &str, installed: &str, channel: Option<&str>) -> Option<String> {
        let feed = Feed::parse(&std::fs::read_to_string(self.feed_cache(id, channel.unwrap_or("stable"))).ok()?).ok()?;
        (Version::parse(&feed.version)? > Version::parse(installed)?).then_some(feed.version)
    }

    /// The version a catalog entry offers on `channel`: read from its feed (the cache when
    /// offline). `None` when no feed can be read; that is not an error for a list.
    pub fn offered(&self, e: &CatalogEntry, channel: &str) -> Option<String> {
        self.feed(&e.app, channel).ok().map(|(f, _)| f.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
    #[test]
    fn only_inline_or_https_catalog_icons_pass() {
        assert!(catalog_icon(Some("data:image/png;base64,AA==")).is_some());
        assert!(catalog_icon(Some("https://x.example/i.png")).is_some());
        assert!(catalog_icon(Some("file:///c:/x.png")).is_none());
        assert!(catalog_icon(Some("javascript:alert(1)")).is_none());
        assert!(catalog_icon(Some("icons/a.png")).is_none());
        assert!(catalog_icon(None).is_none());
    }
}
