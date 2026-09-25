//! Contracts for the cloud feeds: routing, card mapping, and load state.
//!
//! No test here opens a socket. What is asserted is everything that decides what a
//! request would be and what the operator sees around it, because those are the parts
//! that were wrong when the surface did not exist:
//!
//! - The BYOK rule. A configured stock key must route the query at the provider, not
//!   at the Aria backend. Getting this backwards leaks every search to our servers.
//! - Failure is not emptiness. An outage and an empty catalog must not render the
//!   same, or an outage looks like a server with nothing on it.
//! - A card must be importable. An entry with no file, or of a kind this build does
//!   not render, is dropped rather than shown as a row that does nothing.
//!
//! Feeds are enumerated from `CloudFeed::ALL` at run time, so a new feed fails here
//! until it is given a label and a kind.

use super::premiere_tests::with_clip;
use crate::cloud::{
    CloudFeed, FeedState, STOCK_PROVIDERS, StockRoute, cards_from_catalog, cards_from_stock,
    extension_of, stock_route,
};
use aria_cloud::dto::{
    AssetKind, CatalogEntry, CatalogResponse, StockFile, StockItem, StockKind, StockProviderId,
    StockSearchResponse,
};
use aria_settings::{ProviderSettings, Settings};
fn entry(id: &str, kind: AssetKind, file_url: &str) -> CatalogEntry {
    CatalogEntry {
        id: id.into(),
        kind,
        name: format!("{id} name"),
        category: String::new(),
        tags: Vec::new(),
        file_url: file_url.into(),
        preview_url: None,
        size_bytes: 2 * 1024 * 1024,
        checksum_sha256: String::new(),
        min_schema_version: None,
        author: "Aria".into(),
        license: "CC0".into(),
        duration_seconds: Some(4.0),
        slot_count: Some(3),
    }
}
fn stock_item(id: &str, files: Vec<StockFile>) -> StockItem {
    StockItem {
        id: id.into(),
        provider: StockProviderId::Pexels,
        kind: StockKind::Video,
        width: 1920,
        height: 1080,
        duration_seconds: Some(12.0),
        thumbnail_url: "https://cdn.example/thumb.jpg".into(),
        files,
        author: "A Creator".into(),
        attribution: "Video by A Creator on Pexels".into(),
        license: "Pexels License".into(),
        source_url: "https://example/item".into(),
    }
}
fn file(url: &str) -> StockFile {
    StockFile {
        url: url.into(),
        quality: "1080p".into(),
        width: 1920,
        height: 1080,
        content_type: "video/mp4".into(),
        size_bytes: 1024,
    }
}
/// A file inside the session's download cache, so `protect_path` accepts it.
fn cached(state: &crate::app::AppState, name: &str, body: &str) -> std::path::PathBuf {
    let path = state.download_cache_root().join(name);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the cache dir");
    std::fs::write(&path, body).expect("the cached file");
    path
}
fn card(id: &str, extension: &str) -> crate::cloud::CloudCard {
    crate::cloud::CloudCard {
        id: id.into(),
        name: format!("{id} name").into(),
        detail: String::new().into(),
        file_url: format!("https://cdn.example/{id}.{extension}"),
        extension: extension.into(),
        attribution: String::new(),
    }
}

// The cases live in sibling files, declared here so the fixtures above stay in
// one place. Only `#[test]` fns moved.
#[path = "cloud_tests_feed.rs"]
mod cloud_tests_feed;
#[path = "cloud_tests_provider.rs"]
mod cloud_tests_provider;
