//! Contracts for per-lane display options: height, waveform, filmstrip.
//!
//! The class closed here is "a view option is kept in the session, so reopening
//! the project throws the timeline layout away". Every assertion goes
//! through the projected store — the panel draws from the projection — and the
//! persistence case round-trips a save and an open, because that is the failure
//! the class is about.
//!
//! Not covered: the drawn grip and the pointer arithmetic of the drag itself,
//! which need a render surface. The preview/commit split is covered through
//! `set_lane_preview` / `take_lane_preview`, which is the state the drag drives.

use aria_models::TrackDisplay;

use super::AppState;
use super::premiere_tests::with_clip;
use crate::types::UiTrack;

fn lane(state: &AppState, id: u64) -> &UiTrack {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == id)
        .expect("lane still projected")
}

/// The lane the fixture clip sits on.
fn fixture_lane(state: &AppState) -> u64 {
    state.store.sequence.tracks[0].id
}

#[test]
fn a_new_lane_ships_with_the_default_display() {
    let (state, _clip) = with_clip();
    let track = fixture_lane(&state);
    let display = state.lane_display(track);
    assert_eq!(display.height, TrackDisplay::DEFAULT_HEIGHT);
    assert!(display.waveform, "waveforms are on until turned off");
    assert!(display.filmstrip, "filmstrips are on until turned off");
    assert_eq!(
        lane(&state, track).height,
        TrackDisplay::DEFAULT_HEIGHT as f32
    );
}

#[test]
fn a_height_change_is_projected_and_undoable() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    state.set_lane_height(track, 96.0).expect("resize");
    assert_eq!(lane(&state, track).height, 96.0);
    assert!(state.undo(), "a resize is an edit, so it undoes");
    assert_eq!(
        lane(&state, track).height,
        TrackDisplay::DEFAULT_HEIGHT as f32
    );
    assert!(state.redo());
    assert_eq!(lane(&state, track).height, 96.0);
}

#[test]
fn a_height_outside_the_range_is_clamped_rather_than_refused() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    state
        .set_lane_height(track, 4.0)
        .expect("a tiny drag lands");
    assert_eq!(
        lane(&state, track).height,
        *TrackDisplay::HEIGHT.start() as f32
    );
    state
        .set_lane_height(track, 4000.0)
        .expect("a huge drag lands");
    assert_eq!(
        lane(&state, track).height,
        *TrackDisplay::HEIGHT.end() as f32
    );
}

#[test]
fn one_flag_change_leaves_the_others_alone() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    state.set_lane_height(track, 72.0).expect("resize");
    state.set_lane_waveform(track, false).expect("hide waves");
    let display = state.lane_display(track);
    assert_eq!(display.height, 72, "a flag toggle must not resize the lane");
    assert!(!display.waveform);
    assert!(display.filmstrip, "the other flag is untouched");
    state.set_lane_filmstrip(track, false).expect("hide strips");
    assert!(!state.lane_display(track).filmstrip);
    assert!(!state.lane_display(track).waveform, "still off");
    assert_eq!(state.lane_display(track).height, 72);
}

#[test]
fn a_locked_lane_still_takes_a_display_change() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    state
        .set_track_locked(aria_models::TrackId::from_raw(track), true)
        .expect("lock");
    state
        .set_lane_height(track, 120.0)
        .expect("how a lane is drawn is not an edit to its clips");
    assert_eq!(lane(&state, track).height, 120.0);
}

#[test]
fn the_layout_survives_save_and_open() {
    let dir = crate::scratch_pool::scratch("lane-display");
    let path = dir.join("layout.aria");
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    state.set_lane_height(track, 88.0).expect("resize");
    state.set_lane_filmstrip(track, false).expect("hide strips");
    state.project_path = Some(path.clone());
    state.save_project().expect("save");

    state.new_project().expect("new");
    state.open_project(path).expect("open");
    let track = fixture_lane(&state);
    let display = state.lane_display(track);
    assert_eq!(
        display.height, 88,
        "the lane height is part of the sequence"
    );
    assert!(!display.filmstrip, "so is the strip choice");
    assert!(display.waveform);
}

