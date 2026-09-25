//! Contracts for the project-level settings and pool maintenance.
//!
//! The class closed here is the one every other cluster hit: a one-shot command
//! carrying a whole struct is sent with the untouched fields defaulted, so editing
//! one knob resets its siblings. `SetCanvas` carries both the aspect and the
//! background, and the aspect list is enumerated from `CanvasAspect::ALL` so a new
//! preset is exercised without a second list.

use aria_models::{AudioLayout, CanvasAspect, MediaId, Rational};

use super::AppState;
use super::premiere_tests::with_clip;

fn pool_ids(state: &AppState) -> Vec<u64> {
    state.store.media.iter().map(|item| item.id).collect()
}

#[test]
fn every_canvas_aspect_applies_and_reads_back() {
    let (mut state, _) = with_clip();
    assert_eq!(
        state.canvas().aspect,
        CanvasAspect::Auto,
        "a new project follows the footage"
    );
    assert!(!CanvasAspect::ALL.is_empty());

    for candidate in CanvasAspect::ALL {
        state
            .set_canvas_aspect(candidate)
            .unwrap_or_else(|err| panic!("{}: {err:?}", candidate.name()));
        assert_eq!(
            state.canvas().aspect,
            candidate,
            "{} did not read back",
            candidate.name()
        );
    }
}

#[test]
fn picking_an_aspect_keeps_the_background_and_the_reverse() {
    let (mut state, _) = with_clip();
    state.set_canvas_background([20, 30, 40]).unwrap();
    assert_eq!(state.canvas().background, [20, 30, 40]);

    // The aspect edit resends the background from the project; a defaulted field
    // here would silently reset it to black.
    state.set_canvas_aspect(CanvasAspect::Tall9x16).unwrap();
    assert_eq!(
        state.canvas().background,
        [20, 30, 40],
        "an aspect edit reset the background"
    );

    // And the reverse: a background edit must not drop the aspect.
    state.set_canvas_background([255, 255, 255]).unwrap();
    assert_eq!(state.canvas().aspect, CanvasAspect::Tall9x16);
    assert_eq!(state.canvas().background, [255, 255, 255]);
}

#[test]
fn a_canvas_edit_is_undoable_and_a_no_op_leaves_no_entry() {
    let (mut state, _) = with_clip();
    state.set_canvas_aspect(CanvasAspect::Square1x1).unwrap();
    assert!(state.undo());
    assert_eq!(state.canvas().aspect, CanvasAspect::Auto);

    // Drain the fixture's own edits (import, placement) so the next assertion is
    // about the canvas and nothing else.
    while state.undo() {}
    assert!(!state.store.can_undo);

    // Setting the value it already holds must not reach the engine, or undo would
    // consume an entry that changed nothing.
    let before = state.canvas();
    state.set_canvas_aspect(before.aspect).unwrap();
    state.set_canvas_background(before.background).unwrap();
    assert!(
        !state.store.can_undo,
        "a no-op canvas edit pushed an undo entry"
    );
}

/// The size the dialog offers is the size the project renders at: every offered
/// choice applies, reads back, and is what `canvas_size` reports. A refused size
/// keeps the stored one, so a mistake never leaves the project sizeless.
#[test]
fn every_offered_canvas_size_applies_and_reads_back() {
    let (mut state, _) = with_clip();
    assert!(
        state.canvas().resolution.is_none(),
        "a new project derives its size"
    );
    let derived = state.canvas_size();

    let choices = state.canvas_resolution_choices();
    assert!(
        !choices.is_empty() && choices.len() <= super::project_settings::CANVAS_BASELINES.len(),
        "the offered sizes come from the baseline table, minus the unencodable ones"
    );
    for (baseline, size) in choices {
        state
            .set_canvas_resolution(Some(size))
            .unwrap_or_else(|err| panic!("{baseline}p ({size:?}): {err:?}"));
        assert_eq!(state.canvas().resolution, Some(size));
        assert_eq!(
            state.canvas_size(),
            size,
            "{baseline}p rendered at a different size than it named"
        );
        assert_eq!(
            size[0].min(size[1]),
            baseline,
            "{baseline}p is not the shorter edge of {size:?}"
        );
    }

    // A size the encoder cannot take is refused, and the stored one survives.
    let stored = state.canvas().resolution;
    state
        .set_canvas_resolution(Some([1921, 1080]))
        .expect_err("an odd edge must be refused");
    assert_eq!(state.canvas().resolution, stored);

    // Derived is a choice too, and it goes back to the footage size.
    state.set_canvas_resolution(None).unwrap();
    assert!(state.canvas().resolution.is_none());
    assert_eq!(state.canvas_size(), derived);
}

