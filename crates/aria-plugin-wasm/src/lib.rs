//! WebAssembly component plugin tier (`world aria:plugin/plugin@1.0.0`) and hot reloader.
//!
//! Loads a [`Package`] whose tier is [`Tier::Wasm`] into a [`Box<dyn Plugin>`](Plugin)
//! backed by `wasmtime`'s Component Model runtime without WASI. Resource consumption is
//! bounded on every guest entry by [`Limits`]: instruction fuel, linear memory size, and
//! a wall-clock epoch deadline.

use std::path::PathBuf;
use std::time::Duration;

use aria_plugin::Plugin;
use aria_plugin::package::{API_MAJOR, Package, Tier};
use thiserror::Error;

#[allow(
    missing_debug_implementations,
    unreachable_pub,
    clippy::all,
    clippy::pedantic
)]
pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        world: "plugin",
        path: "wit",
    });
}

#[allow(
    missing_debug_implementations,
    unreachable_pub,
    clippy::all,
    clippy::pedantic
)]
pub(crate) mod legacy_bindings {
    wasmtime::component::bindgen!({
        inline: "
            package aria:legacy@1.0.0;
            world legacy-plugin {
                export mount: func() -> result<_, string>;
                export unmount: func() -> result<_, string>;
                export command-run: func(id: string, args-json: string) -> result<_, string>;
                export event: func(kind: string, event-json: string) -> result<option<string>, string>;
            }
        ",
    });
}

mod ai_tools;
mod audio;
mod contributions;
mod decoders;
mod encoders;
mod events;
mod host;
mod overlays;
mod panels;
mod reloader;
mod runtime;
mod scopes;

pub use host::WasmPlugin;
pub use reloader::{ReloadOutcome, Reloader};

/// The WIT world a component implements, as `package/world@version`.
pub const WORLD: &str = "aria:plugin/plugin@1.0.0";

/// Default instruction fuel granted to a single host-to-guest call.
pub const DEFAULT_FUEL_PER_CALL: u64 = 500_000_000;

/// Default linear memory ceiling per plugin instance (64 MiB).
pub const DEFAULT_MEMORY_BYTES: usize = 64 * 1024 * 1024;

/// Default wall-clock deadline for a single host-to-guest call (2 seconds).
pub const DEFAULT_CALL_DEADLINE: Duration = Duration::from_secs(2);

/// Resource limits enforced on a WebAssembly plugin instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Maximum WebAssembly instructions (fuel units) consumed by a single call.
    pub fuel_per_call: u64,
    /// Maximum linear memory size in bytes across the component instance.
    pub memory_bytes: usize,
    /// Maximum wall-clock duration for a single guest export call.
    pub call_deadline: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel_per_call: DEFAULT_FUEL_PER_CALL,
            memory_bytes: DEFAULT_MEMORY_BYTES,
            call_deadline: DEFAULT_CALL_DEADLINE,
        }
    }
}

/// Why a WebAssembly plugin package could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LoadError {
    /// The package is not a `Tier::Wasm` package.
    #[error("package at {} is not a WebAssembly component package", path.display())]
    NotWasmTier { path: PathBuf },
    /// The package manifest declares an incompatible plugin API major version.
    #[error("plugin {plugin} declares API major {declared}, host supports API major {supported}")]
    ApiMismatch {
        plugin: String,
        declared: u32,
        supported: u32,
    },
    /// The `.wasm` component file could not be read from disk.
    #[error("failed to read component {}: {reason}", path.display())]
    ReadComponent { path: PathBuf, reason: String },
    /// The shared `wasmtime::Engine` could not be initialized.
    #[error("failed to initialize WebAssembly engine: {0}")]
    Engine(String),
    /// The `.wasm` file is not a valid component for `aria:plugin/plugin@1.0.0`.
    #[error("failed to compile component {}: {reason}", path.display())]
    Compile { path: PathBuf, reason: String },
}

