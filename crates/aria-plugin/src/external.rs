//! Native C-ABI plugin loader (`aria-plugin-abi` v1).
//!
//! A native plugin package (`plugin.toml` + `lib/<os>-<arch>.<so|dylib|dll>`)
//! exports `aria_plugin_entry(host, api_major) -> *const AriaPluginV1`. Both
//! tables carry `struct_size` and `api_major` headers and their own allocator
//! release functions (`free_string`, `free_bytes`), so a plugin compiled by a
//! different `rustc` version or a C compiler loads without sharing Aria's Rust
//! vtable layout or allocator.
//!
//! # Lifetime and Unload
//!
//! [`PluginManifest`] and [`Package`] own their strings, so no `'static` slice
//! borrows memory inside the mapped shared library. The loaded plugin's state
//! holds the [`libloading::Library`] as its final field inside an [`Arc`]
//! shared by the plugin handle and every closure or processor registered into
//! [`PluginContext`]. Unmounting or failing the plugin drops its registrations;
//! once no callback is in flight and the plugin handle is dropped, the `Arc`
//! count reaches zero and the operating system unloads the shared library.

#![allow(unsafe_code)]

mod ai_tools;
mod contributions;
mod decoders;
mod events;
mod host_table;
mod scopes;

use std::ffi::c_void;
use std::panic::resume_unwind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};

use aria_plugin_abi::{
    ARIA_API_MAJOR, ARIA_BYTES_FOUND, ARIA_BYTES_PANIC, ARIA_INSTANCE_OK, ARIA_INSTANCE_PANIC,
    ARIA_PLUGIN_ENTRY_SYMBOL, ARIA_STATUS_OK, ARIA_STATUS_PANIC, AriaAudioBlock, AriaAudioParam,
    AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame, AriaInstanceResult, AriaJobStep,
    AriaOwnedStr, AriaPluginEntryFn, AriaPluginV1, AriaResult, AriaStr, AriaTableHeader,
    validate_plugin_table,
};
use libloading::{Library, Symbol};
use parking_lot::Mutex;

use self::events::UnmountSentinel;
use self::host_table::{HOST_V1, register_native_command};
use crate::context::{MountFailure, PluginContext};
use crate::error::PluginError;
use crate::package::{DataPlugin, Package, Tier, discover, register_declared};
use crate::plugin::{Plugin, PluginManifest};
use crate::project_host::TransactError;
use crate::service::StorageService;

/// Stable C ABI major version loaded by this build.
pub const ABI_VERSION: u32 = ARIA_API_MAJOR;

/// Platform file extension for a native plugin shared library (`so`, `dylib`, or `dll`).
pub const LIBRARY_EXTENSION: &str = std::env::consts::DLL_EXTENSION;

/// Maximum JSON byte size accepted from `panel_view` before parsing (`MAX_TREE_TEXT_BYTES + MAX_TREE_IMAGE_BYTES * 4`).
pub const MAX_PANEL_VIEW_JSON_BYTES: usize =
    crate::MAX_TREE_TEXT_BYTES + crate::MAX_TREE_IMAGE_BYTES * 4;

/// Maximum JSON byte size accepted from `overlay_scene` before parsing: 256 bytes of JSON envelope per shape (`MAX_SHAPES`), 32 bytes per path point (`MAX_PATH_POINTS`), plus the scene text budget (`MAX_TREE_TEXT_BYTES`).
pub const MAX_OVERLAY_SCENE_JSON_BYTES: usize = crate::viewer_overlay::MAX_SHAPES * 256
    + crate::viewer_overlay::MAX_PATH_POINTS * 32
    + crate::MAX_TREE_TEXT_BYTES;

fn refused_tree(reason: impl std::fmt::Display) -> crate::UiNode {
    crate::UiNode::Button {
        id: format!("\0{reason}").into(),
        label: Arc::from(""),
        disabled: true,
    }
}

