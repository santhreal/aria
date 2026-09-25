//! Every pass in this module binds through the cache.
//!
//! # Why
//!
//! A frame's accounting ([`PassStats`](super::PassStats)) can only count passes
//! that ask the cache for a bind group. A pass family that builds its own
//! descriptor is therefore invisible to it: it allocates a bind group and a
//! uniform buffer per call, per frame, and the counters still read zero built.
//! That is exactly the state this module was in — the effect chain was cached
//! and the grade, LUT, blit, transition, matte, blend and style passes were not
//! — and no count could have shown it.
//!
//! So the guard is on the source: within this module, `create_bind_group` and
//! `create_buffer` belong to `pass_cache.rs` and nowhere else. A new pass
//! family that writes its own fails here, whatever its own tests report.
//!
//! Layer bind groups outside this module (`compositor/layers.rs`,
//! `compositor/glyphs.rs`) are deliberately out of scope: they bind the frame a
//! decoder just produced, so their descriptors are new every frame by nature and
//! caching one would hold a decoded frame resident to no purpose.

use std::fs;
use std::path::{Path, PathBuf};

/// The one file in this module that may build a bind group or a uniform buffer.
const OWNER: &str = "pass_cache.rs";

/// Calls that allocate a descriptor per invocation.
///
/// `create_bind_group_layout` is a different call and stays with the registry;
/// the trailing `(` is what keeps the two apart.
const ALLOCATIONS: [&str; 2] = ["create_bind_group(", "create_buffer("];

/// Every `.rs` file of the pass module, including the parent file itself.
fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let parent = root.join("effect_render.rs");
    out.push((
        "effect_render.rs".to_string(),
        fs::read_to_string(&parent).expect("the pass module's own file"),
    ));
    let dir = root.join("effect_render");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("the pass module's directory")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .expect("a file with an extension has a name")
            .to_string_lossy()
            .into_owned();
        out.push((name, fs::read_to_string(&path).expect("a source file")));
    }
    out
}

#[test]
fn every_pass_builds_its_bind_group_through_the_cache() {
    let mut escapes = Vec::new();
    for (name, source) in sources() {
        // This file names the calls it forbids, so it cannot be read by its own
        // rule; it builds nothing, being a test of the source text.
        if name == OWNER || name == "pass_census.rs" {
            continue;
        }
        for call in ALLOCATIONS {
            if source.contains(call) {
                escapes.push(format!("{name} calls {call}"));
            }
        }
    }
    assert!(
        escapes.is_empty(),
        "a pass allocates its own descriptor instead of asking {OWNER}: {escapes:?}"
    );
}

/// The guard above passes if the call is spelled some other way, so the owner is
/// checked for the same strings: they are the ones that mean an allocation.
#[test]
fn the_cache_is_the_one_place_that_allocates() {
    let owner = sources()
        .into_iter()
        .find(|(name, _)| name == OWNER)
        .map(|(_, source)| source)
        .unwrap_or_else(|| panic!("{OWNER} is missing"));
    for call in ALLOCATIONS {
        assert!(
            owner.contains(call),
            "{OWNER} no longer calls {call}, so the guard on the other files means nothing"
        );
    }
}
