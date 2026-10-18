//! aria-render: turn a [`aria_models::Project`] timeline into composited
//! frames, and export those frames to a video file.
//!
//! Two layers sit here:
//!
//! - [`resolve`] is pure: it maps a project at a timeline instant to a [`Scene`]
//!   — the canvas plus an ordered stack of placed layers — without decoding or
//!   touching the GPU. All the geometry (canvas sizing, z-order, transforms,
//!   crop/mirror) lives here so it can be tested deterministically anywhere.
//! - [`Renderer`] realizes a [`Scene`]: it decodes each media frame with the
//!   platform codec, rasterizes text, and composites the stack on a `wgpu`
//!   device into an [`RgbaImage`]. `render_frame` ties the two together.
//!
//! The export path (`composite every frame 0..duration → encode`) builds on
//! `render_frame` and drives a [`aria_core::VideoEncoder`].

mod animation;
mod audio_dsp;
mod audio_fx;
mod audio_processor;
mod driver;
mod error;
mod export;
mod export_audio;
mod export_sound;
mod grade;
mod lut_pack;
mod motion_blur;
mod parent;
mod playback_meter;
mod render;
mod render_files;
mod resolve;
mod scene;
mod scene_layers;
mod transactional_file;

pub use animation::renders_animation;
pub use aria_compositor::FrameSink;
pub use aria_core::RgbaImage;
pub use audio_processor::{
    AudioBlock, AudioProcessor, AudioProcessorFactory, CostSink, FaultKind, FaultSink, ParamSignal,
    ProcessorFault, ProcessorRegistration, audio_processor_registered, register_audio_processor,
    register_audio_processor_with_cost,
};
pub use driver::{
    DEFAULT_DRIVER_BUDGET, DriverClipInfo, DriverFault, DriverFrame, DriverInput, DriverLanguage,
    DriverLanguageRegistration, DriverProgram, DriverScope, InputClip, InputProperty, check_driver,
    compile_driver, driven_envelope, driver_budget, driver_fault, driver_faults,
    driver_faults_revision, driver_input, driver_languages, driver_reference, driver_target,
    driver_template, register_driver_language, set_driver_budget,
};
pub use error::{EmptyExport, ExportFormatError, MIN_EXPORT_DIM, RenderError};
pub use export::{
    EncoderOpener, ExportObserver, ExportSettings, Exported, PngSequenceEncoder, decode_png,
    encode_png, even_export_size, export, export_config, export_config_with, export_frame_count,
    export_frame_png, export_observed, export_to_file, export_to_file_observed,
    export_to_file_with_encoder_observed,
};
#[doc(hidden)]
pub use export_audio::export_audio_bench;
pub use export_audio::{
    EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, ExportAudioMixer, sample_boundary,
};
pub use export_sound::{export_audio_config, export_audio_to_file_observed};
pub use lut_pack::{
    STARTER_LUT_SIZE, SWATCH_REFERENCE, StarterLut, StarterLutEntry, bake_starter_lut,
    starter_lut_entries, starter_lut_entry, starter_lut_pack, starter_lut_swatch,
};
pub use motion_blur::attach_motion_blur_passes;
pub use parent::{frame_of, parent_frame, reparented, world_pose};
pub use playback_meter::{DropCause, DroppedFrames, PlaybackMeter, PlaybackReport};
#[doc(hidden)]
pub use render::text_anim_bench;
pub use render::{
    FrameStats, GestureFrames, LayerCaret, Renderer, SeekPolicy, drawn_bounds, path_style,
};
pub use render_files::{FORMAT_VERSION, FrameKey, RenderFiles};
pub use resolve::{
    GESTURE_IDENTITY_TRANSFORM, GestureScenePartition, ResolveOverrides, canvas_size,
    derived_canvas_size, resolve, resolve_gesture_partitions, resolve_with,
};
pub use scene::{LayerSource, Scene, SceneLayer, SizeSpec, TextAnimation};
