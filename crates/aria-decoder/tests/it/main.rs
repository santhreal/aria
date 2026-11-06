//! Consolidated integration test binary for `aria-decoder`.
#![allow(clippy::duplicate_mod)]

#[cfg(target_os = "linux")]
#[path = "../gst_linux.rs"]
mod gst_linux;
#[path = "../image.rs"]
mod image;
