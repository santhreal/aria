//! The Audio workspace shows every mixer strip whole at the windows the
//! editor opens in.
//!
//! The class closed here: a workspace whose split weights leave the panel it
//! is built around too short for its content in the window the editor opens
//! in, so the panel scrolls before the first edit. The Audio workspace is
//! applied in the window [`UiSettings::resolve_window_placement`] opens on a
//! 1920 x 1080 display when it is not maximized, and in the whole display,
//! with an audio lane added beside the new project's video lane. The Audio
//! Track Mixer's strips must end above the bottom of the frame that holds
//! them.
//!
//! Not covered: the panels of the other workspaces, which scroll by design; a
//! strip taller than a new lane's, such as one with sends or a long insert
//! chain; a window smaller than the one the editor opens in, where the strips
//! scroll; and the glyphs, which the test text system does not raster.

use gpui::{Bounds, TestAppContext, point, px, size};

use super::tab_overflow_tests::{frame_bounds, redraw};
use crate::dock::PANEL_MIXER;
use crate::settings::UiSettings;
use crate::test_support::drawn_shell_at;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;
const DISPLAY: [f32; 4] = [0.0, 0.0, 1920.0, 1080.0];

#[gpui::test]
fn the_audio_workspace_shows_every_mixer_strip_whole_in_the_opening_window(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let opening = UiSettings::default().resolve_window_placement(&[DISPLAY]);
    for (width, height) in [(opening.width, opening.height), (DISPLAY[2], DISPLAY[3])] {
        let at = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
        let tag = format!("audio-workspace-fit-{width}x{height}");
        let (state, mut visual) = drawn_shell_at(cx, &tag, at);
        let visual = &mut visual;
        // A split's weights glide; reduced motion lands them on the next draw.
        visual.update(|_, app| app.set_reduce_motion(true));
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state
                    .add_lane_of_kind(aria_models::TrackKind::Audio, None)
                    .unwrap_or_else(|err| panic!("an audio lane: {err:?}"));
                assert!(state.apply_workspace("win-workspace-audio", cx));
            })
        });
        redraw(&state, visual);
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let holder = visual
            .read(|app| state.read(app).dock_tree.find(PANEL_MIXER))
            .expect("the Audio workspace docks the Audio Track Mixer");
        let frame = frame_bounds(&state, visual, holder);
        let strips = visual
            .debug_bounds("mixer-strips")
            .unwrap_or_else(|| panic!("the mixer drew no strips at {width} x {height}"));
        assert!(
            f32::from(strips.bottom()) <= f32::from(frame.bottom()) + EPS,
            "at {width} x {height} the mixer strips end at {:?}, past their frame's bottom \
             {:?}: the frame is {:?} tall and the strips need {:?} from its top",
            strips.bottom(),
            frame.bottom(),
            frame.size.height,
            strips.bottom() - frame.origin.y,
        );
    }
}
