//! Structured editor commands.
//!
//! UI gestures and the AI agent both emit these values; the engine applies them
//! against project/timeline state with undo/redo.
//!
//! ## Wire format
//!
//! Every command (de)serializes as a single-level JSON object discriminated by
//! a `"type"` field holding the variant name — `{"type": "SplitClip", "clip":
//! 5, "at": …}`. [`Command`] itself is untagged: project and edit variant
//! names never collide, so the same flat object shape serves both (the shells'
//! FFI protocol and the future AI-agent stream speak this format). The
//! `tests/wire_format.rs` goldens lock it.
//!
//! ## Where a command is described
//!
//! This file holds the shapes and nothing else. A command's label, reach,
//! target shape, junction behaviour and undo are one row in `spec_rows.rs`,
//! and its handler is one row in `aria-engine`'s dispatch table. Both tables
//! generate the only exhaustive matches over these enums, so a variant added
//! here does not compile until both rows exist.

use std::path::PathBuf;

use aria_core::ExportFormat;
use aria_models::{
    AnimationRef, AnimationSlot, AudioInsertTarget, AudioLayout, AudioRole, AutomationMode, BinId,
    BlendMode, CanvasAspect, ChromaKey, ClipAttribute, ClipId, ClipParam, ClipTransform,
    ColorAdjustments, CropRect, DriverCode, Easing, EffectInstance, Filter, Generator, LabelColor,
    LayerStyles, Lut, MarkerColor, MarkerId, Mask, MediaId, MotionBlur, MulticamAngle,
    MulticamGroupId, Param, ParamValue, ParentKeep, Patch, PiecewiseEasingPreset, Preset, Rational,
    RationalTime, Replaceable, SequenceId, SoundPlacement, SourceChannel, StabilizeLevel,
    TemplateMeta, TemporalEase, TimeDisplay, TimeRange, TrackId, TrackKind, TrackMatte, Transition,
    TransitionAlignment,
};
use serde::{Deserialize, Serialize};

/// One media choice for [`ProjectCommand::ApplyTemplate`], in slot order: a
/// file to probe (like `Import`) and an optional in-point into it. `None`
/// starts from the beginning, matching CapCut ("takes from the start of the
/// clip"); the slot's locked timeline duration decides how much source is
/// drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplatePick {
    pub path: PathBuf,
    #[serde(default)]
    pub source_in: Option<RationalTime>,
}

/// A project-level action (media pool, not timeline placement).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProjectCommand {
    /// Register a file in the media pool.
    Import { path: PathBuf },
    /// Drop a source from the media pool (delete from the Library bin).
    /// Rejected while any clip still references it — callers that want to
    /// delete used media remove those clips first (one history group). The
    /// inverse re-inserts the source with its original id, so undo restores
    /// it and any clips removed alongside it reattach.
    RemoveMedia { media: MediaId },
    /// Write the current project to a `.aria` file.
    Save { path: PathBuf },
    /// Replace the session from a project file; every media path must exist.
    Open { path: PathBuf },
    /// Replace the session from a project file; missing media paths are kept but not relinked.
    Load { path: PathBuf },
    /// Re-point a media-pool entry at a new file (missing-media relink):
    /// re-probe `path` and refresh the entry's metadata in place, keeping
    /// its id so clips stay attached. Not undoable by design — it repairs
    /// project state to match the disk, and undoing back to a dead path is
    /// never what the user wants.
    RelinkMedia { media: MediaId, path: PathBuf },
    /// Set (or clear) a media-pool entry's label colour. Undoable: the inverse
    /// restores the previous colour.
    SetMediaLabel {
        media: MediaId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<LabelColor>,
    },
    /// Render the timeline to a video file.
    ///
    /// `format` is what the caller asked for, one field per choice: absent
    /// fields mean the project's own output, so `{"path": …}` alone still
    /// exports exactly the sequence. It is resolved and validated against the
    /// installed encoder by `aria_render::ExportSettings::resolved`, which
    /// refuses a codec or a bitrate this machine cannot honour instead of
    /// writing a file nobody asked for.
    Export {
        path: PathBuf,
        #[serde(default)]
        format: ExportFormat,
    },
    /// Write the current project as a `.ariat` template file (CapCut
    /// "export template"): the finished timeline plus its `Replaceable` /
    /// text-editable markers, wrapped in `meta` for the gallery. The session
    /// itself is untouched — no dirty-flag change, no project-path change.
    SaveTemplate { path: PathBuf, meta: TemplateMeta },
    /// Replace the session with a `.ariat` template filled by `picks`
    /// (CapCut "use template"). Each pick is probed like `Import` and fills
    /// the next slot in order; fewer picks than slots keeps sample media in
    /// the rest (template preview), more is refused. Like `Open`/`Load` the
    /// history is cleared, but the result is a *new unsaved* project: the
    /// project path resets and the session reads dirty until saved. Missing
    /// sample-media paths are tolerated (matches `Load`).
    ApplyTemplate {
        path: PathBuf,
        picks: Vec<TemplatePick>,
    },
}

