//! Contracts for the clipboard and for zoom fitting.
//!
//! Two classes are closed here. "A paste variant exists as a menu label but
//! does the same thing as plain paste": each assertion reads the projected
//! store after the edit, because that is what the timeline draws: Insert has to
//! be visible as a *moved* neighbor, Attributes as a changed look on an
//! *unmoved* clip. And "the clipboard holds a reference to a clip": Cut then
//! Paste has to put the clip back, a change after the copy must not reach the
//! paste, and a copy taken in one sequence has to paste into another.
//!
//! Fitting is asserted as a relation between two zooms rather than an absolute
//! number, so the test survives a change to the chrome widths it divides by.
//!
//! Not covered: the drawn rows, and dragging the playhead — both need a render
//! surface. A copy refused for playing media the project lost is asserted in
//! `aria-engine`'s paste tests.

use aria_models::{ClipId, ClipParam, Generator, MediaId, TrackId};

use super::AppState;
use super::decline::Decline;
use super::premiere_tests::with_clip;

fn clips(state: &AppState) -> Vec<(u64, i64, i64)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|c| (c.id, c.timeline_start_ticks, c.timeline_end_ticks))
        .collect()
}

fn span(state: &AppState, clip: ClipId) -> (i64, i64) {
    clips(state)
        .into_iter()
        .find(|(id, _, _)| *id == clip.raw())
        .map(|(_, start, end)| (start, end))
        .expect("clip still projected")
}

/// The fixture clip's lane and media, for a second placement on the same lane.
fn lane_and_media(state: &AppState, clip: ClipId) -> (TrackId, MediaId) {
    let ui = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.id == clip.raw())
        .expect("fixture clip");
    (
        TrackId::from_raw(ui.track_id),
        MediaId::from_raw(ui.media_id.expect("fixture carries media")),
    )
}

/// Two clips packed on one lane: the fixture, then a copy of the same media
/// directly after it.
fn packed() -> (AppState, ClipId, ClipId) {
    let (mut state, first) = with_clip();
    let (track, media) = lane_and_media(&state, first);
    let (_, end) = span(&state, first);
    state
        .drop_media_on_timeline(media, Some(track), end)
        .expect("place the neighbor");
    let second = clips(&state)
        .into_iter()
        .map(|(id, _, _)| id)
        .find(|id| *id != first.raw())
        .map(ClipId::from_raw)
        .expect("two clips on the lane");
    (state, first, second)
}

#[test]
fn plain_paste_lands_at_the_playhead_on_the_source_lane() {
    let (mut state, clip) = with_clip();
    let (_, end) = span(&state, clip);
    state.copy_selected();
    // The playhead clamps to the sequence end, so the paste target is whatever
    // the transport settled on rather than the tick asked for.
    state.set_playhead(end * 4);
    let at = state.playhead.value;
    state.paste_clipboard().expect("paste");
    let starts: Vec<i64> = clips(&state)
        .into_iter()
        .map(|(_, start, _)| start)
        .collect();
    assert!(
        starts.contains(&at),
        "pasted at the playhead {at}: {starts:?}"
    );
    assert_eq!(clips(&state).len(), 2);
    assert_eq!(span(&state, clip), (0, end), "the source did not move");
}

#[test]
fn paste_insert_opens_a_hole_instead_of_overwriting() {
    let (mut state, first, second) = packed();
    let (start, end) = span(&state, first);
    let duration = end - start;
    let (second_start, _) = span(&state, second);
    state.select_clip(first);
    state.copy_selected();
    state.set_playhead(second_start);
    state.paste_insert().expect("paste insert");
    assert_eq!(clips(&state).len(), 3, "the copy is a third clip");
    assert_eq!(
        span(&state, second).0,
        second_start + duration,
        "the neighbor rippled right by the copy's length"
    );
    assert_eq!(span(&state, first), (start, end), "the source did not move");
    let pasted = clips(&state)
        .into_iter()
        .find(|(id, _, _)| *id != first.raw() && *id != second.raw())
        .expect("the pasted clip");
    assert_eq!(
        (pasted.1, pasted.2),
        (second_start, second_start + duration),
        "the copy fills the hole exactly"
    );
}

