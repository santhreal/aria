//! Loading a plugin that was not compiled into the app.
//!
//! A built-in plugin is a `Box<dyn Plugin>` the binary already contains. An
//! external one is a shared library in a directory the host scans at boot. Both
//! reach the same [`PluginContext::mount`], so an external plugin has every
//! capability a built-in has and no separate path through the host.
//!
//! # ABI
//!
//! Rust has no stable ABI, so a library built by a different compiler cannot be
//! trusted to hand back a `Box<dyn Plugin>`. Two symbols guard that:
//!
//! - `aria_plugin_abi_version() -> u32` must return [`ABI_VERSION`].
//! - `aria_plugin_build() -> *const c_char` must return the same
//!   `rustc`/crate build string the host was compiled with.
//!
//! Both come free from [`crate::export_plugin!`]. A mismatch is refused before
//! the plugin's own code runs, because a mismatched vtable layout is a crash
//! rather than an error.
//!
//! # Lifetime
//!
//! A loaded library stays mapped for the life of the process. A
//! [`PluginManifest`] borrows `&'static str` out of the
//! library's own data, and the context keeps a copy of that manifest, so
//! unloading the code would leave those strings pointing at an unmapped page.
//! Unmounting an external plugin therefore drops its registrations — its
//! services and event handlers — and leaves the code loaded.

use std::ffi::{CStr, OsStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use libloading::{Library, Symbol};

use crate::context::{MountFailure, PluginContext};
use crate::plugin::{Plugin, PluginManifest};

/// The loader contract's version. Bumped when the symbols, the `Plugin` trait,
/// the event bus, or a shipped event type change shape; a library built against
/// another one is refused. Version 8: `PluginContext` registers importers and
/// exporters and holds their registry.
pub const ABI_VERSION: u32 = 8;

/// Identifies the compiler and crate version the host was built with. Two
/// libraries agree on the `dyn Plugin` vtable only if this string matches.
pub const BUILD_TAG: &str = concat!(
    "aria-plugin ",
    env!("CARGO_PKG_VERSION"),
    " rustc ",
    env!("ARIA_PLUGIN_RUSTC")
);

/// The file extension a plugin library has on this platform.
pub const LIBRARY_EXTENSION: &str = if cfg!(target_os = "windows") {
    "dll"
} else if cfg!(target_os = "macos") {
    "dylib"
} else {
    "so"
};

type AbiVersionFn = unsafe extern "C" fn() -> u32;
type BuildTagFn = unsafe extern "C" fn() -> *const c_char;
type CreateFn = unsafe extern "C" fn() -> *mut c_void;

/// Export a plugin from a `cdylib`. One line in the plugin's `lib.rs`:
///
/// ```ignore
/// aria_plugin::export_plugin!(NotesPlugin);
/// ```
///
/// The expression is evaluated once, when the host asks for the plugin.
#[macro_export]
macro_rules! export_plugin {
    ($plugin:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn aria_plugin_abi_version() -> u32 {
            $crate::external::ABI_VERSION
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn aria_plugin_build() -> *const ::core::ffi::c_char {
            $crate::external::build_tag_ptr()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn aria_plugin_create() -> *mut ::core::ffi::c_void {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                let plugin: ::std::boxed::Box<dyn $crate::Plugin> = ::std::boxed::Box::new($plugin);
                ::std::boxed::Box::into_raw(::std::boxed::Box::new(plugin))
                    as *mut ::core::ffi::c_void
            }));
            match result {
                ::core::result::Result::Ok(ptr) => ptr,
                ::core::result::Result::Err(_) => ::core::ptr::null_mut(),
            }
        }
    };
}

/// The host's build tag as a C string, for [`export_plugin!`]. Not called
/// directly.
pub fn build_tag_ptr() -> *const c_char {
    static TAG: LazyLock<std::ffi::CString> =
        LazyLock::new(|| std::ffi::CString::new(BUILD_TAG).expect("build tag has no interior nul"));
    TAG.as_ptr()
}

/// A plugin and the library it came from. The library field is last so the
/// plugin is dropped before the code that defined it, on the one path that drops
/// this at all.
struct ExternalPlugin {
    plugin: Box<dyn Plugin>,
    _library: Library,
}

