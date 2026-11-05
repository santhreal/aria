//! A press on a transition band opens the tab that edits the transition, with
//! the Transition block in view.
//!
//! Closes the class "a press on a transition shows something else": a press on
//! the band selected the clip on the left of the cut and left Effect Controls
//! on the tab it was on, Transform, so the transition just pressed had no
//! control on screen, and the tab that holds the block drew it past the end of
//! the body. Each transition media is pressed where the timeline draws its
//! band, through the window's own events: a blend on V1 opens Look and a
//! crossfade on A1 opens Audio. The tab drawn at rest first puts the block
//! outside the body, so the reveal is what brings it in, and the frames the
//! press requests are delivered until none is left. Not covered: a press on
//! the band's edges, which start a length drag.

use std::time::Duration;

use aria_models::{ClipId, TransitionMedia};
use gpui::{Bounds, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, point, px};

use crate::app::AppState;
use crate::app::clip_transitions::default_transition;
use crate::app::fixtures::asset;
use crate::app::premiere_tests::place_media;
use crate::transport::TIMELINE_BASE_PPS;

/// Where a junction of one media is built and pressed, and the tab the press
/// opens.
struct Case {
    asset: &'static str,
    canvas: &'static str,
    header: &'static str,
    tab: &'static str,
}

fn case(media: TransitionMedia) -> Case {
    match media {
        TransitionMedia::Picture => Case {
            asset: "texture.png",
            canvas: "timeline-video-canvas",
            header: "track-header-V1",
            tab: "look",
        },
        TransitionMedia::Audio => Case {
            asset: "fixture-sine-48k-2s.wav",
            canvas: "timeline-audio-canvas",
            header: "track-header-A1",
            tab: "audio",
        },
    }
}

/// Place the fixture `file`, cut it in two at its middle, and put the default
/// transition of `media` at the cut. Returns the clip on the left of the cut,
/// which holds the transition.
fn cut_under(state: &mut AppState, file: &str, media: TransitionMedia) -> ClipId {
    let whole = place_media(state, asset(file));
    let (lane, start, end) = {
        let ui = state
            .ui_clip_ref(whole)
            .expect("the placed clip is projected");
        (ui.track_id, ui.timeline_start_ticks, ui.timeline_end_ticks)
    };
    state.set_playhead((start + end) / 2);
    state.split_at_playhead().expect("cut the clip in two");
    let left = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.id == lane)
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.timeline_start_ticks == start)
        .map(|clip| ClipId::from_raw(clip.id))
        .expect("the left half starts where the clip did");
    let id = default_transition(media).expect("a default transition for the media");
    state
        .set_clip_transition(left, &id)
        .expect("the cut takes the transition");
    assert_eq!(
        state.clip_transition_id(left),
        Some(id),
        "the cut holds no {media:?} transition"
    );
    left
}

/// The window bounds of the inspector's body and of the Transition block.
fn body_and_block(visual: &mut VisualTestContext) -> (Bounds<Pixels>, Bounds<Pixels>) {
    let body = visual
        .debug_bounds("inspector-body")
        .expect("Effect Controls draws its body");
    let block = visual
        .debug_bounds("insp-transition")
        .expect("Effect Controls draws no Transition block");
    (body, block)
}

fn wholly_in(body: Bounds<Pixels>, block: Bounds<Pixels>) -> bool {
    body.contains(&block.origin) && body.contains(&block.bottom_right())
}

/// One frame at 60 Hz.
const FRAME: Duration = Duration::from_millis(16);

/// Frames a press may keep drawing: about a second, past the end of every
/// one-shot motion in the chrome. A press still drawing then is a draw loop.
const SETTLE_FRAMES: usize = 64;

