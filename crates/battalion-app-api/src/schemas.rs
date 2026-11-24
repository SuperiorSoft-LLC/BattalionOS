//! JSON schemas (draft 2020-12) of the contract files, for editors and CI of catalog repos.

pub const APP_JSON: &str = include_str!("../schemas/app.schema.json");
pub const READY_JSON: &str = include_str!("../schemas/ready.schema.json");
pub const HEALTH_JSON: &str = include_str!("../schemas/health.schema.json");
pub const CATALOG_JSON: &str = include_str!("../schemas/catalog.schema.json");
pub const FEED_JSON: &str = include_str!("../schemas/feed.schema.json");

pub const ALL: &[(&str, &str)] = &[
    ("app", APP_JSON),
    ("ready", READY_JSON),
    ("health", HEALTH_JSON),
    ("catalog", CATALOG_JSON),
    ("feed", FEED_JSON),
];