#[test]
fn paste_insert_is_inert_with_an_empty_clipboard() {
    let (mut state, _) = with_clip();
    let before = clips(&state);
    state.paste_insert().expect("inert");
    assert_eq!(clips(&state), before);
}

#[test]
fn paste_attributes_copies_the_look_and_leaves_timing_alone() {
    let (mut state, first, second) = packed();
    // Give the source a look: a scaled, half-opaque transform, a filter, and one
    // effect.
    state.select_clip(first);
    let mut transform = state.selected_ui_clip().expect("source").transform;
    transform.scale_x = 1.5;
    transform.opacity = 0.5;
    state.set_selected_transform(transform).expect("transform");
    let (source_start, source_end) = span(&state, first);
    for tick in [source_start + 1, source_end - 1] {
        state.set_playhead(tick);
        state
            .add_keyframe_at_playhead(ClipParam::Opacity)
            .expect("source opacity keyframe");
    }
    state.set_selected_filter("warm").expect("filter");
    let amount = ClipParam::Look {
        param: aria_models::LookParam::FilterIntensity,
    };
    state.set_param_scalar(amount, None, 0.75).expect("amount");
    let effect = state
        .effect_browse_list()
        .first()
        .map(|(spec, _)| spec.id.clone())
        .expect("the catalog ships an effect");
    state.add_effect_to_selected(&effect).expect("effect");
    state.copy_selected();

    let (start, end) = span(&state, second);
    state.select_clip(second);
    let before = state.selected_ui_clip().expect("target before paste");
    let projections = state.projections;
    let history = state.history_depth();
    state.paste_attributes().expect("paste attributes");

    let target = state.selected_ui_clip().expect("target");
    assert!((target.transform.scale_x - 1.5).abs() < 1e-4);
    assert!((target.transform.opacity - 0.5).abs() < 1e-4);
    assert_eq!(
        target.filter.as_ref().map(|f| f.id.as_str()),
        Some("warm"),
        "the filter came across"
    );
    assert!((target.filter.as_ref().expect("filter").amount - 0.75).abs() < 1e-3);
    assert_eq!(
        target
            .effects
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        vec![effect.as_str()],
        "the chain came across"
    );
    assert_eq!(
        state.keyframes_of(ClipParam::Opacity).len(),
        2,
        "the complete animated transform came across"
    );
    assert_eq!(
        span(&state, second),
        (start, end),
        "timing is not an attribute"
    );
    assert_eq!(
        state.projections,
        projections + 1,
        "one attribute paste must produce one projection"
    );
    assert_eq!(
        state.history_depth(),
        history + 1,
        "one attribute paste must produce one undo entry"
    );
    assert!(state.undo(), "the attribute transaction must undo");
    assert_eq!(
        state.selected_ui_clip().expect("target after undo"),
        before,
        "one undo must restore every target attribute and its timing"
    );
    assert!(
        state.keyframes_of(ClipParam::Opacity).is_empty(),
        "undo left pasted transform keyframes behind"
    );
}

#[test]
fn paste_attributes_replaces_the_target_chain_rather_than_appending() {
    let (mut state, first, second) = packed();
    let effect = state
        .effect_browse_list()
        .first()
        .map(|(spec, _)| spec.id.clone())
        .expect("the catalog ships an effect");
    state.select_clip(second);
    state
        .add_effect_to_selected(&effect)
        .expect("target effect");
    state
        .add_effect_to_selected(&effect)
        .expect("target effect");
    state.select_clip(first);
    state.copy_selected();
    state.select_clip(second);
    state.paste_attributes().expect("paste attributes");
    assert!(
        state.selected_ui_clip().expect("target").effects.is_empty(),
        "an empty source chain empties the target"
    );
}

#[test]
fn paste_attributes_onto_the_source_is_inert() {
    let (mut state, clip) = with_clip();
    state.copy_selected();
    let mut transform = state.selected_ui_clip().expect("clip").transform;
    transform.opacity = 0.25;
    state.set_selected_transform(transform).expect("transform");
    state.select_clip(clip);
    let before = state.selected_ui_clip().expect("clip");
    state.paste_attributes().expect("inert");
    assert_eq!(state.selected_ui_clip().expect("clip"), before);
}

