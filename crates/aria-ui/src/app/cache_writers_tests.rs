//! Every disk cache's writer writes where its Caches row points.
//!
//! The class closed here: a cache whose row moves while its writer keeps a
//! directory of its own. The model store, the analysis root, and the catalog
//! client each joined a literal onto the profile root, and render files opened
//! before the config was read, so a storage root, an override, or **Move
//! storage…** changed the row and not the bytes. Each case visits every member
//! of `CacheId::ALL` and derives its expectation from the registry, never from
//! the session's own layout. `writer_dir` has no wildcard: a cache added to the
//! registry does not compile until its writer is named here.
//!
//! Not caught: a writer that resolves the right directory and then writes
//! beside it, and the memory caches, which have no directory to compare.

use std::path::{Path, PathBuf};

use aria_models::MediaId;
use aria_plugin::PluginContext;
use aria_settings::Settings;
use aria_storage::{CacheId, CacheKind};

use crate::app::AppState;
use crate::app::premiere_tests::place_media;

fn texture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// A session over a scratch profile whose config `configure` wrote before
/// launch, holding one pool item so the proxy writer has a source to name.
fn session(configure: impl FnOnce(&Path, &mut Settings)) -> AppState {
    let dir = crate::scratch_pool::scratch("cache-writers");
    let mut settings = Settings::default();
    configure(&dir, &mut settings);
    aria_settings::save(&aria_settings::config_in(&dir), &settings).expect("write the config");
    let mut state = AppState::new(PluginContext::new(), dir).expect("state");
    state.new_project().expect("a project");
    place_media(&mut state, texture());
    state
}

fn disk_caches() -> impl Iterator<Item = CacheId> {
    CacheId::ALL
        .into_iter()
        .filter(|id| id.descriptor().kind == CacheKind::Disk)
}

/// `id`'s directory under a storage root that gives it no path of its own.
fn under(root: &Path, id: CacheId) -> Option<PathBuf> {
    id.descriptor().default_relative.map(|rel| root.join(rel))
}

/// The directory `id`'s writer uses now, read from the writer.
fn writer_dir(state: &AppState, id: CacheId) -> Option<PathBuf> {
    let parent = |path: PathBuf| path.parent().map(Path::to_path_buf);
    match id {
        CacheId::PreviewFrames | CacheId::TimelineFilmstrips | CacheId::TimelineWaveforms => None,
        CacheId::LibraryThumbnails => state.library_thumbnail_dir(),
        CacheId::Proxies => {
            let media = MediaId::from_raw(state.store.media.first().expect("a pool item").id);
            state.proxy_file(media).ok().and_then(parent)
        }
        CacheId::Analysis => Some(state.analysis_cache_root()),
        CacheId::RenderFiles => Some(state.render_files().dir().to_path_buf()),
        CacheId::AiModels => state.model_manager().ok().map(|m| m.root().to_path_buf()),
        CacheId::Download => Some(state.download_cache_root()),
        CacheId::Catalog => state.cloud_client().cache_dir().map(Path::to_path_buf),
        CacheId::Luts => state.starter_look_path("probe").and_then(parent),
    }
}

/// Every cache's writer and row both at `expected(id)`.
fn assert_writers(state: &AppState, case: &str, expected: impl Fn(CacheId) -> Option<PathBuf>) {
    for id in CacheId::ALL {
        let want = expected(id);
        assert_eq!(writer_dir(state, id), want, "{case}: the {id} writer");
        assert_eq!(
            state.storage_layout().resolve(id),
            want,
            "{case}: the {id} row"
        );
    }
}

#[test]
fn a_configured_root_holds_every_writer_from_launch() {
    let mut root = PathBuf::new();
    let state = session(|dir, settings| {
        root = dir.join("root-b");
        settings.storage.root = Some(root.clone());
    });
    assert_writers(&state, "configured root", |id| under(&root, id));
}

#[test]
fn a_configured_override_holds_its_writer_from_launch() {
    let mut own = PathBuf::new();
    let state = session(|dir, settings| {
        own = dir.join("own");
        for id in disk_caches() {
            *settings
                .storage
                .paths
                .slot(id.as_str())
                .expect("a disk key") = Some(own.join(id.as_str()));
        }
    });
    assert_writers(&state, "configured overrides", |id| {
        under(&own, id).map(|_| own.join(id.as_str()))
    });
}

#[test]
fn moving_the_storage_root_carries_every_writer_and_its_files() {
    let mut state = session(|_, _| {});
    let old = state.data_dir.clone();
    for id in disk_caches() {
        let dir = writer_dir(&state, id).expect("a disk writer");
        std::fs::create_dir_all(&dir).expect("the cache directory");
        std::fs::write(dir.join("seed.bin"), id.as_str()).expect("a cached file");
    }
    let root = old.join("root-b");

    state.relocate_storage_root(&root).expect("move storage");

    assert_writers(&state, "moved root", |id| under(&root, id));
    for id in disk_caches() {
        let to = under(&root, id).expect("a disk cache");
        assert_eq!(
            std::fs::read_to_string(to.join("seed.bin")).ok().as_deref(),
            Some(id.as_str()),
            "{id}'s files stayed behind"
        );
        assert!(
            !under(&old, id).expect("a disk cache").exists(),
            "{id} still has a directory on the old root"
        );
    }
    let written = aria_settings::load(&state.config_path()).expect("parses");
    assert_eq!(written.storage.root.as_deref(), Some(root.as_path()));
}

#[test]
fn moving_redirecting_and_resetting_one_cache_carries_its_writer() {
    let mut state = session(|_, _| {});
    let data = state.data_dir.clone();
    for id in disk_caches() {
        let key = id.as_str();
        let moved = data.join("moved").join(key);
        state.relocate_registered_cache(key, &moved).expect("move");
        assert_eq!(writer_dir(&state, id), Some(moved), "{id} after Move…");

        let row = state
            .cache_rows()
            .into_iter()
            .find(|row| row.key == key)
            .expect("a row");
        let redirected = data.join("redirected").join(key);
        let redirect = state.set_cache_override(key, &redirected);
        assert_eq!(
            redirect.is_ok(),
            row.redirectable,
            "{id}: Redirect… against its row"
        );
        if row.redirectable {
            assert_eq!(
                writer_dir(&state, id),
                Some(redirected),
                "{id} after Redirect…"
            );
        }

        state.clear_cache_override(key).expect("use default");
        assert_eq!(
            writer_dir(&state, id),
            under(&data, id),
            "{id} after Use default"
        );
    }
}
