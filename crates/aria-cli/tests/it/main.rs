//! Consolidated integration test binary for `aria-cli`.
#![allow(clippy::duplicate_mod)]

#[path = "../command_help.rs"]
mod command_help;
#[path = "../drivers.rs"]
mod drivers;
#[path = "../export_encoder.rs"]
mod export_encoder;
#[path = "../headless_render.rs"]
mod headless_render;
#[path = "../inspect_pool.rs"]
mod inspect_pool;
#[cfg(unix)]
#[path = "../interrupt.rs"]
mod interrupt;
#[path = "../missing_plugins.rs"]
mod missing_plugins;
#[path = "../new_failure.rs"]
mod new_failure;
#[path = "../output_names.rs"]
mod output_names;
#[path = "../temp_dir.rs"]
mod temp_dir;
