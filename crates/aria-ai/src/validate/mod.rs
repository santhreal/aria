//! Validation + lowering: wire commands → engine commands, against a live
//! project snapshot.
//!
//! This is the guardrail chokepoint. Every rejection carries a
//! model-readable message (including the ids that *do* exist) so the agent
//! loop can feed it straight back as a tool result and let the model correct
//! course. Checks here are for good error messages and the whitelist; the
//! engine remains the authority (overlaps, source bounds, rate math) and
//! re-validates everything on apply.

mod audio;
mod command;
mod look;
mod lookup;
mod lower;
#[cfg(test)]
mod param_parity_tests;
mod routing;
#[cfg(test)]
mod tests;
mod time;

use std::collections::HashSet;

use aria_commands::{Command, EditCommand};
use aria_models::{
    AnimationRef, AnimationSlot, AudioRole, BlendMode, CanvasAspect, ChromaKey, Clip, ClipId,
    ClipParam, ClipTransform, CropRect, Easing, Filter, Generator, LayerBackground, LayerGlow,
    LayerOutline, LayerShadow, LayerStyles, Marker, MarkerColor, MarkerId, Mask, MaskKind,
    MatteSource, MediaId, Param, ParamValue, PiecewiseEasingPreset, Project, Rational,
    RationalTime, StabilizeLevel, TimeRange, TrackId, TrackKind, TrackMatte, animation_spec,
    filter_catalog, filter_spec,
};

use crate::wire::{
    MAX_MULTI_CLIP_REFS, WireAnimationSlot, WireAudioRole, WireBlendMode, WireCanvasAspect,
    WireChromaKey, WireClipParam, WireCommand, WireEasing, WireEasingPreset, WireGenerator,
    WireLayerStyles, WireMarkerColor, WireMask, WireMaskKind, WireMatteSource, WireShape,
    WireStabilizeLevel, WireTrackKind, WireTrackMatte,
};

use audio::*;
use look::*;
use lookup::*;
use lower::*;
use routing::*;
use time::*;

/// A wire command the project as it stands cannot accept. The message is
/// written for the model: it names the problem and the valid alternatives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub message: String,
}

impl Rejection {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Rejection {}

/// What a checked tool call becomes.
///
/// Almost every call is an edit: the turn applies it, the model is told it
/// happened, and the group it belongs to can be undone in one step. A render is
/// the exception, and not by omission — the file it swaps in does not exist
/// yet, and writing it takes longer than a turn lasts. So lowering can also say
/// "this is work, and it is not mine to do": the shell starts it and the turn
/// reports that it started, which is the honest thing to tell a model that
/// cannot wait for it.
#[derive(Debug, Clone, PartialEq)]
pub enum Lowered {
    /// An engine command, applied inside the turn.
    Edit(Command),
    /// Work the shell owns. Nothing has happened to the project yet.
    Queued(QueuedWork),
}

/// Work a tool call asks for that no turn can finish.
///
/// A closed set rather than a callback, for the same reason the shell's host
/// requests are: this crate cannot name a renderer, a file path or a job board,
/// and the shell is the only thing that can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedWork {
    /// Render `clip` to a file and decode that file in its place.
    RenderAndReplace { clip: ClipId },
}

pub use command::validate;
