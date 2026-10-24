//! Aria editing data model: project, media pool, timeline, tracks, clips.
//!
//! Design goals:
//! - **Correct types**: strongly-typed IDs (no mixing a [`TrackId`] with a
//!   [`ClipId`]), OTIO-style [`RationalTime`] (exact NTSC rates) sourced from
//!   the shared [`aria_core`] contract, and explicit timeline-vs-source
//!   ranges each carrying their own rate.
//! - **Fast lookup**: entities are stored in hash maps keyed by ID, so finding
//!   a clip/track/media by ID is O(1). The timeline keeps a `ClipId -> TrackId`
//!   index so a clip can be located across all tracks without scanning.
//! - **Independent testing**: this crate has no dependency on decode/render, so
//!   the model can be exercised in isolation.
//!
//! A [`Project`] owns one [`Timeline`] and a media pool of [`MediaSource`]s.
//!
//! ## Templates
//!
//! A [`Template`] is a finished [`Project`] whose author has marked specific
//! clips as user-replaceable ([`Replaceable`]), mirroring CapCut: the slot
//! keeps its sample media (so the template previews), and applying the template
//! swaps the media while the slot's duration, transform, effects, transitions,
//! and animations stay locked.

mod audio_effects;
mod audio_transition;
mod audit;
mod bin;
mod capabilities;
pub mod catalog;
mod clip;
mod clip_types;
mod content_hash;
mod effect_graph;
mod effects;
mod error;
mod ids;
mod label_color;
mod look;
mod media;
mod metadata;
mod multicam;
mod param;
mod param_walk;
mod persist;
mod preset;
mod project;
mod registered;
mod schema;
mod sequence;
mod serde_map;
mod source_patch;
mod sticker;
mod template;
pub mod template_bundle;
mod time;
mod timeline;
mod track;
mod track_catalog;
mod track_routing;
mod transition;

/// Fast hash map for integer-keyed entities (FxHash; no DoS resistance needed
/// for an in-process editing model, but much faster than SipHash for `u64`).
pub type Map<K, V> = rustc_hash::FxHashMap<K, V>;

