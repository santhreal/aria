//! The intent wire types: the gesture tags the shells send and the small
//! enums their fields carry.
//!
//! Declared from `intents.rs` with `#[path]`; `use super::*` reaches that
//! module's private items and its own imports, so the move changed no name
//! resolution.

use super::*;

/// Which clip edge a trim gesture drags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrimEdge {
    Leading,
    Trailing,
}

/// Lane flavor for generated full-canvas bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectLaneKind {
    Effect,
    Filter,
    Adjustment,
}

impl EffectLaneKind {
    pub(super) fn track_kind(self) -> TrackKind {
        match self {
            EffectLaneKind::Effect => TrackKind::Effect,
            EffectLaneKind::Filter => TrackKind::Filter,
            EffectLaneKind::Adjustment => TrackKind::Adjustment,
        }
    }

    pub(super) fn generator(self) -> Generator {
        match self {
            EffectLaneKind::Effect => Generator::Effect,
            EffectLaneKind::Filter => Generator::Filter,
            EffectLaneKind::Adjustment => Generator::Adjustment,
        }
    }
}

/// A gesture-level operation. See the module docs for the wire shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum Intent {
    /// Import each file and append it to the end of the main track (creating
    /// the main track on first use). Audio-only files land on an audio lane
    /// instead.
    AppendMain { paths: Vec<PathBuf> },
    /// Import a file and ripple-insert it at the main-track clip boundary
    /// nearest `at_seconds` (later clips shift right).
    InsertMain { path: PathBuf, at_seconds: f64 },
    /// Split any clip at an absolute timeline position.
    Split { clip: ClipId, seconds: f64 },
    /// Trim a *main-track* clip with ripple semantics: the track stays
    /// gapless, later clips slide. `delta_seconds` is the signed change of
    /// the dragged edge (leading: + trims content in; trailing: + extends).
    RippleTrimMain {
        clip: ClipId,
        edge: TrimEdge,
        delta_seconds: f64,
    },
    /// Re-place a free lane clip (start and/or length), non-ripple.
    TrimLane {
        clip: ClipId,
        start_seconds: f64,
        length_seconds: f64,
    },
    /// Move a lane clip (or lift a main clip) to a lane at `start_seconds`:
    /// a specific lane when `track` is given, otherwise the first same-kind
    /// lane with a free span (a new lane is created when none fits).
    MoveLane {
        clip: ClipId,
        #[serde(default)]
        track: Option<TrackId>,
        start_seconds: f64,
    },
    /// Insert a clip into the main track at slot `index` (post-removal
    /// indexing when the clip already lives on main — a reorder).
    InsertIntoMain { clip: ClipId, index: usize },
    /// Drop a text clip at the playhead on a text lane.
    AddText {
        text: String,
        at_seconds: f64,
        #[serde(default = "default_overlay_seconds")]
        duration_seconds: f64,
    },
    /// Drop a sticker clip. `asset` is a catalog id
    /// ([`aria_models::sticker_catalog`]); omitted (older callers) means
    /// the first bundled sticker.
    AddSticker {
        at_seconds: f64,
        #[serde(default = "default_overlay_seconds")]
        duration_seconds: f64,
        #[serde(default)]
        asset: Option<String>,
    },
    /// Drop an effect / filter / adjustment bar.
    AddEffect {
        kind: EffectLaneKind,
        at_seconds: f64,
        #[serde(default = "default_overlay_seconds")]
        duration_seconds: f64,
    },
    /// Import a file and drop it as picture-in-picture on an overlay video
    /// lane (half scale, upper half of the canvas — the CapCut drop pose).
    AddPip { path: PathBuf, at_seconds: f64 },
    /// Import a file and drop it on an audio lane. `role` tags the clip with
    /// the picker tab it came from (music / sfx / voiceover).
    AddAudio {
        path: PathBuf,
        at_seconds: f64,
        #[serde(default)]
        role: Option<AudioRole>,
    },
    /// Duplicate a clip right after itself (ripple on main, free slot on a
    /// lane). Copies content, speed, transform, audio mix, crop, effect ids,
    /// and look fields (mask/chroma/stabilize/filter/adjust/animations/role);
    /// keyframed animation flattens to its start value, speed curves reset.
    Duplicate { clip: ClipId },
    /// Swap a media clip's source for a new file, keeping its slot. The
    /// replacement must be at least as long as the clip needs (images are
    /// exempt: one frame repeats).
    ReplaceMedia { clip: ClipId, path: PathBuf },
    /// CapCut "extract audio": place the clip's audio on an audio lane,
    /// linked to the original (whose own lane goes silent via linkage).
    ExtractAudio { clip: ClipId },
    /// CapCut "freeze": extract the main-track clip's frame under the
    /// playhead, write it to `png_path`, import it as a still, and
    /// ripple-insert it at the playhead (splitting the clip when mid-clip).
    Freeze {
        clip: ClipId,
        seconds: f64,
        /// Destination for the extracted frame — the shell owns media
        /// placement (project media dir), Rust owns the pixels.
        png_path: PathBuf,
        #[serde(default = "default_freeze_seconds")]
        duration_seconds: f64,
    },
    /// Retime a media clip. `speed` is the positive rate multiplier.
    SetSpeed {
        clip: ClipId,
        speed: f64,
        #[serde(default)]
        reversed: bool,
    },
    /// Apply a speed-curve preset from the catalog (`None` restores constant
    /// speed). The clip's duration re-derives from the curve in one undo step.
    SetSpeedPreset {
        clip: ClipId,
        #[serde(default)]
        preset: Option<String>,
    },
    /// Volume slider + fade handles. `volume: None` keeps the current gain.
    SetAudio {
        clip: ClipId,
        #[serde(default)]
        volume: Option<f32>,
        #[serde(default)]
        fade_in_seconds: f64,
        #[serde(default)]
        fade_out_seconds: f64,
    },
    /// Canvas placement in UI coordinates (`pos` 0..1, 0.5 = center). With
    /// `at_seconds` the edit composes with animation (keyframe write on
    /// animated properties); without it every property flattens to constant.
    SetTransform {
        clip: ClipId,
        pos_x: f32,
        pos_y: f32,
        scale: f32,
        rotation_degrees: f32,
        opacity: f32,
        #[serde(default)]
        at_seconds: Option<f64>,
    },
    /// Stamp (or, when one already sits there, remove) a transform keyframe
    /// at an absolute timeline position — the timeline's diamond toggle.
    ToggleTransformKeyframe { clip: ClipId, seconds: f64 },
    /// Set, change, or clear (`transition_id: None`) the transition at a
    /// main-track clip's right junction.
    SetTransition {
        clip: ClipId,
        #[serde(default)]
        transition_id: Option<String>,
        #[serde(default)]
        duration_seconds: f64,
    },
}

fn default_overlay_seconds() -> f64 {
    3.0
}

fn default_freeze_seconds() -> f64 {
    3.0
}
