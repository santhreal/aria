//! What a proxy request does before any frame is rendered.
//!
//! The class these close: a capability whose first step does not exist, and its
//! twin — a first step that reports success while nothing decodes differently.
//! Rendering a proxy needs a GPU and a native encoder, so what is asserted here
//! is everything on either side of the render: which file a source's proxy is,
//! that the file is keyed by content so a changed source does not inherit a
//! stale proxy, that a proxy already on disk is adopted without a job, that the
//! pool row states the substitution, and that a refusal never reaches the job
//! board.
//!
//! What they do not catch: whether the written file decodes to the same frames.
//! `aria_engine::proxy_project` covers the shape of the pass, and the export
//! path covers the encode.

use std::path::Path;

use aria_models::MediaId;
use aria_storage::CacheId;

use crate::app::premiere_tests::with_clip;

/// The one pool item the fixture imports.
fn pool_item(state: &super::AppState) -> MediaId {
    MediaId::from_raw(state.store.media.first().expect("a pool item").id)
}

/// Stand in for a rendered proxy: `set_media_proxy` records a path and drops the
/// open decoders, and nothing reads the bytes until a frame is rendered.
fn write_stand_in(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the proxies directory");
    std::fs::write(path, b"not a real mp4").expect("the stand-in proxy");
}

#[test]
fn a_proxy_lives_under_the_registered_cache_and_is_named_by_its_source() {
    let (state, _) = with_clip();
    let media = pool_item(&state);
    let path = state.proxy_file(media).expect("a proxy path");

    let root = state
        .storage_layout()
        .resolve(CacheId::Proxies)
        .expect("the proxies cache resolves");
    assert!(
        path.starts_with(&root),
        "{} is not under the registered cache {}",
        path.display(),
        root.display()
    );
    assert_eq!(
        path.extension().and_then(|e| e.to_str()),
        Some("mp4"),
        "a proxy is a video file"
    );
    // Asking twice is asking for the same file: the name is a function of the
    // source, not of when the question was asked.
    assert_eq!(state.proxy_file(media).expect("again"), path);
    // And an item that is no longer in the pool has no proxy path at all.
    assert!(state.proxy_file(MediaId::from_raw(9_999)).is_err());
}

#[test]
fn editing_the_source_orphans_its_proxy_instead_of_reusing_it() {
    // The name carries the source's size and modification time, so a re-encoded
    // or re-exported file cannot inherit the proxy of the frames it replaced —
    // which would preview a picture the file no longer has.
    let (mut state, _) = with_clip();
    // Its own copy of the fixture: this is the test that rewrites the file, and
    // the shipped asset is read by every other test in the crate.
    let copy = state.data_dir.join("proxy-source.png");
    std::fs::copy(&*state.store.media[0].path, &copy).expect("copy the fixture");
    let media = state.import_media(copy.clone()).expect("import the copy");
    let before = state.proxy_file(media).expect("a proxy path");

    let mut bytes = std::fs::read(&copy).expect("the copy is readable");
    bytes.extend_from_slice(b"one more byte");
    std::fs::write(&copy, &bytes).expect("rewrite the copy");

    let after = state.proxy_file(media).expect("a proxy path");
    assert_ne!(
        before, after,
        "a changed source kept the proxy name of the file it replaced"
    );
}

#[test]
fn a_proxy_already_on_disk_is_adopted_without_a_job_and_the_pool_row_says_so() {
    let (mut state, _) = with_clip();
    let media = pool_item(&state);
    let path = state.proxy_file(media).expect("a proxy path");
    write_stand_in(&path);

    assert!(!state.media_has_proxy(media), "nothing is proxied yet");
    assert!(
        !state.store.media[0].proxied,
        "the row claimed a substitution that was not in force"
    );

    let started = state.use_media_proxy(media).expect("adopt the proxy");
    assert!(
        started.is_none(),
        "a file already on disk was queued for rendering again"
    );
    assert!(state.job_view.is_empty(), "no job was needed");
    assert!(state.media_has_proxy(media));
    assert!(
        state.store.media[0].proxied,
        "the pool row does not state the substitution"
    );
    let caption = crate::app::media::media_caption(&state.store.media[0]);
    assert!(
        caption.ends_with("proxy"),
        "the caption does not name the proxy: {caption}"
    );

    // Asking again is refused by name rather than starting a second pass.
    let refusal = state.use_media_proxy(media).expect_err("already proxied");
    assert!(
        refusal.contains("already"),
        "the refusal does not say why: {refusal}"
    );

    // And the way back leaves the row honest.
    state.clear_media_proxy(media);
    assert!(!state.media_has_proxy(media));
    assert!(!state.store.media[0].proxied);
    let caption = crate::app::media::media_caption(&state.store.media[0]);
    assert!(!caption.contains("proxy"), "a cleared row still claims one");
}

#[test]
fn a_source_that_cannot_be_proxied_is_refused_before_the_board_sees_it() {
    // The fixture's pool item is a still: decoded once and kept, so a proxy
    // would buy nothing. Whether this build has an encoder decides which
    // refusal it is, and neither may leave an entry on the job board.
    let (mut state, _) = with_clip();
    let media = pool_item(&state);
    let refusal = state.use_media_proxy(media).expect_err("a still");
    assert!(!refusal.is_empty(), "a refusal with nothing in it");
    assert!(
        state.job_view.is_empty(),
        "a job that could never have run was queued anyway"
    );
    assert!(!state.media_has_proxy(media));
}

#[test]
fn proxying_the_whole_pool_reports_one_count_per_outcome() {
    let (mut state, _) = with_clip();
    let media = pool_item(&state);
    let path = state.proxy_file(media).expect("a proxy path");
    write_stand_in(&path);

    state.proxy_whole_pool();
    let message = state.status_message.clone().expect("a report");
    assert!(
        message.contains("1 already proxied"),
        "the adopted item was not counted: {message}"
    );
    assert!(
        message.contains("0 cannot be proxied"),
        "the sweep invented a refusal: {message}"
    );

    // An empty pool is the one case with nothing to count.
    state.new_project().expect("clear the session");
    state.proxy_whole_pool();
    assert_eq!(
        state.status_message.as_deref(),
        Some("The pool is empty"),
        "an empty pool got a count instead of an answer"
    );
}
