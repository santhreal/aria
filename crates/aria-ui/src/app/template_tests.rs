//! Contracts for replace-media and the template authoring marks.
//!
//! The class closed here is "an authoring mark is written without the validation
//! the model applies, so a click produces a refusal". Every slot restriction in
//! `clip_template::SLOT_MEDIA` is exercised, and both refused directions (a visual
//! restriction on an audio lane, a slot on a generated clip) are asserted inert.

use aria_models::SlotMedia;

use super::premiere_tests::with_clip;
use crate::app::AppState;
use crate::app::clip_template::{SLOT_MEDIA, slot_media_label};
use crate::types::UiClip;

fn selected(state: &AppState) -> UiClip {
    state.selected_ui_clip().expect("a selected clip")
}

#[test]
fn marking_a_media_clip_writes_a_slot_and_unmarking_clears_it() {
    let (mut state, _) = with_clip();
    assert!(state.selected_can_be_slot(), "a media clip can be a slot");
    assert!(state.selected_slot().is_none());

    state.toggle_selected_replaceable().expect("mark");
    let slot = state.selected_slot().expect("slot written");
    assert_eq!(slot.order, 0, "the first slot fills first");
    assert_eq!(
        slot.accepts,
        SlotMedia::Any,
        "a video lane defaults to visual"
    );
    assert_eq!(slot.label, None);
    assert_eq!(
        selected(&state).replaceable.as_ref().map(|s| s.order),
        Some(0),
        "the mark must reach the projected clip"
    );

    state.toggle_selected_replaceable().expect("unmark");
    assert!(state.selected_slot().is_none());
    assert!(selected(&state).replaceable.is_none());
}

#[test]
fn every_offered_restriction_applies_and_the_others_are_refused() {
    let (mut state, _) = with_clip();
    state.toggle_selected_replaceable().expect("mark");

    let offered = state.selected_slot_choices();
    assert!(
        !offered.is_empty(),
        "a marked clip must offer a restriction"
    );
    // A video lane must not offer the audio restriction, and vice versa: the
    // model refuses the mismatch, so a chip for it would be a dead button.
    assert!(
        !offered
            .iter()
            .any(|(kind, _)| *kind == SlotMedia::AudioOnly),
        "a video lane must not offer AudioOnly"
    );

    for (kind, label) in &offered {
        state
            .set_selected_slot_accepts(*kind)
            .unwrap_or_else(|err| panic!("{label}: {err:?}"));
        assert_eq!(
            state.selected_slot().expect("slot").accepts,
            *kind,
            "{label} did not land"
        );
        assert_eq!(slot_media_label(*kind), *label);
    }

    // Every restriction the table knows about but this lane does not offer must
    // be inert rather than an error.
    let before = state.selected_slot().expect("slot").accepts;
    for (kind, _) in SLOT_MEDIA {
        if offered.iter().any(|(offered, _)| *offered == kind) {
            continue;
        }
        state.set_selected_slot_accepts(kind).expect("inert");
        assert_eq!(
            state.selected_slot().expect("slot").accepts,
            before,
            "a restriction this lane refuses changed the slot anyway"
        );
    }
}

#[test]
fn order_and_hint_survive_each_others_edits() {
    let (mut state, _) = with_clip();
    state.toggle_selected_replaceable().expect("mark");
    let restriction = state.selected_slot_choices()[0].0;
    state
        .set_selected_slot_accepts(restriction)
        .expect("restriction");

    state
        .set_selected_slot_label("Your clip here")
        .expect("hint");
    let slot = state.selected_slot().expect("slot");
    assert_eq!(slot.label.as_deref(), Some("Your clip here"));
    assert_eq!(
        slot.accepts, restriction,
        "the hint edit reset the restriction"
    );
    assert_eq!(slot.order, 0, "the hint edit reset the order");

    state.nudge_selected_slot_order(3).expect("order up");
    let slot = state.selected_slot().expect("slot");
    assert_eq!(slot.order, 3);
    assert_eq!(
        slot.label.as_deref(),
        Some("Your clip here"),
        "the order edit dropped the hint"
    );
    assert_eq!(slot.accepts, restriction);

    // Order never goes below zero, and a no-op nudge is inert.
    state.nudge_selected_slot_order(-99).expect("order floor");
    assert_eq!(state.selected_slot().expect("slot").order, 0);
    state.nudge_selected_slot_order(0).expect("no move");
    assert_eq!(state.selected_slot().expect("slot").order, 0);

    // Whitespace clears the hint rather than storing a blank one.
    state.set_selected_slot_label("   ").expect("clear hint");
    assert_eq!(state.selected_slot().expect("slot").label, None);
}