/// Press the middle of the band `left`'s transition draws on the lane at the
/// height of `header` on `canvas`, then deliver the frames the press requests
/// until none is left, as the platform's frame loop does: one 60 Hz interval
/// of clock per frame, so a hover or glide the press starts runs out.
fn press_band(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    left: ClipId,
    case: &Case,
) {
    let canvas = visual
        .debug_bounds(case.canvas)
        .unwrap_or_else(|| panic!("{} is drawn", case.canvas));
    let header = visual
        .debug_bounds(case.header)
        .unwrap_or_else(|| panic!("{} is drawn", case.header));
    let x = visual.read(|app| {
        let state = state.read(app);
        let ui = state.ui_clip_ref(left).expect("the left clip is projected");
        let transition = ui.transition.as_ref().expect("the left clip holds it");
        let (low, high) = transition.window(ui.timeline_end_ticks);
        let tick_px = TIMELINE_BASE_PPS * state.zoom / state.store.sequence.ticks_per_second();
        assert!(
            (high - low) as f32 * tick_px >= 24.0,
            "the band is {} px wide, too narrow to press its middle",
            (high - low) as f32 * tick_px
        );
        f32::from(canvas.left()) - state.scroll_x + (low + high) as f32 / 2.0 * tick_px
    });
    let at = point(px(x), header.center().y);
    visual.simulate_mouse_move(at, None, Modifiers::none());
    visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    for _ in 0..SETTLE_FRAMES {
        visual.executor().advance_clock(FRAME);
        let ran = visual.update(|window, app| window.simulate_next_frame(app));
        visual.run_until_parked();
        if ran == 0 {
            return;
        }
    }
    panic!("the window still requests a frame {SETTLE_FRAMES} frames after the press");
}

/// Build a cut of `media` under its default transition, draw its tab at rest,
/// then press the band with Transform in front and nothing selected.
fn pressed(cx: &mut TestAppContext, media: TransitionMedia) {
    cx.executor().allow_parking();
    let case = case(media);
    let tag = format!("transition-reveal-{}", case.tab);
    let (state, visual) = crate::test_support::drawn_shell(cx, &tag);
    let left = visual.update(|window, app| {
        app.set_reduce_motion(true);
        let left = state.update(app, |state, _| {
            let left = cut_under(state, case.asset, media);
            state.zoom_fit_sequence();
            state.dock_tree.open(crate::dock::PANEL_INSPECTOR);
            state.select_clip(left);
            state.inspector_tab = case.tab.to_owned();
            left
        });
        window.refresh();
        left
    });
    visual.run_until_parked();
    crate::test_support::wait_out_splash(visual);
    let (body, block) = body_and_block(visual);
    assert!(
        !wholly_in(body, block),
        "the {} tab at rest draws the block at {block:?} inside the body {body:?}, \
         so the press has nothing to bring into view",
        case.tab
    );

    visual.update(|window, app| {
        state.update(app, |state, _| {
            state.clear_selection();
            state.inspector_tab = crate::app::DEFAULT_INSPECTOR_TAB.to_owned();
        });
        window.refresh();
    });
    visual.run_until_parked();
    press_band(visual, &state, left, &case);

    let (selection, tab) = visual.read(|app| {
        let state = state.read(app);
        (state.selection.clone(), state.inspector_tab.clone())
    });
    assert_eq!(
        selection,
        vec![left],
        "a press on the {media:?} band selects"
    );
    assert_eq!(tab, case.tab, "a press on the {media:?} band opens");
    let (body, block) = body_and_block(visual);
    assert!(
        wholly_in(body, block),
        "after a press on the {media:?} band the block is drawn at {block:?}, \
         outside the body {body:?}"
    );
}

#[gpui::test]
fn a_press_on_a_blend_opens_look_on_the_transition(cx: &mut TestAppContext) {
    pressed(cx, TransitionMedia::Picture);
}

#[gpui::test]
fn a_press_on_a_crossfade_opens_audio_on_the_transition(cx: &mut TestAppContext) {
    pressed(cx, TransitionMedia::Audio);
}
