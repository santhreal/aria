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
mod error;
mod export;
mod export_audio;
mod export_sound;
mod grade;
mod lut_pack;
mod motion_blur;
mod playback_meter;
mod render;
mod resolve;
mod scene;
mod scene_layers;
mod transactional_file;

pub use animation::renders_animation;
pub use aria_compositor::FrameSink;
pub use aria_core::RgbaImage;
pub use error::{EmptyExport, ExportFormatError, MIN_EXPORT_DIM, RenderError};
pub use export::{
    ExportObserver, ExportSettings, PngSequenceEncoder, decode_png, encode_png, even_export_size,
    export, export_config, export_config_with, export_frame_count, export_observed, export_to_file,
    export_to_file_observed,
};
#[doc(hidden)]
pub use export_audio::export_audio_bench;
pub use export_audio::{EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, ExportAudioMixer};
pub use export_sound::{export_audio_config, export_audio_to_file_observed};
pub use lut_pack::{
    STARTER_LUT_SIZE, SWATCH_REFERENCE, StarterLut, StarterLutEntry, bake_starter_lut,
    starter_lut_entries, starter_lut_entry, starter_lut_pack, starter_lut_swatch,
};
pub use playback_meter::{PlaybackMeter, PlaybackReport};
#[doc(hidden)]
pub use render::text_anim_bench;
pub use render::{FrameStats, GestureFrames, Renderer, SeekPolicy};
pub use resolve::{
    GESTURE_IDENTITY_TRANSFORM, GestureScenePartition, ResolveOverrides, canvas_size,
    derived_canvas_size, resolve, resolve_gesture_partitions, resolve_with,
};
pub use scene::{LayerSource, Scene, SceneLayer, SizeSpec, TextAnimation};
