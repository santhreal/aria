//! A Source tab's own transport: its clock, and where each transport key goes.
//!
//! The class these close: a transport key pressed on a Source tab moving the
//! sequence instead, and two clocks running at once. Each key is pressed both
//! ways: with the Viewer active on a Source tab it moves the tab and leaves the
//! playhead and the sequence clock alone, and with another panel active it
//! moves the sequence as it did before Source tabs had a transport. The clock
//! is asserted on [`SourceTabs`] with explicit instants, so no assertion depends
//! on how fast the test runs.
//!
//! Not covered: the vsync loop in `start_playback`, which needs a window. Its
//! hand-over rule is covered through [`SourceTabs::drive`].

use std::time::{Duration, Instant};

use crate::app::AppState;
use crate::app::pool_preview::SourceView;
use crate::app::viewer_tabs::SourceTabs;
use crate::scratch_pool::scratch;
use crate::types::ActivePanel;
use aria_plugin::PluginContext;

/// A Source tab on a take of `frames` frames, parked on `tick`.
fn tab(frames: i64, tick: i64) -> SourceTabs {
    let mut tabs = SourceTabs::default();
    tabs.open(SourceView {
        media: 1,
        name: "take".into(),
        duration_ticks: frames,
        tick,
        mark_in: None,
        mark_out: None,
        clip: None,
    });
    tabs
}

fn tick(tabs: &SourceTabs) -> i64 {
    tabs.focused().expect("a Source tab is focused").tick
}

/// A 24 fps, three-second shot open in a focused Source tab, with the Viewer
/// the active panel: the state every transport key is pressed in.
fn viewer_on_source() -> AppState {
    let mut state =
        AppState::new(PluginContext::new(), scratch("source-transport")).expect("state");
    state.new_project().expect("project");
    let video = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4");
    let media = state.import_media(video).expect("import").raw();
    assert!(state.open_source(media), "the Source tab opens");
    state.active_panel = ActivePanel::Preview;
    assert!(state.source_has_keys());
    state
}

fn source_tick(state: &AppState) -> i64 {
    tick(&state.sources)
}

fn last_frame(state: &AppState) -> i64 {
    state.sources.focused().expect("focused").duration_ticks - 1
}

/// Far enough past any start that every run has reached its end.
fn later() -> Instant {
    Instant::now() + Duration::from_secs(3600)
}

// --- the clock ----------------------------------------------------------------

#[test]
fn the_clock_moves_the_frame_by_the_take_rate_and_the_speed() {
    let start = Instant::now();
    for (rate, after_ms, from, want) in [
        (1, 500, 10, 22),
        (2, 500, 10, 34),
        (8, 250, 10, 58),
        (-1, 250, 30, 24),
        (-4, 250, 30, 6),
    ] {
        let mut tabs = tab(240, from);
        assert!(tabs.play(rate, None, start));
        assert!(tabs.advance(24.0, start + Duration::from_millis(after_ms)));
        assert_eq!(tick(&tabs), want, "{rate}x for {after_ms} ms from {from}");
    }
}

#[test]
fn forward_play_stops_on_the_last_frame_it_may_show() {
    let start = Instant::now();
    for (until, want) in [(None, 99), (Some(40), 39), (Some(400), 99)] {
        let mut tabs = tab(100, 10);
        tabs.play(1, until, start);
        assert!(
            !tabs.advance(24.0, start + Duration::from_secs(60)),
            "{until:?}"
        );
        assert_eq!(tick(&tabs), want, "{until:?}");
        assert_eq!(tabs.play_rate(), None, "the clock stops at {until:?}");
    }
}

#[test]
fn an_until_behind_the_frame_plays_to_the_end() {
    let start = Instant::now();
    let mut tabs = tab(100, 50);
    tabs.play(1, Some(20), start);
    assert!(!tabs.advance(24.0, start + Duration::from_secs(60)));
    assert_eq!(tick(&tabs), 99);
}

