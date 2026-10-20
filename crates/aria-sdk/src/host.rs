//! Safe Rust wrapper around the [`AriaHostV1`] function table.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;

use aria_plugin_abi::{
    ARIA_BYTES_ERR, ARIA_BYTES_FOUND, ARIA_BYTES_NOT_FOUND, ARIA_INSTANCE_OK, ARIA_STATUS_OK,
    AriaBytes, AriaHostV1, AriaResult, AriaStr,
};
use parking_lot::Mutex;

use crate::{
    AiToolSpec, Analyzer, AnalyzerSpec, AudioEffectSpec, ClipTypeSpec, CpuEffect, Decoder,
    DecoderSpec, DriverLanguage, DriverLanguageSpec, EffectSpec, Encoder, EncoderSpec,
    ExporterSpec, GraphPass, ImporterSpec, LogLevel, Panel, PanelSpec, PresetSpec, ScopeSpec,
    TransitionSpec, WorkspaceSpec,
};

mod clip_types;
mod project;

pub(crate) type PanelRegistry = Arc<Mutex<HashMap<String, Arc<dyn Panel>>>>;
pub(crate) type EncoderRegistry = Arc<Mutex<HashMap<String, Arc<dyn Encoder>>>>;
pub(crate) type DecoderRegistry = Arc<Mutex<HashMap<String, Arc<dyn Decoder>>>>;
pub(crate) type CpuEffectRegistry = Arc<Mutex<HashMap<String, Arc<dyn CpuEffect>>>>;
pub(crate) type CpuEffectSpecRegistry = Arc<Mutex<HashMap<String, Arc<EffectSpec>>>>;
pub(crate) type DriverLanguageRegistry = Arc<Mutex<HashMap<String, Arc<dyn DriverLanguage>>>>;
pub(crate) type AnalyzerRegistry = Arc<Mutex<HashMap<String, Arc<dyn Analyzer>>>>;

/// Safe wrapper over the host's [`AriaHostV1`] table and per-plugin context pointer.
#[derive(Clone)]
pub struct Host {
    table: &'static AriaHostV1,
    ctx: *mut c_void,
    panels: Option<PanelRegistry>,
    encoders: Option<EncoderRegistry>,
    decoders: Option<DecoderRegistry>,
    cpu_effects: Option<CpuEffectRegistry>,
    cpu_effect_specs: Option<CpuEffectSpecRegistry>,
    driver_languages: Option<DriverLanguageRegistry>,
    analyzers: Option<AnalyzerRegistry>,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("api_major", &self.table.api_major)
            .finish_non_exhaustive()
    }
}

impl Host {
    /// Create a [`Host`] from a validated static host table and context pointer.
    ///
    /// # Safety
    ///
    /// `table` must be a valid static [`AriaHostV1`] and `ctx` must be the
    /// `host_ctx` pointer passed by the host for the current call.
    #[allow(unsafe_code)]
    pub unsafe fn from_raw(table: &'static AriaHostV1, ctx: *mut c_void) -> Self {
        Self {
            table,
            ctx,
            panels: None,
            encoders: None,
            decoders: None,
            cpu_effects: None,
            cpu_effect_specs: None,
            driver_languages: None,
            analyzers: None,
        }
    }

    pub(crate) fn with_panels(mut self, panels: PanelRegistry) -> Self {
        self.panels = Some(panels);
        self
    }

    pub(crate) fn with_encoders(mut self, encoders: EncoderRegistry) -> Self {
        self.encoders = Some(encoders);
        self
    }

    pub(crate) fn with_decoders(mut self, decoders: DecoderRegistry) -> Self {
        self.decoders = Some(decoders);
        self
    }

    pub(crate) fn with_cpu_effects(mut self, cpu_effects: CpuEffectRegistry) -> Self {
        self.cpu_effects = Some(cpu_effects);
        self
    }

    pub(crate) fn with_cpu_effect_specs(mut self, specs: CpuEffectSpecRegistry) -> Self {
        self.cpu_effect_specs = Some(specs);
        self
    }

