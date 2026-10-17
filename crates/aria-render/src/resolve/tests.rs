//! Tests for this module.
//!
//! The cases live in sibling `tests_*.rs` files, one per area, declared at the
//! bottom. This file holds only what every area needs: the constants and the
//! fixture builders. A child sees them through `use super::*`, and the two
//! suites declared up in `resolve.rs` (so their names read `resolve::…`) see
//! them through `use super::tests::*` — which is why the fixtures below are
//! `pub(super)` rather than private.

pub(super) use super::*;
pub(super) use crate::grade::effective_grade;
pub(super) use aria_models::{
    AnimationRef, BlendMode, CanvasAspect, CanvasSettings, ClipParam, LayerGlow, LayerOutline,
    LayerShadow, MediaSource, Param, ParamValue, Rational, RationalTime, Scale2, Shape,
    SpatialTangents, StyleParam, TextStyle as ModelTextStyle, TimeRange, TrackKind,
};

pub(super) const FPS_24: Rational = Rational::FPS_24;

pub(super) fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS_24)
}

pub(super) fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS_24)
}

pub(super) fn video(width: u32, height: u32) -> MediaSource {
    MediaSource::new("/tmp/v.mp4", width, height, FPS_24, 600, false)
}

pub(super) fn approx(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-3, "expected ~{b}, got {a}");
}

pub(super) fn approx2(a: [f32; 2], b: [f32; 2]) {
    approx(a[0], b[0]);
    approx(a[1], b[1]);
}

#[path = "tests_empty_project.rs"]
mod empty_project;

#[path = "tests_look_animation.rs"]
mod look_animation;

#[path = "tests_quad_center.rs"]
mod quad_center;

#[path = "tests_mask_and.rs"]
mod mask_and;

#[path = "tests_gesture_partitions.rs"]
mod gesture_partitions;

#[path = "tests_resolve_with.rs"]
mod resolve_with;

#[path = "tests_missing_plugin.rs"]
mod missing_plugin;

#[path = "tests_effect_bypass.rs"]
mod effect_bypass;

#[path = "tests_nested_sequences.rs"]
mod nested_sequences;