#[test]
fn reverse_play_stops_on_the_first_frame() {
    let start = Instant::now();
    let mut tabs = tab(100, 10);
    tabs.play(-8, None, start);
    assert!(!tabs.advance(24.0, start + Duration::from_secs(60)));
    assert_eq!(tick(&tabs), 0);
    assert_eq!(tabs.play_rate(), None);
}

#[test]
fn a_zero_rate_or_program_in_front_does_not_play() {
    let now = Instant::now();
    let mut tabs = tab(100, 10);
    assert!(!tabs.play(0, None, now));
    assert_eq!(tabs.play_rate(), None);
    tabs.focus_program();
    assert!(!tabs.play(1, None, now));
    assert!(!tabs.advance(24.0, now + Duration::from_secs(1)));
}

#[test]
fn changing_tabs_stops_the_clock() {
    let now = Instant::now();
    let mut tabs = tab(100, 10);
    tabs.play(1, None, now);
    tabs.focus_program();
    assert_eq!(tabs.play_rate(), None, "Program in front");
    tabs.focus(0);
    tabs.play(1, None, now);
    tabs.close(0);
    assert_eq!(tabs.play_rate(), None, "the playing tab closed");
}

#[test]
fn one_loop_drives_a_playing_tab() {
    let now = Instant::now();
    let mut tabs = tab(100, 10);
    assert_eq!(tabs.drive(), None, "nothing plays, so no loop starts");
    tabs.play(1, None, now);
    let first = tabs.drive().expect("a loop starts");
    assert!(tabs.driven_by(first));
    let second = tabs.drive().expect("a second start hands over");
    assert!(!tabs.driven_by(first), "the first loop ends");
    assert!(tabs.driven_by(second));
    tabs.stop();
    assert!(!tabs.driven_by(second), "a stopped tab has no loop");
}

// --- where the keys go --------------------------------------------------------

#[test]
fn space_on_a_source_tab_runs_its_clock_and_not_the_sequence() {
    let mut state = viewer_on_source();
    let playhead = state.playhead.value;
    state.space();
    assert_eq!(state.sources.play_rate(), Some(1));
    assert!(!state.playing, "the sequence stays stopped");
    assert!(state.viewer_playing());
    assert!(!state.advance_source_playback(later()));
    assert_eq!(source_tick(&state), last_frame(&state));
    assert_eq!(state.playhead.value, playhead, "the playhead stays put");
    state.space();
    assert_eq!(state.sources.play_rate(), Some(1));
    assert_eq!(
        source_tick(&state),
        0,
        "play from the last frame starts over"
    );
    state.space();
    assert_eq!(state.sources.play_rate(), None, "Space stops");
}

#[test]
fn space_with_another_panel_active_plays_the_sequence() {
    let mut state = viewer_on_source();
    state.active_panel = ActivePanel::Timeline;
    state.space();
    assert!(state.playing);
    assert_eq!(state.sources.play_rate(), None);
    assert!(
        state.sources.focused().is_none(),
        "Program comes to the front"
    );
}

#[test]
fn one_clock_runs_at_a_time() {
    let mut state = viewer_on_source();
    state.toggle_playback();
    assert!(state.playing);
    state.space();
    assert!(!state.playing, "the source takes the clock");
    assert_eq!(state.sources.play_rate(), Some(1));
    state.toggle_playback();
    assert!(state.playing);
    assert!(
        !state.advance_source_playback(Instant::now()),
        "the sequence takes the clock back"
    );
    assert_eq!(state.sources.play_rate(), None);
}

#[test]
fn shift_space_plays_the_marked_range_on_the_source() {
    let mut state = viewer_on_source();
    let view = state.sources.focused_mut().expect("focused");
    view.mark_in = Some(12);
    view.mark_out = Some(30);
    view.tick = 50;
    state.shift_space();
    assert!(!state.playing);
    assert_eq!(source_tick(&state), 12, "play starts at In");
    assert!(!state.advance_source_playback(later()));
    assert_eq!(
        source_tick(&state),
        29,
        "and stops on the last frame Out keeps"
    );
}

