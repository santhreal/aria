use std::sync::Arc;
use std::sync::atomic::Ordering;

use aria_plugin::{JobStep, PluginContext};

use super::{CallPhase, HostState, WasmPluginShared};
use crate::bindings;
use crate::overlays::register_wasm_overlay;
use crate::panels::register_wasm_panel;

impl WasmPluginShared {
    pub(crate) fn call_panel_view(&self, ctx: &PluginContext, id: &str) -> Vec<bindings::UiNode> {
        self.run_live_call(ctx, CallPhase::Panel, |live| {
            live.plugin.current()?.call_panel_view(&mut live.store, id)
        })
    }

    pub(crate) fn call_panel_event(
        &self,
        ctx: &PluginContext,
        id: &str,
        event: &bindings::PanelEvent,
    ) -> Result<(), String> {
        self.run_live_call(ctx, CallPhase::Panel, |live| {
            live.plugin
                .current()?
                .call_panel_event(&mut live.store, id, event)
        })
    }

    pub(crate) fn call_overlay_scene(
        &self,
        ctx: &PluginContext,
        id: &str,
    ) -> Vec<bindings::OverlayShape> {
        self.run_live_call(ctx, CallPhase::Overlay, |live| {
            live.plugin
                .current()?
                .call_overlay_scene(&mut live.store, id)
        })
    }

    pub(crate) fn call_overlay_event(
        &self,
        ctx: &PluginContext,
        id: &str,
        event: &bindings::OverlayEvent,
    ) -> Result<(), String> {
        self.run_live_call(ctx, CallPhase::Overlay, |live| {
            live.plugin
                .current()?
                .call_overlay_event(&mut live.store, id, event)
        })
    }

    fn call_job_step(&self, ctx: &PluginContext, job: u64) -> Result<JobStep, String> {
        let step = self.run_with_instance(ctx, CallPhase::Job, |live, _engine| {
            live.plugin.current()?.call_job_step(&mut live.store, job)
        })?;
        Ok(JobStep {
            progress: step.progress,
            detail: step.detail,
            done: step.done,
            commit: step.commit,
        })
    }

    fn call_job_end(&self, ctx: &PluginContext, job: u64, cancelled: bool) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Job, |live, _engine| {
            live.plugin
                .current()?
                .call_job_end(&mut live.store, job, cancelled)
                .map(Ok)
        })
    }
}

impl HostState {
    pub(super) fn host_start_job(&mut self, label: String) -> Result<u64, String> {
        let ctx = self.require_ctx()?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let job = shared.next_job.fetch_add(1, Ordering::Relaxed);
        let (stepping, step_ctx, end_ctx) = (Arc::clone(&shared), ctx.clone(), ctx.clone());
        ctx.start_stepped_job(
            &label,
            move || stepping.call_job_step(&step_ctx, job),
            move |cancelled| {
                // A faulted or unmounted instance has no job to end.
                let _ = shared.call_job_end(&end_ctx, job, cancelled);
            },
        )
        .map(|_| job)
        .map_err(|err| err.to_string())
    }

    pub(super) fn host_register_panel(&mut self, spec: bindings::PanelSpec) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-panel")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        register_wasm_panel(&ctx, shared, spec)
    }

    pub(super) fn host_refresh_panel(&mut self, id: String) -> Result<(), String> {
        let ctx = self.require_ctx()?;
        ctx.refresh_panel(&id).map_err(|err| err.to_string())
    }

    pub(super) fn host_register_viewer_overlay(
        &mut self,
        spec: bindings::OverlaySpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-viewer-overlay")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        register_wasm_overlay(&ctx, shared, spec)
    }

    pub(super) fn host_refresh_viewer_overlay(&mut self, id: String) -> Result<(), String> {
        let ctx = self.require_ctx()?;
        ctx.refresh_viewer_overlay(&id)
            .map_err(|err| err.to_string())
    }
}