#[test]
fn a_resize_in_flight_is_a_preview_and_costs_one_undo_entry() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    let depth = state.history_depth();
    for y in [60.0, 72.0, 84.0] {
        state.set_lane_preview(track, y);
    }
    assert_eq!(state.lane_preview(track), Some(84.0));
    assert_eq!(state.lane_preview(track + 1), None, "one lane at a time");
    assert_eq!(
        state.history_depth(),
        depth,
        "a preview must not reach the engine"
    );
    let height = state.take_lane_preview().expect("the drag ended somewhere");
    state.set_lane_height(track, height).expect("commit");
    assert_eq!(state.lane_preview(track), None);
    assert_eq!(lane(&state, track).height, 84.0);
    assert_eq!(
        state.history_depth(),
        depth + 1,
        "the whole gesture is one edit"
    );
}

#[test]
fn an_abandoned_preview_changes_nothing() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    state.set_lane_preview(track, 150.0);
    assert!(state.take_lane_preview().is_some());
    assert_eq!(
        lane(&state, track).height,
        TrackDisplay::DEFAULT_HEIGHT as f32,
        "escaping a resize leaves the lane as it was"
    );
}

#[test]
fn a_preview_is_clamped_to_the_accepted_range() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    state.set_lane_preview(track, -40.0);
    assert_eq!(
        state.lane_preview(track),
        Some(*TrackDisplay::HEIGHT.start() as f32),
        "the preview never shows a height the engine would refuse"
    );
    state.set_lane_preview(track, 9_000.0);
    assert_eq!(
        state.lane_preview(track),
        Some(*TrackDisplay::HEIGHT.end() as f32)
    );
}

#[test]
fn collapsing_keeps_the_height_it_will_expand_back_to() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    state.set_lane_height(track, 110.0).expect("resize");
    state.toggle_lane_collapsed(track).expect("collapse");
    let display = state.lane_display(track);
    assert!(display.collapsed);
    assert_eq!(display.height, 110, "a collapsed lane keeps its set height");
    state.toggle_lane_collapsed(track).expect("expand");
    let display = state.lane_display(track);
    assert!(!display.collapsed);
    assert_eq!(display.height, 110, "expanding restores that height");
}

#[test]
fn a_collapse_is_projected_undoable_and_persisted() {
    let dir = crate::scratch_pool::scratch("lane-collapse");
    let path = dir.join("collapsed.aria");
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);
    state.set_lane_collapsed(track, true).expect("collapse");
    assert!(lane(&state, track).collapsed, "the panel draws from this");
    assert!(state.undo());
    assert!(!lane(&state, track).collapsed);
    assert!(state.redo());

    state.project_path = Some(path.clone());
    state.save_project().expect("save");
    state.new_project().expect("new");
    state.open_project(path).expect("open");
    let track = fixture_lane(&state);
    assert!(
        state.lane_display(track).collapsed,
        "a collapsed lane comes back collapsed"
    );
}

#[test]
fn collapsing_one_lane_leaves_the_others_expanded() {
    let (mut state, _clip) = with_clip();
    state
        .add_track(aria_models::TrackKind::Video, "V2")
        .expect("a second lane");
    let rows: Vec<u64> = state
        .store
        .sequence
        .tracks
        .iter()
        .map(|lane| lane.id)
        .collect();
    assert!(rows.len() >= 2, "the fixture has two lanes to tell apart");
    state.set_lane_collapsed(rows[0], true).expect("collapse");
    assert!(lane(&state, rows[0]).collapsed);
    for other in &rows[1..] {
        assert!(
            !lane(&state, *other).collapsed,
            "lane {other} was collapsed by proxy"
        );
    }
}

