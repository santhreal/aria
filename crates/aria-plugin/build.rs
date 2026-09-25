//! Records the compiler that built this crate.
//!
//! Rust has no stable ABI, so a plugin library compiled by a different `rustc`
//! cannot be trusted to produce a `Box<dyn Plugin>` this binary can call. The
//! loader compares this string; nothing else in the crate uses it.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let version = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|out| out.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ARIA_PLUGIN_RUSTC={version}");
}
