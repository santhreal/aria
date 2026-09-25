//! The seam between platform media decoding and Aria's detectors.
//!
//! `aria-analysis` is deliberately dependency-light: it compares RGBA frames and
//! interleaved PCM and knows nothing about files or codecs. `aria-decoder` knows
//! codecs and knows nothing about detection. Driving one from the other is a real
//! job with real decisions — sample rate, bounds, pixel-format conversion,
//! cancellation — and it has exactly one owner, here, so that every caller
//! (the editor, the CLI, a plugin) detects the same way on the same input.
//!
//! Nothing in this crate touches a UI, a job runner, or a project. A caller
//! passes a path and a `cancelled` predicate and gets detector input back.

mod audio;
mod shots;

pub use audio::{
    ANALYSIS_SAMPLE_RATE, DEFAULT_MAX_ANALYSIS_FRAMES, MAX_ANALYSIS_FRAMES, decode_mono,
    decode_mono_with_limit, seconds_of, validated_pcm,
};
pub use shots::{
    MAX_SHOT_SECONDS, SAMPLE_EDGE, SHOT_SAMPLE_FPS, convert_into, detect_shots_sampled,
};
