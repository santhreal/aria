//! What the render-lane surfaces have to keep true.
//!
//! The class closed here is the one this module exists for: a capability that
//! ships in `aria-render`, is tested there, and reaches no operator. A pack
//! install that writes part of the pack and a sequence render that quietly asks
//! for an audio track are the same defect in two costumes.
//!
//! Both sides of every list are enumerated at run time. Nothing below writes
//! down how many looks ship or what they are called: the pack is the filter
//! catalog, so a preset added to `filters.toml` flows through here unedited.
//!
//! Not covered: the pixels. Rendering needs a GPU, so the sequence job is
//! asserted at its refusals, which arrive before a renderer is built. The last
//! frame's cost is `info_tests`'.

use std::collections::BTreeSet;

use aria_core::count::counted;
use aria_storage::CacheId;

use super::super::premiere_tests::with_clip;
use super::*;

fn scratch() -> PathBuf {
    crate::scratch_pool::scratch("render_reach")
}

/// Installing the pack writes one file per shipped look into the registered LUT
/// cache. A subset rather than an equality: that directory is also where
/// downloaded LUTs land, and those are not this action's business.
#[test]
fn installing_the_pack_writes_every_look_into_the_lut_cache() {
    let (mut state, _) = with_clip();
    state.install_starter_looks();
    assert_eq!(state.last_error, None, "the pack refused to install");

    let dir = state
        .storage_layout()
        .resolve(CacheId::Luts)
        .expect("the LUT cache is a disk cache and resolves to a path");
    let on_disk: BTreeSet<String> = std::fs::read_dir(&dir)
        .expect("the install created the cache directory")
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter_map(|name| name.strip_suffix(".cube").map(str::to_string))
        .collect();
    let expected: BTreeSet<String> = aria_render::starter_lut_entries()
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert!(!expected.is_empty(), "the build ships no looks at all");
    let missing: Vec<&String> = expected.difference(&on_disk).collect();
    assert!(missing.is_empty(), "never written: {missing:?}");
}

/// A folder of stills has no audio track. The sequence settings say so while
/// the sheet's own settings still carry audio, and every other choice on the
/// sheet survives — or the sequence and the video render different pictures.
#[test]
fn a_frame_sequence_never_asks_for_an_audio_track() {
    let (mut state, _) = with_clip();
    state.export_options.audio = true;

    let sheet = state.export_settings();
    let sequence = state.image_sequence_settings();

    assert!(sheet.audio, "the sheet stopped carrying audio");
    assert!(
        !sequence.audio,
        "a folder of PNGs was asked for an audio track"
    );
    assert_eq!(
        sequence,
        sheet.without_audio(),
        "the sequence changed a second setting"
    );
}

/// `PngSequenceEncoder` numbers from zero on every render, so a used folder is
/// refused rather than half-overwritten. An unrelated file is not a render.
#[test]
fn a_sequence_refuses_a_folder_that_already_holds_frames() {
    let (mut state, _) = with_clip();
    let dir = scratch();

    std::fs::write(dir.join("notes.txt"), b"unrelated").expect("seed a stray file");
    assert!(
        !holds_rendered_frames(&dir),
        "an unrelated file was mistaken for a previous render"
    );

    std::fs::write(dir.join("frame_00000.png"), b"not really a png").expect("seed a frame");
    assert!(
        holds_rendered_frames(&dir),
        "a rendered frame went unnoticed"
    );

    let err = state
        .export_image_sequence_job(dir)
        .expect_err("a used folder must be refused before a renderer is built");
    assert!(
        err.contains("already holds rendered frames"),
        "unhelpful refusal: {err}"
    );
}

/// An empty timeline renders nothing, and saying so up front is the difference
/// between a refusal and a job that spins up a GPU to write zero files.
#[test]
fn a_sequence_refuses_an_empty_timeline() {
    let (mut state, _) = with_clip();
    state.new_project().expect("empty the timeline");

    let err = state
        .export_image_sequence_job(scratch())
        .expect_err("an empty timeline must be refused");

    assert!(
        err.contains("timeline is empty"),
        "unhelpful refusal: {err}"
    );
}

/// Reloading re-reads the file the pool row names, and leaves the entry in
/// place: the whole point is that every clip referencing it recovers untouched.
#[test]
fn reloading_a_pool_item_re_reads_it_without_dropping_the_entry() {
    let (mut state, _) = with_clip();
    let before: Vec<u64> = state.store.media.iter().map(|item| item.id).collect();
    let media = MediaId::from_raw(*before.first().expect("one pool item"));

    state.reload_media_from_disk(media);

    let after: Vec<u64> = state.store.media.iter().map(|item| item.id).collect();
    assert_eq!(before, after, "reloading changed the pool");
    assert!(
        state
            .status_message
            .as_deref()
            .is_some_and(|msg| msg.contains("Reloaded")),
        "reload said nothing: {:?}",
        state.status_message
    );
}

/// Reloading a row that is gone is a sentence, not a panic and not silence.
#[test]
fn reloading_a_missing_pool_item_says_so() {
    let (mut state, _) = with_clip();
    let absent = state
        .store
        .media
        .iter()
        .map(|item| item.id)
        .max()
        .expect("one pool item")
        + 1;

    state.reload_media_from_disk(MediaId::from_raw(absent));

    assert_eq!(
        state.status_message.as_deref(),
        Some("That pool item is no longer in the project")
    );
}

/// Reloading everything reports the count it managed, and an empty pool is a
/// sentence rather than a silent no-op.
#[test]
fn reloading_everything_reports_what_it_read() {
    let (mut state, _) = with_clip();
    let count = state.store.media.len();
    assert!(count > 0, "the fixture imported nothing");

    state.reload_all_media_from_disk();
    assert_eq!(
        state.status_message.as_deref(),
        Some(format!("Reloaded {} from disk", counted(count, "item", "items")).as_str())
    );

    state.new_project().expect("empty the pool");
    state.reload_all_media_from_disk();
    assert_eq!(state.status_message.as_deref(), Some("The pool is empty"));
}
