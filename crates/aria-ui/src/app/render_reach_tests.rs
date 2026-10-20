//! What the render-lane surfaces have to keep true.
//!
//! The class closed here is the one this module exists for: a capability that
//! ships in `aria-render`, is tested there, and reaches no operator. A picker
//! whose list drifts from the pack, a look that stops applying, and a sequence
//! render that quietly asks for an audio track are the same defect in three
//! costumes.
//!
//! Both sides of every list are enumerated at run time. Nothing below writes
//! down how many looks ship or what they are called: the pack is the filter
//! catalog, so a preset added to `filters.toml` flows through here unedited.
//!
//! Not covered: the pixels. Rendering needs a GPU, so the cost readout is
//! asserted in its unmeasured state and the sequence job is asserted at its
//! refusals, which are the two answers that arrive before a renderer is built.

use std::collections::BTreeSet;

use aria_storage::CacheId;

use super::super::premiere_tests::with_clip;
use super::*;

fn scratch() -> PathBuf {
    crate::scratch_pool::scratch("render_reach")
}

/// The picker's list is the pack's list. A look the pack bakes and the picker
/// never offers is unreachable; a look the picker offers and the pack cannot
/// bake is a row that errors on click.
#[test]
fn the_picker_offers_exactly_the_looks_the_pack_bakes() {
    let (state, _) = with_clip();
    let offered: BTreeSet<String> = state
        .starter_looks()
        .into_iter()
        .map(|look| look.id)
        .collect();
    let baked: BTreeSet<String> = aria_render::starter_lut_pack()
        .into_iter()
        .map(|lut| lut.id)
        .collect();
    assert_eq!(offered, baked);
    assert!(!offered.is_empty(), "the build ships no looks at all");
}

/// Every offered row carries the swatch that shows what it does, as wide as the
/// reference it grades. A row that can only name itself is a row nobody clicks.
#[test]
fn every_offered_look_carries_a_full_swatch() {
    let (state, _) = with_clip();
    for look in state.starter_looks() {
        assert_eq!(
            look.swatch.len(),
            aria_render::SWATCH_REFERENCE.len(),
            "{} swatched {} colours",
            look.id,
            look.swatch.len()
        );
        assert!(!look.label.is_empty(), "{} has no label", look.id);
    }
}

/// Applying a look bakes it to disk, points the clip at that file, and reads
/// back as applied — the three halves of "this row did something".
#[test]
fn applying_a_look_bakes_it_and_grades_the_selection() {
    let (mut state, _) = with_clip();
    let first = state
        .starter_looks()
        .into_iter()
        .next()
        .expect("the build ships at least one look");
    assert!(!first.applied, "nothing is graded before the first click");

    state.apply_starter_look(&first.id);
    assert_eq!(state.last_error, None, "the click failed");

    let applied = state
        .selected_lut()
        .expect("the clip carries a LUT after the click");
    let file = format!("{}.cube", first.id);
    assert!(
        applied.path.ends_with(&file),
        "the clip points at {} instead of {file}",
        applied.path
    );
    assert!(
        Path::new(&*applied.path).exists(),
        "{} was never written",
        applied.path
    );

    let marked: Vec<String> = state
        .starter_looks()
        .into_iter()
        .filter(|look| look.applied)
        .map(|look| look.id)
        .collect();
    assert_eq!(marked, vec![first.id]);
}

/// A look applied off a selection would bake a table nothing carries, so the
/// row refuses by name instead of grading nothing in silence.
#[test]
fn a_look_needs_a_selection() {
    let (mut state, _) = with_clip();
    let first = state.starter_looks().into_iter().next().expect("a look");
    state.selection.clear();

    state.apply_starter_look(&first.id);

    assert_eq!(state.selected_lut(), None);
    assert!(
        state
            .status_message
            .as_deref()
            .is_some_and(|msg| msg.contains("Select a clip")),
        "unhelpful refusal: {:?}",
        state.status_message
    );
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

/// A zeroed split means nothing has been measured, not that a frame was free.
#[test]
fn an_unmeasured_frame_reads_as_unmeasured() {
    let (mut state, _) = with_clip();
    let stats = state.last_render_stats();
    if stats.measured() {
        // A session that already drew a preview reports a real split, and the
        // stages have to add up to the frame.
        let sum = stats.decode_ms + stats.raster_ms + stats.composite_ms;
        assert!(
            (sum - stats.total_ms).abs() < 1e-9,
            "{stats:?} does not add up"
        );
        return;
    }
    state.report_render_cost();
    assert_eq!(
        state.status_message.as_deref(),
        Some("No frame has been rendered yet")
    );
}

/// The readout shows every number the split holds. A stage dropped from the
/// row list is a cost that is paid and never shown.
#[test]
fn the_readout_names_every_number_in_the_split() {
    let stats = UiRenderStats {
        decode_ms: 1.0,
        raster_ms: 2.0,
        composite_ms: 4.0,
        total_ms: 7.0,
        scaled_cost_ms: 6.0,
        effect_passes: 0,
        effect_pass_bind_groups: 0,
    };
    let rows = stats.rows();
    let labels: BTreeSet<&str> = rows.iter().map(|(name, _)| *name).collect();
    assert_eq!(labels.len(), rows.len(), "two rows share a label");

    // Every field's value appears, and the values were chosen distinct so a row
    // reading the wrong field cannot pass by coincidence.
    let shown: BTreeSet<u64> = rows.iter().map(|(_, ms)| ms.to_bits()).collect();
    let held: BTreeSet<u64> = [
        stats.decode_ms,
        stats.raster_ms,
        stats.composite_ms,
        stats.total_ms,
        stats.scaled_cost_ms,
    ]
    .iter()
    .map(|ms| ms.to_bits())
    .collect();
    assert_eq!(shown, held);
}

/// The pass count is a count, not a stage: it is absent while the frame drew no
/// effect pass, and both readouts print the same line once it did.
#[test]
fn the_effect_pass_line_appears_only_once_a_pass_was_drawn() {
    let mut stats = UiRenderStats {
        decode_ms: 1.0,
        raster_ms: 2.0,
        composite_ms: 4.0,
        total_ms: 7.0,
        scaled_cost_ms: 6.0,
        effect_passes: 0,
        effect_pass_bind_groups: 0,
    };
    assert_eq!(stats.passes(), None, "a frame with no effect pass reported");
    assert_eq!(stats.passes_line(), None);

    stats.effect_passes = 24;
    stats.effect_pass_bind_groups = 2;
    assert_eq!(stats.passes(), Some((24, 2)));
    let line = stats.passes_line().expect("a drawn pass has a line");
    assert!(
        line.contains("24") && line.contains('2'),
        "the line dropped a number: {line}"
    );

    let (mut state, _) = with_clip();
    state.frame_stats = aria_render::FrameStats {
        decode_ms: 1.0,
        raster_ms: 2.0,
        composite_ms: 4.0,
        effect_passes: 24,
        effect_pass_bind_groups: 2,
    };
    state.report_render_cost();
    let status = state.status_message.clone().expect("a reported cost");
    assert!(
        status.contains(&line),
        "the strip and the panel print different lines: {status} vs {line}"
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
        Some(format!("Reloaded {count} item(s) from disk").as_str())
    );

    state.new_project().expect("empty the pool");
    state.reload_all_media_from_disk();
    assert_eq!(state.status_message.as_deref(), Some("The pool is empty"));
}