#[test]
fn the_hint_is_editable_through_the_rename_primitive() {
    use crate::types::RenameTarget;
    let (mut state, _) = with_clip();
    state.toggle_selected_replaceable().expect("mark");

    // An unset hint still opens a draft, because the slot exists.
    state.begin_rename(RenameTarget::SlotLabel);
    assert_eq!(state.rename, Some(RenameTarget::SlotLabel));
    assert_eq!(state.rename_draft, "");
    for ch in "Intro".chars() {
        state.rename_push(ch);
    }
    state.commit_rename().expect("commit");
    assert_eq!(state.rename, None);
    assert_eq!(
        state.selected_slot().expect("slot").label.as_deref(),
        Some("Intro")
    );

    // With no slot there is nothing to rename, so no draft opens.
    state.toggle_selected_replaceable().expect("unmark");
    state.begin_rename(RenameTarget::SlotLabel);
    assert_eq!(state.rename, None);
}

#[test]
fn text_editability_is_offered_only_on_a_text_clip() {
    // The fixture clip is media, not text: the model refuses the flag, so the
    // toggle must be inert rather than an error.
    let (mut state, _) = with_clip();
    assert!(!state.selected_is_text());
    state.toggle_selected_text_editable().expect("inert");
    assert!(!selected(&state).text_editable);

    // A real text clip takes it, and toggles back.
    state
        .add_generated(aria_models::Generator::text("Title"))
        .expect("place a text clip");
    let text = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.kind == crate::types::UiClipKind::Text)
        .map(|clip| aria_models::ClipId::from_raw(clip.id))
        .expect("a text clip");
    state.select_clip(text);
    assert!(state.selected_is_text());
    assert!(!selected(&state).text_editable);

    state.toggle_selected_text_editable().expect("on");
    assert!(selected(&state).text_editable);
    state.toggle_selected_text_editable().expect("off");
    assert!(!selected(&state).text_editable);

    // A generated clip can never be a template slot.
    assert!(!state.selected_can_be_slot());
    state.toggle_selected_replaceable().expect("inert");
    assert!(state.selected_slot().is_none());
}

#[test]
fn replacing_media_keeps_the_placement_and_refuses_the_impossible() {
    let (mut state, clip) = with_clip();
    let before = selected(&state);
    let original = before.media_id.expect("media clip");

    // A second pool item, so there is something to repoint at.
    let other = state
        .import_media(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/texture.png"),
        )
        .expect("import a second item");

    state.replace_selected_media(other).expect("replace");
    let after = selected(&state);
    assert_eq!(
        after.media_id,
        Some(other.raw()),
        "the source did not change"
    );
    assert_eq!(
        (after.timeline_start_ticks, after.timeline_end_ticks),
        (before.timeline_start_ticks, before.timeline_end_ticks),
        "replacing media moved the clip"
    );
    assert_eq!(
        after.source_end_ticks - after.source_start_ticks,
        before.source_end_ticks - before.source_start_ticks,
        "replacing media changed how much of the source plays"
    );
    assert_eq!(after.id, clip.raw(), "replacing media replaced the clip");

    // Unknown media, and no selection, are both inert.
    state
        .replace_selected_media(aria_models::MediaId::from_raw(99_999))
        .expect("inert");
    assert_eq!(selected(&state).media_id, Some(other.raw()));
    state.clear_selection();
    state.replace_selected_media(other).expect("inert");
    let _ = original;
}

/// The drop gesture behind the replace-edit: Alt over an occupied position
/// repoints that clip, Alt over empty lane reports that it found nothing so the
/// drop can place the media instead.
///
/// The class closed here is "a modifier turns a drop into nothing": the gesture
/// has to say whether it acted, and it must address the clip under the pointer
/// rather than the selection, because a drop does not move the selection.
#[test]
fn an_alt_drop_repoints_the_clip_under_it_and_reports_an_empty_position() {
    let (mut state, clip) = with_clip();
    let before = selected(&state);
    let track = aria_models::TrackId::from_raw(before.track_id);
    let other = state
        .import_media(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/texture.png"),
        )
        .expect("import a second item");

    // Nothing is selected, so a gesture that read the selection would be inert.
    state.clear_selection();
    let inside =
        before.timeline_start_ticks + (before.timeline_end_ticks - before.timeline_start_ticks) / 2;
    assert!(
        state
            .replace_media_at(other, track, inside)
            .expect("replace at a covered tick"),
        "the clip under the tick was not repointed"
    );
    let after = state.ui_clip_ref(clip).expect("the clip survived");
    assert_eq!(after.media_id, Some(other.raw()));
    assert_eq!(
        (after.timeline_start_ticks, after.timeline_end_ticks),
        (before.timeline_start_ticks, before.timeline_end_ticks),
        "the replace moved the clip"
    );

    // Past the clip's end, and on a lane that is not there: both report that
    // there was nothing to repoint rather than erroring.
    assert!(
        !state
            .replace_media_at(other, track, before.timeline_end_ticks + 1)
            .expect("empty position"),
    );
    assert!(
        !state
            .replace_media_at(other, aria_models::TrackId::from_raw(9_999), inside)
            .expect("unknown lane"),
    );
}
