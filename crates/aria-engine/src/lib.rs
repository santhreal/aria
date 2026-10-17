//! Aria editing engine: structured commands applied to a project with
//! inverse-based undo/redo.
//!
//! UI gestures and the AI agent both emit [`Command`]s; [`Engine::apply`]
//! executes them against the timeline and records an inverse, so every edit is
//! undoable (and gestures group into one history entry). Preview frames and
//! file export run on the GPU `aria-render` pipeline and the native
//! `aria-encoder` — there is no CPU compositing in this crate.

mod action;
mod bake;
mod engine;
mod error;
mod frame_stats_catalog;
mod import;
mod proxy;

pub use action::ApplyOutcome;
pub use aria_commands::{Command, EditCommand, EditOutcome, ProjectCommand};
pub use aria_models::BakeMedium;
pub use aria_render::{FrameKey, FrameStats, RenderFiles, SeekPolicy};
pub use bake::{BakePass, bake_project};
pub use engine::{Engine, EngineConfig, HistoryEntry};
pub use error::EngineError;
pub use frame_stats_catalog::{FrameStageSpec, frame_stages};
// The one reading of "what does this file state about itself": relink verifies
// a candidate with the same call that would adopt it, so a match cannot be
// judged by one conversion and applied by another.
pub use import::import_media;
pub use proxy::{PROXY_MAX_EDGE, proxy_project, proxy_size};
