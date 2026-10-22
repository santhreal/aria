//! Tests for plugin packages.
//!
//! Closes the class "a package is read differently from what its manifest and
//! files state": every manifest rule is a row with its exact refusal, tier
//! detection runs against real directories, and the declared WGSL registers and
//! unregisters through a real mount. It does not cover loading code; the tier
//! crates own that.

use std::path::Path;

use super::*;

const ID: &str = "com.example.tools";

fn text(id: &str, name: &str, version: &str, api: &str, rest: &str) -> String {
    format!("[plugin]\nid = {id:?}\nname = {name:?}\nversion = {version:?}\napi = {api}\n{rest}")
}

fn with(rest: &str) -> String {
    text(ID, "Tools", "1.0.0", "1", rest)
}

fn refusal(text: &str) -> String {
    PackageManifest::parse(text).expect_err("refused")
}

fn write(dir: &Path, relative: &str, contents: &str) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("create parent");
    std::fs::write(path, contents).expect("write");
}

fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("scratch dir")
}

#[path = "declared_tests.rs"]
mod declared;
#[path = "discovery_tests.rs"]
mod discovery;
#[path = "manifest_tests.rs"]
mod manifest;
#[path = "manifest_refusal_tests.rs"]
mod manifest_refusal;
