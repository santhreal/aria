//! The Timeline's ruler and each lane side fade the edges their content
//! continues past, as deep as the content past that edge up to their band,
//! and no other edge.
//!
//! The class closed here is a view that scrolls by its own model and either
//! cuts its content at an edge with nothing to show that more lies past it, or
//! cuts a few pixels of it and fades a full band over the rest, the clip names
//! and ruler labels under the band included. Video lanes stand on the A/V
//! divider, so a short video side cuts its top lane under the ruler; a ruler
//! scrolled a few pixels into the sequence cuts its first label. Each view is
//! drawn with content that fits, and with content that overflows it at rest,
//! scrolled a few pixels, halfway, a few pixels short of its far end, and to
//! its far end, and the fade over the view is read at each: an edge fades as
//! deep as the content continues past it, up to the band, and a view whose
//! content fits fades none. An edge across the scroll axis never fades.
//!
//! Not caught: the fade's pixels, which the test platform does not raster on
//! Linux, and a view during a glide, drawn here with motion reduced.

use aria_models::TrackKind;
use gpui::{Edges, Entity, Pixels, VisualTestContext};

use crate::app::AppState;
use crate::edge_fade::{SCROLL_FADE, take_painted};
use crate::interned::intern;

use super::ruler::RULER_FADE;

const VIDEO: &str = "timeline-video-group";
const AUDIO: &str = "timeline-audio-group";
const RULER: &str = "timeline-ruler";
/// More lanes than either side holds in the test window.
const LANES: usize = 16;
/// A scroll short of a band: the edge it uncovers fades this deep.
const NUDGE: f32 = 6.0;

/// The bands of the fade over the view drawn as `view`, on the draw after
/// `edit`: zeros where no fade is painted. The playhead layer paints a second
/// fade over the ruler while its head reaches into a faded band; every fade
/// over the view fades each edge as deep as the view does or not at all.
fn faded(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
    view: &str,
    edit: impl FnOnce(&mut AppState),
) -> Edges<f32> {
    visual.update(|window, app| {
        state.update(app, |state, _| edit(state));
        window.refresh();
    });
    visual.run_until_parked();
    take_painted();
    visual.update(|window, _| window.refresh());
    let fades = take_painted();
    let drawn = visual
        .debug_bounds(intern(view))
        .unwrap_or_else(|| panic!("{view} is not drawn"));
    let over: Vec<Edges<f32>> = fades
        .iter()
        .filter(|fade| fade.bounds == drawn)
        .map(|fade| fade.bands.map(|&band: &Pixels| f32::from(band)))
        .collect();
    let deepest = |edge: fn(&Edges<f32>) -> f32| over.iter().map(edge).fold(0.0, f32::max);
    let own = Edges {
        top: deepest(|b| b.top),
        right: deepest(|b| b.right),
        bottom: deepest(|b| b.bottom),
        left: deepest(|b| b.left),
    };
    for bands in &over {
        for (edge, band, deep) in [
            ("top", bands.top, own.top),
            ("right", bands.right, own.right),
            ("bottom", bands.bottom, own.bottom),
            ("left", bands.left, own.left),
        ] {
            assert!(
                band == 0.0 || band == deep,
                "{view}: one fade over the view fades its {edge} edge {band} px deep and \
                 another {deep} px"
            );
        }
    }
    own
}

/// Whether band depths agree to within the half pixel layout rounds an edge
/// by.
fn same(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() <= 0.51 && (a.1 - b.1).abs() <= 0.51
}

