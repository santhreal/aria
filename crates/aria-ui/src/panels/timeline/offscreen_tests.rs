//! A frame builds the lanes and clips on screen, all of them, and nothing else.
//!
//! The class closed here is per-frame work that grows with the sequence rather
//! than with the viewport: a lane scrolled out of its group built anyway, a
//! clip outside the visible ticks handed to the lane painter, and waveform bars
//! or filmstrip cells built, and their decodes asked for, for lanes nobody can
//! see. The sequence holds more lanes than either group shows and more clips
//! per lane than the zoomed view spans, and the frame is drawn after a view
//! change routed the way the scroll listeners route one.
//!
//! Each lane a group built must overlap the group's viewport, each clip built
//! in it must overlap the ticks on screen, and every row given waveform bars or
//! filmstrip cells must be a lane the group built. Not caught: work inside the
//! lane painter for a clip it was handed, which the paint stats count.
//!
//! The converse, a clip under the lanes that no frame builds, is closed at
//! each window size the editor is laid out at: the ticks a group takes as on
//! screen must reach the right edge of the canvas it is laid out in, once the
//! window has settled after a resize and with no other input. Not caught: a
//! lane under the viewport that the vertical culling drops.
//!
//! The video lanes show a still. Each clip stacked under the playhead holds a
//! decoder of its own, so thirty stacked clips of a video file put thirty
//! decoders behind every Viewer frame and filmstrip cell, and gigabytes of
//! decoder threads and frame pools behind this file, none of which it reads.

use aria_models::{
    MediaSource, Project, Rational, RationalTime, STILL_TICK_RATE, TimeRange, TrackKind, resample,
};

use super::lanes::built;
use crate::shell::{Moved, Shell};
use crate::test_support::drawn_shell;

const LANES_PER_SIDE: usize = 30;
const CLIPS_PER_LANE: usize = 80;
const CLIP_TICKS: i64 = 12;

/// A cut with [`LANES_PER_SIDE`] video and audio lanes of [`CLIPS_PER_LANE`]
/// clips each, saved where `open_project` reads it: a still on the video
/// lanes and a sound file on the audio lanes, each clip [`CLIP_TICKS`] long.
fn crowded_cut() -> std::path::PathBuf {
    let rate = Rational::FPS_24;
    let mut project = Project::new("crowded", rate);
    let fixtures =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let still = project.add_media(MediaSource::image(fixtures.join("texture.png"), 1, 1));
    let sound = project.add_media(MediaSource::new(
        fixtures.join("fixture-640x360-24fps-3s-av.mp4"),
        640,
        360,
        rate,
        72,
        true,
    ));
    // CLIP_TICKS of the timeline in the still's own ticks, exact at 24 fps.
    let still_ticks = resample(RationalTime::new(CLIP_TICKS, rate), STILL_TICK_RATE).value;
    for kind in [TrackKind::Video, TrackKind::Audio] {
        let (media, source) = match kind {
            TrackKind::Video => (still, TimeRange::at_rate(0, still_ticks, STILL_TICK_RATE)),
            _ => (sound, TimeRange::at_rate(0, CLIP_TICKS, rate)),
        };
        for lane in 0..LANES_PER_SIDE {
            let track = project.add_track(kind, format!("{kind:?} {lane}"));
            for idx in 0..CLIPS_PER_LANE {
                let start = (idx as i64) * CLIP_TICKS;
                project
                    .add_clip(track, media, source, RationalTime::new(start, rate))
                    .unwrap_or_else(|err| panic!("clip {idx} on lane {lane}: {err}"));
            }
        }
    }
    let path = crate::scratch_pool::scratch("offscreen-cut").join("crowded.aria");
    project
        .save_to_file(&path)
        .unwrap_or_else(|err| panic!("save {}: {err}", path.display()));
    path
}