fn refused_scene(reason: impl std::fmt::Display) -> crate::OverlayScene {
    let raw = reason.to_string();
    let mut sanitized = String::with_capacity(raw.len().min(crate::MAX_WIDGET_ID_BYTES));
    for ch in raw.chars().filter(|c| !c.is_control()) {
        if sanitized.len() + ch.len_utf8() > crate::MAX_WIDGET_ID_BYTES {
            break;
        }
        sanitized.push(ch);
    }
    let id: Arc<str> = if sanitized.trim().is_empty() {
        Arc::from("malformed overlay scene JSON")
    } else {
        Arc::from(sanitized)
    };
    let handle = crate::OverlayShape::Handle {
        id,
        at: [0.0, 0.0],
        color: crate::OverlayColor::Accent,
        look: crate::HandleLook::Dot,
    };
    crate::OverlayScene {
        shapes: vec![handle.clone(), handle],
    }
}

/// Registry of packages mounted through [`mount_dir`].
static LOADED: LazyLock<Mutex<Vec<PluginManifest>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Return a reference to the static [`aria_plugin_abi::AriaHostV1`] callback
/// table.
pub fn host_table() -> &'static aria_plugin_abi::AriaHostV1 {
    &HOST_V1
}

/// Check whether a plugin table declaring `api_major` and `struct_size` is
/// compatible with this host build.
pub fn check_compat(api_major: u32, struct_size: usize) -> Result<(), String> {
    validate_plugin_table(api_major, struct_size)
}

/// Load a native plugin from `package`.
///
/// Storage operations resolve `<data_root>/plugin-data/<id>` from the
/// [`StorageService`] registered in the [`PluginContext`] when mounted, or
/// fall back to the parent directory of `package.dir`.
pub fn load(package: &Package) -> Result<Box<dyn Plugin>, String> {
    load_optional_root(package, None)
}

/// Load a native plugin from `package` with an explicit `data_root` for
/// `<data_root>/plugin-data/<id>`.
pub fn load_with_data_root(package: &Package, data_root: &Path) -> Result<Box<dyn Plugin>, String> {
    load_optional_root(package, Some(data_root))
}

fn load_optional_root(
    package: &Package,
    data_root: Option<&Path>,
) -> Result<Box<dyn Plugin>, String> {
    if package.manifest.api != ABI_VERSION {
        return Err(format!(
            "package `{}` declares api = {}, host requires api = {ABI_VERSION}",
            package.manifest.id, package.manifest.api
        ));
    }
    let Tier::Native { library } = &package.tier else {
        return Err(format!(
            "package `{}` is not a native plugin",
            package.manifest.id
        ));
    };
    load_library(package, library, data_root)
}

/// Load a native plugin library from `path` for `package`.
pub fn load_library(
    package: &Package,
    path: &Path,
    data_root: Option<&Path>,
) -> Result<Box<dyn Plugin>, String> {
    // SAFETY: `Library::new` calls `dlopen`/`LoadLibrary` on the package's shared object.
    let library = unsafe { Library::new(path) }.map_err(|err| err.to_string())?;
    let table = {
        // SAFETY: Looking up the C entry function pointer symbol by name.
        let entry: Symbol<AriaPluginEntryFn> = unsafe { library.get(ARIA_PLUGIN_ENTRY_SYMBOL) }
            .map_err(|_| "not an aria plugin: no aria_plugin_entry symbol".to_owned())?;
        // SAFETY: Passing a valid static `AriaHostV1` pointer and `ABI_VERSION` to the C entry point.
        let table_ptr = unsafe { entry(&raw const HOST_V1, ABI_VERSION) };
        if table_ptr.is_null() {
            return Err(format!(
                "aria_plugin_entry returned null for API major {ABI_VERSION}"
            ));
        }
        // SAFETY: Every `AriaPluginV1` starts with `AriaTableHeader` (`struct_size`, `api_major`, `reserved`).
        let header = unsafe { *(table_ptr as *const AriaTableHeader) };
        check_compat(header.api_major, header.struct_size)?;
        // SAFETY: `check_compat` verified `struct_size >= ARIA_PLUGIN_V1_MIN_SIZE` and `api_major == ABI_VERSION`.
        unsafe { AriaPluginV1::copy_from_prefix(table_ptr, header.struct_size) }
    };
    from_table_with_library(package, &table, data_root, Some(library))
}

