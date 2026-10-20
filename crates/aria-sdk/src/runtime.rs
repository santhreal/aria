//! Guest runtime dispatching [`AriaPluginV1`](aria_plugin_abi::AriaPluginV1) callbacks
//! into a [`Plugin`](crate::Plugin) implementation inside `catch_unwind`.

mod audio;
mod frames;
mod media;

use parking_lot::Mutex;
use std::any::Any;
use std::collections::HashMap;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use aria_plugin_abi::{
    AriaBytesResult, AriaHostV1, AriaJobStep, AriaResult, AriaStr, validate_host_table,
};

use crate::host::{
    AnalyzerRegistry, CpuEffectRegistry, CpuEffectSpecRegistry, DecoderRegistry,
    DriverLanguageRegistry, EncoderRegistry, PanelRegistry,
};
use crate::{AnalyzerRun, AudioProcessor, EncoderSession, Host, OverlayEvent, PanelEvent, Plugin};

use media::DecoderInstance;

/// Extract a human-readable message from a panic payload.
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "plugin panicked".to_owned()
    }
}

/// Runtime state holding the plugin instance, host table pointer, and active audio instances.
#[doc(hidden)]
pub struct PluginRuntime<P: Plugin> {
    host: AtomicPtr<AriaHostV1>,
    plugin: P,
    audio_next_id: AtomicU64,
    audio_instances: Mutex<HashMap<u64, Box<dyn AudioProcessor>>>,
    panels: PanelRegistry,
    encoders: EncoderRegistry,
    encoder_next_id: AtomicU64,
    encoder_instances: Mutex<HashMap<u64, Box<dyn EncoderSession>>>,
    decoders: DecoderRegistry,
    decoder_next_id: AtomicU64,
    decoder_instances: Mutex<HashMap<u64, DecoderInstance>>,
    cpu_effects: CpuEffectRegistry,
    cpu_effect_specs: CpuEffectSpecRegistry,
    driver_languages: DriverLanguageRegistry,
    analyzers: AnalyzerRegistry,
    analyzer_next_id: AtomicU64,
    analyzer_instances: Mutex<HashMap<u64, Box<dyn AnalyzerRun>>>,
}

impl<P: Plugin> std::fmt::Debug for PluginRuntime<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRuntime").finish_non_exhaustive()
    }
}

impl<P: Plugin> PluginRuntime<P> {
    /// Construct a new runtime wrapping `plugin`.
    pub fn new(plugin: P) -> Self {
        Self {
            host: AtomicPtr::new(std::ptr::null_mut()),
            plugin,
            audio_next_id: AtomicU64::new(1),
            audio_instances: Mutex::new(HashMap::new()),
            panels: Arc::new(Mutex::new(HashMap::new())),
            encoders: Arc::new(Mutex::new(HashMap::new())),
            encoder_next_id: AtomicU64::new(1),
            encoder_instances: Mutex::new(HashMap::new()),
            decoders: Arc::new(Mutex::new(HashMap::new())),
            decoder_next_id: AtomicU64::new(1),
            decoder_instances: Mutex::new(HashMap::new()),
            cpu_effects: Arc::new(Mutex::new(HashMap::new())),
            cpu_effect_specs: Arc::new(Mutex::new(HashMap::new())),
            driver_languages: Arc::new(Mutex::new(HashMap::new())),
            analyzers: Arc::new(Mutex::new(HashMap::new())),
            analyzer_next_id: AtomicU64::new(1),
            analyzer_instances: Mutex::new(HashMap::new()),
        }
    }

    /// Validate and store the host table pointer on entry.
    ///
    /// # Safety
    ///
    /// `host` must be null or point to a valid [`AriaHostV1`] prefix.
    #[allow(unsafe_code)]
    pub unsafe fn bind_host(&self, host: *const AriaHostV1, api_major: u32) -> bool {
        if host.is_null() {
            return false;
        }
        // SAFETY: The caller guarantees `host` points to at least `struct_size` bytes.
        let struct_size = unsafe { (*host).struct_size };
        if validate_host_table(api_major, struct_size).is_err() {
            return false;
        }
        self.host.store(host.cast_mut(), Ordering::Release);
        true
    }

    #[allow(unsafe_code)]
    fn host_for(&self, host_ctx: *mut c_void) -> Result<Host, String> {
        let ptr = self.host.load(Ordering::Acquire);
        if ptr.is_null() {
            return Err("host table is not initialized".to_owned());
        }
        // SAFETY: `ptr` was validated in `bind_host` and points to the host's static table.
        let host = unsafe { Host::from_raw(&*ptr, host_ctx) };
        Ok(host
            .with_panels(Arc::clone(&self.panels))
            .with_encoders(Arc::clone(&self.encoders))
            .with_decoders(Arc::clone(&self.decoders))
            .with_cpu_effects(Arc::clone(&self.cpu_effects))
            .with_cpu_effect_specs(Arc::clone(&self.cpu_effect_specs))
            .with_driver_languages(Arc::clone(&self.driver_languages))
            .with_analyzers(Arc::clone(&self.analyzers)))
    }