    pub(crate) fn with_driver_languages(
        mut self,
        driver_languages: DriverLanguageRegistry,
    ) -> Self {
        self.driver_languages = Some(driver_languages);
        self
    }
    #[allow(unsafe_code)]
    fn decode_unit(&self, res: AriaResult) -> Result<(), String> {
        // SAFETY: `res.value` was allocated by `self.table`.
        let text = unsafe { res.value.into_string_with(self.table.free_string) };
        if res.status == ARIA_STATUS_OK {
            Ok(())
        } else {
            Err(text)
        }
    }

    #[allow(unsafe_code)]
    fn decode_string(&self, res: AriaResult) -> Result<String, String> {
        // SAFETY: `res.value` was allocated by `self.table`.
        let text = unsafe { res.value.into_string_with(self.table.free_string) };
        if res.status == ARIA_STATUS_OK {
            Ok(text)
        } else {
            Err(text)
        }
    }

    /// Emit a log line at `level`.
    #[allow(unsafe_code)]
    pub fn log(&self, level: LogLevel, text: &str) {
        // SAFETY: `self.table.log` is a valid host function for the duration of the call.
        unsafe {
            (self.table.log)(self.ctx, level.to_raw(), AriaStr::from_str(text));
        }
    }

    /// Read a key from the plugin's storage directory.
    #[allow(unsafe_code)]
    pub fn storage_get(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        // SAFETY: `self.table.storage_get` is a valid host function.
        let res = unsafe { (self.table.storage_get)(self.ctx, AriaStr::from_str(key)) };
        // SAFETY: `res.bytes` and `res.error` were allocated by `self.table`.
        let bytes = unsafe { res.bytes.into_vec_with(self.table.free_bytes) };
        // SAFETY: `res.error` was allocated by `self.table`.
        let err = unsafe { res.error.into_string_with(self.table.free_string) };
        match res.status {
            ARIA_BYTES_FOUND => Ok(Some(bytes)),
            ARIA_BYTES_NOT_FOUND => Ok(None),
            ARIA_BYTES_ERR => Err(err),
            other => Err(format!("unknown storage_get status {other}: {err}")),
        }
    }

    /// Write `value` under `key` in the plugin's storage directory.
    #[allow(unsafe_code)]
    pub fn storage_set(&self, key: &str, value: &[u8]) -> Result<(), String> {
        // SAFETY: `self.table.storage_set` is a valid host function.
        let res = unsafe {
            (self.table.storage_set)(
                self.ctx,
                AriaStr::from_str(key),
                AriaBytes::from_slice(value),
            )
        };
        self.decode_unit(res)
    }

    /// Delete `key` from the plugin's storage directory.
    #[allow(unsafe_code)]
    pub fn storage_delete(&self, key: &str) -> Result<(), String> {
        // SAFETY: `self.table.storage_delete` is a valid host function.
        let res = unsafe { (self.table.storage_delete)(self.ctx, AriaStr::from_str(key)) };
        self.decode_unit(res)
    }

    /// List all keys in the plugin's storage directory, in sorted order.
    #[allow(unsafe_code)]
    pub fn storage_keys(&self) -> Result<Vec<String>, String> {
        // SAFETY: `self.table.storage_keys` is a valid host function.
        let res = unsafe { (self.table.storage_keys)(self.ctx) };
        let json = self.decode_string(res)?;
        serde_json::from_str(&json).map_err(|err| err.to_string())
    }