/// Construct a native [`Plugin`] from a validated [`AriaPluginV1`] table and [`Package`].
pub fn from_table(
    package: &Package,
    table: AriaPluginV1,
    data_root: Option<&Path>,
) -> Result<Box<dyn Plugin>, String> {
    check_compat(table.api_major, table.struct_size)?;
    from_table_with_library(package, &table, data_root, None)
}

fn from_table_with_library(
    package: &Package,
    table: &AriaPluginV1,
    data_root: Option<&Path>,
    library: Option<Library>,
) -> Result<Box<dyn Plugin>, String> {
    let manifest = package.plugin_manifest();
    let state = Arc::new(NativePluginState {
        id: package.manifest.id.clone(),
        package: package.clone(),
        data_root: data_root.map(Path::to_path_buf),
        table: *table,
        mounted: AtomicBool::new(false),
        failed: AtomicBool::new(false),
        next_job: AtomicU64::new(1),
        _library: library,
    });
    Ok(Box::new(NativePlugin {
        package: package.clone(),
        manifest,
        state,
    }))
}

/// Discover and mount every native or data package in `dir`.
pub fn mount_dir(ctx: &PluginContext, dir: &Path) -> Vec<MountFailure> {
    let mut failures = Vec::new();
    for entry in discover(dir) {
        match entry {
            Ok(package) => match &package.tier {
                Tier::Native { .. } => match load(&package) {
                    Ok(plugin) => {
                        let manifest = plugin.manifest().clone();
                        let mounted = ctx.mount(plugin.as_ref());
                        record_loaded(&manifest);
                        if let Err(err) = mounted {
                            failures.push(MountFailure {
                                id: manifest.id.to_string(),
                                name: manifest.name.to_string(),
                                reason: err.to_string(),
                            });
                        }
                    }
                    Err(reason) => {
                        let failure = ctx.record_external_failure(
                            package.manifest.id.clone(),
                            package.manifest.name.clone(),
                            reason,
                        );
                        failures.push(failure);
                    }
                },
                Tier::Data => {
                    let plugin = DataPlugin::new(package);
                    let manifest = plugin.manifest().clone();
                    let mounted = ctx.mount(&plugin);
                    record_loaded(&manifest);
                    if let Err(err) = mounted {
                        failures.push(MountFailure {
                            id: manifest.id.to_string(),
                            name: manifest.name.to_string(),
                            reason: err.to_string(),
                        });
                    }
                }
                Tier::Wasm { .. } => {}
            },
            Err(err) => {
                let name = err
                    .path
                    .file_name()
                    .unwrap_or(err.path.as_os_str())
                    .to_string_lossy()
                    .into_owned();
                let failure = ctx.record_external_failure(name.clone(), name, err.reason);
                failures.push(failure);
            }
        }
    }
    failures
}

fn record_loaded(manifest: &PluginManifest) {
    let mut list = LOADED.lock();
    list.retain(|existing| existing.id != manifest.id);
    list.push(manifest.clone());
}

/// Manifests of the external plugins loaded through [`mount_dir`], in load order.
pub fn loaded() -> Vec<PluginManifest> {
    LOADED.lock().clone()
}

pub(crate) struct HostCallContext<'a> {
    pub(crate) state: &'a Arc<NativePluginState>,
    pub(crate) scoped_ctx: &'a PluginContext,
    pub(crate) unscoped_ctx: PluginContext,
    pub(crate) in_mount: bool,
}

impl HostCallContext<'_> {
    pub(crate) fn storage_dir(&self) -> PathBuf {
        self.state.resolve_storage_dir(self.scoped_ctx)
    }
}

pub(crate) enum ExportCallError {
    Failed(String),
    Panicked(String),
}

struct NativePlugin {
    package: Package,
    manifest: PluginManifest,
    state: Arc<NativePluginState>,
}

