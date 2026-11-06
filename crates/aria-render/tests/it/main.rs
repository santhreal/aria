//! Consolidated integration test binary for `aria-render`.
#![allow(clippy::duplicate_mod)]

#[path = "../audio_inserts.rs"]
mod audio_inserts;
#[path = "../export_frame.rs"]
mod export_frame;
#[cfg(target_vendor = "apple")]
#[path = "../export_mp4.rs"]
mod export_mp4;
#[path = "../export_sequence.rs"]
mod export_sequence;
#[path = "../export_sound.rs"]
mod export_sound;
#[path = "../lottie_render.rs"]
mod lottie_render;
#[path = "../lut_render.rs"]
mod lut_render;
#[path = "../offline_slate.rs"]
mod offline_slate;
#[path = "../registered_render.rs"]
mod registered_render;
#[path = "../render_files.rs"]
mod render_files;
#[path = "../render_frame.rs"]
mod render_frame;
#[path = "../sticker_render.rs"]
mod sticker_render;
#[path = "../still_render.rs"]
mod still_render;
#[path = "../text_caret_placement.rs"]
mod text_caret_placement;
#[path = "../text_composite.rs"]
mod text_composite;
#[path = "../text_glyph_smoke.rs"]
mod text_glyph_smoke;
#[path = "../text_keyframe_smoke.rs"]
mod text_keyframe_smoke;
#[path = "../zero_copy.rs"]
mod zero_copy;