    /// Read a setting value as raw JSON text, or `None` if unset.
    #[allow(unsafe_code)]
    pub fn setting_get_json(&self, key: &str) -> Result<Option<String>, String> {
        // SAFETY: `self.table.setting_get` is a valid host function.
        let res = unsafe { (self.table.setting_get)(self.ctx, AriaStr::from_str(key)) };
        let is_null = res.value.is_null();
        let text = self.decode_string(res)?;
        if is_null || text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(text))
        }
    }

    /// Read a setting value as a parsed [`serde_json::Value`], or `None` if unset.
    pub fn setting_get(&self, key: &str) -> Result<Option<serde_json::Value>, String> {
        match self.setting_get_json(key)? {
            Some(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|err| err.to_string()),
            None => Ok(None),
        }
    }

    /// Register a command during `mount`.
    #[allow(unsafe_code)]
    pub fn register_command(&self, id: &str, title: &str) -> Result<(), String> {
        // SAFETY: `self.table.register_command` is a valid host function.
        let res = unsafe {
            (self.table.register_command)(self.ctx, AriaStr::from_str(id), AriaStr::from_str(title))
        };
        self.decode_unit(res)
    }

    /// Subscribe to an event kind during `mount`.
    #[allow(unsafe_code)]
    pub fn subscribe(&self, event_kind: &str) -> Result<(), String> {
        // SAFETY: `self.table.subscribe` is a valid host function.
        let res = unsafe { (self.table.subscribe)(self.ctx, AriaStr::from_str(event_kind)) };
        self.decode_unit(res)
    }

    /// Subscribe to a serial event kind with an explicit handler priority during `mount`.
    ///
    /// Higher `priority` values run first; equal priorities run in mount order.
    pub fn subscribe_with_priority(&self, event_kind: &str, priority: i32) -> Result<(), String> {
        self.subscribe(&format!("{event_kind}:{priority}"))
    }

    /// Register a timeline importer during `mount`.
    #[allow(unsafe_code)]
    pub fn register_importer(&self, spec: &ImporterSpec) -> Result<(), String> {
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.register_importer` is a valid host function.
        let res =
            unsafe { (self.table.register_importer)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    /// Register a timeline exporter during `mount`.
    #[allow(unsafe_code)]
    pub fn register_exporter(&self, spec: &ExporterSpec) -> Result<(), String> {
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.register_exporter` is a valid host function.
        let res =
            unsafe { (self.table.register_exporter)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    /// Register an audio effect during `mount`.
    #[allow(unsafe_code)]
    pub fn register_audio_effect(&self, spec: &AudioEffectSpec) -> Result<(), String> {
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.register_audio_effect` is a valid host function.
        let res =
            unsafe { (self.table.register_audio_effect)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    /// Register a panel specification during `mount` and bind `panel` to handle its
    /// [`Panel::view`] and [`Panel::event`] callbacks.
    pub fn register_panel(&self, spec: PanelSpec, panel: impl Panel) -> Result<(), String> {
        self.register_panel_spec(&spec)?;
        if let Some(panels) = &self.panels {
            panels.lock().insert(spec.id, Arc::new(panel));
        }
        Ok(())
    }

    /// Register a panel specification during `mount` whose callbacks are handled by
    /// [`Plugin::panel_view`](crate::Plugin::panel_view) and
    /// [`Plugin::panel_event`](crate::Plugin::panel_event).
    #[allow(unsafe_code)]
    pub fn register_panel_spec(&self, spec: &PanelSpec) -> Result<(), String> {
        if !self.table.has_panels() {
            return Err("host table does not support panels".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_panels()` confirmed `register_panel` is present in `self.table`.
        let res = unsafe { (self.table.register_panel)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    /// Mark a panel owned by this plugin as needing a view rebuild on the next flush.
    #[allow(unsafe_code)]
    pub fn refresh_panel(&self, id: &str) -> Result<(), String> {
        if !self.table.has_panels() {
            return Err("host table does not support panels".to_owned());
        }
        // SAFETY: `self.table.has_panels()` confirmed `refresh_panel` is present in `self.table`.
        let res = unsafe { (self.table.refresh_panel)(self.ctx, AriaStr::from_str(id)) };
        self.decode_unit(res)
    }

    /// Register a media encoder specification during `mount` and bind `encoder` to handle
    /// its [`Encoder::open`] callback.
    pub fn register_encoder(&self, spec: EncoderSpec, encoder: impl Encoder) -> Result<(), String> {
        self.register_encoder_spec(&spec)?;
        if let Some(encoders) = &self.encoders {
            encoders.lock().insert(spec.id, Arc::new(encoder));
        }
        Ok(())
    }

    /// Register a media encoder specification during `mount` whose sessions are opened by
    /// [`Plugin::encoder_open`](crate::Plugin::encoder_open).
    #[allow(unsafe_code)]
    pub fn register_encoder_spec(&self, spec: &EncoderSpec) -> Result<(), String> {
        if !self.table.has_encoders() {
            return Err("host table does not support encoders".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_encoders()` confirmed `register_encoder` is present in `self.table`.
        let res = unsafe { (self.table.register_encoder)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    /// Register a media decoder during `mount`. Aria opens every file whose
    /// extension `spec` lists through `decoder` before the platform decoder.
    #[allow(unsafe_code)]
    pub fn register_decoder(&self, spec: DecoderSpec, decoder: impl Decoder) -> Result<(), String> {
        if !self.table.has_decoders() {
            return Err("host table does not support decoders".to_owned());
        }
        let Some(decoders) = &self.decoders else {
            return Err("decoders register only through the plugin runtime".to_owned());
        };
        let spec_json = serde_json::to_string(&spec).map_err(|err| err.to_string())?;
        // Held before the host lists the decoder, so the first open finds it.
        let previous = decoders.lock().insert(spec.id.clone(), Arc::new(decoder));
        // SAFETY: `self.table.has_decoders()` confirmed `register_decoder` is present in `self.table`.
        let res = unsafe { (self.table.register_decoder)(self.ctx, AriaStr::from_str(&spec_json)) };
        let registered = self.decode_unit(res);
        if registered.is_err() {
            let mut held = decoders.lock();
            match previous {
                Some(previous) => held.insert(spec.id, previous),
                None => held.remove(&spec.id),
            };
        }
        registered
    }

    /// Offer the Aria assistant the tool `spec` during `mount`. The assistant
    /// calls it as `plugin_<name>`, and [`Plugin::ai_tool_call`](crate::Plugin::ai_tool_call)
    /// runs it.
    #[allow(unsafe_code)]
    pub fn register_ai_tool(&self, spec: &AiToolSpec) -> Result<(), String> {
        if !self.table.has_ai_tools() {
            return Err("host table does not support AI tools".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_ai_tools()` confirmed `register_ai_tool` is present in `self.table`.
        let res = unsafe { (self.table.register_ai_tool)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    /// Add the scope `spec` to the scope strip during `mount`.
    /// [`Plugin::scope_measure`](crate::Plugin::scope_measure) measures each
    /// frame with it.
    #[allow(unsafe_code)]
    pub fn register_scope(&self, spec: &ScopeSpec) -> Result<(), String> {
        if !self.table.has_scopes() {
            return Err("host table does not support scopes".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_scopes()` confirmed `register_scope` is present in `self.table`.
        let res = unsafe { (self.table.register_scope)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    /// Register a GPU WGSL visual effect during `mount`.
    #[allow(unsafe_code)]
    pub fn register_effect(&self, spec: &EffectSpec, wgsl: &str) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support effect registration".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_effect` is present in `self.table`.
        let res = unsafe {
            (self.table.register_effect)(
                self.ctx,
                AriaStr::from_str(&spec_json),
                AriaStr::from_str(wgsl),
            )
        };
        self.decode_unit(res)
    }

    /// Register a GPU WGSL visual effect of several fragment passes during
    /// `mount`, each reading the pass before it.
    pub fn register_effect_passes(&self, spec: &EffectSpec, passes: &[&str]) -> Result<(), String> {
        let passes_json = serde_json::to_string(passes).map_err(|err| err.to_string())?;
        self.register_effect(spec, &passes_json)
    }

    /// Register a GPU WGSL visual effect whose passes form `graph` during
    /// `mount`: compute and fragment passes, each reading the source frame,
    /// earlier passes, referenced layers, and plugin textures, and writing its
    /// own size. The last pass writes the effect's frame.
    pub fn register_effect_graph(
        &self,
        spec: &EffectSpec,
        graph: &[GraphPass],
    ) -> Result<(), String> {
        let graph_json = serde_json::to_string(graph).map_err(|err| err.to_string())?;
        self.register_effect(spec, &graph_json)
    }

    /// Register a CPU visual effect during `mount` and bind `effect` to handle its
    /// [`CpuEffect::process`] callback.
    pub fn register_cpu_effect(
        &self,
        spec: EffectSpec,
        effect: impl CpuEffect,
    ) -> Result<(), String> {
        let previous = self
            .cpu_effects
            .as_ref()
            .and_then(|reg| reg.lock().insert(spec.id.clone(), Arc::new(effect)));
        let res = self.register_cpu_effect_spec(&spec);
        if res.is_err()
            && let Some(reg) = &self.cpu_effects
        {
            let mut held = reg.lock();
            match previous {
                Some(prev) => held.insert(spec.id, prev),
                None => held.remove(&spec.id),
            };
        }
        res
    }

    /// Register a CPU visual effect specification during `mount` whose frames are processed
    /// by [`Plugin::cpu_effect_process`](crate::Plugin::cpu_effect_process).
    #[allow(unsafe_code)]
    pub fn register_cpu_effect_spec(&self, spec: &EffectSpec) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support CPU effect registration".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_cpu_effect` is present in `self.table`.
        let res =
            unsafe { (self.table.register_cpu_effect)(self.ctx, AriaStr::from_str(&spec_json)) };
        let out = self.decode_unit(res);
        if out.is_ok()
            && let Some(specs) = &self.cpu_effect_specs
        {
            specs.lock().insert(spec.id.clone(), Arc::new(spec.clone()));
        }
        out
    }

    /// Register a GPU WGSL transition during `mount`.
    #[allow(unsafe_code)]
    pub fn register_transition(&self, spec: &TransitionSpec, wgsl: &str) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support transition registration".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_transition` is present in `self.table`.
        let res = unsafe {
            (self.table.register_transition)(
                self.ctx,
                AriaStr::from_str(&spec_json),
                AriaStr::from_str(wgsl),
            )
        };
        self.decode_unit(res)
    }

    /// Register a GPU WGSL generator during `mount`.
    #[allow(unsafe_code)]
    pub fn register_generator(&self, spec: &EffectSpec, wgsl: &str) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support generator registration".to_owned());
        }
        let mut spec = spec.clone();
        spec.generator = true;
        let spec_json = serde_json::to_string(&spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_generator` is present in `self.table`.
        let res = unsafe {
            (self.table.register_generator)(
                self.ctx,
                AriaStr::from_str(&spec_json),
                AriaStr::from_str(wgsl),
            )
        };
        self.decode_unit(res)
    }

    /// Register a GPU WGSL generator of several fragment passes during
    /// `mount`, each reading the pass before it.
    pub fn register_generator_passes(
        &self,
        spec: &EffectSpec,
        passes: &[&str],
    ) -> Result<(), String> {
        let passes_json = serde_json::to_string(passes).map_err(|err| err.to_string())?;
        self.register_generator(spec, &passes_json)
    }

    /// Register a GPU WGSL generator whose passes form `graph` during `mount`,
    /// on the terms of [`register_effect_graph`](Self::register_effect_graph).
    pub fn register_generator_graph(
        &self,
        spec: &EffectSpec,
        graph: &[GraphPass],
    ) -> Result<(), String> {
        let graph_json = serde_json::to_string(graph).map_err(|err| err.to_string())?;
        self.register_generator(spec, &graph_json)
    }

    /// Register a CPU generator during `mount` and bind `generator` to handle its
    /// [`CpuEffect::process`] callback.
    pub fn register_cpu_generator(
        &self,
        mut spec: EffectSpec,
        generator: impl CpuEffect,
    ) -> Result<(), String> {
        spec.generator = true;
        let previous = self
            .cpu_effects
            .as_ref()
            .and_then(|reg| reg.lock().insert(spec.id.clone(), Arc::new(generator)));
        let res = self.register_cpu_generator_spec(&spec);
        if res.is_err()
            && let Some(reg) = &self.cpu_effects
        {
            let mut held = reg.lock();
            match previous {
                Some(prev) => held.insert(spec.id, prev),
                None => held.remove(&spec.id),
            };
        }
        res
    }

    /// Register a CPU generator specification during `mount` whose frames are processed
    /// by [`Plugin::cpu_effect_process`](crate::Plugin::cpu_effect_process).
    #[allow(unsafe_code)]
    pub fn register_cpu_generator_spec(&self, spec: &EffectSpec) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support generator registration".to_owned());
        }
        let mut spec = spec.clone();
        spec.generator = true;
        let spec_json = serde_json::to_string(&spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_generator` is present in `self.table`.
        let res = unsafe {
            (self.table.register_generator)(
                self.ctx,
                AriaStr::from_str(&spec_json),
                AriaStr::from_str(""),
            )
        };
        let out = self.decode_unit(res);
        if out.is_ok()
            && let Some(specs) = &self.cpu_effect_specs
        {
            specs.lock().insert(spec.id.clone(), Arc::new(spec));
        }
        out
    }

    /// Register a generator-backed clip type during `mount`.
    #[allow(unsafe_code)]
    pub fn register_clip_type(&self, spec: &ClipTypeSpec) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support clip type registration".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_clip_type` is present in `self.table`.
        let res =
            unsafe { (self.table.register_clip_type)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    /// Register a parameter-driver expression language during `mount` and bind
    /// `language` to handle its [`DriverLanguage::evaluate`] callback.
    pub fn register_driver_language(
        &self,
        spec: DriverLanguageSpec,
        language: impl DriverLanguage,
    ) -> Result<(), String> {
        let previous = self
            .driver_languages
            .as_ref()
            .and_then(|reg| reg.lock().insert(spec.id.clone(), Arc::new(language)));
        let res = self.register_driver_language_spec(&spec);
        if res.is_err()
            && let Some(reg) = &self.driver_languages
        {
            let mut held = reg.lock();
            match previous {
                Some(prev) => held.insert(spec.id, prev),
                None => held.remove(&spec.id),
            };
        }
        res
    }

    /// Register a parameter-driver expression language specification during `mount`
    /// whose expressions are evaluated by [`Plugin::driver_evaluate`](crate::Plugin::driver_evaluate).
    #[allow(unsafe_code)]
    pub fn register_driver_language_spec(&self, spec: &DriverLanguageSpec) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support driver language registration".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_driver_language` is present in `self.table`.
        let res = unsafe {
            (self.table.register_driver_language)(self.ctx, AriaStr::from_str(&spec_json))
        };
        self.decode_unit(res)
    }

    /// Register an animation and effect preset during `mount`.
    pub fn register_preset(&self, spec: &PresetSpec) -> Result<(), String> {
        let preset_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        self.register_preset_json(&preset_json)
    }

    /// Register an animation and effect preset from raw `.ariapreset` JSON during `mount`.
    #[allow(unsafe_code)]
    pub fn register_preset_json(&self, preset_json: &str) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support preset registration".to_owned());
        }
        // SAFETY: `self.table.has_contributions()` confirmed `register_preset` is present in `self.table`.
        let res = unsafe { (self.table.register_preset)(self.ctx, AriaStr::from_str(preset_json)) };
        self.decode_unit(res)
    }

    /// Register a theme (`.toml` text) under `id` during `mount`.
    #[allow(unsafe_code)]
    pub fn register_theme(&self, id: &str, toml: &str) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support theme registration".to_owned());
        }
        // SAFETY: `self.table.has_contributions()` confirmed `register_theme` is present in `self.table`.
        let res = unsafe {
            (self.table.register_theme)(self.ctx, AriaStr::from_str(id), AriaStr::from_str(toml))
        };
        self.decode_unit(res)
    }

    /// Register `rgba`, `width * height * 4` bytes of straight-alpha RGBA8, as
    /// the plugin texture `id` during `mount`. A graph pass reads it with
    /// [`PassInput::Texture`](crate::PassInput::Texture). The texture is
    /// removed when the plugin unmounts.
    #[allow(unsafe_code)]
    pub fn register_texture(
        &self,
        id: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), String> {
        if !self.table.has_textures() {
            return Err("host table does not support texture registration".to_owned());
        }
        // SAFETY: `self.table.has_textures()` confirmed `register_texture` is present in
        // `self.table`; `id` and `rgba` outlive the call.
        let res = unsafe {
            (self.table.register_texture)(
                self.ctx,
                AriaStr::from_str(id),
                width,
                height,
                AriaBytes::from_slice(rgba),
            )
        };
        self.decode_unit(res)
    }

    /// Register a named workspace layout during `mount`.
    #[allow(unsafe_code)]
    pub fn register_workspace(&self, spec: &WorkspaceSpec) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support workspace registration".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_workspace` is present in `self.table`.
        let res =
            unsafe { (self.table.register_workspace)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    pub(crate) fn with_analyzers(mut self, analyzers: AnalyzerRegistry) -> Self {
        self.analyzers = Some(analyzers);
        self
    }

    /// Register an audio analyzer during `mount` and bind `analyzer` to handle
    /// its [`Analyzer::open`] callback.
    pub fn register_analyzer(
        &self,
        spec: AnalyzerSpec,
        analyzer: impl Analyzer,
    ) -> Result<(), String> {
        let previous = self
            .analyzers
            .as_ref()
            .and_then(|reg| reg.lock().insert(spec.id.clone(), Arc::new(analyzer)));
        let res = self.register_analyzer_spec(&spec);
        if res.is_err()
            && let Some(reg) = &self.analyzers
        {
            let mut held = reg.lock();
            match previous {
                Some(prev) => held.insert(spec.id, prev),
                None => held.remove(&spec.id),
            };
        }
        res
    }

    /// Register an audio analyzer specification during `mount` whose sessions
    /// are opened by [`Plugin::analyzer_open`](crate::Plugin::analyzer_open).
    #[allow(unsafe_code)]
    pub fn register_analyzer_spec(&self, spec: &AnalyzerSpec) -> Result<(), String> {
        if !self.table.has_contributions() {
            return Err("host table does not support analyzer registration".to_owned());
        }
        let spec_json = serde_json::to_string(spec).map_err(|err| err.to_string())?;
        // SAFETY: `self.table.has_contributions()` confirmed `register_analyzer` is present in `self.table`.
        let res =
            unsafe { (self.table.register_analyzer)(self.ctx, AriaStr::from_str(&spec_json)) };
        self.decode_unit(res)
    }

    /// Register a viewer overlay during `mount` whose scene and drag events are
    /// handled by [`Plugin::overlay_scene`](crate::Plugin::overlay_scene) and
    /// [`Plugin::overlay_event`](crate::Plugin::overlay_event).
    #[allow(unsafe_code)]
    pub fn register_viewer_overlay(&self, id: &str, title: &str) -> Result<(), String> {
        if !self.table.has_overlays() {
            return Err("host table does not include register_viewer_overlay callback".to_owned());
        }
        let spec_json = serde_json::json!({ "id": id, "title": title }).to_string();
        // SAFETY: `self.table.has_overlays()` confirmed `register_viewer_overlay` is present in `self.table`.
        let res = unsafe {
            (self.table.register_viewer_overlay)(self.ctx, AriaStr::from_str(&spec_json))
        };
        self.decode_unit(res)
    }

    /// Mark a viewer overlay owned by this plugin as needing a scene rebuild on the next flush.
    #[allow(unsafe_code)]
    pub fn refresh_viewer_overlay(&self, id: &str) -> Result<(), String> {
        if !self.table.has_overlays() {
            return Err("host table does not include refresh_viewer_overlay callback".to_owned());
        }
        // SAFETY: `self.table.has_overlays()` confirmed `refresh_viewer_overlay` is present in `self.table`.
        let res = unsafe { (self.table.refresh_viewer_overlay)(self.ctx, AriaStr::from_str(id)) };
        self.decode_unit(res)
    }

    /// Start a job labelled `label` on the host's jobs board and return its id.
    ///
    /// The host calls [`Plugin::job_step`](crate::Plugin::job_step) on the job's thread
    /// until a step is `done` or fails, or the job is cancelled, then calls
    /// [`Plugin::job_end`](crate::Plugin::job_end). A job started during `mount` runs
    /// after the mount succeeds and never runs when it fails.
    #[allow(unsafe_code)]
    pub fn start_job(&self, label: &str) -> Result<u64, String> {
        if !self.table.has_jobs() {
            return Err("host table does not include start_job callback".to_owned());
        }
        // SAFETY: `self.table.has_jobs()` confirmed `start_job` is present in `self.table`.
        let res = unsafe { (self.table.start_job)(self.ctx, AriaStr::from_str(label)) };
        // SAFETY: `res.error` was allocated by `self.table`.
        let error = unsafe { res.error.into_string_with(self.table.free_string) };
        if res.status == ARIA_INSTANCE_OK {
            Ok(res.instance)
        } else {
            Err(error)
        }
    }
}