#[test]
fn cut_then_paste_puts_the_clip_back_as_one_step() {
    let (mut state, clip) = with_clip();
    let (_, end) = span(&state, clip);
    state.cut_selected().expect("cut");
    assert!(clips(&state).is_empty(), "cut removed the clip");
    state.set_playhead(0);
    let history = state.history_depth();
    state.paste_clipboard().expect("paste");
    let pasted = clips(&state);
    assert_eq!(pasted.len(), 1, "the cut clip came back: {pasted:?}");
    assert_eq!((pasted[0].1, pasted[0].2), (0, end));
    assert_ne!(pasted[0].0, clip.raw(), "a paste is a new clip");
    assert_eq!(state.history_depth(), history + 1);
    assert!(
        state.paste_clipboard().is_err(),
        "a second paste over the first overlaps it"
    );
    assert_eq!(clips(&state).len(), 1, "the refused paste placed nothing");
}

#[test]
fn a_paste_places_the_clip_as_it_was_when_copied() {
    let (mut state, clip) = with_clip();
    let (_, end) = span(&state, clip);
    state.copy_selected();
    let mut transform = state.selected_ui_clip().expect("clip").transform;
    transform.opacity = 0.25;
    state.set_selected_transform(transform).expect("transform");
    state.set_playhead(end * 4);
    let at = state.playhead.value;
    state.paste_clipboard().expect("paste");
    let pasted = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.timeline_start_ticks == at && c.id != clip.raw())
        .expect("the pasted clip");
    assert!(
        (pasted.transform.opacity - 1.0).abs() < 1e-4,
        "the change after the copy reached the paste: {}",
        pasted.transform.opacity
    );
}

#[test]
fn a_copy_pastes_into_another_sequence() {
    let (mut state, clip) = with_clip();
    let (start, end) = span(&state, clip);
    state.copy_selected();
    state.new_sequence().expect("a new sequence");
    assert!(clips(&state).is_empty());
    state.paste_clipboard().expect("paste");
    let pasted = clips(&state);
    assert_eq!(pasted.len(), 1, "the copy landed in the open sequence");
    assert_eq!((pasted[0].1, pasted[0].2), (0, end - start));
}

#[test]
fn paste_insert_is_one_undo_step() {
    let (mut state, first, second) = packed();
    let before = clips(&state);
    let (second_start, _) = span(&state, second);
    state.select_clip(first);
    state.copy_selected();
    state.set_playhead(second_start);
    let history = state.history_depth();
    state.paste_insert().expect("paste insert");
    assert_eq!(state.history_depth(), history + 1);
    assert!(state.undo(), "undo the paste insert");
    assert_eq!(clips(&state), before, "one undo closes the hole too");
}

/// Cut takes every selected clip, so a paste puts them all back, spaced as
/// they were, as one undo step.
#[test]
fn cut_of_several_clips_pastes_them_all_back() {
    let (mut state, _, _) = packed();
    let spans = |state: &AppState| -> Vec<(i64, i64)> {
        let mut spans: Vec<_> = clips(state).into_iter().map(|(_, s, e)| (s, e)).collect();
        spans.sort_unstable();
        spans
    };
    let before = spans(&state);
    assert_eq!(before.len(), 2);
    state.select_all_unlocked();
    state.cut_selected().expect("cut");
    assert!(clips(&state).is_empty(), "cut removed both clips");
    state.set_playhead(before[0].0);
    let history = state.history_depth();
    state.paste_clipboard().expect("paste");
    assert_eq!(
        spans(&state),
        before,
        "both clips came back where they were"
    );
    assert_eq!(state.history_depth(), history + 1, "one paste is one step");
    assert!(state.undo(), "undo the paste");
    assert!(clips(&state).is_empty(), "one undo takes both back");
}

