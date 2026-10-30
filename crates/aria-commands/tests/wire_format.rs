//! Wire-format lock for the command protocol.
//!
//! The shells (iOS/Android FFI) and the future AI agent speak commands as
//! JSON, so the serialized shape is a public contract. These tests (a) golden
//! the exact JSON for representative commands and (b) roundtrip **every**
//! variant through the untagged [`Command`] surface, checking each one's
//! `"type"` against the tag its registry row declares. A variant cannot reach
//! the wire without a row, and the census and the registry are compared as sets
//! below, so a variant that never got a sample fails here.

use std::path::PathBuf;

use aria_commands::{
    AnimationRef, AnimationSlot, AudioLayout, AudioRole, BlendMode, CanvasAspect, ChromaKey,
    ClipId, ClipParam, ColorAdjustments, Command, Easing, EditCommand, EditOutcome, ExportFormat,
    Filter, Generator, LayerShadow, LayerStyles, MarkerId, Mask, MaskKind, MaskMode, MediaId,
    MotionBlur, Param, ParamValue, ProjectCommand, Rational, RationalTime, SpatialTangents,
    StabilizeLevel, TimeDisplay, TimeRange, TrackId, VideoCodec,
};
use serde_json::{Value, json};

const FPS: Rational = Rational::FPS_30;

fn t(value: i64) -> RationalTime {
    RationalTime::new(value, FPS)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS)
}

fn clip(raw: u64) -> ClipId {
    ClipId::from_raw(raw)
}

fn track(raw: u64) -> TrackId {
    TrackId::from_raw(raw)
}

fn media(raw: u64) -> MediaId {
    MediaId::from_raw(raw)
}

/// The census lives in the library so the editor and the reach declaration can
/// read it too. These tests keep it complete.
fn project_samples() -> Vec<ProjectCommand> {
    aria_commands::every_project_command()
}

fn edit_samples() -> Vec<EditCommand> {
    aria_commands::every_edit_command()
}

/// The wire tag a command's registry row declares. The row is the single
/// description of a command, so the tag it names is what a shell branches on;
/// asserting the serialized `"type"` against it is what keeps the two together.
fn project_variant_name(cmd: &ProjectCommand) -> &'static str {
    cmd.spec().id
}

fn edit_variant_name(cmd: &EditCommand) -> &'static str {
    cmd.spec().id
}

// Each child module reads the helpers above through `use super::*`. `Cargo.toml`
// lists only this file as the test target, so each child is a module here rather
// than a standalone test binary.
#[path = "wire_format_goldens.rs"]
mod goldens;
#[path = "wire_format_outcomes.rs"]
mod outcomes;
#[path = "wire_format_roundtrips.rs"]
mod roundtrips;
