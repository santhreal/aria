//! Panel and viewer overlay calls into a native plugin: building a view,
//! delivering its events, and the placeholder drawn when a view is refused.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::panic::resume_unwind;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use aria_plugin_abi::{ARIA_BYTES_FOUND, ARIA_BYTES_PANIC, AriaStr};

use crate::context::PluginContext;

use super::{
    HostCallContext, MAX_OVERLAY_SCENE_JSON_BYTES, MAX_PANEL_VIEW_JSON_BYTES, NativePluginState,
};

impl NativePluginState {
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

fn refused_tree(reason: impl std::fmt::Display) -> crate::UiNode {
    crate::UiNode::Button {
        id: format!("\0{reason}").into(),
        label: Arc::from(""),
        disabled: true,
    }
}

fn refused_scene(reason: impl std::fmt::Display) -> crate::OverlayScene {
    let id = crate::host_wire::sanitize_fault_widget_id(
        &reason.to_string(),
        "malformed overlay scene JSON",
    );
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
