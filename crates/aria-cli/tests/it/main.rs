//! Consolidated integration test binary for `aria-cli`.
#![allow(clippy::duplicate_mod)]

#[path = "../drivers.rs"]
mod drivers;
#[path = "../export_encoder.rs"]
mod export_encoder;
#[path = "../headless_render.rs"]
mod headless_render;
#[path = "../inspect_pool.rs"]
mod inspect_pool;
#[path = "../output_names.rs"]
mod output_names;
