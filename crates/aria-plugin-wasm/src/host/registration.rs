use std::sync::Arc;

use aria_plugin::{PluginCommand, PluginContext, ScopeSpec, event_wire};

use super::{ActiveThreadGuard, CallPhase, HostState, InstanceState, PluginKind, WasmPluginShared};
use crate::ai_tools::{self, WasmAiTool};
use crate::bindings;
use crate::contributions;
use crate::runtime::classify_trap;
use crate::scopes::WasmScope;

impl WasmPluginShared {
    pub(crate) fn call_command_run(
        &self,
        ctx: &PluginContext,
        id: &str,
        args_json: &str,
    ) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Command, |live, _engine| {
            match &live.plugin {
                PluginKind::Current(p) => p.call_command_run(&mut live.store, id, args_json),
                PluginKind::Legacy(p) => p.call_command_run(&mut live.store, id, args_json),
            }
        })
    }

    pub(crate) fn call_event(
        &self,
        ctx: &PluginContext,
        kind: &str,
        event_json: &str,
    ) -> Result<Option<String>, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Ok(None);
        }
        self.run_with_instance(ctx, CallPhase::Event, |live, _engine| match &live.plugin {
            PluginKind::Current(p) => p.call_event(&mut live.store, kind, event_json),
            PluginKind::Legacy(p) => p.call_event(&mut live.store, kind, event_json),
        })
    }

    pub(crate) fn call_ai_tool(
        &self,
        ctx: &PluginContext,
        name: &str,
        arguments_json: &str,
    ) -> Result<bindings::AiToolOutput, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(format!(
                "plugin {} cannot run its AI tool {name} from inside one of its own calls",
                self.manifest.id
            ));
        }
        self.run_with_instance(ctx, CallPhase::AiTool, |live, _engine| {
            live.plugin
                .current()?
                .call_ai_tool_call(&mut live.store, name, arguments_json)
        })
    }

    /// Measure `frame` with the scope registered as `id`. A trap fails the
    /// plugin.
    pub(crate) fn call_scope_measure(
        &self,
        ctx: &PluginContext,
        id: &str,
        frame: &bindings::ScopeFrame,
    ) -> Result<bindings::ScopeDrawing, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(format!(
                "plugin {} cannot measure its scope {id} from inside one of its own calls",
                self.manifest.id
            ));
        }
        self.run_with_instance(ctx, CallPhase::Scope, |live, _engine| {
            live.plugin
                .current()?
                .call_scope_measure(&mut live.store, id, frame)
        })
    }

    pub(crate) fn call_cpu_effect_process(
        &self,
        ctx: &PluginContext,
        id: &str,
        params: &[f32],
        frame: &bindings::VideoFrame,
    ) -> Result<bindings::VideoFrame, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(format!(
                "plugin {} cannot run CPU effect {id} from inside one of its own calls",
                self.manifest.id
            ));
        }
        let mut guard = self.state.lock();
        let live = match &mut *guard {
            InstanceState::Mounted(live) => live,
            InstanceState::Faulted(reason) => {
                return Err(format!("plugin {} has faulted: {reason}", self.manifest.id));
            }
            InstanceState::Unmounted => {
                return Err(format!("plugin {} is not mounted", self.manifest.id));
            }
        };
        self.prepare_call(&mut live.store, CallPhase::CpuEffect, ctx);
        let result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            live.plugin
                .current()
                .and_then(|p| p.call_cpu_effect_process(&mut live.store, id, params, frame))
        };
        Self::finish_call(&mut live.store);
        match result {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(guest_err)) => Err(guest_err),
            Err(trap_err) => {
                let reason = classify_trap(&trap_err);
                *guard = InstanceState::Faulted(reason.clone());
                drop(guard);
                std::panic::resume_unwind(Box::new(reason));
            }
        }
    }

    pub(crate) fn call_driver_evaluate(
        &self,
        ctx: &PluginContext,
        id: &str,
        expr: &str,
        vars: bindings::DriverVars,
    ) -> Result<bindings::ParamValue, String> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(format!(
                "plugin {} cannot evaluate driver {id} from inside one of its own calls",
                self.manifest.id
            ));
        }
        self.run_with_instance(ctx, CallPhase::Driver, |live, _engine| {
            live.plugin
                .current()?
                .call_driver_evaluate(&mut live.store, id, expr, vars)
        })
    }

    pub(crate) fn call_analyzer_open(
        &self,
        ctx: &PluginContext,
        id: &str,
        sample_rate: u32,
        channels: u16,
    ) -> Result<u64, String> {
        self.run_with_instance(ctx, CallPhase::Analyzer, |live, _engine| {
            live.plugin
                .current()?
                .call_analyzer_open(&mut live.store, id, sample_rate, channels)
        })
    }

    pub(crate) fn call_analyzer_push(
        &self,
        ctx: &PluginContext,
        instance: u64,
        samples: &[f32],
    ) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Analyzer, |live, _engine| {
            live.plugin
                .current()?
                .call_analyzer_push(&mut live.store, instance, samples)
        })
    }

    pub(crate) fn call_analyzer_finish(
        &self,
        ctx: &PluginContext,
        instance: u64,
    ) -> Result<String, String> {
        self.run_with_instance(ctx, CallPhase::Analyzer, |live, _engine| {
            live.plugin
                .current()?
                .call_analyzer_finish(&mut live.store, instance)
        })
    }

    pub(crate) fn call_analyzer_drop(&self, ctx: &PluginContext, instance: u64) {
        let _ = self.run_with_instance(ctx, CallPhase::Analyzer, |live, _engine| {
            live.plugin
                .current()?
                .call_analyzer_drop(&mut live.store, instance)
                .map(Ok)
        });
    }
}

