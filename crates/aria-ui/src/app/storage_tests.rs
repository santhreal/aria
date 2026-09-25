//! Contracts for the download-cache surface.
//!
//! The class closed here is "a setting the operator can change but the running
//! session does not honour". A quota that is written to the config and not applied to
//! the live cache, or applied and not written, both look like the control worked and
//! fail at the next eviction or the next launch, so every setter is asserted in both
//! places. Bounds come from `aria_settings` at run time, so widening the accepted
//! range cannot leave this file asserting the old one.
//!
//! Every path here is under the session's own scratch dir: `AppState::config_path`
//! is `data_dir/config.toml`, which is the operator's real file in production and a
//! temporary one here.

use std::path::PathBuf;

use aria_settings::{
    DEFAULT_DOWNLOAD_QUOTA_MIB, MAX_DOWNLOAD_QUOTA_MIB, MIN_DOWNLOAD_QUOTA_MIB, StorageSettings,
};

use super::AppState;
use super::premiere_tests::with_clip;
use super::storage::{DOWNLOADS_DIR, format_bytes};

/// A cached file of `bytes` bytes, so size and clearing have something to observe.
fn seed_cache(state: &AppState, name: &str, bytes: usize) -> PathBuf {
    let path = state.download_cache_root().join(name);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("create the cache root");
    std::fs::write(&path, vec![0u8; bytes]).expect("seed a cached file");
    path
}

#[test]
fn a_fresh_session_reports_the_shipped_quota_and_an_empty_cache() {
    let (state, _) = with_clip();
    assert_eq!(state.download_quota_mib(), DEFAULT_DOWNLOAD_QUOTA_MIB);
    assert_eq!(state.download_cache_size_bytes(), 0);
    assert_eq!(state.protected_download_count(), 0);
    assert!(
        state.download_cache_root().ends_with(DOWNLOADS_DIR),
        "the cache root is not under the storage root"
    );
}

#[test]
fn the_cache_reports_what_is_in_it() {
    let (state, _) = with_clip();
    seed_cache(&state, "one.bin", 4_096);
    seed_cache(&state, "two.bin", 2_048);
    assert_eq!(state.download_cache_size_bytes(), 6_144);
}

#[test]
fn a_quota_edit_reaches_both_the_live_cache_and_the_config() {
    // Written and not applied fails at the next eviction; applied and not written
    // fails at the next launch. Both are asserted.
    let (mut state, _) = with_clip();
    state.set_download_quota_mib(64).expect("in range");

    assert_eq!(
        state.download_quota_mib(),
        64,
        "the live cache kept the old quota"
    );
    let written = aria_settings::load(&state.config_path()).expect("the config parses");
    assert_eq!(
        written.storage.download_quota_mib, 64,
        "the config kept the old quota"
    );
    assert!(
        state
            .status_message
            .as_deref()
            .is_some_and(|line| line.contains("64")),
        "the change was silent"
    );
}

#[test]
fn the_accepted_quota_bounds_are_the_ones_the_config_validates() {
    let (mut state, _) = with_clip();
    for accepted in [MIN_DOWNLOAD_QUOTA_MIB, MAX_DOWNLOAD_QUOTA_MIB] {
        assert!(StorageSettings::is_valid_download_quota_mib(accepted));
        state
            .set_download_quota_mib(accepted)
            .unwrap_or_else(|err| panic!("{accepted} MiB: {err}"));
        assert_eq!(state.download_quota_mib(), accepted);
    }

    // Out of range is refused by name rather than clamped: a quota that is not the
    // one the operator typed is worse than a refusal.
    let before = state.download_quota_mib();
    for refused in [
        MIN_DOWNLOAD_QUOTA_MIB - 1,
        MAX_DOWNLOAD_QUOTA_MIB.saturating_add(1),
    ] {
        let err = state
            .set_download_quota_mib(refused)
            .expect_err("out of range must be refused");
        assert!(
            err.contains(&refused.to_string()),
            "the refusal did not name the value: {err}"
        );
        assert_eq!(
            state.download_quota_mib(),
            before,
            "a refused quota still changed the cache"
        );
        let written = aria_settings::load(&state.config_path()).expect("parses");
        assert_eq!(
            written.storage.download_quota_mib, before,
            "a refused quota still reached the config"
        );
    }
}

#[test]
fn lowering_the_quota_evicts_down_to_it() {
    // The quota is not a label: setting it enforces it.
    let (mut state, _) = with_clip();
    seed_cache(&state, "big.bin", 3 * 1024 * 1024);
    assert!(state.download_cache_size_bytes() >= 3 * 1024 * 1024);

    state.set_download_quota_mib(1).expect("1 MiB is in range");
    assert!(
        state.download_cache_size_bytes() <= 1024 * 1024,
        "the cache is still {} over a 1 MiB quota",
        format_bytes(state.download_cache_size_bytes())
    );
}

