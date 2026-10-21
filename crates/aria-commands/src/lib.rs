//! Editor command layer: a closed set of structured, deterministic edits.
//!
//! Every edit is an explicit, serializable value — auditable, replayable, and
//! undo-friendly once wired through `aria-engine`.

mod command;
mod edit_command;
mod reach;
mod samples;
mod spec;

pub use command::{Command, EditOutcome, EditTargets, ProjectCommand, TemplatePick};
pub use edit_command::EditCommand;
pub use reach::Reach;
pub use samples::{every_edit_command, every_project_command};
pub use spec::{EditSpec, Inverse, ProjectSpec, edit_specs, project_specs};
// Every model type a command field carries, so callers (shell FFI, the AI
// agent, tests) can build any command from this crate alone.
pub use aria_models::{
    AnimationRef, AnimationSlot, AudioInsertTarget, AudioLayout, AudioRole, AutomationMode,
    BlendMode, CanvasAspect, ChromaKey, ClipId, ClipParam, ClipTransform, ColorAdjustments,
    CropRect, DriverCode, Easing, EffectInstance, Filter, Generator, LabelColor, LayerShadow,
    LayerStyles, Lut, MarkerColor, MarkerId, Mask, MaskKind, MaskMode, MaskParam, MatteSource,
    MediaId, MotionBlur, MulticamAngle, MulticamCut, MulticamGroup, MulticamGroupId, Param,
    ParamValue, ParentKeep, Patch, PiecewiseEasingPreset, Preset, PresetParts, PresetProperty,
    Rational, RationalTime, Replaceable, Sequence, SequenceId, SoundPlacement, SourceChannel,
    SourcePatching, SpatialTangents, StabilizeLevel, TemplateMeta, TemporalEase, TimeDisplay,
    TimeRange, TrackId, TrackKind, TrackMatte, Transition, TransitionAlignment,
};
// The output shape an export command asks for, from the same contract crate the
// encoder reads it from.
pub use aria_core::{ExportFormat, VideoCodec};

use tracing::info;

pub fn init() {
    info!("aria-commands ready");
}