/// A single structured edit against the timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EditCommand {
    /// Add a track to the timeline stack.
    AddTrack {
        kind: TrackKind,
        name: String,
        /// Stack position (0 = bottom layer, composited first). `None`
        /// appends to the top of the stack. Clamped to the stack height.
        index: Option<usize>,
        /// When true, the lane survives auto-removal while empty until the user
        /// deletes it (manual "Add track" from the timeline UI).
        #[serde(default)]
        pinned: bool,
    },
    /// Place a trimmed range of imported media on a track.
    AddClip {
        track: TrackId,
        media: MediaId,
        source: TimeRange,
        start: RationalTime,
    },
    /// Detach a video clip's embedded sound onto an audio-lane companion.
    ///
    /// `to_track: Some` targets that unlocked audio lane exactly; `None`
    /// chooses the first unlocked lane without an overlap and creates an
    /// unpinned `A{n}` lane when none fits. The companion reuses the media and
    /// source window, copies only audio/retime properties, and is linked back
    /// to the source. One command is one atomic undo entry.
    ExtractAudio {
        clip: ClipId,
        to_track: Option<TrackId>,
    },
    /// Deep-copy one clip to an explicit track/start. The copy receives a
    /// fresh id and is unlinked; every other clip property and keyframe is
    /// preserved. This is a pure placement edit: no search, ripple, transition
    /// copy, or batch/link policy.
    DuplicateClip {
        clip: ClipId,
        to_track: TrackId,
        start: RationalTime,
    },
    /// Deep-copy one clip to an explicit track/start with time remap (target
    /// duration or target playback rate). The copy receives a fresh id, is
    /// unlinked, and is retimed to match the requested duration or speed;
    /// every other clip property and keyframe is preserved. One atomic undo
    /// entry.
    PasteRetimed {
        clip: ClipId,
        to_track: TrackId,
        start: RationalTime,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_duration: Option<RationalTime>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_speed: Option<Rational>,
    },
    /// Copy the source clip's transform, filter, and complete effect chain onto
    /// the target as one atomic edit. Timing, content, and audio properties are
    /// unchanged. The target must be visual.
    PasteAttributes { source: ClipId, clip: ClipId },
    /// Return each group in `attributes` to the values a new clip starts with,
    /// keyframes and drivers included, as one atomic edit. Clearing the speed
    /// ramp re-derives the clip's duration and is refused when the longer clip
    /// would overlap a neighbour.
    RemoveAttributes {
        clip: ClipId,
        attributes: Vec<ClipAttribute>,
    },
    /// Hold one resolved frame from a video clip for `duration`, opening a
    /// same-track hole at `at`. `at` may be either edge or an interior tick;
    /// interior insertion splits the source first. The created clip remains
    /// media-backed but is silent and cannot be retimed. One command is one
    /// atomic undo entry.
    FreezeFrame {
        clip: ClipId,
        at: RationalTime,
        duration: RationalTime,
    },
    /// Render one clip to an intermediate file and put that file in its place
    /// (Premiere "render and replace"). The clip keeps its placement, its
    /// blend mode, its matte and its links; everything the renderer folded
    /// into the picture — effects, grade, transform, crop, speed, masks — is
    /// spent and cleared, and the pre-bake clip is kept inside the new one so
    /// the edit is reversible. `media` is the rendered file, already in the
    /// pool, at the sequence's rate and at least as long as the clip.
    BakeClip { clip: ClipId, media: MediaId },
    /// Put back the clip a bake replaced (Premiere "restore unrendered"),
    /// mapping the restored source window through whatever trimming and
    /// moving happened while the clip was rendered. Refused if the baked clip
    /// was retimed, because a retime has no inverse on the original.
    RestoreBakedClip { clip: ClipId },
    /// Place a generated clip (text, solid, shape, …) on a track.
    AddGenerated {
        track: TrackId,
        generator: Generator,
        timeline: TimeRange,
    },
    /// Replace a generated clip's content (e.g. edit a title's text, recolor
    /// a shape). Rejected for media-backed clips. The inverse restores the
    /// previous generator.
    SetGenerator { clip: ClipId, generator: Generator },
    /// Replace a clip's content with a trimmed window of pooled media,
    /// keeping its timeline placement, speed, transform, and effects — the
    /// primitive behind swapping a template's music and re-picking a filled
    /// slot's in-point (CapCut "replace"). `source` is at the media's native
    /// rate and must lie within it (images repeat one frame for any window).
    /// The inverse restores the previous clip content.
    SetClipMedia {
        clip: ClipId,
        media: MediaId,
        source: TimeRange,
    },
    /// Mark (or with `None` unmark) a media clip as a user-replaceable
    /// template slot (CapCut "set replaceable material clips"). Template
    /// authoring metadata only — renders nothing. Validated at mark time:
    /// the clip must be media-backed and the slot's `accepts` must match the
    /// lane. The inverse restores the previous marker.
    SetReplaceable {
        clip: ClipId,
        replaceable: Option<Replaceable>,
    },
    /// Mark a text clip's wording as user-editable when the project is used
    /// as a template (the text keeps its style and animation). Rejected on
    /// non-text clips. The inverse restores the previous flag.
    SetTextEditable { clip: ClipId, editable: bool },
    /// Set a clip's spatial transform (position/scale/rotation/opacity on
    /// the canvas). Rejected for audio-track clips. The inverse restores the
    /// previous transform.
    ///
    /// `at` composes the edit with animation: `Some(playhead)` writes a
    /// keyframe at that timeline position on properties that already have
    /// keyframes (the CapCut gesture semantics); `None` flattens every
    /// property to a constant. Identical behavior on never-animated clips.
    SetClipTransform {
        clip: ClipId,
        transform: ClipTransform,
        at: Option<RationalTime>,
    },
    /// Parent a clip to another clip on its sequence, or with `None` release
    /// it. Refused when the parent does not overlap the clip in time, is on
    /// another sequence, sits under the clip, or would make the chain deeper
    /// than [`aria_models::MAX_PARENT_DEPTH`].
    ///
    /// `keep` selects whether the clip keeps its place on the canvas at
    /// timeline time `at` (its transform, keyframes included, is rewritten into
    /// the new frame) or snaps to the parent's anchor, scale, and rotation.
    /// The inverse restores the previous parent and transform.
    SetClipParent {
        clip: ClipId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<ClipId>,
        at: RationalTime,
        #[serde(default)]
        keep: ParentKeep,
    },
    /// Insert or replace a keyframe on one animatable clip property at an
    /// absolute timeline position (must fall inside the clip). A constant
    /// property becomes a single-keyframe curve. Optional spatial `tangents`
    /// shape a position motion path (rejected on non-position params). The
    /// inverse restores the previous parameter state (full-clip snapshot).
    SetParamKeyframe {
        clip: ClipId,
        param: ClipParam,
        at: RationalTime,
        value: ParamValue,
        easing: Easing,
        /// Spatial bezier handles for position motion paths. `None` is
        /// straight-line (legacy). Only valid when `param` is [`ClipParam::Position`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tangents: Option<aria_models::SpatialTangents>,
    },
    /// Remove the keyframe at exactly `at` on one property. Removing the
    /// last keyframe collapses the property to a constant of that
    /// keyframe's value. Rejected when no keyframe sits at `at`.
    RemoveParamKeyframe {
        clip: ClipId,
        param: ClipParam,
        at: RationalTime,
    },
    /// Attach (or with `None` remove) a driver: code that replaces one
    /// property's keyframed value per frame. Refused when the driver would read
    /// its own value back through other drivers; code that does not compile is
    /// kept and reported at render. The inverse restores the previous driver.
    SetParamDriver {
        clip: ClipId,
        param: ClipParam,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<DriverCode>,
    },
    /// Replace one animatable property with a constant, dropping all its
    /// keyframes.
    SetParamConstant {
        clip: ClipId,
        param: ClipParam,
        value: ParamValue,
    },
    /// Expand the outgoing keyframe segment at `at` into a multi-keyframe
    /// bounce / elastic / back approximation. Scalar and vec2 params only;
    /// inverse is a full-clip restore.
    ApplyEasingPreset {
        clip: ClipId,
        param: ClipParam,
        at: RationalTime,
        preset: PiecewiseEasingPreset,
    },
    /// Retime a media clip (CapCut speed, M1): `speed` is the positive
    /// playback-rate multiplier (2/1 = double speed), `reversed` plays the
    /// source window backward. Keeps the clip's timeline start and source
    /// window; the timeline duration re-derives (source ÷ speed). Rejected
    /// on generated clips and when the new extent would overlap a
    /// neighbor. The inverse restores the previous clip state.
    SetClipSpeed {
        clip: ClipId,
        speed: Rational,
        reversed: bool,
    },
    /// Set (or clear) a media clip's playback-rate ramp (CapCut speed curves,
    /// M2): `curve` is the normalized speed-multiplier curve over the clip's
    /// span (ticks `0..=SPEED_CURVE_SCALE`); `None` clears it to a flat unit
    /// ramp. Keeps the clip's base `speed`/`reversed` and source window; the
    /// timeline duration re-derives from `source ÷ (base_speed × average
    /// curve)`. Rejected on generated clips, malformed curves, and when the
    /// new extent would overlap a neighbor. The inverse restores the previous
    /// clip state.
    SetSpeedCurve {
        clip: ClipId,
        curve: Option<Param<f32>>,
    },
    /// Toggle pitch preservation on a retimed media clip (CapCut's "pitch"
    /// switch, M8 Phase 3): `true` time-stretches so the audio keeps its
    /// pitch, `false` lets pitch follow the speed ("chipmunk"). A pure audio
    /// property — changes no duration. Rejected on generated clips. The
    /// inverse restores the previous clip state.
    SetClipPitch { clip: ClipId, preserve_pitch: bool },
    /// Set a clip's framing (CapCut crop, M1): `crop` is the normalized
    /// kept region of the content (applied before placement, so the kept
    /// pixels aspect-fit and transform exactly like the full frame did);
    /// `flip_h`/`flip_v` mirror the content. Rejected on audio-track clips
    /// and on degenerate/out-of-frame rects. The inverse restores the
    /// previous framing.
    ///
    /// `at: Some(playhead)` writes a crop keyframe when the property is
    /// already animated (M2 compose); `None` flattens to a constant.
    SetClipCrop {
        clip: ClipId,
        crop: CropRect,
        flip_h: bool,
        flip_v: bool,
        at: Option<RationalTime>,
    },
    /// Set a media clip's audio mix (CapCut volume + fades): `volume` is the
    /// flat gain (`0` mutes, `1` = unchanged, up to 10× boost) — `Some`
    /// overwrites the clip's gain with that constant (the basic slider,
    /// flattening any M8 envelope), `None` leaves the gain (constant or
    /// envelope) untouched and only updates the fades. Fades are linear
    /// in/out durations at the timeline rate. Audible for clips on audio
    /// lanes; rejected on generated clips and on fades longer than the clip.
    /// The inverse restores the previous clip state.
    SetClipAudio {
        clip: ClipId,
        volume: Option<f32>,
        fade_in: RationalTime,
        fade_out: RationalTime,
    },
    /// Toggle noise reduction on a media clip (CapCut "Reduce noise", M8
    /// Phase 5): both audio mixers run the clip's audio through RNNoise to
    /// suppress steady background noise. Rejected on generated clips. The
    /// inverse restores the previous flag.
    SetClipDenoise { clip: ClipId, denoise: bool },
    /// Set (or with `None` clear) a clip's organizational label colour
    /// (Premiere label colours). Purely visual grouping, accepted on every
    /// clip kind; the inverse restores the previous colour.
    SetClipLabel {
        clip: ClipId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<LabelColor>,
    },
    /// Set (or with `None` clear) a clip's operator-given name. Clearing it
    /// gives the card back the name its source provides. Accepted on every clip
    /// kind; the inverse restores the previous name.
    SetClipName {
        clip: ClipId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// Set (or with `None` clear) the registered clip type a clip is
    /// (`aria_models::ClipTypeSpec`). The type need not be registered, so a
    /// project keeps the type of a plugin that is not installed. Accepted on
    /// every clip kind; the inverse restores the previous type.
    SetClipType {
        clip: ClipId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clip_type: Option<String>,
    },
    /// Set (or with `None` clear) the property a clip's bar on the timeline
    /// draws as its line and keyframe dots: `None` draws Volume on an audio
    /// lane and Opacity on any other. Refused for a property the clip does not
    /// have, and for a picture property on an audio lane. The inverse restores
    /// the previous property.
    SetClipLineParam {
        clip: ClipId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        param: Option<ClipParam>,
    },
    /// Append a mask to the bottom of a visual clip's mask stack, which holds
    /// at most `aria_models::MAX_CLIP_MASKS`. The inverse restores the
    /// previous clip state.
    AddClipMask { clip: ClipId, mask: Mask },
    /// Replace the mask at `index` in a clip's mask stack. Drivers on its
    /// properties stay attached. The inverse restores the previous clip state.
    SetClipMask {
        clip: ClipId,
        index: u32,
        mask: Mask,
    },
    /// Remove the mask at `index` from a clip's mask stack, with the drivers
    /// on its properties. The inverse restores the previous clip state.
    RemoveClipMask { clip: ClipId, index: u32 },
    /// Move the mask at `from_index` to `to_index` in a clip's mask stack.
    /// Both index the stack before the move, and drivers follow their masks.
    /// The inverse restores the previous clip state.
    MoveClipMask {
        clip: ClipId,
        from_index: u32,
        to_index: u32,
    },
    /// Set how a clip composites over the stack below (visual clips only).
    SetClipBlendMode { clip: ClipId, mode: BlendMode },
    /// Set per-clip motion blur (temporal supersampling of the animated
    /// transform). Visual clips only. Params are plain values — not
    /// animatable. The inverse restores the previous clip state.
    SetClipMotionBlur {
        clip: ClipId,
        motion_blur: MotionBlur,
    },
    /// Set layer-quad styles (shadow/glow/outline/background) on a visual
    /// clip. Empty styles clear every block. The inverse restores the
    /// previous clip state.
    SetClipLayerStyles { clip: ClipId, styles: LayerStyles },
    /// Set (or clear) chroma keying (CapCut chroma key, Phase I;
    /// render-neutral). Media-backed visual clips only. The inverse restores
    /// the previous clip state.
    SetClipChroma {
        clip: ClipId,
        chroma: Option<ChromaKey>,
    },
    /// Set (or clear) a clip's track matte: the layer immediately above it
    /// becomes its stencil instead of being drawn. Any visual clip. The inverse
    /// restores the previous clip state.
    SetClipMatte {
        clip: ClipId,
        matte: Option<TrackMatte>,
    },
    /// Set (or clear) stabilization (CapCut stabilize, Phase I;
    /// render-neutral). Media-backed *video* clips only — stills have no
    /// motion. The inverse restores the previous clip state.
    SetClipStabilize {
        clip: ClipId,
        stabilize: Option<StabilizeLevel>,
    },
    /// Set (or clear) a color-grade filter preset (CapCut filters, Phase I;
    /// render-neutral). Any visual clip, including `Generator::Filter` lane
    /// bars; the id must exist in the filter catalog. The inverse restores
    /// the previous clip state.
    SetClipFilter {
        clip: ClipId,
        filter: Option<Filter>,
    },
    /// Set (or clear) a `.cube` 3D LUT, applied after filter + adjust. Any
    /// visual clip, including `Generator::Filter` lane bars (canvas-wide).
    /// The inverse restores the previous clip state.
    SetClipLut { clip: ClipId, lut: Option<Lut> },
    /// Set a clip's manual color grade (CapCut adjust, Phase I;
    /// render-neutral): all five sliders in one shot, neutral values clear.
    /// Any visual clip, including `Generator::Adjustment` lane bars. The
    /// inverse restores the previous clip state.
    SetClipAdjustments {
        clip: ClipId,
        adjust: ColorAdjustments,
    },
    /// Set (or clear) one animation slot (CapCut animation In / Out / Combo,
    /// Phase I; render-neutral). The preset must exist in the animation
    /// catalog and match the slot; CapCut exclusivity (a combo replaces both
    /// sides and vice versa) is applied by the model. The inverse restores
    /// the previous clip state.
    SetClipAnimation {
        clip: ClipId,
        slot: AnimationSlot,
        animation: Option<AnimationRef>,
    },
    /// Tag (or untag) what an audio-lane clip *is* (music / sound FX /
    /// voiceover / extracted, Phase I) — badges and future mixing defaults.
    /// Audio-track clips only. The inverse restores the previous tag.
    SetAudioRole {
        clip: ClipId,
        role: Option<AudioRole>,
    },
    /// Append a GPU effect (M4) to a visual clip's chain. `effect_id` must
    /// exist in the effect catalog. The inverse removes it (clip snapshot).
    AddEffect { clip: ClipId, effect_id: String },
    /// Remove the effect at `index` from a clip's chain. The inverse restores
    /// it (clip snapshot).
    RemoveEffect { clip: ClipId, index: usize },
    /// Move an effect within a clip's chain. Both indices address the pre-move
    /// chain; `to_index` is the effect's final index after the move. The
    /// inverse restores the previous clip state.
    MoveEffect {
        clip: ClipId,
        from_index: usize,
        to_index: usize,
    },
    /// Set one effect parameter to a constant (the non-animated quick edit;
    /// animated edits go through `SetParamKeyframe` with `ClipParam::Effect`).
    /// `param` is the catalog slot index. The inverse restores the previous
    /// clip state.
    SetEffectParam {
        clip: ClipId,
        index: usize,
        param: usize,
        value: f32,
    },
    /// Replace the effect instance at `index` in `clip`'s effect chain after
    /// validating its parameter state.
    SetEffectInstance {
        clip: ClipId,
        index: usize,
        effect: EffectInstance,
    },
    /// Switch the effect at `index` in `clip`'s chain off (`bypass: true`) or
    /// back on. A bypassed effect keeps its parameters and its place in the
    /// chain and renders nothing. The inverse restores the previous clip state.
    SetEffectBypass {
        clip: ClipId,
        index: usize,
        bypass: bool,
    },
    /// Apply an animation preset to a clip anchored at `at`.
    /// The inverse restores the previous clip state.
    ApplyPreset {
        clip: ClipId,
        preset: Preset,
        at: RationalTime,
    },
    /// Add a transition (M4) at the junction where `clip` abuts the next clip
    /// on its track. `transition_id` must exist in the transition catalog. The
    /// inverse removes it (track-transitions snapshot).
    AddTransition { clip: ClipId, transition_id: String },
    /// Remove the transition at `clip`'s right junction. The inverse restores
    /// it (track-transitions snapshot).
    RemoveTransition { clip: ClipId },
    /// Set the window length (timeline ticks) of the transition at `clip`'s
    /// right junction. The inverse restores the previous length.
    SetTransition { clip: ClipId, duration: i64 },
    /// Set how the transition window at `clip`'s right junction sits relative
    /// to the cut. The inverse restores the previous alignment.
    SetTransitionAlignment {
        clip: ClipId,
        alignment: TransitionAlignment,
    },
    /// Replace the transition at `clip`'s right junction after validating its
    /// duration and parameter state.
    SetTransitionState {
        clip: ClipId,
        transition: Transition,
    },
    /// Split a clip at a timeline position into two abutting clips.
    SplitClip { clip: ClipId, at: RationalTime },
    /// Re-place / trim a clip to occupy `timeline`. Linked companions take the
    /// same start and end deltas.
    TrimClip { clip: ClipId, timeline: TimeRange },
    /// Re-place / trim one clip to occupy `timeline`, leaving its linked
    /// companions where they are: the trim sent with linked selection off.
    TrimClipUnlinked { clip: ClipId, timeline: TimeRange },
    /// Move a media clip's source window by `by` source ticks, keeping its
    /// timeline placement. Clamped at the ends of the take; refused on a
    /// generated clip, a freeze frame, or a still.
    SlipClip { clip: ClipId, by: i64 },
    /// Move a clip to `to_track` starting at `start`, keeping its duration.
    /// Linked companions take the same time delta on their own lanes.
    MoveClip {
        clip: ClipId,
        to_track: TrackId,
        start: RationalTime,
    },
    /// Move a clip to `to_track` starting at `start`, leaving its linked
    /// companions where they are: the move sent with linked selection off.
    /// Clips grouped with it still take the same time delta on their lanes.
    MoveClipUnlinked {
        clip: ClipId,
        to_track: TrackId,
        start: RationalTime,
    },
    /// Move a clip to `to_track` starting at `start`, closing the hole it left
    /// and opening one at the drop. Same-track drops past the clip's old end are
    /// measured on the closed timeline.
    RippleMoveClip {
        clip: ClipId,
        to_track: TrackId,
        start: RationalTime,
    },
    /// Remove a clip, leaving a gap where it sat.
    RemoveClip { clip: ClipId },
    /// Remove a track (and any clips still on it) from the stack.
    RemoveTrack { track: TrackId },
    /// Reorder a track in the stack (0 = bottom layer). Clamped to stack height.
    MoveTrack { track: TrackId, index: usize },
    /// Rename a track lane (display name in the header).
    SetTrackName { track: TrackId, name: String },
    /// Toggle whether a (visual) track contributes to the composite.
    SetTrackEnabled { track: TrackId, enabled: bool },
    /// Toggle whether an audio track is silenced.
    SetTrackMuted { track: TrackId, muted: bool },
    /// Solo a track: while any lane of its kind is soloed, the other lanes of that
    /// kind stop contributing.
    SetTrackSolo { track: TrackId, solo: bool },
    /// Toggle whether a track's clips are editable (selection/move/trim).
    SetTrackLocked { track: TrackId, locked: bool },
    /// Tag (or untag) an audio track as a sidechain "voice" source for audio
    /// ducking (M8 Phase 4): a "duck under voice" gesture dips music under the
    /// clips on every lane flagged here. The inverse restores the flag.
    SetTrackDuckSource { track: TrackId, duck_source: bool },
    /// Make a lane the target of its kind, or stop it being one: an edit that
    /// names no lane lands on the first targeted lane of the kind it needs. The
    /// inverse restores the flag.
    SetTrackTargeted { track: TrackId, targeted: bool },
    /// Turn a lane's sync lock on or off: an insert on another lane moves a
    /// sync-locked lane's clips with it, so material that was in sync stays in
    /// sync. The inverse restores the flag.
    SetTrackSyncLock { track: TrackId, sync_lock: bool },
    /// Change a lane's persisted view options. Each field is optional so one
    /// gesture writes only what it changed; the inverse restores the previous
    /// values. Allowed on a locked lane: how a lane is drawn is not an edit to
    /// its clips.
    SetTrackDisplay {
        track: TrackId,
        height: Option<u32>,
        waveform: Option<bool>,
        filmstrip: Option<bool>,
        collapsed: Option<bool>,
    },
    /// Move a lane's fader, as linear gain (`1.0` is unity). Refused outside
    /// `0..=4` rather than clamped, so the number in the mix is the number the
    /// caller asked for. The inverse restores the lane's previous routing.
    SetTrackGain { track: TrackId, gain: f32 },
    /// Move a lane's stereo pan (`-1.0` full left, `0.0` center, `+1.0` full
    /// right). Refused outside `-1..=1`. The inverse restores the lane's
    /// previous routing.
    SetTrackPan { track: TrackId, pan: f32 },
    /// Set a lane's automation mode and its volume and pan envelopes. The
    /// inverse restores the lane's previous mode and envelopes.
    SetTrackAutomation {
        track: TrackId,
        mode: AutomationMode,
        volume: Param<f32>,
        pan: Param<f32>,
    },
    /// Point a lane's output at a bus lane, or at the master (`None`). Refused
    /// when the destination is not a bus, or when the route would feed the lane
    /// back into itself. The inverse restores the lane's previous routing.
    SetTrackOutput { track: TrackId, to: Option<TrackId> },
    /// Add a send: a copy of the lane, at its own level, added to a bus.
    /// `pre_fader` takes the copy before the lane's own fader — how a reverb is
    /// fed, rather than a parallel chain. The inverse restores the lane's
    /// previous routing.
    AddSend {
        track: TrackId,
        to: TrackId,
        level: f32,
        pre_fader: bool,
    },
    /// Change an existing send, addressed by its index on the lane. Each field
    /// is optional so one gesture writes only what it changed. The inverse
    /// restores the lane's previous routing.
    SetSend {
        track: TrackId,
        index: usize,
        level: Option<f32>,
        pre_fader: Option<bool>,
    },
    /// Drop a send from a lane. The inverse restores the lane's previous
    /// routing, so the send comes back at the same index and level.
    RemoveSend { track: TrackId, index: usize },
    /// Append a registered audio effect to an insert chain: a clip's, a
    /// lane's, or the master's. A lane or the master holds at most five. The
    /// inverse restores the chain as it was.
    AddAudioEffect {
        target: AudioInsertTarget,
        effect_id: String,
    },
    /// Take the insert at `index` out of its chain. The inverse restores the
    /// chain, so the insert comes back at the same index with its values.
    RemoveAudioEffect {
        target: AudioInsertTarget,
        index: usize,
    },
    /// Move one insert within its chain. Both indices address the chain before
    /// the move; `to_index` is the insert's index afterwards. The inverse
    /// restores the chain's previous order.
    MoveAudioEffect {
        target: AudioInsertTarget,
        from_index: usize,
        to_index: usize,
    },
    /// Set parameter slot `param` of an insert, constant or keyframed.
    /// Keyframes on a clip insert are clip-relative ticks; on a lane or master
    /// insert they are sequence ticks. The inverse restores the chain.
    SetAudioEffectParam {
        target: AudioInsertTarget,
        index: usize,
        param: usize,
        value: Param<f32>,
    },
    /// Bypass an insert, or bring it back. A bypassed insert passes its input
    /// through and adds no latency. The inverse restores the chain.
    SetAudioEffectBypass {
        target: AudioInsertTarget,
        index: usize,
        bypass: bool,
    },
    /// Key an insert from another lane's sound, or stop keying it (`None`).
    /// Refused on a clip insert, for an effect that reads no key, and for a
    /// key that would feed a lane back into itself. The inverse restores the
    /// chain.
    SetAudioEffectSidechain {
        target: AudioInsertTarget,
        index: usize,
        key: Option<TrackId>,
    },
    /// Remove a clip and slide later clips on its track left to close the gap.
    RippleDelete { clip: ClipId },
    /// Shift every clip on `track` whose start is ≥ `from` by `delta` ticks
    /// (signed). The ripple primitive: opens a hole for an insert when
    /// positive, closes a gap when negative; rejected if a left shift would
    /// collide or push below tick 0.
    ShiftClips {
        track: TrackId,
        from: RationalTime,
        delta: RationalTime,
    },
    /// Insert a trimmed range of media at `at`, first shifting every clip
    /// starting at/after `at` right by the new clip's duration (CapCut
    /// main-track insert). Atomic: a rejected placement restores the shift.
    RippleInsert {
        track: TrackId,
        media: MediaId,
        source: TimeRange,
        at: RationalTime,
    },
    /// Place a trimmed range of media at `at`, covering whatever is already in
    /// the span it occupies: a clip wholly inside is removed, one reaching into
    /// the span is trimmed to the edge it crosses, and one spanning the whole
    /// window is cut at both edges. Nothing downstream moves. Atomic: a
    /// rejected placement restores everything it cleared.
    OverwriteClip {
        track: TrackId,
        media: MediaId,
        source: TimeRange,
        at: RationalTime,
    },
    /// Four-point edit: place a trimmed range of media at `at` and retime it so
    /// it occupies exactly `duration` on the timeline. `ripple` opens the span
    /// by pushing the lane right; otherwise the span is covered the way
    /// [`Self::OverwriteClip`] covers one. The span is opened before the clip is
    /// placed, so the retime never has to grow into a neighbour. Atomic: a
    /// rejected step restores everything already applied.
    FitToFill {
        track: TrackId,
        media: MediaId,
        source: TimeRange,
        at: RationalTime,
        duration: RationalTime,
        ripple: bool,
    },
    /// Put `clips` into one fresh link group (CapCut linkage): linked clips
    /// select, move, and trim together. Any previous links on the clips are
    /// replaced; the inverse restores them.
    LinkClips { clips: Vec<ClipId> },
    /// Dissolve every link group touched by `clips`. Passing any member clears
    /// the link from the complete group; the inverse restores every group.
    UnlinkClips { clips: Vec<ClipId> },
    /// Enable or disable `clips` as one undo step (`Clip` > `Enable`).
    /// Disabled clips stay on the timeline at reduced opacity and are excluded
    /// from preview/export rendering and audio mixing.
    SetClipsEnabled { clips: Vec<ClipId>, enabled: bool },
    /// Put `clips` (2+ clips) into one fresh clip group (`Clip` > `Group`):
    /// grouped clips select and move together while trims stay per-clip.
    GroupClips { clips: Vec<ClipId> },
    /// Dissolve every clip group touched by `clips` (`Clip` > `Ungroup`).
    UngroupClips { clips: Vec<ClipId> },
    /// Replace a media clip's beat grid with `beats` (CapCut "Beat", M8 Phase
    /// 6): source ticks at the media frame rate, which the model sorts, dedupes,
    /// and clamps to the clip's source window. Rejected on generated clips. The
    /// inverse restores the clip's previous beats.
    ///
    /// Detection itself is not an edit: decoding a clip's audio and running
    /// onset analysis is a job, and its result arrives here as this command, so
    /// history holds one reversible step rather than a re-run of the analysis.
    ///
    /// Ducking works the same way, through [`EditCommand::SetParamKeyframe`] on
    /// [`aria_models::ClipParam::Volume`]: a measured dip is ordinary, editable
    /// volume keyframes, not a command of its own.
    SetClipBeats { clip: ClipId, beats: Vec<i64> },
    /// Clear a clip's detected beat markers (M8 Phase 6). The inverse restores
    /// them.
    ClearBeats { clip: ClipId },
    /// Drop a named, colored marker on the timeline ruler at `at` (M1
    /// markers). `color: None` cycles the fixed palette. The inverse
    /// removes it.
    AddMarker {
        at: RationalTime,
        name: String,
        color: Option<MarkerColor>,
    },
    /// Remove a ruler marker. The inverse restores it (same id).
    RemoveMarker { marker: MarkerId },
    /// Move / rename / recolor / re-annotate a ruler marker in one shot
    /// (callers resolve "keep current" before dispatch). The inverse restores
    /// the previous marker state.
    SetMarker {
        marker: MarkerId,
        at: RationalTime,
        name: String,
        color: MarkerColor,
        /// The marker's note. Absent from the wire while empty, so a payload
        /// written before comments existed still parses.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        comment: String,
    },
    /// Rename the project (the app-owned draft's display name shown in the
    /// title bar and project gallery). The inverse restores the prior name.
    SetProjectName { name: String },
    /// Set the project canvas (M1 canvas settings): aspect-ratio preset,
    /// opaque background color, and an optional explicit pixel size, in one
    /// shot (callers resolve "keep current" before dispatch). `resolution`
    /// `None` derives the size from the aspect or the footage. The inverse
    /// restores the previous settings.
    SetCanvas {
        aspect: CanvasAspect,
        background: [u8; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolution: Option<[u32; 2]>,
    },
    /// Set the sequence editing/playback frame rate. Resamples every clip's
    /// timeline placement, fade durations, keyframes, transitions, and marker
    /// ticks to the new rate. The inverse restores the previous rate and exact ticks.
    SetSequenceRate { frame_rate: Rational },
    /// Set the sequence audio channel layout (mono, stereo, 5.1 surround).
    /// The inverse restores the previous layout.
    SetAudioLayout { layout: AudioLayout },
    /// Set how the sequence's ruler and timecode count time: timecode, frames,
    /// or feet and frames. The inverse restores the previous display.
    SetTimeDisplay { display: TimeDisplay },
    /// Set the master bus's fader, pan, mute, automation mode, and volume/pan
    /// envelopes. The inverse restores the previous master mix state.
    SetMasterMix {
        gain: f32,
        pan: f32,
        muted: bool,
        automation: AutomationMode,
        volume: Param<f32>,
        pan_envelope: Param<f32>,
    },
    /// Create a pool folder, optionally inside `parent`. The inverse deletes it.
    CreateBin {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<BinId>,
    },
    /// Rename a pool folder. The inverse restores the prior name.
    RenameBin { bin: BinId, name: String },
    /// Delete a pool folder. Its media and child bins move up to its parent, so
    /// nothing in the pool is removed with it. The inverse restores the folder
    /// with its own id and puts its contents back.
    RemoveBin { bin: BinId },
    /// Move a pool folder into `parent`, or to the pool root with `None`.
    /// Refused when the move would put a folder inside its own subtree.
    MoveBin {
        bin: BinId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<BinId>,
    },
    /// Put a source in a pool folder, or at the pool root with `None`.
    MoveMediaToBin {
        media: MediaId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bin: Option<BinId>,
    },
    /// Place a pool item at `(x, y)` in `bin` (or at the pool root with `None`) in Freeform view.
    SetPoolItemPosition {
        media: MediaId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bin: Option<BinId>,
        x: f32,
        y: f32,
    },
    /// Create a subclip in the media pool from an in/out range of parent media.
    /// The subclip receives a fresh MediaId, references the parent media
    /// and its bounded source window, and appears in the pool/bin as its own item.
    CreateSubclip {
        parent: MediaId,
        name: String,
        source: TimeRange,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bin: Option<BinId>,
    },
    /// Create a multicam camera group from media angles. The inverse deletes it.
    CreateMulticamGroup {
        name: String,
        angles: Vec<MulticamAngle>,
    },
    /// Place a multicam clip referencing a multicam group on a track at the given source and start time.
    AddMulticamClip {
        track: TrackId,
        group: MulticamGroupId,
        source: TimeRange,
        start: RationalTime,
    },
    /// Switch the active camera angle of a multicam group at a timeline tick.
    SwitchMulticamAngle {
        group: MulticamGroupId,
        at: i64,
        angle: usize,
    },
    /// Move one camera angle on its group's common clock. The inverse restores
    /// the offset it replaced.
    ///
    /// The offset is where that camera's first frame sits on the common clock,
    /// which is the convention the resolver reads it by: a camera that started
    /// rolling late carries a positive offset and is read that much earlier in
    /// its own file.
    SetMulticamSync {
        group: MulticamGroupId,
        angle: usize,
        offset: i64,
    },
    /// Create a secondary sequence containing the specified clips, removing them
    /// from the active timeline and replacing them with a single nested sequence clip.
    CreateNestedSequence { name: String, clips: Vec<ClipId> },
    /// Place a nested sequence on a track at the given source and start time.
    PlaceNestedSequence {
        track: TrackId,
        sequence: SequenceId,
        source: TimeRange,
        start: RationalTime,
    },
    /// Open a secondary sequence for editing (`None` opens the main root timeline).
    OpenSequence { sequence: Option<SequenceId> },
    /// Create an empty secondary sequence with one video and one audio lane,
    /// at the open timeline's frame rate, canvas, and audio layout, and open
    /// it. The inverse removes it and reopens the sequence that was open.
    CreateSequence { name: String },
    /// Set incoming/outgoing temporal ease (`0.1%..=100%` influence and speed)
    /// and/or hold state on the keyframe at `at`. The inverse restores the
    /// clip's previous parameter state.
    SetKeyframeEase {
        clip: ClipId,
        param: ClipParam,
        at: RationalTime,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        in_ease: Option<TemporalEase>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        out_ease: Option<TemporalEase>,
        #[serde(default)]
        hold: bool,
    },
    /// Set or clear roving across time on an interior keyframe at `at`. The
    /// inverse restores the clip's previous parameter state, including the
    /// keyframe's anchored tick.
    SetKeyframeRoving {
        clip: ClipId,
        param: ClipParam,
        at: RationalTime,
        roving: bool,
    },
    /// Patch one source channel of the open sequence: the lane a Source tab's
    /// picture or sound lands on in a three- or four-point edit, `Auto`, or off.
    /// The inverse restores the channel's previous patch.
    SetSourcePatch {
        channel: SourceChannel,
        patch: Patch,
    },
    /// A three- or four-point edit from a Source tab or a media drop, placed
    /// through source patching. The picture lands on `track`; `None` leaves
    /// it out. `sound` puts the sound on a lane of its own as a clip linked to
    /// the picture, keeps it in the picture clip, or leaves it out, which
    /// places a picture with sound silent. `fill` retimes the source
    /// to that span, the way [`Self::FitToFill`] does; `None` places it at its
    /// own length. `ripple` opens the span on both lanes and on every
    /// sync-locked lane, otherwise both lanes are covered the way
    /// [`Self::OverwriteClip`] covers one. Atomic: a rejected step restores
    /// everything already applied. The outcome names the clip on the first
    /// patched lane.
    SourceEdit {
        media: MediaId,
        source: TimeRange,
        at: RationalTime,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<RationalTime>,
        ripple: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        track: Option<TrackId>,
        sound: SoundPlacement,
    },
}

/// Top-level command surface: media registration or a timeline edit.
///
/// Untagged on the wire: the flat `{"type": …}` object of the inner enums is
/// the whole format (variant names are disjoint across the two sets), so
/// callers never spell the project/edit split in JSON.
///
/// `Edit` outweighs `Project` because some edits (layer styles, masks)
/// carry whole param blocks inline. Commands are transient — built once per
/// user action, applied, dropped — so boxing buys nothing and would churn
/// every construction site.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Command {
    Project(ProjectCommand),
    Edit(EditCommand),
}

