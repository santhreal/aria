//! Guest runtime dispatching [`AriaPluginV1`](aria_plugin_abi::AriaPluginV1) callbacks
//! into a [`Plugin`](crate::Plugin) implementation inside `catch_unwind`.

use parking_lot::Mutex;
use std::any::Any;
use std::collections::HashMap;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use aria_plugin_abi::{
    AriaAudioBlock, AriaAudioParam, AriaBytesResult, AriaCpuEffectCall, AriaDriverEvalCall,
    AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame, AriaHostV1, AriaInstanceResult,
    AriaJobStep, AriaResult, AriaScopeCall, AriaStr, AriaVideoFrameOut, AriaVideoInfo,
    validate_host_table,
};

use crate::host::{
    AnalyzerRegistry, CpuEffectRegistry, CpuEffectSpecRegistry, DecoderRegistry,
    DriverLanguageRegistry, EncoderRegistry, PanelRegistry,
};
use crate::{
    AnalyzerRun, AudioBlock, AudioParam, AudioProcessor, AudioStream, CpuFrameIn, CpuFrameOut,
    DecodeFailure, Decoder, DriverVars, EncoderAudio, EncoderConfig, EncoderFrame, EncoderSession,
    FramePlane, Host, OverlayEvent, PanelEvent, ParamValue, Plugin, ScopeFrame, ScopeImage,
    VideoStream,
};
/// A stream a registered decoder opened, locked on its own so streams decode
/// in parallel.
enum DecoderInstance {
    Video(Arc<Mutex<Box<dyn VideoStream>>>),
    Audio(Arc<Mutex<Box<dyn AudioStream>>>),
}

fn failure_result(failure: DecodeFailure) -> AriaResult {
    match failure {
        DecodeFailure::Unsupported(reason) => AriaResult::unsupported(reason),
        DecodeFailure::Failed(reason) => AriaResult::err(reason),
    }
}