/// Every library loaded this process. Holds the code mapped for as long as the
/// manifests taken out of it are readable, which is until exit.
static LOADED: LazyLock<Mutex<Vec<ExternalPlugin>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// The plugin libraries in `dir`, in path order.
///
/// Sorted so mount order is the same on every run: two plugins that both claim a
/// file type must not elect a different winner because a directory listing came
/// back in another order. A missing directory is not an error — most installs
/// have no external plugins.
pub fn library_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension() == Some(OsStr::new(LIBRARY_EXTENSION)))
        .collect();
    paths.sort();
    paths
}

/// Whether a library that declares `abi` and `tag` may be called.
///
/// Pulled out of [`load`] because it is the whole safety argument for the
/// `Box<dyn Plugin>` that follows, and it is the one part that can be checked
/// without building a library for every wrong answer. Both refusals name what the
/// library says and what this build requires, because the fix is to rebuild
/// against the named one.
fn check_compat(abi: u32, tag: &str) -> Result<(), String> {
    if abi != ABI_VERSION {
        return Err(format!(
            "built for plugin ABI {abi}, this build loads ABI {ABI_VERSION}"
        ));
    }
    if tag != BUILD_TAG {
        return Err(format!("built by {tag}, this build loads {BUILD_TAG}"));
    }
    Ok(())
}

#[cfg(test)]
mod compat_tests {
    use super::*;

    /// A library built by this compiler against this crate is the only accepted
    /// combination, and the exported constants are what a plugin reports.
    #[test]
    fn the_hosts_own_build_is_accepted() {
        assert_eq!(check_compat(ABI_VERSION, BUILD_TAG), Ok(()));
        assert!(BUILD_TAG.contains(env!("CARGO_PKG_VERSION")));
    }

    /// A stale plugin names both versions, so the reader knows what to rebuild
    /// against rather than being told it is incompatible.
    #[test]
    fn a_stale_abi_is_refused_and_names_both_versions() {
        for stale in [0, ABI_VERSION - 1, ABI_VERSION + 1, u32::MAX] {
            let err = check_compat(stale, BUILD_TAG).expect_err("stale ABI must be refused");
            assert!(err.contains(&stale.to_string()), "{err}");
            assert!(err.contains(&ABI_VERSION.to_string()), "{err}");
        }
    }

    /// The same ABI compiled by a different rustc is still a different vtable, so
    /// the build tag is checked as well as the version number.
    #[test]
    fn a_matching_abi_from_another_compiler_is_refused() {
        let err = check_compat(
            ABI_VERSION,
            "aria-plugin 0.7.0-alpha.0 rustc 1.0.0 (deadbeef)",
        )
        .expect_err("another compiler must be refused");
        assert!(err.contains("1.0.0"), "{err}");
        assert!(err.contains(BUILD_TAG), "{err}");
    }
}

/// Load one plugin library and take the plugin out of it.
///
/// The ABI and build checks run before `aria_plugin_create`, so a library built
/// against a different compiler is refused rather than called.
fn load(path: &Path) -> Result<ExternalPlugin, String> {
    // SAFETY: dlopen runs the library's initialisers. There is no way to load
    // native code without trusting it; the guard below is against a stale build,
    // not against a hostile one.
    let library = unsafe { Library::new(path) }.map_err(|err| err.to_string())?;

    // The symbols borrow the library, so they are resolved and dropped here,
    // before the library moves into the returned value.
    let plugin = {
        let abi: Symbol<AbiVersionFn> = unsafe { library.get(b"aria_plugin_abi_version") }
            .map_err(|_| "not an aria plugin: no aria_plugin_abi_version symbol".to_string())?;
        let abi = unsafe { abi() };

        let build: Symbol<BuildTagFn> = unsafe { library.get(b"aria_plugin_build") }
            .map_err(|_| "not an aria plugin: no aria_plugin_build symbol".to_string())?;
        let tag = unsafe { CStr::from_ptr(build()) }
            .to_str()
            .map_err(|_| "build tag is not UTF-8".to_string())?
            .to_string();

        check_compat(abi, &tag)?;

        let create: Symbol<CreateFn> = unsafe { library.get(b"aria_plugin_create") }
            .map_err(|_| "not an aria plugin: no aria_plugin_create symbol".to_string())?;
        let raw = unsafe { create() } as *mut Box<dyn Plugin>;
        if raw.is_null() {
            return Err("aria_plugin_create returned nothing".to_string());
        }
        // SAFETY: `export_plugin!` produced this pointer with `Box::into_raw` on
        // a `Box<Box<dyn Plugin>>`, and the ABI and build checks above establish
        // that the library was compiled against this crate.
        *unsafe { Box::from_raw(raw) }
    };

    Ok(ExternalPlugin {
        plugin,
        _library: library,
    })
}