pub use audio_effects::{
    AudioEffectInstance, AudioEffectSpec, AudioInsertTarget, MAX_AUDIO_LATENCY,
    OTHER_AUDIO_CATEGORY, STRIP_INSERTS, audio_effect_catalog, audio_effect_spec,
    shipped_audio_effect_catalog,
};
pub use audio_transition::{AudioTransition, EXPONENTIAL_FLOOR_DB, TransitionMedia};
pub use audit::{
    ClipFact, ClipFactSpec, FactValue, FactWalk, ProjectFact, ProjectFactSpec, clip_fact_by_id,
    clip_facts, clips_with_fact, project_facts,
};
pub use bin::{Bin, PoolPosition};
pub use capabilities::{ClipAction, ClipCapabilities};
pub use clip::{
    AnimatedTransform, Clip, ClipAttribute, ClipParam, ClipParamClass, ClipSource, ClipTransform,
    CropRect, DriverCode, Generator, LookParam, MAX_CLIP_PAN, MAX_CLIP_VOLUME, MAX_DRIVER_LANGUAGE,
    MAX_DRIVER_SOURCE, MAX_PARENT_DEPTH, MAX_SHAPE_DIM, MAX_SPEED, MAX_STAR_POINTS,
    MAX_STROKE_WIDTH, MAX_TEXT_RUNS, MIN_CLIP_PAN, MIN_CROP_FRACTION, MIN_SPEED, MaskParam,
    ParamAxis, ParamExclusion, ParamKind, ParamRange, ParamScope, ParamSpec, ParamTime, ParamValue,
    ParentFrame, ParentKeep, PropertyDriver, Replaceable, SHAPE_DROP_HEIGHT, SHAPE_DROP_WIDTH,
    SPEED_CURVE_SCALE, Scale2, Shape, ShapeParam, ShapePath, ShapePathPoint, ShapeStroke,
    SlotMedia, SpeedPresetSpec, StyleParam, TEXT_RUN_VERSION, TextAlignH, TextAlignV,
    TextBackground, TextCase, TextParam, TextShadow, TextStroke, TextStyle, TextStyleRun,
    audio_gain_at, clip_param_catalog, clip_params_for, default_line_param,
    look_animation_combo_period_ticks, look_animation_window_ticks, mask_param_group, param_spec,
    property_row, retimed_duration, speed_curve_integral, speed_curve_source_fraction,
    speed_preset, speed_preset_catalog, speed_preset_id, validate_pan, validate_pan_envelope,
    validate_speed_curve, validate_volume, validate_volume_envelope, visit_clip_keyframes,
    visit_clip_params,
};
pub use clip::{GeneratorSpec, generator_spec, generator_spec_by_id, generator_specs};
pub use clip_types::{
    CLIP_TYPE_ID_MAX_BYTES, CLIP_TYPE_MAX_HANDLES, CLIP_TYPE_NAME_MAX_BYTES, ClipTypeIcon,
    ClipTypeSpec, clip_type_catalog, clip_type_spec,
};
pub use effect_graph::{
    GraphError, GraphPass, GraphPlan, MAX_GRAPH_PASSES, MAX_PASS_INPUTS, MAX_PASS_SCALE,
    MAX_PASS_SIDE, MAX_WORKGROUP_INVOCATIONS, OutputSize, PassInput, PassStage, plan_graph,
};
pub use effects::{
    CANVAS_PX_UNIT, EFFECT_CURVE_LUT_SAMPLES, EFFECT_GRADIENT_LUT_SAMPLES, EFFECT_NAME_MAX_CHARS,
    EffectBounds, EffectChoiceOption, EffectCurvePoint, EffectGradientStop, EffectGroupSpec,
    EffectInstance, EffectParamDisplay, EffectParamKind, EffectParamSpec, EffectSpec,
    EffectVisibilityCondition, OTHER_VIDEO_CATEGORY, StoredParams, canvas_px_float_ranges,
    default_flattened_param_floats, effect_catalog, effect_spec, evaluate_effect_curve_lut,
    evaluate_effect_gradient_lut, sample_effect_curve, sample_effect_gradient,
    shipped_effect_catalog,
};
pub use error::ModelError;
pub use ids::{
    BinId, ClipId, GroupId, LinkId, MarkerId, MediaId, MulticamGroupId, ProjectId, SequenceId,
    TemplateId, TrackId,
};
pub use label_color::LabelColor;
pub use look::{
    ANIMATION_INTENSITY_RANGE, ANIMATION_PARAM_DEFAULT, ANIMATION_SPEED_RANGE,
    ANIMATION_STAGGER_RANGE, AdjustSlider, AnimationKnobs, AnimationRef, AnimationSlot,
    AnimationSpec, AudioRole, BlendMode, ChromaKey, ColorAdjustments, Filter, FilterGrade,
    FilterOffset, FilterSpec, GAIN_NEUTRAL, GAMMA_NEUTRAL, LIFT_NEUTRAL, LayerBackground,
    LayerGlow, LayerOutline, LayerShadow, LayerStyles, LookAnimations, Lut, MASK_PATH_VERSION,
    MAX_CLIP_MASKS, MAX_MASK_EXPANSION, MAX_MASK_PATH_POINTS, Mask, MaskKind, MaskMode, MaskSpec,
    MatteSource, MotionBlur, StabilizeLevel, TextEffectSpec, TrackMatte, animation_catalog,
    animation_spec, filter_catalog, filter_spec, mask_catalog, text_effect_catalog,
    text_effect_spec,
};
pub use media::{MediaKind, MediaSource, STILL_DEFAULT_DURATION_TICKS, STILL_TICK_RATE};
pub use metadata::ProjectMetadata;
pub use multicam::{MulticamAngle, MulticamCut, MulticamGroup, angle_at};
pub use param::{
    EASING_PRESETS, EASY_EASE_INFLUENCE, Easing, EasingPreset, Extrapolate, Keyframe, Lerp,
    MAX_TEMPORAL_INFLUENCE, MIN_PRESET_SEGMENT_TICKS, MIN_TEMPORAL_INFLUENCE, Param,
    PiecewiseEasingPreset, PiecewisePresetSpec, SegmentSample, SpatialTangents, TemporalEase,
    Waypoint, easing_preset, expand_preset, piecewise_preset_by_id, piecewise_presets,
};
pub use persist::{PROJECT_FILE_EXTENSION, PROJECT_FILE_VERSION};
pub use preset::{
    MAX_PRESET_AUDIO_EFFECTS, MAX_PRESET_EFFECTS, MAX_PRESET_KEYFRAMES, MAX_PRESET_PROPERTIES,
    PRESET_FILE_EXTENSION, PRESET_MAX_BYTES, PRESET_MAX_NAME_CHARS, PRESET_VERSION, Preset,
    PresetParts, PresetProperty,
};
pub use project::{EnableAssignment, GroupAssignment, LinkAssignment, Project};
pub use registered::{
    Catalog, CatalogIter, Registered, register_audio_effect, register_clip_type, register_effect,
    register_transition, set_registered_effect_category,
};
pub use schema::{PROJECT_SCHEMA_KIND, PROJECT_SCHEMA_VERSION, ProjectSchema};
pub use sequence::Sequence;
pub use source_patch::{Patch, SoundPlacement, SourceChannel, SourcePatching};
pub use sticker::{ArtSource, StickerSpec, sticker_catalog, sticker_spec};
pub use template::{
    Pick, TEMPLATE_FILE_EXTENSION, TEMPLATE_SCHEMA_KIND, Template, TemplateCategory, TemplateMeta,
};
pub use time::{
    Rational, RationalTime, TimeError, TimeRange, check_same_rate, rate_eq, resample, time_add,
    time_sub,
};
pub use timeline::{
    AudioLaneLinks, AudioLayout, CanvasAspect, CanvasSettings, Marker, MarkerColor, MarkerKind,
    RemovedTrack, TimeDisplay, Timeline,
};
pub use track::{ClipMut, ClipsOrdered, Track, TrackDisplay, TrackKind};
pub use track_catalog::{
    BakeMedium, LaneZone, TrackKindSpec, track_kind_by_id, track_kind_spec, track_kind_specs,
};
pub use track_routing::{
    AudioBus, AudioSend, AutomationMode, MAX_TRACK_PAN, MIN_TRACK_PAN, TrackOutput,
    check_track_pan, check_track_pan_envelope, check_track_volume_envelope,
};
pub use transition::{
    DEFAULT_TRANSITION_TICKS, Transition, TransitionAlignment, TransitionSpec,
    shipped_transition_catalog, transition_catalog, transition_spec, transition_window,
};

#[cfg(test)]
mod catalog_doc_tests;