#[test]
fn clearing_the_cache_removes_the_files_and_says_what_it_removed() {
    let (mut state, _) = with_clip();
    seed_cache(&state, "one.bin", 1_024);
    seed_cache(&state, "two.bin", 1_024);

    let cleared = state.clear_download_cache().expect("clear");
    assert_eq!(cleared.removed_files, 2);
    assert_eq!(cleared.removed_bytes, 2_048);
    assert_eq!(state.download_cache_size_bytes(), 0);
    let line = state.status_message.clone().expect("a status line");
    assert!(
        line.contains('2'),
        "the clear did not name what it removed: {line}"
    );
}

#[test]
fn clearing_an_empty_cache_is_not_an_error() {
    // The button must be usable before anything has been downloaded.
    let (mut state, _) = with_clip();
    let cleared = state.clear_download_cache().expect("clear");
    assert_eq!(cleared.removed_files, 0);
    assert_eq!(cleared.removed_bytes, 0);
}

#[test]
fn relocating_carries_the_cached_files_and_persists_the_new_root() {
    let (mut state, _) = with_clip();
    seed_cache(&state, "keep.bin", 512);
    let destination = state.data_dir.join("moved-root");

    state
        .relocate_storage_root(&destination)
        .expect("relocate to a fresh directory");

    assert_eq!(
        state.download_cache_root(),
        destination.join(DOWNLOADS_DIR),
        "the live cache is still on the old root"
    );
    assert!(
        destination.join(DOWNLOADS_DIR).join("keep.bin").exists(),
        "the cached file was left behind"
    );
    assert_eq!(state.download_cache_size_bytes(), 512);
    let written = aria_settings::load(&state.config_path()).expect("parses");
    assert_eq!(
        written.storage.root.as_deref(),
        Some(destination.as_path()),
        "the next launch would use the old root"
    );
}

#[test]
fn relocating_before_the_first_download_still_works() {
    // A fresh install has no cache directory yet. Refusing here would make the
    // setting unusable exactly when it is easiest to change.
    let (mut state, _) = with_clip();
    assert!(!state.download_cache_root().exists());
    let destination = state.data_dir.join("early-root");
    state.relocate_storage_root(&destination).expect("relocate");
    assert_eq!(state.download_cache_root(), destination.join(DOWNLOADS_DIR));
}

#[test]
fn a_refused_relocation_leaves_the_cache_where_it_was() {
    let (mut state, _) = with_clip();
    seed_cache(&state, "stay.bin", 128);
    let before = state.download_cache_root();

    // Relative, and a root whose cache directory already exists: both are refused,
    // and each refusal names what to do instead. A root that merely exists is fine —
    // only the cache directory inside it must be Aria's to create.
    let relative = PathBuf::from("relative-root");
    let occupied = state.data_dir.join("occupied");
    std::fs::create_dir_all(occupied.join(DOWNLOADS_DIR)).expect("occupy the destination");

    for refused in [relative, occupied] {
        let err = state
            .relocate_storage_root(&refused)
            .expect_err("must be refused");
        assert!(!err.is_empty(), "the refusal said nothing");
        assert_eq!(
            state.download_cache_root(),
            before,
            "a refused relocation moved the cache"
        );
        assert!(
            before.join("stay.bin").exists(),
            "a refused relocation moved the files"
        );
        let written = aria_settings::load(&state.config_path()).expect("parses");
        assert_eq!(
            written.storage.root, None,
            "a refused relocation reached the config"
        );
    }
}

#[test]
fn byte_sizes_are_reported_in_the_unit_the_operator_reads() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1_024), "1.0 KiB");
    assert_eq!(format_bytes(1_048_576), "1.0 MiB");
    assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
    // The boundary below a unit must not round up into it.
    assert_eq!(format_bytes(1_023), "1023 B");
}

/// A relative profile root is resolved where the session is built, so nothing
/// under it has to assert that it is absolute.
///
/// The class closed here is "a path the operator can type takes the editor
/// down". `ARIA_DATA_DIR=scratch/profile` reached `storage_layout`, which builds
/// a `StorageLayout` that refuses a relative root, and the shipped binary
/// panicked before the first frame. Resolving it at the single owner covers the
/// desktop entry point, the FFI one, and every test.
#[test]
fn a_relative_profile_root_is_resolved_before_anything_reads_it() {
    // Nothing here reaches the disk: a fresh session reads what is under the
    // root and creates none of it, so the resolved directory never has to exist.
    let relative = PathBuf::from("aria-relative-profile-root");
    let state = AppState::new(aria_plugin::PluginContext::new(), relative.clone())
        .expect("a relative profile root starts a session");
    assert!(
        state.data_dir.is_absolute(),
        "the session kept a relative profile root: {}",
        state.data_dir.display()
    );
    assert!(
        state.data_dir.ends_with(&relative),
        "the resolved root is not the one that was asked for: {}",
        state.data_dir.display()
    );
    let layout = state.storage_layout();
    assert_eq!(
        layout.root(),
        state.data_dir,
        "the layout is not rooted at the profile directory"
    );
    assert!(
        !state.cache_rows().is_empty(),
        "no cache resolved under a relative profile root"
    );
    assert!(
        !state.data_dir.exists(),
        "reading the profile created it: {}",
        state.data_dir.display()
    );
}
