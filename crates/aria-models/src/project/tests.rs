//! Tests for the project model.
//!
//! The cases live in sibling `tests_*.rs` files, one per area, declared below.
//! This file holds only what every area needs: the rate constants and the
//! fixture builders. A child sees them through `use super::*`.

use super::*;

const R24: Rational = Rational::FPS_24;
const R30: Rational = Rational::FPS_30;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, R24)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, R24)
}

fn tr_at(start: i64, duration: i64, rate: Rational) -> TimeRange {
    TimeRange::at_rate(start, duration, rate)
}

fn sample_media(fps: Rational, duration: i64) -> MediaSource {
    MediaSource::new("/tmp/sample.mp4", 1920, 1080, fps, duration, true)
}

fn project_with_media(duration: i64) -> (Project, MediaId, TrackId) {
    let mut project = Project::new("test", R24);
    let media_id = project.add_media(sample_media(R24, duration));
    let track = project.add_track(TrackKind::Video, "V1");
    (project, media_id, track)
}

// --- transitions (M4) -------------------------------------------------

/// Two abutting solid clips on one sticker track (a lane kind that
/// supports transitions); returns `(project, left, right, track)`.
fn project_with_abutting_pair() -> (Project, ClipId, ClipId, TrackId) {
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 24),
        )
        .unwrap();
    let right = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            tr(24, 24),
        )
        .unwrap();
    (project, left, right, track)
}

// --- shift_clips ------------------------------------------------------

fn packed_track() -> (Project, MediaId, TrackId, [ClipId; 3]) {
    let (mut project, media_id, track) = project_with_media(1000);
    let a = project.add_clip(track, media_id, tr(0, 50), rt(0)).unwrap();
    let b = project
        .add_clip(track, media_id, tr(50, 50), rt(50))
        .unwrap();
    let c = project
        .add_clip(track, media_id, tr(100, 50), rt(100))
        .unwrap();
    (project, media_id, track, [a, b, c])
}

// --- Phase I look properties --------------------------------------------

use crate::look::{BlendMode, LayerShadow, LayerStyles, MaskKind, MotionBlur};

#[path = "tests_style_params.rs"]
mod style_params;

#[path = "tests_add_transition.rs"]
mod add_transition;

#[path = "tests_missing_plugin.rs"]
mod missing_plugin;

#[path = "tests_add_clip.rs"]
mod add_clip;

#[path = "tests_split_clip.rs"]
mod split_clip;

#[path = "tests_set_clip.rs"]
mod set_clip;

#[path = "tests_set_clip_2.rs"]
mod set_clip_2;

#[path = "tests_shift_clips.rs"]
mod shift_clips;

#[path = "tests_look_setters.rs"]
mod look_setters;

#[path = "tests_mask_stack.rs"]
mod mask_stack;

#[path = "tests_pool_position.rs"]
mod pool_position;

#[path = "tests_multicam.rs"]
mod multicam;

#[path = "tests_pool_cadence.rs"]
mod pool_cadence;

#[path = "tests_bake.rs"]
mod bake;

#[path = "tests_drivers.rs"]
mod drivers;

#[path = "tests_line_param.rs"]
mod line_param;

#[path = "tests_link_drift.rs"]
mod link_drift;

#[path = "tests_attributes.rs"]
mod attributes;

#[path = "tests_parent.rs"]
mod parent;

#[path = "tests_layer_refs.rs"]
mod layer_refs;

#[path = "tests_largest_video.rs"]
mod largest_video;

#[path = "tests_trim_reach.rs"]
mod trim_reach;

#[path = "tests_stretch.rs"]
mod stretch;