#[test]
fn j_and_l_shuttle_the_source_and_k_stops_it() {
    let mut state = viewer_on_source();
    let mut rates = Vec::new();
    // L five times, then J twice.
    for dir in [1, 1, 1, 1, 1, -1, -1] {
        state.shuttle_key_down(dir, false);
        rates.push(state.sources.play_rate());
    }
    let want = [1, 2, 4, 8, 8, -1, -2].map(Some);
    assert_eq!(rates, want);
    assert!(!state.playing);
    state.shuttle_key_down(1, true);
    assert_eq!(
        state.sources.play_rate(),
        Some(-2),
        "a key repeat is not a press"
    );
    state.k_down(false);
    assert_eq!(state.sources.play_rate(), None);
}

#[test]
fn k_held_with_j_or_l_steps_the_source_a_frame() {
    let mut state = viewer_on_source();
    state.step_here(10);
    state.k_down(false);
    state.shuttle_key_down(1, false);
    assert_eq!(source_tick(&state), 11);
    state.shuttle_key_down(-1, false);
    state.shuttle_key_down(-1, false);
    assert_eq!(source_tick(&state), 9);
    assert_eq!(state.sources.play_rate(), None, "a step does not play");
    state.k_up();
    state.shuttle_key_down(1, false);
    assert_eq!(state.sources.play_rate(), Some(1), "K released: L plays");
}

#[test]
fn stepping_and_jumping_move_the_source_and_stop_it() {
    let mut state = viewer_on_source();
    let playhead = state.playhead.value;
    let last = last_frame(&state);
    state.step_here(5);
    assert_eq!(source_tick(&state), 5);
    state.step_here(-1);
    assert_eq!(source_tick(&state), 4);
    state.viewer_arrow(10.0, 0.0).expect("arrow");
    assert_eq!(source_tick(&state), 9, "Shift+Right steps five frames");
    state.space();
    state.step_here(-100);
    assert_eq!(source_tick(&state), 0, "a step clamps to the take");
    assert_eq!(state.sources.play_rate(), None, "a step stops the clock");
    state.go_to_end_here();
    assert_eq!(source_tick(&state), last);
    state.go_to_start_here();
    assert_eq!(source_tick(&state), 0);
    assert_eq!(state.playhead.value, playhead, "the playhead stays put");
}

#[test]
fn go_to_in_and_out_reach_the_source_marks() {
    let mut state = viewer_on_source();
    state.go_to_out();
    assert_eq!(
        source_tick(&state),
        last_frame(&state),
        "no Out: the last frame"
    );
    state.go_to_in();
    assert_eq!(source_tick(&state), 0, "no In: the first frame");
    let view = state.sources.focused_mut().expect("focused");
    view.mark_in = Some(7);
    view.mark_out = Some(21);
    state.go_to_in();
    assert_eq!(source_tick(&state), 7);
    state.go_to_out();
    assert_eq!(source_tick(&state), 20, "the last frame Out keeps");
}

#[test]
fn the_same_keys_on_the_timeline_move_the_playhead() {
    let mut state = viewer_on_source();
    state
        .cut_source_in(crate::types::EditMode::Overwrite)
        .expect("the take lands in the sequence");
    state.active_panel = ActivePanel::Timeline;
    state.set_playhead(10);
    state.step_here(3);
    assert_eq!(state.playhead.value, 13);
    assert_eq!(source_tick(&state), 0, "the source stays put");
    state.go_to_start_here();
    assert_eq!(state.playhead.value, 0);
    state.shuttle_key_down(1, false);
    assert_eq!(state.sources.play_rate(), None);
    assert!(state.playing, "L plays the sequence");
}