#[test]
fn copy_with_nothing_selected_keeps_the_clipboard() {
    let (mut state, _) = with_clip();
    state.copy_selected();
    let copied = state.clipboard.clone();
    assert!(copied.is_some());
    state.clear_selection();
    state.copy_selected();
    assert_eq!(state.clipboard, copied);
}

#[test]
fn paste_to_fit_with_several_copied_says_so() {
    let (mut state, _, _) = packed();
    state.select_all_unlocked();
    state.copy_selected();
    state.mark_in = Some(0);
    state.mark_out = Some(10);
    let before = clips(&state);
    state.paste_to_fit().expect("declines");
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::SeveralCopied.reason())
    );
    assert_eq!(clips(&state), before);
}

/// WHY: Paste to Fit of a copied title, shape or other generated clip answered
/// "That value isn't accepted here. Enter another value, then try again.": the
/// retimed paste took media clips alone and refused every other copy as a bad
/// value. A copy that plays no source fills the marked span by taking its
/// length; one that plays a source is retimed to it. Each content kind the
/// fixtures dress is pasted over the same span. Not caught: a multicam or
/// nested copy, which no fixture here dresses; the engine's paste tests hold
/// the speed a retimed copy derives.
#[test]
fn paste_to_fit_fills_the_marked_span_with_every_kind_of_copy() {
    let dressed: [(&str, fn() -> AppState); 3] = [
        ("media", || with_clip().0),
        ("title", super::fixtures::a_title),
        ("shape", || {
            super::fixtures::generated(Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            })
        }),
    ];
    let (from, to) = (600, 648);
    for (name, dress) in dressed {
        let mut state = dress();
        state.copy_selected();
        let before = clips(&state);
        state.mark_in = Some(from);
        state.mark_out = Some(to);
        state
            .paste_to_fit()
            .unwrap_or_else(|err| panic!("{name}: Paste to Fit was refused: {err}"));
        let added: Vec<(i64, i64)> = clips(&state)
            .into_iter()
            .filter(|clip| !before.contains(clip))
            .map(|(_, start, end)| (start, end))
            .collect();
        assert_eq!(
            added,
            vec![(from, to)],
            "{name}: the paste does not fill the marked span"
        );
        assert!(state.undo(), "{name}: the paste left no history entry");
        assert_eq!(
            clips(&state),
            before,
            "{name}: one undo did not take the paste back"
        );
    }
}

#[test]
fn every_paste_with_nothing_copied_says_so() {
    type Paste = fn(&mut AppState) -> Result<(), aria_engine::EngineError>;
    let pastes: [(&str, Paste); 4] = [
        ("paste", AppState::paste_clipboard),
        ("paste insert", AppState::paste_insert),
        ("paste attributes", AppState::paste_attributes),
        ("paste to fit", AppState::paste_to_fit),
    ];
    for (name, paste) in pastes {
        let (mut state, _) = with_clip();
        state.mark_in = Some(0);
        state.mark_out = Some(10);
        let before = clips(&state);
        state.status_message = None;
        paste(&mut state).expect("declines");
        assert_eq!(
            state.status_message.as_deref(),
            Some(Decline::NothingCopied.reason()),
            "{name}"
        );
        assert_eq!(clips(&state), before, "{name}");
    }
}

#[test]
fn fitting_a_selection_zooms_in_further_than_fitting_the_sequence() {
    let (mut state, first, _) = packed();
    state.last_window_w = 1440.0;
    state.zoom_fit_sequence();
    let whole = state.zoom;
    state.select_clip(first);
    state.zoom_fit_selection();
    assert!(
        state.zoom > whole,
        "a clip is shorter than the sequence: {} vs {whole}",
        state.zoom
    );
    assert_eq!(state.store.zoom, state.zoom, "the store followed");
}

#[test]
fn fitting_a_selection_with_nothing_selected_is_inert() {
    let (mut state, _, _) = packed();
    state.last_window_w = 1440.0;
    state.zoom_fit_sequence();
    let whole = state.zoom;
    state.clear_selection();
    assert_eq!(state.selection_span(), None);
    state.zoom_fit_selection();
    assert_eq!(state.zoom, whole);
}
