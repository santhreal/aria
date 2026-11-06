//! Consolidated integration test binary for `aria-analysis`.
#![allow(clippy::duplicate_mod)]

#[path = "../audio_analysis.rs"]
mod audio_analysis;
#[path = "../moment_adapters.rs"]
mod moment_adapters;
#[path = "../moments.rs"]
mod moments;
#[cfg(feature = "sqlite")]
#[path = "../moments_sqlite.rs"]
mod moments_sqlite;
#[path = "../shot_detection.rs"]
mod shot_detection;
