//! Editor command layer: a closed set of structured, deterministic edits.
//!
//! Every edit is an explicit, serializable value — auditable, replayable, and
//! undo-friendly once wired through `aria-engine`.

mod command;
mod reach;
mod samples;
mod spec;

pub use command::{Command, EditCommand, EditOutcome, EditTargets, ProjectCommand, TemplatePick};
pub use reach::Reach;
pub use samples::{every_edit_command, every_project_command};
pub use spec::{EditSpec, Inverse, ProjectSpec, edit_specs, project_specs};
// Every model type a command field carries, so callers (shell FFI, the AI
// agent, tests) can build any command from this crate alone.
pub use aria_models::{
    AnimationRef, AnimationSlot, AudioLayout, AudioRole, BlendMode, CanvasAspect, ChromaKey,
    ClipId, ClipParam, ClipTransform, ColorAdjustments, CropRect, Easing, EffectInstance,
    EffectPreset, Filter, Generator, LayerShadow, LayerStyles, Lut, MarkerColor, MarkerId, Mask,
    MaskKind, MatteSource, MediaId, MotionBlur, MulticamAngle, MulticamCut, MulticamGroup,
    MulticamGroupId, Param, ParamValue, PiecewiseEasingPreset, Rational, RationalTime, Replaceable,
    Sequence, SequenceId, SpatialTangents, StabilizeLevel, TemplateMeta, TimeRange, TrackId,
    TrackKind, TrackMatte,
};
// The output shape an export command asks for, from the same contract crate the
// encoder reads it from.
pub use aria_core::{ExportFormat, VideoCodec};

use tracing::info;

pub fn init() {
    info!("aria-commands ready");
}
