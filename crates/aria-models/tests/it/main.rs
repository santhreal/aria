//! Consolidated integration test binary for `aria-models`.
#![allow(clippy::duplicate_mod)]

#[path = "../freeze_frame.rs"]
mod freeze_frame;
#[path = "../model.rs"]
mod model;
#[path = "../move_effect.rs"]
mod move_effect;
#[path = "../registered_audio_effects.rs"]
mod registered_audio_effects;
#[path = "../registered_catalog.rs"]
mod registered_catalog;
#[path = "../schema_stability.rs"]
mod schema_stability;
#[path = "../template.rs"]
mod template;
#[path = "../workflow.rs"]
mod workflow;
