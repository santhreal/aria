//! Tests for this module.
//!
//! The cases live in sibling `tests_*.rs` files, one per area, declared at the
//! bottom. This file holds only what every area needs: the constants and the
//! fixture builders. A child sees them through `use super::*`.

use super::*;
use crate::param::{Easing, Keyframe};
use crate::time::Rational;

const R24: Rational = Rational::FPS_24;
const R30: Rational = Rational::FPS_30;

fn rt(value: i64, rate: Rational) -> RationalTime {
    RationalTime::new(value, rate)
}

fn tr(start: i64, duration: i64, rate: Rational) -> TimeRange {
    TimeRange::at_rate(start, duration, rate)
}

fn media_clip(media: MediaId, source: TimeRange, timeline: TimeRange) -> Clip {
    Clip::from_media(media, source, timeline)
}

// --- speed curves (M2) ---------------------------------------------------

fn linear_ramp(v0: f32, v1: f32) -> Param<f32> {
    Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: v0,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: SPEED_CURVE_SCALE,
                value: v1,
                easing: Easing::Linear,
                tangents: None,
            },
        ],
    }
}

#[path = "tests_source_time.rs"]
mod source_time;

#[path = "tests_source_time_2.rs"]
mod source_time_2;

#[path = "tests_clip_without.rs"]
mod clip_without;

#[path = "tests_keyframed_transform.rs"]
mod keyframed_transform;

#[path = "tests_shape_param.rs"]
mod shape_param;