/// The scrolls a view with `over` px of content past it is drawn at, each
/// with the depth its start and end edges fade there, at most `band` deep.
fn sweep(over: f32, band: f32) -> [(f32, (f32, f32), &'static str); 5] {
    [
        (0.0, (0.0, band), "at rest"),
        (NUDGE, (NUDGE, band), "scrolled a few pixels"),
        (over / 2.0, (band, band), "scrolled halfway"),
        (
            over - NUDGE,
            (band, NUDGE),
            "a few pixels short of its far end",
        ),
        (over, (band, 0.0), "scrolled to its far end"),
    ]
}

/// How far the lanes of the side drawn as `side` overflow it: the lanes'
/// height less the side's.
fn overflow(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
    side: &str,
    lanes: std::ops::Range<usize>,
) -> f32 {
    let content: f32 = visual.read(|app| {
        state.read(app).store.sequence.tracks[lanes]
            .iter()
            .map(|track| track.height)
            .sum()
    });
    let drawn = visual
        .debug_bounds(intern(side))
        .unwrap_or_else(|| panic!("{side} is not drawn"));
    content - f32::from(drawn.size.height)
}

#[gpui::test]
fn each_lane_side_fades_the_edge_its_lanes_continue_past(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "lanes-fade");
    visual.update(|_, app| app.set_reduce_motion(true));
    // A second draw: the first records the panel's frame, which sizes the sides.
    for _ in 0..2 {
        faded(visual, &state, VIDEO, |state| {
            if state.store.sequence.tracks.len() < 2 {
                state
                    .add_track(TrackKind::Audio, "Audio 1")
                    .expect("add A1");
            }
        });
    }
    for side in [VIDEO, AUDIO] {
        assert_eq!(
            faded(visual, &state, side, |_| {}),
            Edges::default(),
            "one lane fits {side}, and the side fades an edge"
        );
    }

    faded(visual, &state, VIDEO, |state| {
        for n in 2..=LANES {
            state
                .add_track(TrackKind::Video, format!("Video {n}"))
                .expect("add a video lane");
            state
                .add_track(TrackKind::Audio, format!("Audio {n}"))
                .expect("add an audio lane");
        }
    });
    let videos = visual.read(|app| state.read(app).visual_lane_count());
    let total = visual.read(|app| state.read(app).store.sequence.tracks.len());
    for (side, lanes) in [(VIDEO, 0..videos), (AUDIO, videos..total)] {
        let over = overflow(visual, &state, side, lanes);
        assert!(
            over > 2.0 * SCROLL_FADE,
            "{LANES} lanes overflow {side} by {over} px, too little to scroll it halfway"
        );
        let scroll = |state: &mut AppState, by: f32| {
            if side == VIDEO {
                state.video_scroll_y = by;
            } else {
                state.audio_scroll_y = by;
            }
        };
        // A scroll moves a side away from the A/V divider: video lanes number
        // upward from it at the side's foot, audio lanes hang from it at the
        // top. The divider's edge is the sweep's start.
        for (by, (divider, away), what) in sweep(over, SCROLL_FADE) {
            let bands = faded(visual, &state, side, |state| scroll(state, by));
            let want = if side == VIDEO {
                (away, divider)
            } else {
                (divider, away)
            };
            assert_eq!(
                (bands.left, bands.right),
                (0.0, 0.0),
                "{side} {what}: an edge across the lanes fades"
            );
            assert!(
                same((bands.top, bands.bottom), want),
                "{side} {what}: the top and bottom fade {:?} deep, not {want:?}",
                (bands.top, bands.bottom)
            );
        }
    }
}

#[gpui::test]
fn the_ruler_fades_the_edge_the_sequence_continues_past(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "ruler-fade");
    visual.update(|_, app| app.set_reduce_motion(true));
    let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    // A second draw: the first records the panel's frame, which sizes the view.
    for _ in 0..2 {
        faded(visual, &state, RULER, |state| {
            if state
                .store
                .sequence
                .tracks
                .iter()
                .all(|t| t.clips.is_empty())
            {
                let media = state.import_media(still.clone()).expect("import the still");
                state
                    .drop_media_on_timeline(media, None, 0)
                    .expect("place the still");
            }
        });
    }
    let (seconds, width) = visual.read(|app| {
        let state = state.read(app);
        let fps = state.store.sequence.ticks_per_second();
        (
            state.store.sequence.duration_ticks as f32 / fps,
            state.timeline_viewport_px(),
        )
    });
    assert_eq!(
        faded(visual, &state, RULER, |state| {
            let pps = crate::transport::TIMELINE_BASE_PPS * state.zoom;
            state.set_zoom(state.zoom * width / (seconds * pps) * 0.5, false);
            state.scroll_timeline_to(0.0, false);
        }),
        Edges::default(),
        "a sequence that fits the ruler, and the ruler fades an edge"
    );

    // The still across three widths of the ruler.
    let over = {
        faded(visual, &state, RULER, |state| {
            let pps = crate::transport::TIMELINE_BASE_PPS * state.zoom;
            state.set_zoom(state.zoom * 3.0 * width / (seconds * pps), false);
        });
        let pps = visual.read(|app| crate::transport::TIMELINE_BASE_PPS * state.read(app).zoom);
        seconds * pps - width
    };
    assert!(
        over > 2.0 * RULER_FADE,
        "the still runs {over} px past the ruler, too little to scroll it halfway"
    );
    for (by, want, what) in sweep(over, RULER_FADE) {
        let bands = faded(visual, &state, RULER, |state| {
            state.scroll_timeline_to(by, false);
        });
        assert_eq!(
            (bands.top, bands.bottom),
            (0.0, 0.0),
            "the ruler {what}: an edge across the time axis fades"
        );
        assert!(
            same((bands.left, bands.right), want),
            "the ruler {what}: the left and right fade {:?} deep, not {want:?}",
            (bands.left, bands.right)
        );
    }
}