impl std::fmt::Debug for NativePlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativePlugin")
            .field("id", &self.manifest.id)
            .field("version", &self.manifest.version)
            .finish()
    }
}

struct UnmountGuard {
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl Drop for UnmountGuard {
    fn drop(&mut self) {
        self.state.call_unmount(&self.scoped_ctx);
    }
}

impl Plugin for NativePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        if self.state.is_failed() {
            return Err(PluginError::MountFailed(
                self.manifest.id.clone(),
                "plugin has faulted in this session".to_owned(),
            ));
        }
        register_declared(ctx, &self.package)?;
        let guard = UnmountGuard {
            state: Arc::clone(&self.state),
            scoped_ctx: ctx.clone(),
        };
        ctx.on_event(move |_ev: &UnmountSentinel| {
            let _keep = &guard;
        });
        self.state.call_mount(ctx)?;
        for cmd in &self.package.manifest.commands {
            let already_registered = ctx.commands().iter().any(|existing| existing.id == cmd.id);
            if !already_registered {
                register_native_command(ctx, &self.state, cmd.id.clone(), cmd.title.clone())
                    .map_err(|reason| PluginError::MountFailed(self.manifest.id.clone(), reason))?;
            }
        }
        self.state.mounted.store(true, Ordering::Release);
        Ok(())
    }

    fn unmount(&self, ctx: &PluginContext) {
        self.state.call_unmount(ctx);
    }
}

pub(crate) struct NativePluginState {
    pub(crate) id: String,
    pub(crate) package: Package,
    pub(crate) data_root: Option<PathBuf>,
    pub(crate) table: AriaPluginV1,
    pub(crate) mounted: AtomicBool,
    pub(crate) failed: AtomicBool,
    /// The id the next `start_job` hands the plugin.
    pub(crate) next_job: AtomicU64,
    // Field order is load-bearing: `_library` drops last, after all state references release.
    _library: Option<Library>,
}

impl NativePluginState {
    pub(crate) fn is_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    pub(crate) fn resolve_storage_dir(&self, scoped_ctx: &PluginContext) -> PathBuf {
        if let Some(root) = &self.data_root {
            return self.package.data_dir(root);
        }
        let unscoped = scoped_ctx.scoped(None);
        if let Some(storage) = unscoped.try_get::<dyn StorageService>() {
            let cache = storage.cache_dir();
            let root = cache.parent().unwrap_or(cache);
            return self.package.data_dir(root);
        }
        let pkg_dir = &self.package.dir;
        let inferred_root = pkg_dir
            .parent()
            .and_then(|parent| {
                if parent.file_name().is_some_and(|name| name == "plugins") {
                    parent.parent()
                } else {
                    Some(parent)
                }
            })
            .unwrap_or(pkg_dir);
        self.package.data_dir(inferred_root)
    }

    fn take_guest_string(&self, owned: AriaOwnedStr) -> String {
        if owned.ptr.is_null() {
            return String::new();
        }
        // SAFETY: `owned` was returned by the guest and is valid until `free_string` is called.
        let text = unsafe {
            let bytes = std::slice::from_raw_parts(owned.ptr, owned.len);
            String::from_utf8_lossy(bytes).into_owned()
        };
        // SAFETY: `self.table.free_string` is the guest's allocator release function for `owned`.
        unsafe { (self.table.free_string)(owned) };
        text
    }

    fn decode_result(&self, result: AriaResult) -> Result<String, (bool, String)> {
        let text = self.take_guest_string(result.value);
        match result.status {
            ARIA_STATUS_OK => Ok(text),
            ARIA_STATUS_PANIC => Err((true, text)),
            _ => Err((false, text)),
        }
    }

    fn decode_instance(&self, result: AriaInstanceResult) -> Result<u64, (bool, String)> {
        let err_text = self.take_guest_string(result.error);
        match result.status {
            ARIA_INSTANCE_OK => Ok(result.instance),
            ARIA_INSTANCE_PANIC => Err((true, err_text)),
            _ => Err((false, err_text)),
        }
    }