#[gpui::test]
fn a_scroll_frame_builds_no_lane_or_clip_off_screen(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "offscreen-lanes");
    visual.update(|_, app| app.set_reduce_motion(true));
    let cut = crowded_cut();
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_project(cut).expect("open the crowded cut");
            cx.notify();
        })
    });
    visual.run_until_parked();
    let total_clips = 2 * LANES_PER_SIDE * CLIPS_PER_LANE;

    // Zoomed in, a few lanes down each side, and partway along the cut.
    let shell = visual.update(|window, _| {
        window
            .root::<Shell>()
            .flatten()
            .expect("the window's root is the shell")
    });
    built::take();
    visual.update(|_, app| {
        shell.update(app, |shell, cx| {
            shell.state.update(cx, |state, _| {
                state.set_zoom(6.0, false);
                state.scroll_timeline_to(1_800.0, false);
                state.video_scroll_y = 150.0;
                state.audio_scroll_y = 150.0;
            });
            shell.redraw(Moved::View, cx);
        })
    });
    visual.run_until_parked();
    let (built_lanes, media_rows) = built::take();
    // A notification can draw the frame more than once; each lane counts once.
    let mut lanes: Vec<built::Lane> = Vec::new();
    for lane in built_lanes {
        match lanes.iter_mut().find(|seen| seen.row == lane.row) {
            Some(seen) => *seen = lane,
            None => lanes.push(lane),
        }
    }

    assert!(!lanes.is_empty(), "the scroll frame built no lane");
    let built_clips: usize = lanes.iter().map(|lane| lane.clips.len()).sum();
    assert!(
        built_clips > 0,
        "the scroll frame built no clip: the view is not over the cut"
    );
    assert!(
        lanes.len() < 2 * LANES_PER_SIDE,
        "every one of {} lanes was built: the view does not scroll any out ({:?})",
        2 * LANES_PER_SIDE,
        lanes
            .iter()
            .map(|lane| (lane.row, lane.y_top, lane.y_bottom, lane.viewport_h))
            .collect::<Vec<_>>()
    );
    assert!(
        built_clips < total_clips / 4,
        "{built_clips} of {total_clips} clips built for a view a sixth of the cut wide"
    );
    for lane in &lanes {
        assert!(
            lane.y_bottom >= 0.0 && lane.y_top <= lane.viewport_h,
            "lane {} built at {}..{} outside a viewport 0..{}",
            lane.row,
            lane.y_top,
            lane.y_bottom,
            lane.viewport_h
        );
        let (from, to) = lane.view.ticks;
        for &(start, end) in &lane.clips {
            assert!(
                end > from && start < to,
                "lane {} built the clip at {start}..{end} outside the ticks {from}..{to}",
                lane.row
            );
        }
    }
    for row in &media_rows {
        assert!(
            lanes.iter().any(|lane| lane.row == *row),
            "waveform bars or filmstrip cells were built for row {row}, which no group drew"
        );
    }
}

/// Window sizes, in pixels, that lay the timeline out narrower and wider than
/// the side panes of the editor layout leave it.
const WINDOWS: [(f32, f32); 4] = [
    (1600.0, 1000.0),
    (1300.0, 800.0),
    (1024.0, 1000.0),
    (1024.0, 640.0),
];

#[gpui::test]
fn a_settled_frame_builds_every_clip_under_the_lanes(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "onscreen-lanes");
    visual.update(|_, app| app.set_reduce_motion(true));
    let cut = crowded_cut();
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_project(cut).expect("open the crowded cut");
            cx.notify();
        })
    });
    visual.run_until_parked();

    for (window_w, window_h) in WINDOWS {
        built::take();
        built::take_canvases();
        visual.simulate_resize(gpui::size(gpui::px(window_w), gpui::px(window_h)));
        visual.run_until_parked();
        let (built_lanes, _) = built::take();
        let at = format!("{window_w}x{window_h}");
        let width = *built::take_canvases()
            .last()
            .unwrap_or_else(|| panic!("no lane group was laid out at {at}"));
        // The last frame is the settled one; each lane counts once.
        let mut lanes: Vec<built::Lane> = Vec::new();
        for lane in built_lanes {
            match lanes.iter_mut().find(|seen| seen.row == lane.row) {
                Some(seen) => *seen = lane,
                None => lanes.push(lane),
            }
        }
        assert!(!lanes.is_empty(), "no lane was built at {at}");
        for lane in &lanes {
            let built::View {
                ticks: (from, to),
                scroll_x,
                pps,
                fps,
            } = lane.view;
            let pixel = |tick: i64| tick as f32 / fps * pps - scroll_x;
            // Every clip with a pixel inside the canvas, less one for rounding.
            let under: Vec<(i64, i64)> = (0..CLIPS_PER_LANE as i64)
                .map(|idx| (idx * CLIP_TICKS, (idx + 1) * CLIP_TICKS))
                .filter(|&(start, end)| pixel(end) > 1.0 && pixel(start) < width - 1.0)
                .collect();
            let missed: Vec<&(i64, i64)> = under
                .iter()
                .filter(|clip| !lane.clips.contains(clip))
                .collect();
            assert!(
                missed.is_empty(),
                "at {at}, lane {} took ticks {from}..{to} as on screen in a canvas \
                 {width} px wide ({} px), and built none of the clips at {missed:?}",
                lane.row,
                pixel(to),
            );
        }
    }
}
