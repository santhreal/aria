//! A Source tab opened from a timeline clip: the frame it opens at, and the
//! match-frame edit its In and Out make.
//!
//! The class these close: the tab and the clip describe one source window, so
//! every path that moves either side (a mark in the tab, an undo, a deletion,
//! a refusal) must leave them agreeing, and every frame the trim keeps must
//! stay where it was on the timeline, forward, reversed, and retimed.
//!
//! Not covered: the double-click routing in the timeline canvas, which needs
//! a window; `double_click_clip` is the method it calls.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, Rational, RationalTime};

use crate::app::AppState;
use crate::scratch_pool::scratch;
use crate::types::{EditMode, UiClipKind};
use aria_plugin::PluginContext;

/// A 24 fps, two-second take with sound, its ticks 12..48 cut into a 24 fps
/// sequence with an audio lane at tick 24 by Overwrite, Program focused.
/// Returns the state, the take, and the picture clip.
fn placed() -> (AppState, u64, u64) {
    let mut state = AppState::new(PluginContext::new(), scratch("viewer-match")).expect("state");
    state.new_project().expect("project");
    let video = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4");
    let media = state.import_media(video).expect("import").raw();
    state
        .add_lane_of_kind(aria_models::TrackKind::Audio, None)
        .expect("an audio lane");
    assert!(state.open_source(media), "the take opens");
    state.hover_pool_item(media, 12);
    state.set_source_in();
    state.hover_pool_item(media, 47);
    state.set_source_out();
    state.playhead = RationalTime::new(24, state.playhead.rate);
    state
        .cut_source_in(EditMode::Overwrite)
        .expect("overwrite the take in");
    state.show_program();
    let clip = picture(&state, media).0;
    (state, media, clip)
}

/// The picture clip of `media`: its id, timeline span, and source window.
fn picture(state: &AppState, media: u64) -> (u64, (i64, i64), (i64, i64)) {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.media_id == Some(media) && clip.kind != UiClipKind::Audio)
        .map(|clip| {
            (
                clip.id,
                (clip.timeline_start_ticks, clip.timeline_end_ticks),
                (clip.source_start_ticks, clip.source_end_ticks),
            )
        })
        .expect("the picture is on the timeline")
}

/// Every clip's timeline span, sorted: the picture and its linked sound.
fn spans(state: &AppState) -> Vec<(i64, i64)> {
    let mut out: Vec<(i64, i64)> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    out.sort_unstable();
    out
}

/// Park the focused Source tab on `tick`.
fn park(state: &mut AppState, tick: i64) {
    state.sources.focused_mut().expect("a Source tab").tick = tick;
}

fn marks(state: &AppState) -> (Option<i64>, Option<i64>) {
    let view = state.sources.focused().expect("a Source tab");
    (view.mark_in, view.mark_out)
}

#[test]
fn a_double_clicked_clip_opens_its_take_at_the_frame_the_sequence_shows() {
    let (mut state, media, clip) = placed();
    state.playhead = RationalTime::new(34, state.playhead.rate);

    state.double_click_clip(clip);

    let view = state.sources.focused().expect("a Source tab opened");
    assert_eq!(view.media, media);
    assert_eq!(view.clip, Some(clip), "the tab is linked to the clip");
    assert_eq!(
        view.tick, 22,
        "timeline 34 is 10 frames into a take cut from 12"
    );
    assert_eq!(
        (view.mark_in, view.mark_out),
        (Some(12), Some(48)),
        "the clip's window"
    );
}

#[test]
fn off_the_clip_the_tab_opens_at_the_clips_first_frame() {
    let (mut state, _media, clip) = placed();
    state.playhead = RationalTime::new(0, state.playhead.rate);

    assert!(state.open_clip_source(clip));

    assert_eq!(state.sources.focused().map(|view| view.tick), Some(12));
}

#[test]
fn a_pool_tab_and_a_clip_tab_on_one_take_are_two_tabs() {
    let (mut state, media, clip) = placed();
    assert_eq!(state.sources.tabs().len(), 1, "the pool tab from the cut");

    state.double_click_clip(clip);
    assert_eq!(state.sources.tabs().len(), 2);
    state.double_click_clip(clip);
    assert_eq!(
        state.sources.tabs().len(),
        2,
        "the clip's tab is focused again"
    );
    assert!(state.open_source(media));
    assert_eq!(
        state.sources.tabs().len(),
        2,
        "the pool tab is focused again"
    );
    assert_eq!(state.sources.focused().and_then(|view| view.clip), None);
}

#[test]
fn source_in_trims_the_head_and_every_kept_frame_stays_put() {
    let (mut state, media, clip) = placed();
    state.double_click_clip(clip);
    park(&mut state, 20);

    state.set_source_in();

    let (_, timeline, window) = picture(&state, media);
    assert_eq!(window, (20, 48));
    assert_eq!(timeline, (32, 60), "source 20 stays at timeline 32");
    assert_eq!(
        spans(&state),
        vec![(32, 60), (32, 60)],
        "the sound trims with it"
    );
    assert_eq!(
        marks(&state),
        (Some(20), Some(48)),
        "the tab reads the clip"
    );
}

#[test]
fn source_out_trims_the_tail() {
    let (mut state, media, clip) = placed();
    state.double_click_clip(clip);
    park(&mut state, 39);

    state.set_source_out();

    let (_, timeline, window) = picture(&state, media);
    assert_eq!(window, (12, 40));
    assert_eq!(timeline, (24, 52));
    assert_eq!(marks(&state), (Some(12), Some(40)));
}

