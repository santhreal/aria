use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use aria_plugin_abi::{AriaInstanceResult, AriaResult, AriaStr};

use super::specs::{RawOverlaySpec, RawPanelSpec, parse_panel_location};
use super::{call_ctx, read_str};
use crate::external::NativePluginState;
use crate::{
    OverlayEvent, OverlayScene, OverlaySpec, PanelEvent, PanelSpec, PanelView, PluginContext,
    UiNode, ViewerOverlay,
};

pub(super) extern "C" fn host_register_panel(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_panel`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_panels() {
                return Err(
                    "plugin table does not include panel callbacks (panel_view, panel_event)"
                        .to_owned(),
                );
            }
            let raw_json = read_str(spec_json, "panel spec")?;
            let raw: RawPanelSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid panel spec JSON: {err}"))?;
            let location = parse_panel_location(&raw.location)?;
            let view = NativePanelView {
                panel_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_panel(
                    PanelSpec {
                        id: raw.id,
                        title: raw.title.into(),
                        location,
                    },
                    view,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_refresh_panel(host_ctx: *mut c_void, id: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `id` are valid for the duration of `host_refresh_panel`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let id = read_str(id, "panel id")?;
            ctx.scoped_ctx
                .refresh_panel(id)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_viewer_overlay(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_viewer_overlay`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_overlays() {
                return Err(
                    "plugin table does not include overlay callbacks (overlay_scene, overlay_event)"
                        .to_owned(),
                );
            }
            let raw_json = read_str(spec_json, "viewer overlay spec")?;
            let raw: RawOverlaySpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid viewer overlay spec JSON: {err}"))?;
            let overlay = NativeOverlay {
                overlay_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_viewer_overlay(
                    OverlaySpec {
                        id: raw.id,
                        title: raw.title.into(),
                    },
                    overlay,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_refresh_viewer_overlay(
    host_ctx: *mut c_void,
    id: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `id` are valid for the duration of `host_refresh_viewer_overlay`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let id = read_str(id, "viewer overlay id")?;
            ctx.scoped_ctx
                .refresh_viewer_overlay(id)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_start_job(
    host_ctx: *mut c_void,
    label: AriaStr,
) -> AriaInstanceResult {
    // SAFETY: `host_ctx` and `label` are valid for the duration of `host_start_job`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let label = read_str(label, "job label")?;
            if !ctx.state.table.has_jobs() {
                return Err("plugin table does not export job_step and job_end".to_owned());
            }
            let job = ctx.state.next_job.fetch_add(1, Ordering::Relaxed);
            let (stepping, ending) = (Arc::clone(ctx.state), Arc::clone(ctx.state));
            let (step_ctx, end_ctx) = (ctx.scoped_ctx.clone(), ctx.scoped_ctx.clone());
            ctx.scoped_ctx
                .start_stepped_job(
                    label,
                    move || stepping.call_job_step(&step_ctx, job),
                    move |cancelled| ending.call_job_end(&end_ctx, job, cancelled),
                )
                .map(|_| job)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(job) => AriaInstanceResult::ok(job),
        Err(err) => AriaInstanceResult::err(err),
    }
}

struct NativeOverlay {
    overlay_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl ViewerOverlay for NativeOverlay {
    fn scene(&self, _ctx: &PluginContext) -> OverlayScene {
        self.state
            .call_overlay_scene(&self.scoped_ctx, &self.overlay_id)
    }

    fn event(&self, _ctx: &PluginContext, event: &OverlayEvent) -> Result<(), String> {
        self.state
            .call_overlay_event(&self.scoped_ctx, &self.overlay_id, event)
    }
}

struct NativePanelView {
    panel_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl PanelView for NativePanelView {
    fn view(&self, _ctx: &PluginContext) -> UiNode {
        self.state.call_panel_view(&self.scoped_ctx, &self.panel_id)
    }

    fn event(&self, _ctx: &PluginContext, event: &PanelEvent) -> Result<(), String> {
        self.state
            .call_panel_event(&self.scoped_ctx, &self.panel_id, event)
    }
}
