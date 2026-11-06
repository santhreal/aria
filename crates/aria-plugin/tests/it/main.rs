//! Consolidated integration test binary for `aria-plugin`.
#![allow(clippy::duplicate_mod)]

#[path = "../audio_effects.rs"]
mod audio_effects;
#[path = "../contributions.rs"]
mod contributions;
#[path = "../decoders.rs"]
mod decoders;
#[path = "../event_wire.rs"]
mod event_wire;
#[path = "../example_packages.rs"]
mod example_packages;
#[path = "../expressions.rs"]
mod expressions;
#[path = "../jobs.rs"]
mod jobs;
#[path = "../manifest_reference/main.rs"]
mod manifest_reference;
#[path = "../native_abi_ai_tools.rs"]
mod native_abi_ai_tools;
#[path = "../native_abi_audio.rs"]
mod native_abi_audio;
#[path = "../native_abi_clip_types.rs"]
mod native_abi_clip_types;
#[path = "../native_abi_contributions.rs"]
mod native_abi_contributions;
#[path = "../native_abi_decoders.rs"]
mod native_abi_decoders;
#[path = "../native_abi_graph.rs"]
mod native_abi_graph;
#[path = "../native_abi_jobs.rs"]
mod native_abi_jobs;
#[path = "../native_abi_overlays.rs"]
mod native_abi_overlays;
#[path = "../native_abi_panels.rs"]
mod native_abi_panels;
#[path = "../native_abi_scopes.rs"]
mod native_abi_scopes;
#[path = "../package_graph.rs"]
mod package_graph;
#[path = "../project_storage.rs"]
mod project_storage;
#[path = "../render_host.rs"]
mod render_host;