#[test]
fn an_undo_puts_the_clip_and_the_tab_back_together() {
    let (mut state, media, clip) = placed();
    state.double_click_clip(clip);
    park(&mut state, 20);
    state.set_source_in();

    assert!(state.undo());

    assert_eq!(picture(&state, media).2, (12, 48));
    assert_eq!(
        marks(&state),
        (Some(12), Some(48)),
        "the tab follows the undo"
    );
}

#[test]
fn clearing_the_marks_extends_the_clip_over_the_whole_take() {
    let (mut state, media, clip) = placed();
    state.double_click_clip(clip);
    let take = state.sources.focused().expect("tab").duration_ticks;

    state.clear_source_marks();

    let (_, timeline, window) = picture(&state, media);
    assert_eq!(window, (0, take));
    assert_eq!(timeline, (12, 12 + take), "source 12 stays at timeline 24");
}

#[test]
fn a_refused_trim_changes_neither_the_clip_nor_the_tab() {
    let (mut state, media, clip) = placed();
    state.double_click_clip(clip);
    let before = picture(&state, media);

    // Out before In.
    park(&mut state, 5);
    state.set_source_out();
    assert_eq!(
        state.status_message.as_deref(),
        Some("In must come before Out")
    );
    assert_eq!(picture(&state, media), before);
    assert_eq!(marks(&state), (Some(12), Some(48)));

    // A head past the start of the sequence: move the clip to 0 first.
    state
        .apply_command(Command::Edit(EditCommand::MoveClip {
            clip: ClipId::from_raw(clip),
            to_track: aria_models::TrackId::from_raw(
                state
                    .ui_clip_ref(ClipId::from_raw(clip))
                    .expect("clip")
                    .track_id,
            ),
            start: RationalTime::new(0, state.playhead.rate),
        }))
        .expect("move to the start");
    let before = picture(&state, media);
    park(&mut state, 2);
    state.set_source_in();
    assert_eq!(
        state.status_message.as_deref(),
        Some("the clip would start before the sequence does")
    );
    assert_eq!(picture(&state, media), before);
}

#[test]
fn a_reversed_clip_trims_its_tail_from_a_new_in() {
    let (mut state, media, clip) = placed();
    state
        .apply_command(Command::Edit(EditCommand::SetClipSpeed {
            clip: ClipId::from_raw(clip),
            speed: Rational::new(1, 1),
            reversed: true,
        }))
        .expect("reverse");
    state.double_click_clip(clip);
    park(&mut state, 20);

    state.set_source_in();

    let (_, timeline, window) = picture(&state, media);
    assert_eq!(window, (20, 48));
    assert_eq!(
        timeline,
        (24, 52),
        "played backward, source 47 stays at timeline 24 and In cuts the tail"
    );
}

#[test]
fn a_double_speed_clip_trims_half_as_many_timeline_frames() {
    let (mut state, media, clip) = placed();
    state
        .apply_command(Command::Edit(EditCommand::SetClipSpeed {
            clip: ClipId::from_raw(clip),
            speed: Rational::new(2, 1),
            reversed: false,
        }))
        .expect("double speed");
    assert_eq!(picture(&state, media).1, (24, 42));
    state.double_click_clip(clip);
    park(&mut state, 35);

    state.set_source_out();

    let (_, timeline, window) = picture(&state, media);
    assert_eq!(window, (12, 36));
    assert_eq!(timeline, (24, 36));
}

#[test]
fn a_deleted_clip_leaves_a_plain_tab_with_its_last_window() {
    let (mut state, _media, clip) = placed();
    state.double_click_clip(clip);
    park(&mut state, 20);
    state.set_source_in();
    let linked = state.sources.focus_index();

    state
        .apply_command(Command::Edit(EditCommand::RemoveClip {
            clip: ClipId::from_raw(clip),
        }))
        .expect("remove");

    let view = state.sources.focused().expect("the tab stays open");
    assert_eq!(state.sources.focus_index(), linked);
    assert_eq!(view.clip, None, "nothing left to trim");
    assert_eq!((view.mark_in, view.mark_out), (Some(20), Some(48)));
    // A mark now marks the take, as on any Source tab.
    park(&mut state, 30);
    state.set_source_in();
    assert_eq!(marks(&state), (Some(30), Some(48)));
}

#[test]
fn match_frame_opens_the_pool_tab_not_a_linked_one() {
    let (mut state, _media, _clip) = placed();
    state.playhead = RationalTime::new(30, state.playhead.rate);

    assert!(state.match_frame());

    let view = state.sources.focused().expect("a Source tab");
    assert_eq!(view.clip, None);
    assert_eq!(view.tick, 18);
    assert_eq!((view.mark_in, view.mark_out), (Some(12), Some(48)));
}

#[test]
fn a_generated_clip_opens_its_property_rows() {
    let mut state =
        AppState::new(PluginContext::new(), scratch("viewer-match-gen")).expect("state");
    state.new_project().expect("project");
    state
        .add_generated(aria_models::Generator::text("Title"))
        .expect("place a title");
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.generator.is_some())
        .map(|clip| clip.id)
        .expect("the title is on the timeline");

    state.double_click_clip(clip);

    assert!(state.expanded_clips.contains(&clip));
    assert!(state.sources.focused().is_none(), "no take to open");
}
