// SPDX-License-Identifier: Apache-2.0
//! BattalionOS app contract v1 (Apache-2.0).
//!
//! Rules that keep it cheap to live with: unknown fields are ignored, optional parts stay optional,
//! `contract` is a number an app and the OS compare, nothing here names an app.

pub mod catalog;
pub mod manifest;
pub mod runtime;
pub mod schemas;
pub mod version;

pub use catalog::{Catalog, CatalogApp, Feed, FeedPlatform};
pub use manifest::{Manifest, ManifestError};
pub use runtime::{env, Health, ReadyError, ReadyFile};
pub use version::Version;

/// The contract version this crate speaks.
pub const CONTRACT: u32 = 1;