/// Changing the shape keeps the delivery baseline and drops the explicit size
/// only for `Auto`, which means "follow the footage" and cannot honour one.
#[test]
fn changing_the_aspect_reshapes_an_explicit_size_and_auto_clears_it() {
    let (mut state, _) = with_clip();
    state.set_canvas_resolution(Some([3840, 2160])).unwrap();

    state.set_canvas_aspect(CanvasAspect::Tall9x16).unwrap();
    assert_eq!(
        state.canvas().resolution,
        Some([2160, 3840]),
        "the 2160 baseline did not survive the reshape"
    );
    assert_eq!(state.canvas_size(), [2160, 3840]);

    state.set_canvas_aspect(CanvasAspect::Square1x1).unwrap();
    assert_eq!(state.canvas().resolution, Some([2160, 2160]));

    state.set_canvas_aspect(CanvasAspect::Auto).unwrap();
    assert!(
        state.canvas().resolution.is_none(),
        "Auto kept an explicit size, so it no longer follows the footage"
    );
}

/// A no-op size edit must not reach the engine, or undo would consume an entry
/// that changed nothing; a real one is undoable in a single step.
#[test]
fn a_canvas_size_edit_is_undoable_and_a_no_op_leaves_no_entry() {
    let (mut state, _) = with_clip();
    state.set_canvas_resolution(Some([1920, 1080])).unwrap();
    assert!(state.undo());
    assert!(state.canvas().resolution.is_none());

    while state.undo() {}
    assert!(!state.store.can_undo);
    state.set_canvas_resolution(None).unwrap();
    assert!(
        !state.store.can_undo,
        "re-deriving an already derived size pushed an undo entry"
    );
}

#[test]
fn a_referenced_pool_item_is_refused_and_an_unused_one_is_removed() {
    let (mut state, clip) = with_clip();
    let media = state.store.media[0].id;
    assert!(
        state.media_is_in_use(MediaId::from_raw(media)),
        "the fixture clip references the pool item"
    );

    // Removing it while a clip holds it is refused, and the pool is untouched.
    let err = state
        .remove_media(MediaId::from_raw(media))
        .expect_err("a referenced item must be refused");
    assert!(
        !format!("{err:?}").is_empty(),
        "the refusal must carry a reason"
    );
    assert_eq!(pool_ids(&state), vec![media]);

    // Drop the clip and the item becomes removable.
    state.select_clip(clip);
    state.delete_selected().unwrap();
    assert!(!state.media_is_in_use(MediaId::from_raw(media)));
    state.remove_media(MediaId::from_raw(media)).unwrap();
    assert!(
        pool_ids(&state).is_empty(),
        "the pool still lists the removed item"
    );
}

#[test]
fn removing_a_pool_item_is_undoable() {
    let (mut state, clip) = with_clip();
    let media = state.store.media[0].id;
    state.select_clip(clip);
    state.delete_selected().unwrap();
    state.remove_media(MediaId::from_raw(media)).unwrap();
    assert!(pool_ids(&state).is_empty());

    assert!(state.undo(), "a pool removal pushes an undo entry");
    assert_eq!(pool_ids(&state), vec![media]);
}

/// The rescale rule is what makes one baseline table serve any footage shape, so
/// it is tested without a project: the ratio survives, the shorter edge is the
/// baseline, and both edges stay even. An extreme ratio can leave the encodable
/// bounds, which is exactly why the caller filters instead of clamping.
#[test]
fn scaling_a_shape_to_a_baseline_keeps_the_ratio_and_the_baseline() {
    use super::project_settings::scale_to_baseline;
    use aria_models::CanvasSettings;

    assert_eq!(scale_to_baseline([1920, 1080], 2160), [3840, 2160]);
    assert_eq!(scale_to_baseline([1080, 1920], 720), [720, 1280]);
    assert_eq!(scale_to_baseline([1080, 1080], 1440), [1440, 1440]);
    // A shape that is already the right size is left alone.
    assert_eq!(scale_to_baseline([1920, 1080], 1080), [1920, 1080]);

    for shape in [[1920, 1080], [1080, 1920], [1440, 1440], [4096, 1716]] {
        for baseline in super::project_settings::CANVAS_BASELINES {
            let size = scale_to_baseline(shape, baseline);
            assert_eq!(
                size[0].min(size[1]),
                baseline,
                "{shape:?} at {baseline} produced {size:?}"
            );
            assert!(
                size.iter().all(|edge| edge.is_multiple_of(2)),
                "{shape:?} at {baseline} produced an odd edge: {size:?}"
            );
            let want = shape[0] as f64 / shape[1] as f64;
            let got = size[0] as f64 / size[1] as f64;
            assert!(
                (want - got).abs() < 0.01,
                "{shape:?} at {baseline} changed the ratio: {size:?}"
            );
        }
    }

    // The extreme case the caller must filter: 21:9 delivery at an 8K baseline
    // needs a 10080-pixel long edge, which the encoder will not take.
    let extreme = scale_to_baseline([2560, 1080], 4320);
    assert!(
        !CanvasSettings::resolution_is_valid(extreme),
        "{extreme:?} should have been out of bounds"
    );
}