    fn run_unit(
        &self,
        host_ctx: *mut c_void,
        f: impl FnOnce(&P, &Host) -> Result<(), String>,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            f(&self.plugin, &host)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    fn run_string(
        &self,
        host_ctx: *mut c_void,
        f: impl FnOnce(&P, &Host) -> Result<String, String>,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            f(&self.plugin, &host)
        })) {
            Ok(Ok(val)) => {
                if val.is_empty() {
                    AriaResult::ok_empty()
                } else {
                    AriaResult::ok(val)
                }
            }
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    #[allow(unsafe_code)]
    fn str_arg<'a>(raw: AriaStr, label: &str) -> Result<&'a str, String> {
        // SAFETY: `raw` is valid for the duration of the FFI call.
        unsafe { raw.as_str() }.map_err(|err| format!("invalid UTF-8 in {label}: {err}"))
    }

    /// Run [`Plugin::mount`].
    pub fn handle_mount(&self, host_ctx: *mut c_void) -> AriaResult {
        self.run_unit(host_ctx, |plugin, host| plugin.mount(host))
    }

    /// Run [`Plugin::unmount`], then drop the plugin's audio instances, panels,
    /// encoders, encoder sessions, CPU effects, and driver languages.
    pub fn handle_unmount(&self, host_ctx: *mut c_void) -> AriaResult {
        let res = self.run_unit(host_ctx, |plugin, host| plugin.unmount(host));
        self.audio_instances.lock().clear();
        self.panels.lock().clear();
        self.encoders.lock().clear();
        self.encoder_instances.lock().clear();
        self.cpu_effects.lock().clear();
        self.driver_languages.lock().clear();
        res
    }

    /// Run the command `id` with `args_json` through [`Plugin::command_run`].
    pub fn handle_command_run(
        &self,
        host_ctx: *mut c_void,
        id: AriaStr,
        args_json: AriaStr,
    ) -> AriaResult {
        self.run_unit(host_ctx, |plugin, host| {
            let id = Self::str_arg(id, "command id")?;
            let args_json = Self::str_arg(args_json, "command args")?;
            plugin.command_run(host, id, args_json)
        })
    }

    /// Run the AI tool `name` with `arguments_json`, returning its
    /// [`AiToolOutput`](crate::AiToolOutput) as JSON.
    pub fn handle_ai_tool_call(
        &self,
        host_ctx: *mut c_void,
        name: AriaStr,
        arguments_json: AriaStr,
    ) -> AriaResult {
        self.run_string(host_ctx, |plugin, host| {
            let name = Self::str_arg(name, "AI tool name")?;
            let arguments: serde_json::Value =
                serde_json::from_str(Self::str_arg(arguments_json, "AI tool arguments")?)
                    .map_err(|err| format!("the arguments of {name} are not JSON: {err}"))?;
            let output = plugin.ai_tool_call(host, name, &arguments)?;
            serde_json::to_string(&output).map_err(|err| err.to_string())
        })
    }

    /// Deliver the event `kind` with `event_json` to [`Plugin::event`] and
    /// return its [`EventResponse`](crate::EventResponse) as JSON, or an empty
    /// string for [`EventResponse::Pass`](crate::EventResponse::Pass).
    pub fn handle_event(
        &self,
        host_ctx: *mut c_void,
        kind: AriaStr,
        event_json: AriaStr,
    ) -> AriaResult {
        self.run_string(host_ctx, |plugin, host| {
            let kind = Self::str_arg(kind, "event kind")?;
            let event_json = Self::str_arg(event_json, "event json")?;
            let response = plugin.event(host, kind, event_json)?;
            Ok(response.into_json().unwrap_or_default())
        })
    }

    /// Read `path` with importer `importer_id` through [`Plugin::import`] and
    /// return its [`ImportResult`](crate::ImportResult) as JSON.
    pub fn handle_import(
        &self,
        host_ctx: *mut c_void,
        importer_id: AriaStr,
        path: AriaStr,
    ) -> AriaResult {
        self.run_string(host_ctx, |plugin, host| {
            let importer_id = Self::str_arg(importer_id, "importer id")?;
            let path_str = Self::str_arg(path, "import path")?;
            let result = plugin.import(host, importer_id, Path::new(path_str))?;
            serde_json::to_string(&result).map_err(|err| err.to_string())
        })
    }

    /// Write `project_json` to `path` with exporter `exporter_id` through
    /// [`Plugin::export`] and return its [`ExportReport`](crate::ExportReport)
    /// as JSON.
    pub fn handle_export(
        &self,
        host_ctx: *mut c_void,
        exporter_id: AriaStr,
        project_json: AriaStr,
        path: AriaStr,
    ) -> AriaResult {
        self.run_string(host_ctx, |plugin, host| {
            let exporter_id = Self::str_arg(exporter_id, "exporter id")?;
            let project_json = Self::str_arg(project_json, "project json")?;
            let path_str = Self::str_arg(path, "export path")?;
            let report = plugin.export(host, exporter_id, project_json, Path::new(path_str))?;
            serde_json::to_string(&report).map_err(|err| err.to_string())
        })
    }

    /// Build the declarative widget tree for `panel_id` as UTF-8 JSON bytes.
    pub fn handle_panel_view(&self, host_ctx: *mut c_void, panel_id: AriaStr) -> AriaBytesResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let panel_id = Self::str_arg(panel_id, "panel id")?;
            let registered = self.panels.lock().get(panel_id).cloned();
            let tree = match registered {
                Some(panel) => panel.view(&host),
                None => self.plugin.panel_view(&host, panel_id)?,
            };
            serde_json::to_vec(&tree).map_err(|err| err.to_string())
        })) {
            Ok(Ok(bytes)) => AriaBytesResult::found(bytes),
            Ok(Err(err)) => AriaBytesResult::err(err),
            Err(payload) => AriaBytesResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Deliver a user interaction event (`event_json`) to `panel_id`.
    pub fn handle_panel_event(
        &self,
        host_ctx: *mut c_void,
        panel_id: AriaStr,
        event_json: AriaStr,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let panel_id = Self::str_arg(panel_id, "panel id")?;
            let event_json = Self::str_arg(event_json, "panel event json")?;
            let event: PanelEvent =
                serde_json::from_str(event_json).map_err(|err| err.to_string())?;
            let registered = self.panels.lock().get(panel_id).cloned();
            match registered {
                Some(panel) => panel.event(&host, &event),
                None => self.plugin.panel_event(&host, panel_id, &event),
            }
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Build the viewer overlay scene for `overlay_id` as UTF-8 JSON bytes.
    pub fn handle_overlay_scene(
        &self,
        host_ctx: *mut c_void,
        overlay_id: AriaStr,
    ) -> AriaBytesResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let overlay_id = Self::str_arg(overlay_id, "overlay id")?;
            let scene = self.plugin.overlay_scene(&host, overlay_id);
            serde_json::to_vec(&scene).map_err(|err| err.to_string())
        })) {
            Ok(Ok(bytes)) => AriaBytesResult::found(bytes),
            Ok(Err(err)) => AriaBytesResult::err(err),
            Err(payload) => AriaBytesResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Deliver a user interaction event (`event_json`) to viewer overlay `overlay_id`.
    pub fn handle_overlay_event(
        &self,
        host_ctx: *mut c_void,
        overlay_id: AriaStr,
        event_json: AriaStr,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let overlay_id = Self::str_arg(overlay_id, "overlay id")?;
            let event_json = Self::str_arg(event_json, "overlay event json")?;
            let event: OverlayEvent =
                serde_json::from_str(event_json).map_err(|err| err.to_string())?;
            self.plugin.overlay_event(&host, overlay_id, &event)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Run one step of job `job`, writing its progress and flags into `step` and returning
    /// its detail line.
    ///
    /// # Safety
    ///
    /// `step` must be null or point to an [`AriaJobStep`] writable for the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_job_step(
        &self,
        host_ctx: *mut c_void,
        job: u64,
        step: *mut AriaJobStep,
    ) -> AriaResult {
        if step.is_null() {
            return AriaResult::err("job_step received a null step");
        }
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            self.plugin.job_step(&host, job)
        })) {
            Ok(Ok(next)) => {
                // SAFETY: `step` is non-null and the caller guarantees it is writable.
                unsafe {
                    step.write(AriaJobStep {
                        progress: next.progress.unwrap_or(-1.0),
                        done: u32::from(next.done),
                        commit: u32::from(next.commit),
                    });
                }
                if next.detail.is_empty() {
                    AriaResult::ok_empty()
                } else {
                    AriaResult::ok(next.detail)
                }
            }
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Report the end of job `job`; `cancelled` is nonzero when it stopped before `done`.
    pub fn handle_job_end(&self, host_ctx: *mut c_void, job: u64, cancelled: u32) -> AriaResult {
        self.run_unit(host_ctx, |plugin, host| {
            plugin.job_end(host, job, cancelled != 0);
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