#[test]
fn stepping_a_lane_through_height_presets() {
    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);

    // Start at Mini (20) and walk up through every preset in TrackDisplay::PRESETS.
    state
        .set_lane_height(track, TrackDisplay::PRESETS[0].1 as f32)
        .expect("set to Mini");
    for window in TrackDisplay::PRESETS.windows(2) {
        let (from_name, from_h) = window[0];
        let (to_name, to_h) = window[1];
        assert_eq!(state.lane_display(track).height, from_h, "at {from_name}");
        let depth = state.history_depth();
        state
            .step_lane_height_preset(track, true)
            .expect("step taller");
        assert_eq!(
            state.lane_display(track).height,
            to_h,
            "stepping up from {from_name} ({from_h}) should land on {to_name} ({to_h})"
        );
        assert_eq!(
            state.history_depth(),
            depth + 1,
            "one undo step per preset notch"
        );
    }

    // At Large (72), stepping taller clamps without adding an undo entry.
    let depth = state.history_depth();
    state
        .step_lane_height_preset(track, true)
        .expect("step taller at Large");
    assert_eq!(state.lane_display(track).height, 72);
    assert_eq!(state.history_depth(), depth);

    // Walk down from Large (72) through every preset to Mini (20).
    for window in TrackDisplay::PRESETS.windows(2).rev() {
        let (to_name, to_h) = window[0];
        let (from_name, from_h) = window[1];
        assert_eq!(state.lane_display(track).height, from_h, "at {from_name}");
        let depth = state.history_depth();
        state
            .step_lane_height_preset(track, false)
            .expect("step shorter");
        assert_eq!(
            state.lane_display(track).height,
            to_h,
            "stepping down from {from_name} ({from_h}) should land on {to_name} ({to_h})"
        );
        assert_eq!(state.history_depth(), depth + 1);
    }

    // At Mini (20), stepping shorter clamps without adding an undo entry.
    let depth = state.history_depth();
    state
        .step_lane_height_preset(track, false)
        .expect("step shorter at Mini");
    assert_eq!(state.lane_display(track).height, 20);
    assert_eq!(state.history_depth(), depth);

    // Intermediate heights between presets step to the nearest preset in the scroll direction.
    state.set_lane_height(track, 25.0).expect("set 25");
    state
        .step_lane_height_preset(track, true)
        .expect("25 up -> 32");
    assert_eq!(state.lane_display(track).height, 32);

    state.set_lane_height(track, 25.0).expect("set 25");
    state
        .step_lane_height_preset(track, false)
        .expect("25 down -> 20");
    assert_eq!(state.lane_display(track).height, 20);

    // Manually dragged above Large (e.g. 100): scrolling up leaves 100 unchanged;
    // scrolling down steps to Large (72).
    state.set_lane_height(track, 100.0).expect("set 100");
    let depth = state.history_depth();
    state
        .step_lane_height_preset(track, true)
        .expect("100 up stays 100");
    assert_eq!(state.lane_display(track).height, 100);
    assert_eq!(state.history_depth(), depth);

    state
        .step_lane_height_preset(track, false)
        .expect("100 down -> 72");
    assert_eq!(state.lane_display(track).height, 72);
}

#[test]
fn the_track_context_menu_offers_every_height_preset_and_marks_the_active_one() {
    use crate::surfaces::context_menu::{MenuItem, TRACK_ITEMS, TrackMenu};

    let (mut state, _clip) = with_clip();
    let track = fixture_lane(&state);

    let preset_rows: Vec<_> = TRACK_ITEMS
        .iter()
        .filter_map(MenuItem::row)
        .filter(|row| {
            let text = (row.label)(&TrackMenu::of(&state, track).expect("track menu"));
            text.contains("Lane Height:")
        })
        .collect();
    assert_eq!(
        preset_rows.len(),
        TrackDisplay::PRESETS.len(),
        "track context menu must offer one row per TrackDisplay::PRESETS entry"
    );

    for (row, &(name, height)) in preset_rows.iter().zip(TrackDisplay::PRESETS.iter()) {
        let expected_id = format!("ctx-lane-{}", name.to_ascii_lowercase());
        assert_eq!(row.id, expected_id);

        let facts = TrackMenu::of(&state, track).expect("track menu");
        (row.run)(&mut state, &facts).expect("apply preset from context menu");
        assert_eq!(
            state.lane_display(track).height,
            height,
            "clicking {} should set height to {height}",
            row.id
        );

        let facts = TrackMenu::of(&state, track).expect("track menu at preset");
        for (other_row, &(other_name, _)) in preset_rows.iter().zip(TrackDisplay::PRESETS.iter()) {
            assert_eq!(
                (other_row.label)(&facts),
                format!("Lane Height: {other_name}"),
                "a preset's label must read the same whether or not it is active"
            );
            assert_eq!(
                other_row.is_checked(&facts),
                other_name == name,
                "with {name} active, {other_name} drew the wrong check mark"
            );
        }
    }

    // A custom dragged height checks none of the presets.
    state.set_lane_height(track, 60.0).expect("set custom 60");
    let facts = TrackMenu::of(&state, track).expect("track menu at custom height");
    for (row, &(name, _)) in preset_rows.iter().zip(TrackDisplay::PRESETS.iter()) {
        assert_eq!((row.label)(&facts), format!("Lane Height: {name}"));
        assert!(!row.is_checked(&facts), "a 60 px lane checked {name}");
    }
}