/// Load a [`Tier::Wasm`] [`Package`] with [`Limits`], returning a [`Box<dyn Plugin>`]
/// ready for [`PluginContext::mount`](aria_plugin::PluginContext::mount).
///
/// The package's storage directory defaults to `<package.dir>/plugin-data/<id>`. Use
/// [`load_with_data_root`] to place plugin storage under a shared profile data directory.
pub fn load(package: &Package, limits: &Limits) -> Result<Box<dyn Plugin>, LoadError> {
    load_with_data_root(package, limits, package.dir.clone())
}

/// Load a [`Tier::Wasm`] [`Package`] with [`Limits`] and an explicit `data_root`
/// for [`Package::data_dir`].
pub fn load_with_data_root(
    package: &Package,
    limits: &Limits,
    data_root: impl Into<PathBuf>,
) -> Result<Box<dyn Plugin>, LoadError> {
    if package.manifest.api != API_MAJOR {
        return Err(LoadError::ApiMismatch {
            plugin: package.manifest.id.clone(),
            declared: package.manifest.api,
            supported: API_MAJOR,
        });
    }
    let Tier::Wasm { component } = &package.tier else {
        return Err(LoadError::NotWasmTier {
            path: package.dir.clone(),
        });
    };
    let plugin = WasmPlugin::new(package, limits, data_root.into(), component)?;
    Ok(Box::new(plugin))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use wasmtime::Trap;

    use super::*;
    use crate::host::sanitize_relative_path;
    use crate::runtime::{classify_trap, deadline_ticks};

    /// The world named in load errors and by `aria plugin check` is the one
    /// `wit/plugin.wit` declares: a version bump in the WIT turns this red
    /// until the constant follows.
    #[test]
    fn the_world_constant_is_the_one_the_wit_declares() {
        let wit = include_str!("../wit/plugin.wit");
        let package = wit
            .lines()
            .find_map(|line| line.trim().strip_prefix("package "))
            .and_then(|rest| rest.strip_suffix(';'))
            .expect("plugin.wit declares a package");
        let (name, version) = package.split_once('@').expect("a versioned package");
        assert!(
            wit.lines().any(|line| line.trim() == "world plugin {"),
            "plugin.wit declares world plugin"
        );
        assert_eq!(WORLD, format!("{name}/plugin@{version}"));
    }

    #[test]
    fn sanitize_relative_path_rejects_traversal_and_absolute_keys() {
        for rejected in [
            "",
            ".",
            "..",
            "../secret",
            "nested/../../secret",
            "/etc/passwd",
            "bad\0key",
        ] {
            assert_eq!(
                sanitize_relative_path(rejected),
                None,
                "key {rejected:?} must be rejected"
            );
        }
        assert_eq!(
            sanitize_relative_path("a/./b/state.json"),
            Some(PathBuf::from("a/b/state.json"))
        );
    }

    #[test]
    fn deadline_ticks_bounds_zero_and_positive_durations() {
        assert_eq!(deadline_ticks(Duration::ZERO), 2);
        assert_eq!(deadline_ticks(Duration::from_millis(1)), 2);
        assert_eq!(deadline_ticks(Duration::from_millis(25)), 26);
        assert_eq!(deadline_ticks(Duration::from_secs(2)), 2001);
    }

    #[test]
    fn classify_trap_distinguishes_fuel_deadline_and_unreachable_traps() {
        let fuel = classify_trap(&wasmtime::Error::from(Trap::OutOfFuel));
        assert_eq!(fuel, "wasm trap: fuel exhausted");

        let deadline = classify_trap(&wasmtime::Error::from(Trap::Interrupt));
        assert_eq!(deadline, "wasm trap: call deadline exceeded");

        let unreachable = classify_trap(&wasmtime::Error::from(Trap::UnreachableCodeReached));
        assert!(
            unreachable.contains("unreachable code reached"),
            "{unreachable}"
        );
    }

    #[test]
    fn default_limits_match_documented_constants() {
        let limits = Limits::default();
        assert_eq!(limits.fuel_per_call, DEFAULT_FUEL_PER_CALL);
        assert_eq!(limits.memory_bytes, DEFAULT_MEMORY_BYTES);
        assert_eq!(limits.call_deadline, DEFAULT_CALL_DEADLINE);
    }
}