    fn mark_faulted(&self, scoped_ctx: &PluginContext, reason: String) {
        self.failed.store(true, Ordering::Release);
        self.mounted.store(false, Ordering::Release);
        let unscoped = scoped_ctx.scoped(None);
        unscoped.fail(&self.id, reason);
    }

    pub(crate) fn call_mount(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
    ) -> Result<(), PluginError> {
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: true,
        };
        // SAFETY: `call_ctx` lives on the stack for the duration of `table.mount`.
        let raw = unsafe { (self.table.mount)((&raw mut call_ctx).cast::<c_void>()) };
        match self.decode_result(raw) {
            Ok(_) => Ok(()),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                Err(PluginError::MountFailed(
                    self.id.clone().into(),
                    format!("panic: {msg}"),
                ))
            }
            Err((false, msg)) => Err(PluginError::MountFailed(self.id.clone().into(), msg)),
        }
    }

    pub(crate) fn call_unmount(self: &Arc<Self>, scoped_ctx: &PluginContext) {
        if !self.mounted.swap(false, Ordering::AcqRel) || self.is_failed() {
            return;
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` lives on the stack for the duration of `table.unmount`.
        let raw = unsafe { (self.table.unmount)((&raw mut call_ctx).cast::<c_void>()) };
        if let Err((true, msg)) = self.decode_result(raw) {
            self.mark_faulted(scoped_ctx, format!("unmount panicked: {msg}"));
        }
    }

    pub(crate) fn call_command_run(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        id: &str,
        args_json: &str,
    ) -> Result<(), String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx`, `id`, and `args_json` outlive `table.command_run`.
        let raw = unsafe {
            (self.table.command_run)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(id),
                AriaStr::from_str(args_json),
            )
        };
        match self.decode_result(raw) {
            Ok(_) => Ok(()),
            Err((false, msg)) => Err(msg),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(msg));
            }
        }
    }

    pub(crate) fn call_event(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        kind: &str,
        event_json: &str,
    ) -> Result<String, ()> {
        if self.is_failed() {
            return Err(());
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx`, `kind`, and `event_json` outlive `table.event`.
        let raw = unsafe {
            (self.table.event)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(kind),
                AriaStr::from_str(event_json),
            )
        };
        match self.decode_result(raw) {
            Ok(response) => Ok(response),
            Err((false, msg)) => {
                tracing::warn!(plugin = %self.id, kind = %kind, error = %msg, "plugin event handler returned error");
                Err(())
            }
            Err((true, msg)) => {
                self.mark_faulted(scoped_ctx, format!("event {kind} panicked: {msg}"));
                Err(())
            }
        }
    }

    pub(crate) fn call_import(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        importer_id: &str,
        path: &str,
    ) -> Result<String, TransactError> {
        if self.is_failed() {
            return Err(TransactError::Aborted(format!(
                "plugin `{}` has faulted",
                self.id
            )));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx`, `importer_id`, and `path` outlive `table.import_file`.
        let raw = unsafe {
            (self.table.import_file)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(importer_id),
                AriaStr::from_str(path),
            )
        };
        match self.decode_result(raw) {
            Ok(json) => Ok(json),
            Err((false, msg)) => Err(TransactError::Aborted(msg)),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                Err(TransactError::Panicked(msg))
            }
        }
    }

    pub(crate) fn call_export(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        exporter_id: &str,
        project_json: &str,
        path: &str,
    ) -> Result<String, ExportCallError> {
        if self.is_failed() {
            return Err(ExportCallError::Failed(format!(
                "plugin `{}` has faulted",
                self.id
            )));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx`, `exporter_id`, `project_json`, and `path` outlive `table.export_file`.
        let raw = unsafe {
            (self.table.export_file)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(exporter_id),
                AriaStr::from_str(project_json),
                AriaStr::from_str(path),
            )
        };
        match self.decode_result(raw) {
            Ok(json) => Ok(json),
            Err((false, msg)) => Err(ExportCallError::Failed(msg)),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                Err(ExportCallError::Panicked(msg))
            }
        }
    }

    pub(crate) fn call_audio_create(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        effect_id: &str,
        sample_rate: u32,
    ) -> Result<u64, String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` and `effect_id` outlive `table.audio_create`.
        let raw = unsafe {
            (self.table.audio_create)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(effect_id),
                sample_rate,
            )
        };
        match self.decode_instance(raw) {
            Ok(instance) => Ok(instance),
            Err((is_panic, msg)) => {
                if is_panic {
                    self.failed.store(true, Ordering::Release);
                    self.mounted.store(false, Ordering::Release);
                }
                Err(msg)
            }
        }
    }

    pub(crate) fn call_audio_process(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        _effect_id: &str,
        instance: u64,
        params: &[AriaAudioParam],
        block: &mut AriaAudioBlock,
    ) -> Result<(), String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx`, `params`, and `block` outlive `table.audio_process`.
        let raw = unsafe {
            (self.table.audio_process)(
                (&raw mut call_ctx).cast::<c_void>(),
                instance,
                params.as_ptr(),
                params.len(),
                &raw mut *block,
            )
        };
        match self.decode_result(raw) {
            Ok(_) => Ok(()),
            Err((is_panic, msg)) => {
                if is_panic {
                    self.failed.store(true, Ordering::Release);
                    self.mounted.store(false, Ordering::Release);
                }
                Err(msg)
            }
        }
    }

    pub(crate) fn call_audio_reset(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        _effect_id: &str,
        instance: u64,
    ) -> Result<(), String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` outlives `table.audio_reset`.
        let raw =
            unsafe { (self.table.audio_reset)((&raw mut call_ctx).cast::<c_void>(), instance) };
        match self.decode_result(raw) {
            Ok(_) => Ok(()),
            Err((is_panic, msg)) => {
                if is_panic {
                    self.failed.store(true, Ordering::Release);
                    self.mounted.store(false, Ordering::Release);
                }
                Err(msg)
            }
        }
    }

    pub(crate) fn call_audio_drop(self: &Arc<Self>, scoped_ctx: &PluginContext, instance: u64) {
        if self.is_failed() {
            return;
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` outlives `table.audio_drop`.
        let raw =
            unsafe { (self.table.audio_drop)((&raw mut call_ctx).cast::<c_void>(), instance) };
        if let Err((true, msg)) = self.decode_result(raw) {
            self.mark_faulted(scoped_ctx, format!("audio_drop panicked: {msg}"));
        }
    }

    pub(crate) fn call_panel_view(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        panel_id: &str,
    ) -> crate::UiNode {
        if self.is_failed() {
            return refused_tree(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` and `panel_id` outlive `table.panel_view`.
        let raw = unsafe {
            (self.table.panel_view)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(panel_id),
            )
        };
        if raw.status == ARIA_BYTES_PANIC {
            if !raw.bytes.ptr.is_null() {
                // SAFETY: `raw.bytes` was allocated by the guest.
                unsafe { (self.table.free_bytes)(raw.bytes) };
            }
            let msg = self.take_guest_string(raw.error);
            self.failed.store(true, Ordering::Release);
            self.mounted.store(false, Ordering::Release);
            resume_unwind(Box::new(msg));
        }
        let err_text = self.take_guest_string(raw.error);
        if raw.status != ARIA_BYTES_FOUND {
            if !raw.bytes.ptr.is_null() {
                // SAFETY: `raw.bytes` was allocated by the guest.
                unsafe { (self.table.free_bytes)(raw.bytes) };
            }
            return refused_tree(format!("panel_view failed: {err_text}"));
        }
        if raw.bytes.len > MAX_PANEL_VIEW_JSON_BYTES {
            let len = raw.bytes.len;
            if !raw.bytes.ptr.is_null() {
                // SAFETY: `raw.bytes` was allocated by the guest; freed before parsing oversized JSON.
                unsafe { (self.table.free_bytes)(raw.bytes) };
            }
            return refused_tree(format!(
                "panel JSON exceeds limit ({len} > {MAX_PANEL_VIEW_JSON_BYTES})"
            ));
        }
        // SAFETY: `raw.bytes` was allocated by the guest and fits within `MAX_PANEL_VIEW_JSON_BYTES`.
        let bytes = unsafe { raw.bytes.into_vec_with(self.table.free_bytes) };
        match serde_json::from_slice::<crate::UiNode>(&bytes) {
            Ok(tree) => tree,
            Err(err) => refused_tree(format!("malformed panel tree JSON: {err}")),
        }
    }

    pub(crate) fn call_panel_event(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        panel_id: &str,
        event: &crate::PanelEvent,
    ) -> Result<(), String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let event_json = serde_json::to_string(event).map_err(|err| err.to_string())?;
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx`, `panel_id`, and `event_json` outlive `table.panel_event`.
        let raw = unsafe {
            (self.table.panel_event)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(panel_id),
                AriaStr::from_str(&event_json),
            )
        };
        match self.decode_result(raw) {
            Ok(_) => Ok(()),
            Err((false, msg)) => Err(msg),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(msg));
            }
        }
    }

    pub(crate) fn call_encoder_open(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        encoder_id: &str,
        path: &str,
        config: &AriaEncoderConfig,
    ) -> Result<u64, String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx`, `encoder_id`, `path`, and `config` outlive `table.encoder_open`.
        let raw = unsafe {
            (self.table.encoder_open)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(encoder_id),
                AriaStr::from_str(path),
                &raw const *config,
            )
        };
        match self.decode_instance(raw) {
            Ok(instance) => Ok(instance),
            Err((false, msg)) => Err(msg),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(msg));
            }
        }
    }

    pub(crate) fn call_encoder_push_frame(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        instance: u64,
        frame: &AriaEncoderFrame,
    ) -> Result<(), String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` and `frame` outlive `table.encoder_push_frame`.
        let raw = unsafe {
            (self.table.encoder_push_frame)(
                (&raw mut call_ctx).cast::<c_void>(),
                instance,
                &raw const *frame,
            )
        };
        match self.decode_result(raw) {
            Ok(_) => Ok(()),
            Err((false, msg)) => Err(msg),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(msg));
            }
        }
    }

    pub(crate) fn call_encoder_push_audio(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        instance: u64,
        audio: &AriaEncoderAudio,
    ) -> Result<(), String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` and `audio` outlive `table.encoder_push_audio`.
        let raw = unsafe {
            (self.table.encoder_push_audio)(
                (&raw mut call_ctx).cast::<c_void>(),
                instance,
                &raw const *audio,
            )
        };
        match self.decode_result(raw) {
            Ok(_) => Ok(()),
            Err((false, msg)) => Err(msg),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(msg));
            }
        }
    }

    pub(crate) fn call_encoder_finish(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        instance: u64,
    ) -> Result<(), String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` outlives `table.encoder_finish`.
        let raw =
            unsafe { (self.table.encoder_finish)((&raw mut call_ctx).cast::<c_void>(), instance) };
        match self.decode_result(raw) {
            Ok(_) => Ok(()),
            Err((false, msg)) => Err(msg),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(msg));
            }
        }
    }

    pub(crate) fn call_encoder_drop(self: &Arc<Self>, scoped_ctx: &PluginContext, instance: u64) {
        if self.is_failed() {
            return;
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` outlives `table.encoder_drop`.
        let raw =
            unsafe { (self.table.encoder_drop)((&raw mut call_ctx).cast::<c_void>(), instance) };
        if let Err((true, msg)) = self.decode_result(raw) {
            self.mark_faulted(scoped_ctx, format!("encoder_drop panicked: {msg}"));
        }
    }

    /// Run one step of plugin job `job`. A panic faults the plugin and unwinds into the
    /// job, which records it as failed.
    pub(crate) fn call_job_step(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        job: u64,
    ) -> Result<crate::JobStep, String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        let mut step = AriaJobStep {
            progress: -1.0,
            done: 0,
            commit: 0,
        };
        // SAFETY: `call_ctx` and `step` outlive `table.job_step`.
        let raw = unsafe {
            (self.table.job_step)((&raw mut call_ctx).cast::<c_void>(), job, &raw mut step)
        };
        match self.decode_result(raw) {
            Ok(detail) => Ok(crate::JobStep {
                progress: (step.progress >= 0.0).then_some(step.progress.min(1.0)),
                detail,
                done: step.done != 0,
                commit: step.commit != 0,
            }),
            Err((false, msg)) => Err(msg),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(msg));
            }
        }
    }

    /// Report the end of plugin job `job`. An error is dropped: the job has already ended.
    pub(crate) fn call_job_end(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        job: u64,
        cancelled: bool,
    ) {
        if self.is_failed() {
            return;
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` outlives `table.job_end`.
        let raw = unsafe {
            (self.table.job_end)(
                (&raw mut call_ctx).cast::<c_void>(),
                job,
                u32::from(cancelled),
            )
        };
        if let Err((true, msg)) = self.decode_result(raw) {
            self.mark_faulted(scoped_ctx, format!("job_end panicked: {msg}"));
        }
    }

    pub(crate) fn call_overlay_scene(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        overlay_id: &str,
    ) -> crate::OverlayScene {
        if self.is_failed() {
            return refused_scene(format!("plugin `{}` has faulted", self.id));
        }
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` and `overlay_id` outlive `table.overlay_scene`.
        let raw = unsafe {
            (self.table.overlay_scene)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(overlay_id),
            )
        };
        if raw.status == ARIA_BYTES_PANIC {
            if !raw.bytes.ptr.is_null() {
                // SAFETY: `raw.bytes` was allocated by the guest.
                unsafe { (self.table.free_bytes)(raw.bytes) };
            }
            let msg = self.take_guest_string(raw.error);
            self.failed.store(true, Ordering::Release);
            self.mounted.store(false, Ordering::Release);
            resume_unwind(Box::new(msg));
        }
        let err_text = self.take_guest_string(raw.error);
        if raw.status != ARIA_BYTES_FOUND {
            if !raw.bytes.ptr.is_null() {
                // SAFETY: `raw.bytes` was allocated by the guest.
                unsafe { (self.table.free_bytes)(raw.bytes) };
            }
            return refused_scene(format!("overlay_scene failed: {err_text}"));
        }
        if raw.bytes.len > MAX_OVERLAY_SCENE_JSON_BYTES {
            let len = raw.bytes.len;
            if !raw.bytes.ptr.is_null() {
                // SAFETY: `raw.bytes` was allocated by the guest; freed before parsing oversized JSON.
                unsafe { (self.table.free_bytes)(raw.bytes) };
            }
            return refused_scene(format!(
                "overlay JSON exceeds limit ({len} > {MAX_OVERLAY_SCENE_JSON_BYTES})"
            ));
        }
        // SAFETY: `raw.bytes` was allocated by the guest and fits within `MAX_OVERLAY_SCENE_JSON_BYTES`.
        let bytes = unsafe { raw.bytes.into_vec_with(self.table.free_bytes) };
        match serde_json::from_slice::<crate::OverlayScene>(&bytes) {
            Ok(scene) => scene,
            Err(err) => refused_scene(format!("malformed overlay scene JSON: {err}")),
        }
    }

    pub(crate) fn call_overlay_event(
        self: &Arc<Self>,
        scoped_ctx: &PluginContext,
        overlay_id: &str,
        event: &crate::OverlayEvent,
    ) -> Result<(), String> {
        if self.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.id));
        }
        let event_json = serde_json::to_string(event).map_err(|err| err.to_string())?;
        let mut call_ctx = HostCallContext {
            state: self,
            scoped_ctx,
            unscoped_ctx: scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx`, `overlay_id`, and `event_json` outlive `table.overlay_event`.
        let raw = unsafe {
            (self.table.overlay_event)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(overlay_id),
                AriaStr::from_str(&event_json),
            )
        };
        match self.decode_result(raw) {
            Ok(_) => Ok(()),
            Err((false, msg)) => Err(msg),
            Err((true, msg)) => {
                self.failed.store(true, Ordering::Release);
                self.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(msg));
            }
        }
    }
}

#[cfg(test)]
mod tests;