struct WasmCommandHandler {
    shared: Arc<WasmPluginShared>,
    id: String,
}

impl PluginCommand for WasmCommandHandler {
    fn run(&self, ctx: &PluginContext) -> Result<(), String> {
        self.shared.call_command_run(ctx, &self.id, "{}")
    }
}

impl HostState {
    pub(super) fn host_register_command(
        &mut self,
        id: String,
        title: String,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-command")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let spec = self.package.command_spec(id.clone(), title);
        ctx.register_command(spec, WasmCommandHandler { shared, id })
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    pub(super) fn host_subscribe(&mut self, event_kind: String) -> Result<(), String> {
        let ctx = self.require_mount_phase("subscribe")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        event_wire::subscribe(&ctx, &event_kind, move |scoped, kind, payload| {
            shared.call_event(scoped, kind, payload).ok().flatten()
        })
    }

    pub(super) fn host_register_ai_tool(
        &mut self,
        spec: bindings::AiToolSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-ai-tool")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let tool = WasmAiTool::new(shared, spec.name.clone());
        ctx.register_ai_tool(ai_tools::host_spec(spec)?, tool)
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    pub(super) fn host_register_scope(&mut self, spec: bindings::ScopeSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-scope")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let scope = WasmScope::new(shared, spec.id.clone());
        ctx.register_scope(
            ScopeSpec {
                id: spec.id,
                name: spec.name,
                width: spec.width,
                height: spec.height,
            },
            scope,
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    pub(super) fn host_set_effect_category(
        &mut self,
        id: String,
        category: String,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("set-effect-category")?;
        ctx.set_effect_category(&id, &category)
    }

    pub(super) fn host_register_effect(
        &mut self,
        spec: bindings::EffectSpec,
        passes: Vec<String>,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-effect")?;
        contributions::register_effect(&ctx, spec, passes)
    }

    pub(super) fn host_register_cpu_effect(
        &mut self,
        spec: bindings::EffectSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-cpu-effect")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        contributions::register_cpu_effect(&ctx, shared, spec)
    }

    pub(super) fn host_register_transition(
        &mut self,
        spec: bindings::TransitionSpec,
        wgsl: String,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-transition")?;
        contributions::register_transition(&ctx, spec, wgsl)
    }

    pub(super) fn host_register_generator(
        &mut self,
        spec: bindings::EffectSpec,
        passes: Vec<String>,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-generator")?;
        contributions::register_generator(&ctx, spec, passes)
    }

    pub(super) fn host_register_effect_graph(
        &mut self,
        spec: bindings::EffectSpec,
        traits: bindings::EffectTraits,
        passes: Vec<bindings::GraphPass>,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-effect-graph")?;
        contributions::register_effect_graph(&ctx, spec, traits, passes)
    }

    pub(super) fn host_register_texture(
        &mut self,
        id: String,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-texture")?;
        contributions::register_texture(&ctx, id, width, height, pixels)
    }

    pub(super) fn host_register_clip_type(
        &mut self,
        spec: bindings::ClipTypeSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-clip-type")?;
        contributions::register_clip_type(&ctx, spec)
    }

    pub(super) fn host_define_clip_type(
        &mut self,
        spec: bindings::ClipTypeSpec,
        schema: bindings::ClipTypeSchema,
        render: bindings::ClipTypeRender,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("define-clip-type")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        contributions::define_clip_type(&ctx, shared, spec, schema, render)
    }

    pub(super) fn host_register_driver_language(
        &mut self,
        spec: bindings::DriverLanguageSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-driver-language")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        contributions::register_driver_language(&ctx, shared, spec)
    }

    pub(super) fn host_register_preset(&mut self, preset_json: String) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-preset")?;
        contributions::register_preset(&ctx, &preset_json)
    }

    pub(super) fn host_register_theme(&mut self, id: String, toml: String) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-theme")?;
        contributions::register_theme(&ctx, &id, &toml)
    }

    pub(super) fn host_register_workspace(
        &mut self,
        spec: bindings::WorkspaceSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-workspace")?;
        contributions::register_workspace(&ctx, spec)
    }

    pub(super) fn host_register_analyzer(
        &mut self,
        spec: bindings::AnalyzerSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-analyzer")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        contributions::register_analyzer(&ctx, shared, spec)
    }
}