impl Command {
    /// The operator-facing name of this command, for the history list and for
    /// any surface that reports what was just applied.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Project(command) => command.label(),
            Self::Edit(command) => command.label(),
        }
    }
}

/// What an applied edit produced, for callers to act on (e.g. select the new clip).
///
/// Adjacently tagged on the wire (`{"type": "Created", "id": 7}`): every
/// payload is a single entity id, and the unit variant carries none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id")]
pub enum EditOutcome {
    Created(ClipId),
    CreatedTrack(TrackId),
    Updated(ClipId),
    Removed(ClipId),
    RemovedTrack(TrackId),
    /// Clips on the track were ripple-shifted (no single clip to point at).
    ShiftedTrack(TrackId),
    /// A track flag (enabled / muted / locked) was changed.
    UpdatedTrack(TrackId),
    /// A ruler marker was added / changed / removed (M1 markers).
    CreatedMarker(MarkerId),
    UpdatedMarker(MarkerId),
    RemovedMarker(MarkerId),
    /// The project canvas settings changed (M1 canvas settings).
    UpdatedCanvas,
    /// The sequence frame rate was changed (and timeline resampled).
    UpdatedSequenceRate,
    /// The sequence audio layout was changed.
    UpdatedAudioLayout,
    /// The sequence time display was changed.
    UpdatedTimeDisplay,
    /// The master insert chain changed.
    UpdatedMaster,
    /// A project-level property (its display name) changed.
    UpdatedProject,
    /// A pool folder was created; the id is what a surface selects or renames.
    CreatedBin(BinId),
    /// The pool's folder structure or a source's folder changed. Which one is
    /// not stated: a pool surface redraws from the project either way.
    UpdatedPool,
    /// A secondary sequence was opened for editing (or returned to root).
    OpenedSequence,
    /// A source channel of the open sequence was patched.
    UpdatedSourcePatch,
}
/// The clips and lanes a command addresses.
///
/// A policy that has to answer "does this edit touch a locked lane" needs the
/// ids a command names, and every caller that worked them out by hand would go
/// stale on the next variant. [`EditCommand::targets`] reads them off the
/// command's registry row, so a new variant declares its target shape in the
/// table or does not compile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditTargets {
    /// Clips named directly. The lane each sits on is resolved by the caller,
    /// which is the only place the project is known.
    pub clips: Vec<ClipId>,
    /// Lanes named directly, including a move's destination.
    pub tracks: Vec<TrackId>,
}