/// Load and mount every plugin library in `dir`.
///
/// A library that will not load, or a plugin that will not mount, is one entry in
/// the returned failures and does not stop the rest: a stale plugin file left in
/// the directory must not keep the app from starting. Failures also land in the
/// context's own ledger, so the plugin list shows them next to a built-in that
/// failed.
pub fn mount_dir(ctx: &PluginContext, dir: &Path) -> Vec<MountFailure> {
    let mut failures = Vec::new();
    for path in library_paths(dir) {
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        match load(&path) {
            Ok(external) => {
                let manifest = *external.plugin.manifest();
                let mounted = ctx.mount(external.plugin.as_ref());
                // Held whether or not the mount took: a refused plugin's code is
                // still mapped, and the manifest copy above points into it.
                LOADED.lock().expect("plugin registry").push(external);
                if let Err(err) = mounted {
                    failures.push(external_failure(
                        manifest.id,
                        manifest.name,
                        err.to_string(),
                    ));
                }
            }
            Err(reason) => {
                let failure = ctx.record_external_failure(name.clone(), name, reason);
                failures.push(failure);
            }
        }
    }
    failures
}

fn external_failure(id: &str, name: &str, reason: String) -> MountFailure {
    MountFailure {
        id: id.to_string(),
        name: name.to_string(),
        reason,
    }
}

/// The manifests of the external plugins loaded this process, in load order.
pub fn loaded() -> Vec<PluginManifest> {
    LOADED
        .lock()
        .expect("plugin registry")
        .iter()
        .map(|external| *external.plugin.manifest())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aria-external-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// A directory with no plugins is the normal install, not an error.
    #[test]
    fn a_missing_directory_has_no_plugins() {
        let dir = std::env::temp_dir().join("aria-external-absent");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(library_paths(&dir).is_empty());
    }

    /// The scan takes libraries only, in a fixed order, so mount order does not
    /// depend on how the filesystem lists a directory.
    #[test]
    fn only_libraries_are_candidates_and_the_order_is_stable() {
        let dir = scratch("scan");
        for name in [
            format!("b.{LIBRARY_EXTENSION}"),
            format!("a.{LIBRARY_EXTENSION}"),
            "notes.txt".to_string(),
            "a.so.disabled".to_string(),
        ] {
            std::fs::write(dir.join(name), b"x").expect("write candidate");
        }
        let found: Vec<String> = library_paths(&dir)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            found,
            vec![
                format!("a.{LIBRARY_EXTENSION}"),
                format!("b.{LIBRARY_EXTENSION}")
            ]
        );
    }

    /// A file with the right extension and the wrong contents names itself in the
    /// failure, and the app keeps going.
    #[test]
    fn a_file_that_is_not_a_library_is_one_named_failure() {
        let dir = scratch("garbage");
        let path = dir.join(format!("broken.{LIBRARY_EXTENSION}"));
        std::fs::write(&path, b"not a shared object").expect("write candidate");

        let ctx = PluginContext::new();
        let failures = mount_dir(&ctx, &dir);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].id, format!("broken.{LIBRARY_EXTENSION}"));
        assert!(
            !failures[0].reason.is_empty(),
            "a refused plugin gives a reason"
        );
        // The host's ledger has it too, so the plugin list can show it.
        assert_eq!(ctx.failures().len(), 1);
        assert!(ctx.plugins().is_empty());
    }

    /// A real library that is not an aria plugin is refused by symbol, not by
    /// crashing on a missing entry point.
    #[test]
    fn a_library_without_the_entry_points_is_refused() {
        let Some(path) = system_library() else {
            return;
        };
        let err = match load(&path) {
            Ok(_) => panic!("a system library must not load as an aria plugin"),
            Err(err) => err,
        };
        assert!(
            err.contains("not an aria plugin"),
            "unexpected refusal: {err}"
        );
    }

    /// A library every Linux and macOS host has, used only to prove the symbol
    /// check refuses a real `.so` rather than trusting the extension.
    fn system_library() -> Option<PathBuf> {
        [
            "/lib/x86_64-linux-gnu/libc.so.6",
            "/lib64/libc.so.6",
            "/usr/lib/libSystem.B.dylib",
        ]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
    }
}