#[gpui::test]
fn shift_scrolling_a_track_header_steps_its_preset_without_scrolling_the_timeline(
    cx: &mut gpui::TestAppContext,
) {
    use gpui::{Modifiers, ScrollDelta, ScrollWheelEvent, TouchPhase, point, px};

    let (state, visual) = crate::test_support::drawn_shell(cx, "lane-shift-scroll");
    visual.update(|_, app| {
        app.set_reduce_motion(true);
        state.update(app, |s, _| {
            s.add_track(aria_models::TrackKind::Audio, "A1")
                .expect("add audio track");
        });
    });
    visual.run_until_parked();

    let (v1, a1) = visual.read(|app| {
        let s = state.read(app);
        let v1 = s
            .store
            .sequence
            .tracks
            .iter()
            .find(|t| t.kind == crate::types::UiTrackKind::Video)
            .expect("video track")
            .id;
        let a1 = s
            .store
            .sequence
            .tracks
            .iter()
            .find(|t| t.kind == crate::types::UiTrackKind::Audio)
            .expect("audio track")
            .id;
        (v1, a1)
    });

    let header_center = visual
        .debug_bounds("track-header-V1")
        .expect("v1 track header is drawn")
        .center();

    let scroll_at =
        |visual: &mut gpui::VisualTestContext, pos: gpui::Point<gpui::Pixels>, dy: f32| {
            visual.simulate_mouse_move(pos, None, Modifiers::shift());
            visual.simulate_event(ScrollWheelEvent {
                position: pos,
                delta: ScrollDelta::Lines(point(0.0, dy)),
                modifiers: Modifiers::shift(),
                touch_phase: TouchPhase::Moved,
            });
            visual.run_until_parked();
        };

    // V1 starts at Medium (48); Shift+scroll down twice steps V1 to Small (32) then Mini (20),
    // without scrolling the timeline horizontally or changing A1's height.
    scroll_at(visual, header_center, -1.0);
    assert_eq!(
        visual.read(|app| state.read(app).lane_display(v1).height),
        32,
        "first Shift+scroll down on V1 header steps 48 -> 32"
    );
    assert_eq!(
        visual.read(|app| state.read(app).lane_display(a1).height),
        48,
        "neighbor track A1 height is untouched"
    );
    assert_eq!(
        visual.read(|app| state.read(app).scroll_x),
        0.0,
        "Shift+scroll on track header must not scroll timeline horizontally"
    );

    let header_center = visual
        .debug_bounds("track-header-V1")
        .expect("v1 track header is drawn")
        .center();
    scroll_at(visual, header_center, -1.0);
    assert_eq!(
        visual.read(|app| state.read(app).lane_display(v1).height),
        20,
        "second Shift+scroll down on V1 header steps 32 -> 20"
    );
    let v1_bounds = visual
        .debug_bounds("track-header-V1")
        .expect("v1 track header is drawn at Mini");
    assert_eq!(
        v1_bounds.size.height,
        px(20.0),
        "Mini lane draws at 20 px height without being floored at 28 px"
    );

    // Shift+scroll up three times steps V1 through Small (32) -> Medium (48) -> Large (72).
    for expected in [32, 48, 72] {
        let pos = visual
            .debug_bounds("track-header-V1")
            .expect("v1 track header is drawn")
            .center();
        scroll_at(visual, pos, 1.0);
        assert_eq!(
            visual.read(|app| state.read(app).lane_display(v1).height),
            expected
        );
    }
    assert_eq!(visual.read(|app| state.read(app).scroll_x), 0.0);
    assert_eq!(
        visual.read(|app| state.read(app).lane_display(a1).height),
        48
    );

    // Shift+scroll down over the time area scrolls one line later without
    // changing track heights.
    let canvas_center = visual
        .debug_bounds("timeline-video-canvas")
        .expect("timeline video canvas is drawn")
        .center();
    scroll_at(visual, canvas_center, -1.0);
    assert_eq!(
        visual.read(|app| state.read(app).scroll_x),
        crate::transport::WHEEL_LINE_PX,
        "Shift+scroll down over the time area scrolls one line later"
    );
    assert_eq!(
        visual.read(|app| state.read(app).lane_display(v1).height),
        72
    );
    assert_eq!(
        visual.read(|app| state.read(app).lane_display(a1).height),
        48
    );
}