#[test]
fn every_audio_layout_applies_and_reads_back() {
    let (mut state, _) = with_clip();
    assert_eq!(
        state.audio_layout(),
        AudioLayout::Stereo,
        "a new project defaults to stereo"
    );
    assert!(!AudioLayout::ALL.is_empty());

    for candidate in AudioLayout::ALL {
        state
            .set_audio_layout(candidate)
            .unwrap_or_else(|err| panic!("{}: {err:?}", candidate.name()));
        assert_eq!(
            state.audio_layout(),
            candidate,
            "{} did not read back",
            candidate.name()
        );
    }
}

#[test]
fn an_audio_layout_edit_is_undoable_and_noop_leaves_no_entry() {
    let (mut state, _) = with_clip();
    state.set_audio_layout(AudioLayout::Surround51).unwrap();
    assert_eq!(state.audio_layout(), AudioLayout::Surround51);

    assert!(state.undo());
    assert_eq!(state.audio_layout(), AudioLayout::Stereo);

    while state.undo() {}
    assert!(!state.store.can_undo);

    // No-op edit leaves no entry.
    let before = state.audio_layout();
    state.set_audio_layout(before).unwrap();
    assert!(
        !state.store.can_undo,
        "a no-op audio layout edit pushed an undo entry"
    );
}

#[test]
fn sequence_frame_rate_applies_resamples_and_is_undoable() {
    let (mut state, _) = with_clip();
    assert_eq!(state.store.sequence.frame_rate, Rational::FPS_24);

    state.set_sequence_rate(Rational::FPS_30).unwrap();
    assert_eq!(state.store.sequence.frame_rate, Rational::FPS_30);

    assert!(state.undo());
    assert_eq!(state.store.sequence.frame_rate, Rational::FPS_24);

    // No-op edit leaves no entry.
    while state.undo() {}
    state.set_sequence_rate(Rational::FPS_24).unwrap();
    assert!(
        !state.store.can_undo,
        "a no-op sequence rate edit pushed an undo entry"
    );
}

#[test]
fn frame_rate_mismatch_identifies_pool_discrepancy() {
    let (mut state, _) = with_clip();
    assert_eq!(state.store.sequence.frame_rate, Rational::FPS_24);

    // The fixture pool holds one still. A still's tick base is 1000/1 so its
    // duration math stays exact; it is not a cadence, so the dialog stays quiet
    // at every sequence rate rather than claiming 1000 fps footage.
    assert_eq!(state.frame_rate_mismatch(), None);
    state.set_sequence_rate(Rational::FPS_30).unwrap();
    assert_eq!(
        state.frame_rate_mismatch(),
        None,
        "a still was read as footage"
    );

    // Real footage at 24fps under a 30fps sequence is the mismatch the note
    // exists for.
    let mut project = aria_models::Project::new("footage", Rational::FPS_24);
    project.add_media(aria_models::MediaSource::new(
        "/media/footage/shot.mp4",
        1920,
        1080,
        Rational::FPS_24,
        48,
        true,
    ));
    state.engine.lock().reset_project(project);
    state.sync_from_engine();
    assert_eq!(state.frame_rate_mismatch(), None);

    state.set_sequence_rate(Rational::FPS_30).unwrap();
    assert_eq!(state.frame_rate_mismatch(), Some(Rational::FPS_24));

    // Restoring sequence rate clears the mismatch.
    state.set_sequence_rate(Rational::FPS_24).unwrap();
    assert_eq!(state.frame_rate_mismatch(), None);
}
