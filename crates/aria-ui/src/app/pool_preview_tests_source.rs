//! Source view reconciliation and In/Out mark tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `pool_preview_tests.rs` through `use super::*`.

use super::*;

#[test]
fn a_mark_past_a_shortened_source_is_dropped_rather_than_moved() {
    let (mut state, _media, frames) = with_monitor();
    if let Some(view) = state.sources.focused_mut() {
        view.mark_in = Some(frames + 500);
        view.mark_out = Some(frames + 900);
    }

    state.reconcile_source_view();

    let view = state.sources.focused().expect("still open");
    assert_eq!(
        view.mark_in, None,
        "a silently moved in point is a wrong edit"
    );
    assert_eq!(view.mark_out, None);
}
#[test]
fn a_timeline_in_point_is_where_the_edit_lands() {
    let (mut state, _media, _frames) = with_monitor();
    state.playhead = aria_models::RationalTime::new(48, state.playhead.rate);
    state.set_mark_in();
    // The playhead moves on; the mark is the destination, not the playhead.
    state.playhead = aria_models::RationalTime::new(0, state.playhead.rate);

    state
        .cut_source_in(crate::types::EditMode::Ripple)
        .expect("three-point insert");

    assert_eq!(
        spans(&state).first().map(|(start, _)| *start),
        Some(48),
        "a three-point edit lands on the timeline in point"
    );
}
#[test]
fn four_points_retime_the_take_to_fill_the_marked_span() {
    let (mut state, media) = with_video_monitor();
    state.hover_pool_item(media, 0);
    state.set_source_in();
    state.hover_pool_item(media, 47);
    state.set_source_out();
    // The span being filled, `[0, 24)`. The playhead of an empty sequence
    // cannot leave 0, so the marks are set directly.
    state.mark_in = Some(0);
    state.mark_out = Some(24);

    state
        .cut_source_in(crate::types::EditMode::Overwrite)
        .expect("four-point overwrite");

    assert_eq!(
        spans(&state),
        vec![(0, 24), (0, 24)],
        "picture and sound both occupy the marked span exactly"
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("Fit to fill at 200% speed"),
        "forty-eight source frames in twenty-four sequence frames play at double speed"
    );
}
#[test]
fn a_source_in_and_a_marked_span_take_the_span_from_the_source() {
    let (mut state, media) = with_video_monitor();
    state.hover_pool_item(media, 10);
    state.set_source_in();
    state.mark_in = Some(4);
    state.mark_out = Some(24);

    state
        .cut_source_in(crate::types::EditMode::Overwrite)
        .expect("three-point overwrite");

    assert_eq!(spans(&state), vec![(4, 24), (4, 24)]);
    assert_eq!(
        state.status_message.as_deref(),
        Some("Overwrite"),
        "no retime"
    );
    let sources: Vec<i64> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| clip.source_start_ticks)
        .collect();
    assert_eq!(
        sources,
        vec![10, 10],
        "both channels start at the source In"
    );
}
#[test]
fn a_still_takes_the_marked_span_without_a_retime() {
    let (mut state, media, _frames) = with_monitor();
    state.hover_pool_item(media, 0);
    state.set_source_in();
    state.hover_pool_item(media, 1);
    state.set_source_out();
    state.mark_in = Some(0);
    state.mark_out = Some(30);

    state
        .cut_source_in(crate::types::EditMode::Overwrite)
        .expect("four-point overwrite of a still");

    assert_eq!(spans(&state), vec![(0, 30)], "one picture, no sound");
    assert_eq!(state.status_message.as_deref(), Some("Overwrite"));
}
#[test]
fn match_frame_opens_the_source_of_the_clip_under_the_playhead() {
    let (mut state, media, _frames) = with_monitor();
    // Cut a take in, then look at the middle of it from the sequence.
    state.hover_pool_item(media, 1);
    state.set_source_in();
    state
        .cut_source_in(crate::types::EditMode::Ripple)
        .expect("cut in");
    state.show_program();
    let (start, end) = spans(&state)[0];
    state.playhead = aria_models::RationalTime::new((start + end) / 2, state.playhead.rate);

    assert!(state.match_frame(), "the clip under the playhead is media");

    let view = state.sources.focused().expect("the monitor opened");
    assert_eq!(view.media, media);
    assert_eq!(
        view.mark_in,
        Some(1),
        "the take that was used is marked, not the whole file"
    );
    assert!(view.tick >= 1, "the monitor lands inside the take");
}
/// Match Frame's row is offered wherever F finds a media clip under the
/// playhead, selected or not, and withheld in F's words where F finds none,
/// selection or not.
#[test]
fn match_frame_row_reads_the_clip_f_finds() {
    let row = |state: &AppState| {
        crate::chrome::menu_items::items_of(crate::types::AppMenu::Sequence, state)
            .into_iter()
            .find(|item| item.id == "seq-match-frame")
            .expect("the Match Frame row")
            .blocked_by(state)
    };
    let reason = Some(crate::app::decline::Decline::NoMediaClipAtPlayhead.reason());
    let (mut state, media, _frames) = with_monitor();
    state.hover_pool_item(media, 1);
    state.set_source_in();
    state
        .cut_source_in(crate::types::EditMode::Ripple)
        .expect("cut in");
    state.show_program();
    let (start, end) = spans(&state)[0];
    let rate = state.playhead.rate;

    state.playhead = aria_models::RationalTime::new(start, rate);
    state.clear_selection();
    assert_eq!(row(&state), None, "an unselected clip under the playhead");
    assert!(state.match_frame(), "F matches the unselected clip");

    state.show_program();
    state.select_at_playhead();
    assert!(!state.selection.is_empty(), "D selects the clip");
    state.playhead = aria_models::RationalTime::new(end, rate);
    assert_eq!(row(&state), reason, "a selected clip off the playhead");
    assert!(
        !state.match_frame(),
        "F finds no clip past the selected one"
    );
    assert_eq!(state.status_message.as_deref(), reason);
}
#[test]
fn match_frame_says_so_when_there_is_nothing_under_the_playhead() {
    let (mut state, _media, _frames) = with_monitor();
    state.show_program();

    assert!(!state.match_frame());
    assert_eq!(
        state.status_message.as_deref(),
        Some("No media clip under the playhead")
    );
}
/// A source mark set across the other one clears it and the status line
/// states which mark went; a mark on the other's own tick counts as crossing,
/// and Out on In's own frame keeps both.
#[test]
fn a_source_mark_set_across_the_other_clears_it_and_says_so() {
    let (mut state, media) = with_video_monitor();
    let marks = |state: &AppState| {
        let view = state.sources.focused().expect("source tab");
        (view.mark_in, view.mark_out)
    };
    state.hover_pool_item(media, 11);
    state.mark_in_here();
    state.hover_pool_item(media, 10);
    state.mark_out_here();
    assert_eq!(marks(&state), (None, Some(11)));
    assert_eq!(
        state.status_message.as_deref(),
        Some("Source Out; cleared the In at or after it")
    );

    state.hover_pool_item(media, 11);
    state.mark_in_here();
    assert_eq!(marks(&state), (Some(11), None));
    assert_eq!(
        state.status_message.as_deref(),
        Some("Source In; cleared the Out at or before it")
    );

    state.mark_out_here();
    assert_eq!(marks(&state), (Some(11), Some(12)));
    assert_eq!(state.status_message.as_deref(), Some("Source Out"));
    state.mark_in_here();
    assert_eq!(marks(&state), (Some(11), Some(12)));
    assert_eq!(state.status_message.as_deref(), Some("Source In"));
}
/// Each Markers row's guard reads the marks its own action acts on. Clear In,
/// Clear Out, and Clear In and Out read the Source tab in front of the Viewer,
/// as Ctrl+Shift+I, Ctrl+Shift+O, and Ctrl+Shift+X do; Go to In and Go to Out
/// read the sequence unless a Source tab has the transport keys, which always
/// has a frame to go to. With Program in front every row reads the sequence.
#[test]
fn each_mark_row_reads_the_marks_its_action_acts_on() {
    let row = |id: &str, state: &AppState| {
        crate::chrome::menu_items::items_of(crate::types::AppMenu::Markers, state)
            .into_iter()
            .find(|item| item.id == id)
            .expect("a Markers row")
            .blocked_by(state)
    };
    let (no_in, no_out) = (Some("No In point is set"), Some("No Out point is set"));
    let (mut state, media) = with_video_monitor();
    state.mark_in = Some(0);
    state.mark_out = Some(12);

    // A bare take in front, the Media panel active: the Clear rows read the
    // take, the Go to rows the sequence.
    assert_eq!(row("seq-clear-in", &state), no_in);
    assert_eq!(row("seq-clear-out", &state), no_out);
    assert!(row("mark-clear", &state).is_some());
    assert_eq!(row("mark-go-in", &state), None);
    assert_eq!(row("mark-go-out", &state), None);

    state.mark_in = None;
    state.mark_out = None;
    assert_eq!(row("mark-go-in", &state), no_in);
    assert_eq!(row("mark-go-out", &state), no_out);
    state.hover_pool_item(media, 5);
    state.mark_in_here();
    assert_eq!(row("seq-clear-in", &state), None);
    assert_eq!(row("seq-clear-out", &state), no_out);
    assert_eq!(row("mark-clear", &state), None);
    state.clear_mark_in_here();
    assert_eq!(state.sources.focused().map(|view| view.mark_in), Some(None));

    // The Viewer active: the take has the transport keys and a frame to go to.
    state.active_panel = crate::types::ActivePanel::Preview;
    assert_eq!(row("mark-go-in", &state), None);
    assert_eq!(row("mark-go-out", &state), None);

    // Program in front: every row reads the sequence again.
    state.show_program();
    assert_eq!(row("seq-clear-in", &state), no_in);
    assert_eq!(row("mark-go-in", &state), no_in);
    state.mark_in = Some(3);
    assert_eq!(row("seq-clear-in", &state), None);
    assert_eq!(row("mark-go-in", &state), None);
    assert_eq!(row("mark-clear", &state), None);
}
