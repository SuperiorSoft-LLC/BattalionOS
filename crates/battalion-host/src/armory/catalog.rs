// SPDX-License-Identifier: MPL-2.0
//! Catalog sources, the lenient read of each, the offline cache, and the per-app feed.
//!
//! A source that cannot be read is served from its last good copy (`<home>/catalog/`), and the
//! listing says so. A source that answers with a broken file never replaces a good copy.

use super::*;
use battalion_app_api::{Catalog, CatalogApp, Feed, CONTRACT};

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogEntry {
    pub app: CatalogApp,
    pub source: String,
    /// The app's contract range includes the contract this OS speaks.
    pub compatible: bool,
    /// LOCAL-REPO-1: from a local repo on this machine (signed with this device's local key).
    pub from_local_repo: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listing {
    pub apps: Vec<CatalogEntry>,
    /// (id or `?`, reason) for entries that did not pass; the others are still listed.
    pub rejected: Vec<(String, String)>,
    /// (source, reason) for sources that could not be read and had no cached copy.
    pub source_errors: Vec<(String, String)>,
    /// Sources served from the cache (stale: the network or the file did not answer).
    pub from_cache: Vec<String>,
}

fn cache_name(key: &str) -> String {
    sha256_hex(key.as_bytes())[..16].to_string()
}

impl Armory {
    fn catalog_cache(&self, source: &str) -> PathBuf {
        self.paths.catalog_dir().join(format!("{}.json", cache_name(source)))
    }
    pub(crate) fn feed_cache(&self, id: &str, channel: &str) -> PathBuf {
        self.paths.catalog_dir().join("feeds").join(format!("{id}-{}.json", cache_name(channel)))
    }

    /// Read every source. `Err(Offline)` only when sources are set and none gave anything, not
    /// even from the cache.
    pub fn catalog(&self) -> Result<Listing> {
        let mut out = Listing::default();
        let mut seen = std::collections::BTreeSet::new();
        let mut any = false;
        for src in &self.settings.sources {
            let cache = self.catalog_cache(src);
            let fresh = self
                .fetch_small(src, self.settings.max_catalog_bytes, "catalog")
                .and_then(|b| String::from_utf8(b).map_err(|e| ArmoryError::Catalog(e.to_string())))
                .and_then(|t| Catalog::parse_lenient(&t).map(|p| (t, p)).map_err(|e| ArmoryError::Catalog(e.to_string())));
            let (apps, bad) = match fresh {
                Ok((text, parsed)) => {
                    let _ = store::write_atomic(&cache, text.as_bytes());
                    parsed
                }
                Err(why) => match std::fs::read_to_string(&cache).ok().and_then(|t| Catalog::parse_lenient(&t).ok()) {
                    Some(parsed) => {
                        out.from_cache.push(src.clone());
                        parsed
                    }
                    None => {
                        out.source_errors.push((src.clone(), why.to_string()));
                        continue;
                    }
                },
            };
            any = true;
            out.rejected.extend(bad.into_iter().map(|(id, e)| (id, e.to_string())));
            for app in apps {
                if !seen.insert(app.id.clone()) {
                    out.rejected.push((app.id.clone(), format!("already listed by an earlier source (not {src})")));
                    continue;
                }
                let compatible = app.contract.min <= CONTRACT && CONTRACT <= app.contract.max;
                out.apps.push(CatalogEntry { app, source: src.clone(), compatible, from_local_repo: false });
            }
        }
        // LOCAL-REPO-1: local repos after the remote sources (a remote id wins), read from disk, never cached.
        let before = out.apps.len();
        self.local_catalog(&mut out, &mut seen);
        if !self.settings.sources.is_empty() && !any && out.apps.len() == before {
            let why = out.source_errors.first().map(|(_, e)| e.clone()).unwrap_or_default();
            return Err(ArmoryError::Offline(why));
        }
        Ok(out)
    }

    /// ARMORY-CHANNEL-SWITCH: the channel names each app's catalog entry lists, from the cached catalogs only (no
    /// network; first source wins, as in [`Armory::catalog`]). An app no cached catalog names is absent.
    pub fn cached_channels(&self) -> std::collections::BTreeMap<String, Vec<String>> {
        let mut out = std::collections::BTreeMap::new();
        for src in &self.settings.sources {
            let Some((apps, _)) = std::fs::read_to_string(self.catalog_cache(src)).ok().and_then(|t| Catalog::parse_lenient(&t).ok()) else { continue };
            for app in apps {
                out.entry(app.id.clone()).or_insert_with(|| app.channels.keys().cloned().collect());
            }
        }
        out
    }

    pub fn find(&self, id: &str) -> Result<CatalogEntry> {
        self.paths.app_dir(id)?;
        let listing = self.catalog()?;
        listing.apps.into_iter().find(|e| e.app.id == id).ok_or_else(|| ArmoryError::NotInCatalog(id.into()))
    }

    /// The feed of one app and channel; `true` = served from the cache.
    pub fn feed(&self, app: &CatalogApp, channel: &str) -> Result<(Feed, bool)> {
        let url = app.channels.get(channel).ok_or_else(|| ArmoryError::NoChannel { id: app.id.clone(), channel: channel.into() })?;
        let cache = self.feed_cache(&app.id, channel);
        let fresh = self
            .fetch_small(url, self.settings.max_feed_bytes, "feed")
            .and_then(|b| String::from_utf8(b).map_err(|e| ArmoryError::Feed(e.to_string())))
            .and_then(|t| Feed::parse(&t).map(|f| (t, f)).map_err(|e| ArmoryError::Feed(e.to_string())));
        match fresh {
            Ok((text, feed)) => {
                let _ = store::write_atomic(&cache, text.as_bytes());
                Ok((feed, false))
            }
            Err(why) => match std::fs::read_to_string(&cache).ok().and_then(|t| Feed::parse(&t).ok()) {
                Some(feed) => Ok((feed, true)),
                None => Err(why),
            },
        }
    }
}