fn failure_instance(failure: DecodeFailure) -> AriaInstanceResult {
    match failure {
        DecodeFailure::Unsupported(reason) => AriaInstanceResult::unsupported(reason),
        DecodeFailure::Failed(reason) => AriaInstanceResult::err(reason),
    }
}
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

    fn decoder(&self, id: &str) -> Result<Arc<dyn Decoder>, DecodeFailure> {
        self.decoders
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| DecodeFailure::Failed(format!("unknown decoder: {id}")))
    }

    fn video_stream(&self, instance: u64) -> Result<Arc<Mutex<Box<dyn VideoStream>>>, String> {
        match self.decoder_instances.lock().get(&instance) {
            Some(DecoderInstance::Video(stream)) => Ok(Arc::clone(stream)),
            _ => Err(format!("unknown video stream {instance}")),
        }
    }

    fn audio_stream(&self, instance: u64) -> Result<Arc<Mutex<Box<dyn AudioStream>>>, String> {
        match self.decoder_instances.lock().get(&instance) {
            Some(DecoderInstance::Audio(stream)) => Ok(Arc::clone(stream)),
            _ => Err(format!("unknown audio stream {instance}")),
        }
    }

    fn open_instance(&self, instance: DecoderInstance) -> u64 {
        let id = self.decoder_next_id.fetch_add(1, Ordering::Relaxed);
        self.decoder_instances.lock().insert(id, instance);
        id
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

    pub fn handle_mount(&self, host_ctx: *mut c_void) -> AriaResult {
        self.run_unit(host_ctx, |plugin, host| plugin.mount(host))
    }

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

    /// Measure the frame in `call` with scope `scope_id`, drawing into the
    /// call's image and returning the readout line.
    ///
    /// # Safety
    ///
    /// `call` is null or points to an [`AriaScopeCall`] whose `frame` holds
    /// `frame_len` readable bytes and whose `image` holds `image_len` writable
    /// bytes, none of them aliased, for the duration of the call.
    pub unsafe fn handle_scope_measure(
        &self,
        host_ctx: *mut c_void,
        scope_id: AriaStr,
        call: *mut AriaScopeCall,
    ) -> AriaResult {
        if call.is_null() {
            return AriaResult::err("scope_measure received a null call");
        }
        // SAFETY: `call` is non-null and the caller guarantees it is valid.
        let call = unsafe { &*call };
        let frame_len = (call.frame_width as usize)
            .checked_mul(call.frame_height as usize)
            .and_then(|px| px.checked_mul(4));
        let image_len = (call.image_width as usize)
            .checked_mul(call.image_height as usize)
            .and_then(|px| px.checked_mul(4));
        if frame_len != Some(call.frame_len)
            || image_len != Some(call.image_len)
            || call.frame.is_null()
            || call.image.is_null()
        {
            return AriaResult::err(format!(
                "scope_measure received a {}x{} frame in {} bytes and a {}x{} image in {} bytes",
                call.frame_width,
                call.frame_height,
                call.frame_len,
                call.image_width,
                call.image_height,
                call.image_len
            ));
        }
        // SAFETY: the lengths match the dimensions and the caller guarantees
        // both buffers are valid and unaliased for the call.
        let (pixels, image) = unsafe {
            (
                std::slice::from_raw_parts(call.frame, call.frame_len),
                std::slice::from_raw_parts_mut(call.image, call.image_len),
            )
        };
        let frame = ScopeFrame {
            width: call.frame_width,
            height: call.frame_height,
            pixels,
        };
        let mut image = ScopeImage {
            width: call.image_width,
            height: call.image_height,
            pixels: image,
        };
        self.run_string(host_ctx, |plugin, host| {
            let id = Self::str_arg(scope_id, "scope id")?;
            plugin.scope_measure(host, id, &frame, &mut image)
        })
    }

    /// Process the frame in `call` with CPU effect or generator `effect_id`.
    ///
    /// # Safety
    ///
    /// `call` is null or points to an [`AriaCpuEffectCall`] whose `input` and
    /// `output` hold `pixels_len` (`width * height * 4`) valid unaliased `u16`
    /// channels and whose `params` holds `params_len` valid `f32` values for the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_cpu_effect_process(
        &self,
        host_ctx: *mut c_void,
        effect_id: AriaStr,
        call: *mut AriaCpuEffectCall,
    ) -> AriaResult {
        if call.is_null() {
            return AriaResult::err("cpu_effect_process received a null call");
        }
        // SAFETY: `call` is non-null and the caller guarantees it is valid.
        let call = unsafe { &*call };
        let expected_len = (call.width as usize)
            .checked_mul(call.height as usize)
            .and_then(|px| px.checked_mul(4));
        if expected_len != Some(call.pixels_len)
            || call.input.is_null()
            || call.output.is_null()
            || (call.params_len > 0 && call.params.is_null())
        {
            return AriaResult::err(format!(
                "cpu_effect_process received a {}x{} frame with {} channel elements (params_len={})",
                call.width, call.height, call.pixels_len, call.params_len
            ));
        }
        // SAFETY: lengths and pointers are validated above and unaliased for the call.
        let (input_pixels, output_pixels, params) = unsafe {
            (
                std::slice::from_raw_parts(call.input, call.pixels_len),
                std::slice::from_raw_parts_mut(call.output, call.pixels_len),
                if call.params_len == 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(call.params, call.params_len)
                },
            )
        };
        let mut output = CpuFrameOut {
            width: call.width,
            height: call.height,
            pixels: output_pixels,
        };
        self.run_unit(host_ctx, |plugin, host| {
            let id = Self::str_arg(effect_id, "cpu effect id")?;
            let params_json = Self::str_arg(call.params_json, "cpu effect params json")?;
            let spec = self.cpu_effect_specs.lock().get(id).cloned();
            let input = CpuFrameIn {
                width: call.width,
                height: call.height,
                pixels: input_pixels,
                params,
                params_json,
                spec: spec.as_deref(),
                time: call.time,
            };
            let registered = self.cpu_effects.lock().get(id).cloned();
            match registered {
                Some(effect) => effect.process(host, &input, &mut output),
                None => plugin.cpu_effect_process(host, id, &input, &mut output),
            }
        })
    }

    /// Evaluate `expr` in driver language `language_id` for `call`.
    ///
    /// # Safety
    ///
    /// `call` is null or points to a valid writable [`AriaDriverEvalCall`] for
    /// the duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_driver_evaluate(
        &self,
        host_ctx: *mut c_void,
        language_id: AriaStr,
        expr: AriaStr,
        call: *mut AriaDriverEvalCall,
    ) -> AriaResult {
        if call.is_null() {
            return AriaResult::err("driver_evaluate received a null call");
        }
        // SAFETY: `call` is non-null and writable for the call.
        let call_ref = unsafe { &mut *call };
        self.run_unit(host_ctx, |plugin, host| {
            let id = Self::str_arg(language_id, "driver language id")?;
            let expr_str = Self::str_arg(expr, "driver expression")?;
            let value = ParamValue::from_raw(call_ref.value)?;
            let vars = DriverVars {
                time: call_ref.time,
                clip_time: call_ref.clip_time,
                frame_rate: call_ref.frame_rate,
                duration: call_ref.duration,
                canvas: [call_ref.canvas_width, call_ref.canvas_height],
                value,
                seed: call_ref.seed,
            };
            let registered = self.driver_languages.lock().get(id).cloned();
            let out = match registered {
                Some(lang) => lang.evaluate(host, expr_str, &vars)?,
                None => plugin.driver_evaluate(host, id, expr_str, &vars)?,
            };
            call_ref.output = out.to_raw();
            Ok(())
        })
    }

    /// Open an analyzer instance for `analyzer_id` at `sample_rate` and `channels`.
    pub fn handle_analyzer_open(
        &self,
        host_ctx: *mut c_void,
        analyzer_id: AriaStr,
        sample_rate: u32,
        channels: u32,
    ) -> AriaInstanceResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let id = Self::str_arg(analyzer_id, "analyzer id")?;
            let ch = u16::try_from(channels)
                .map_err(|_| format!("analyzer channel count {channels} exceeds u16::MAX"))?;
            let registered = self.analyzers.lock().get(id).cloned();
            let run = match registered {
                Some(analyzer) => analyzer.open(&host, sample_rate, ch)?,
                None => self.plugin.analyzer_open(&host, id, sample_rate, ch)?,
            };
            let instance = self.analyzer_next_id.fetch_add(1, Ordering::Relaxed);
            self.analyzer_instances.lock().insert(instance, run);
            Ok::<u64, String>(instance)
        })) {
            Ok(Ok(instance)) => AriaInstanceResult::ok(instance),
            Ok(Err(err)) => AriaInstanceResult::err(err),
            Err(payload) => AriaInstanceResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Push `samples_len` interleaved `f32` samples into analyzer `instance`.
    ///
    /// # Safety
    ///
    /// `samples_ptr` must be null (when `samples_len == 0`) or cover `samples_len`
    /// initialized `f32` values for the duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_analyzer_push(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        samples_ptr: *const f32,
        samples_len: usize,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let samples = if samples_len == 0 {
                &[][..]
            } else if samples_ptr.is_null() {
                return Err("analyzer_push received null samples".to_owned());
            } else {
                // SAFETY: `samples_ptr` covers `samples_len` valid `f32` elements.
                unsafe { std::slice::from_raw_parts(samples_ptr, samples_len) }
            };
            let mut map = self.analyzer_instances.lock();
            let run = map
                .get_mut(&instance)
                .ok_or_else(|| format!("unknown analyzer instance {instance}"))?;
            run.push(samples)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Finish analyzer `instance`, remove it from the active table, and serialize
    /// its [`AnalyzerResult`](crate::AnalyzerResult) as JSON.
    pub fn handle_analyzer_finish(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let run = self
                .analyzer_instances
                .lock()
                .remove(&instance)
                .ok_or_else(|| format!("unknown analyzer instance {instance}"))?;
            let out = run.finish()?;
            serde_json::to_string(&out).map_err(|err| err.to_string())
        })) {
            Ok(Ok(json)) => AriaResult::ok(json),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Drop analyzer `instance` without finishing.
    pub fn handle_analyzer_drop(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let removed = self.analyzer_instances.lock().remove(&instance);
            drop(removed);
            Ok::<(), String>(())
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

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

    pub fn handle_audio_create(
        &self,
        host_ctx: *mut c_void,
        effect_id: AriaStr,
        sample_rate: u32,
    ) -> AriaInstanceResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let effect_id = Self::str_arg(effect_id, "effect id")?;
            let processor = self.plugin.audio_create(&host, effect_id, sample_rate)?;
            let id = self.audio_next_id.fetch_add(1, Ordering::Relaxed);
            self.audio_instances.lock().insert(id, processor);
            Ok::<u64, String>(id)
        })) {
            Ok(Ok(id)) => AriaInstanceResult::ok(id),
            Ok(Err(err)) => AriaInstanceResult::err(err),
            Err(payload) => AriaInstanceResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Process an audio block for `instance`.
    ///
    /// # Safety
    ///
    /// `params_ptr` must be null (when `params_len == 0`) or point to `params_len`
    /// valid [`AriaAudioParam`] elements, and `block_ptr` must be null or point to
    /// a valid [`AriaAudioBlock`] for the duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_audio_process(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        params_ptr: *const AriaAudioParam,
        params_len: usize,
        block_ptr: *mut AriaAudioBlock,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            if block_ptr.is_null() {
                return Err("audio_process received null block".to_owned());
            }
            // SAFETY: `block_ptr` is valid for the duration of `audio_process`.
            let raw_block = unsafe { &mut *block_ptr };
            let frames = raw_block.frames;
            // SAFETY: `left` and `right` point to `frames` f32 samples each.
            let left = if frames == 0 {
                &mut [][..]
            } else {
                unsafe { std::slice::from_raw_parts_mut(raw_block.left, frames) }
            };
            // SAFETY: `right` points to `frames` f32 samples.
            let right = if frames == 0 {
                &mut [][..]
            } else {
                unsafe { std::slice::from_raw_parts_mut(raw_block.right, frames) }
            };
            let sidechain = if raw_block.has_sidechain != 0
                && !raw_block.sidechain_left.is_null()
                && !raw_block.sidechain_right.is_null()
            {
                // SAFETY: Sidechain channels cover `frames` f32 samples when `has_sidechain != 0`.
                let sc_l = if frames == 0 {
                    &[][..]
                } else {
                    unsafe { std::slice::from_raw_parts(raw_block.sidechain_left, frames) }
                };
                // SAFETY: Sidechain right channel covers `frames` f32 samples.
                let sc_r = if frames == 0 {
                    &[][..]
                } else {
                    unsafe { std::slice::from_raw_parts(raw_block.sidechain_right, frames) }
                };
                Some((sc_l, sc_r))
            } else {
                None
            };

            // SAFETY: `params_ptr` points to `params_len` valid `AriaAudioParam` structs.
            let raw_params = if params_len == 0 || params_ptr.is_null() {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(params_ptr, params_len) }
            };

            let mut stack_params = [AudioParam {
                constant: 0.0,
                per_frame: None,
            }; 32];
            let mut heap_params = Vec::new();
            let params: &[AudioParam<'_>] = if raw_params.len() <= stack_params.len() {
                for (slot, raw) in stack_params.iter_mut().zip(raw_params.iter()) {
                    let per_frame = if raw.per_frame.is_null() || raw.per_frame_len == 0 {
                        None
                    } else {
                        // SAFETY: `per_frame` points to `per_frame_len` f32 samples.
                        Some(unsafe {
                            std::slice::from_raw_parts(raw.per_frame, raw.per_frame_len)
                        })
                    };
                    *slot = AudioParam {
                        constant: raw.constant,
                        per_frame,
                    };
                }
                &stack_params[..raw_params.len()]
            } else {
                heap_params.reserve(raw_params.len());
                for raw in raw_params {
                    let per_frame = if raw.per_frame.is_null() || raw.per_frame_len == 0 {
                        None
                    } else {
                        // SAFETY: `per_frame` points to `per_frame_len` f32 samples.
                        Some(unsafe {
                            std::slice::from_raw_parts(raw.per_frame, raw.per_frame_len)
                        })
                    };
                    heap_params.push(AudioParam {
                        constant: raw.constant,
                        per_frame,
                    });
                }
                &heap_params
            };

            let mut block = AudioBlock {
                left,
                right,
                position: raw_block.position,
                sample_rate: raw_block.sample_rate,
                sidechain,
            };

            let mut map = self.audio_instances.lock();
            let processor = map
                .get_mut(&instance)
                .ok_or_else(|| format!("unknown audio instance {instance}"))?;
            processor.process(params, &mut block);
            Ok::<(), String>(())
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    pub fn handle_audio_reset(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let mut map = self.audio_instances.lock();
            let processor = map
                .get_mut(&instance)
                .ok_or_else(|| format!("unknown audio instance {instance}"))?;
            processor.reset();
            Ok::<(), String>(())
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    pub fn handle_audio_drop(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let removed = self.audio_instances.lock().remove(&instance);
            drop(removed);
            Ok::<(), String>(())
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
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

    /// Open an encoder session for `encoder_id` writing `config_ptr` to `path`.
    ///
    /// # Safety
    ///
    /// `config_ptr` must be non-null and point to a valid [`AriaEncoderConfig`] for the
    /// duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_encoder_open(
        &self,
        host_ctx: *mut c_void,
        encoder_id: AriaStr,
        path: AriaStr,
        config_ptr: *const AriaEncoderConfig,
    ) -> AriaInstanceResult {
        match catch_unwind(AssertUnwindSafe(|| {
            if config_ptr.is_null() {
                return Err("encoder_open received null config".to_owned());
            }
            // SAFETY: `config_ptr` is valid for the duration of `encoder_open`.
            let raw_cfg = unsafe { &*config_ptr };
            let config = EncoderConfig {
                width: raw_cfg.width,
                height: raw_cfg.height,
                fps_num: raw_cfg.fps_num,
                fps_den: raw_cfg.fps_den,
                video_bitrate_kbps: raw_cfg.video_bitrate_kbps,
                audio_bitrate_kbps: raw_cfg.audio_bitrate_kbps,
            };
            let host = self.host_for(host_ctx)?;
            let encoder_id = Self::str_arg(encoder_id, "encoder id")?;
            let path_str = Self::str_arg(path, "encoder path")?;
            let registered = self.encoders.lock().get(encoder_id).cloned();
            let session = match registered {
                Some(encoder) => encoder.open(&host, Path::new(path_str), &config)?,
                None => {
                    self.plugin
                        .encoder_open(&host, encoder_id, Path::new(path_str), &config)?
                }
            };
            let id = self.encoder_next_id.fetch_add(1, Ordering::Relaxed);
            self.encoder_instances.lock().insert(id, session);
            Ok::<u64, String>(id)
        })) {
            Ok(Ok(id)) => AriaInstanceResult::ok(id),
            Ok(Err(err)) => AriaInstanceResult::err(err),
            Err(payload) => AriaInstanceResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Push one RGBA video frame to encoder `instance`.
    ///
    /// # Safety
    ///
    /// `frame_ptr` must be non-null and point to a valid [`AriaEncoderFrame`] whose
    /// `rgba` pointer covers `rgba_len` bytes for the duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_encoder_push_frame(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        frame_ptr: *const AriaEncoderFrame,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            if frame_ptr.is_null() {
                return Err("encoder_push_frame received null frame".to_owned());
            }
            // SAFETY: `frame_ptr` is valid for the duration of `encoder_push_frame`.
            let raw = unsafe { &*frame_ptr };
            let rgba = if raw.rgba_len == 0 || raw.rgba.is_null() {
                &[][..]
            } else {
                // SAFETY: `raw.rgba` points to `raw.rgba_len` valid bytes.
                unsafe { std::slice::from_raw_parts(raw.rgba, raw.rgba_len) }
            };
            let frame = EncoderFrame {
                rgba,
                width: raw.width,
                height: raw.height,
                stride: raw.stride,
                pts_value: raw.pts_value,
                pts_rate: raw.pts_rate,
            };
            let mut map = self.encoder_instances.lock();
            let session = map
                .get_mut(&instance)
                .ok_or_else(|| format!("unknown encoder instance {instance}"))?;
            session.push_frame(&frame)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Push one interleaved `f32` audio block to encoder `instance`.
    ///
    /// # Safety
    ///
    /// `audio_ptr` must be non-null and point to a valid [`AriaEncoderAudio`] whose
    /// `samples` pointer covers `samples_len` `f32` elements for the duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_encoder_push_audio(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        audio_ptr: *const AriaEncoderAudio,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            if audio_ptr.is_null() {
                return Err("encoder_push_audio received null audio".to_owned());
            }
            // SAFETY: `audio_ptr` is valid for the duration of `encoder_push_audio`.
            let raw = unsafe { &*audio_ptr };
            let samples = if raw.samples_len == 0 || raw.samples.is_null() {
                &[][..]
            } else {
                // SAFETY: `raw.samples` points to `raw.samples_len` valid f32 samples.
                unsafe { std::slice::from_raw_parts(raw.samples, raw.samples_len) }
            };
            let audio = EncoderAudio {
                samples,
                sample_rate: raw.sample_rate,
                channels: raw.channels,
                pts_value: raw.pts_value,
                pts_rate: raw.pts_rate,
            };
            let mut map = self.encoder_instances.lock();
            let session = map
                .get_mut(&instance)
                .ok_or_else(|| format!("unknown encoder instance {instance}"))?;
            session.push_audio(&audio)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Finalize encoder `instance` and remove it from the active table.
    pub fn handle_encoder_finish(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let session = self
                .encoder_instances
                .lock()
                .remove(&instance)
                .ok_or_else(|| format!("unknown encoder instance {instance}"))?;
            session.finish()
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Drop encoder `instance` without finishing (for example on cancellation).
    pub fn handle_encoder_drop(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let removed = self.encoder_instances.lock().remove(&instance);
            drop(removed);
            Ok::<(), String>(())
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

    /// Describe `path` with decoder `decoder_id`, as probe JSON.
    pub fn handle_decoder_probe(
        &self,
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let decoder = self.decoder(Self::str_arg(decoder_id, "decoder id")?)?;
            let path = Self::str_arg(path, "decoder path")?;
            let probe = decoder.probe(&host, Path::new(path))?;
            serde_json::to_string(&probe).map_err(|err| DecodeFailure::Failed(err.to_string()))
        })) {
            Ok(Ok(json)) => AriaResult::ok(json),
            Ok(Err(failure)) => failure_result(failure),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Open the video stream of `path` with decoder `decoder_id`, writing its
    /// description into `info`.
    ///
    /// # Safety
    ///
    /// `info` must be null or point to an [`AriaVideoInfo`] writable for the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_decoder_open_video(
        &self,
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
        info: *mut AriaVideoInfo,
    ) -> AriaInstanceResult {
        if info.is_null() {
            return AriaInstanceResult::err("decoder_open_video received a null info");
        }
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let decoder = self.decoder(Self::str_arg(decoder_id, "decoder id")?)?;
            let path = Self::str_arg(path, "decoder path")?;
            let (stream_info, stream) = decoder.open_video(&host, Path::new(path))?;
            let instance = self.open_instance(DecoderInstance::Video(Arc::new(Mutex::new(stream))));
            Ok::<_, DecodeFailure>((instance, stream_info.to_raw()))
        })) {
            Ok(Ok((instance, raw))) => {
                // SAFETY: `info` is non-null and the caller guarantees it is writable.
                unsafe { info.write(raw) };
                AriaInstanceResult::ok(instance)
            }
            Ok(Err(failure)) => failure_instance(failure),
            Err(payload) => AriaInstanceResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Seek video stream `instance` to the sync frame at or before `pts`.
    pub fn handle_decoder_seek(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        pts: i64,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            self.video_stream(instance)?.lock().seek(pts)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Fill the planes of `frame` with the next frame of video stream `instance`.
    ///
    /// # Safety
    ///
    /// `frame` must be null or point to an [`AriaVideoFrameOut`] writable for
    /// the call, whose `planes` hold `planes_len` planes, each `data` covering
    /// `len` writable bytes.
    #[allow(unsafe_code)]
    pub unsafe fn handle_decoder_next_frame(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        frame: *mut AriaVideoFrameOut,
    ) -> AriaResult {
        if frame.is_null() {
            return AriaResult::err("decoder_next_frame received a null frame");
        }
        // SAFETY: `frame` is non-null and the caller guarantees it is writable.
        let out = unsafe { &mut *frame };
        let raw_planes = if out.planes.is_null() || out.planes_len == 0 {
            &[][..]
        } else {
            // SAFETY: `out.planes` holds `out.planes_len` planes for the call.
            unsafe { std::slice::from_raw_parts(out.planes, out.planes_len) }
        };
        let mut planes: Vec<FramePlane<'_>> = raw_planes
            .iter()
            .map(|plane| FramePlane {
                data: if plane.data.is_null() || plane.len == 0 {
                    &mut []
                } else {
                    // SAFETY: each plane's `data` covers `len` writable bytes for the call,
                    // and no two planes overlap.
                    unsafe { std::slice::from_raw_parts_mut(plane.data, plane.len) }
                },
                stride: plane.stride as usize,
                rows: plane.rows as usize,
            })
            .collect();
        match catch_unwind(AssertUnwindSafe(|| {
            self.video_stream(instance)?.lock().next_frame(&mut planes)
        })) {
            Ok(Ok(Some(pts))) => {
                out.pts = pts;
                out.produced = 1;
                AriaResult::ok_empty()
            }
            Ok(Ok(None)) => {
                out.produced = 0;
                AriaResult::ok_empty()
            }
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Open the audio of `path` with decoder `decoder_id`, resampled to
    /// `sample_rate` and `channels`.
    pub fn handle_decoder_open_audio(
        &self,
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
        sample_rate: u32,
        channels: u32,
    ) -> AriaInstanceResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let decoder = self.decoder(Self::str_arg(decoder_id, "decoder id")?)?;
            let path = Self::str_arg(path, "decoder path")?;
            let channels = u16::try_from(channels)
                .map_err(|_| DecodeFailure::Failed(format!("{channels} channels is too many")))?;
            let stream = decoder.open_audio(&host, Path::new(path), sample_rate, channels)?;
            Ok::<_, DecodeFailure>(
                self.open_instance(DecoderInstance::Audio(Arc::new(Mutex::new(stream)))),
            )
        })) {
            Ok(Ok(instance)) => AriaInstanceResult::ok(instance),
            Ok(Err(failure)) => failure_instance(failure),
            Err(payload) => AriaInstanceResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Read interleaved samples of audio stream `instance` into `out`.
    ///
    /// # Safety
    ///
    /// `out` must cover `out_len` writable samples and `written` must be null
    /// or writable for the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_decoder_read_audio(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        out: *mut f32,
        out_len: usize,
        written: *mut usize,
    ) -> AriaResult {
        if written.is_null() {
            return AriaResult::err("decoder_read_audio received a null count");
        }
        let samples = if out.is_null() || out_len == 0 {
            &mut [][..]
        } else {
            // SAFETY: `out` covers `out_len` writable samples for the call.
            unsafe { std::slice::from_raw_parts_mut(out, out_len) }
        };
        match catch_unwind(AssertUnwindSafe(|| {
            let read = self.audio_stream(instance)?.lock().read(samples)?;
            if read > out_len {
                return Err(format!("read {read} samples into {out_len}"));
            }
            Ok(read)
        })) {
            Ok(Ok(read)) => {
                // SAFETY: `written` is non-null and the caller guarantees it is writable.
                unsafe { written.write(read) };
                AriaResult::ok_empty()
            }
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Position the next read of audio stream `instance` at sample frame `frame`.
    pub fn handle_decoder_seek_audio(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        frame: i64,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            self.audio_stream(instance)?.lock().seek(frame)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Drop the video or audio stream `instance`.
    pub fn handle_decoder_close(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            drop(self.decoder_instances.lock().remove(&instance));
        })) {
            Ok(()) => AriaResult::ok_empty(),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
