//! Scopes a native plugin registers through `register_scope`: the host side of
//! [`aria_plugin_abi::AriaPluginV1::scope_measure`].

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::Arc;

use aria_plugin_abi::{ARIA_STATUS_OK, ARIA_STATUS_PANIC, AriaScopeCall, AriaStr};
use serde::Deserialize;

use super::{HostCallContext, NativePluginState};
use crate::context::PluginContext;
use crate::scopes::{Scope, ScopeFrame, ScopeImage, ScopeSpec};

#[derive(Deserialize)]
struct RawSpec {
    id: String,
    name: String,
    width: u32,
    height: u32,
}

/// Register the scope `spec_json` states for the plugin `ctx` is mounting.
pub(super) fn register(ctx: &HostCallContext<'_>, spec_json: &str) -> Result<(), String> {
    let raw: RawSpec =
        serde_json::from_str(spec_json).map_err(|err| format!("invalid scope spec JSON: {err}"))?;
    let scope = NativeScope {
        id: raw.id.clone(),
        state: Arc::clone(ctx.state),
    };
    ctx.scoped_ctx
        .register_scope(
            ScopeSpec {
                id: raw.id,
                name: raw.name,
                width: raw.width,
                height: raw.height,
            },
            scope,
        )
        .map(drop)
        .map_err(|err| err.to_string())
}

struct NativeScope {
    id: String,
    state: Arc<NativePluginState>,
}

impl Scope for NativeScope {
    fn measure(
        &self,
        ctx: &PluginContext,
        frame: &ScopeFrame<'_>,
        image: &mut ScopeImage,
    ) -> Result<(), String> {
        if self.state.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.state.id));
        }
        let mut call_ctx = HostCallContext {
            state: &self.state,
            scoped_ctx: ctx,
            unscoped_ctx: ctx.scoped(None),
            in_mount: false,
        };
        let (image_width, image_height) = (image.width(), image.height());
        let pixels = image.pixels_mut();
        let mut call = AriaScopeCall {
            frame: frame.pixels().as_ptr(),
            frame_len: frame.pixels().len(),
            frame_width: frame.width(),
            frame_height: frame.height(),
            image: pixels.as_mut_ptr(),
            image_len: pixels.len(),
            image_width,
            image_height,
        };
        // SAFETY: `call_ctx`, the id, the frame, and the image outlive
        // `scope_measure`; the image buffer is `image_len` writable bytes the
        // plugin may fill and cannot resize.
        let result = unsafe {
            (self.state.table.scope_measure)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(&self.id),
                &raw mut call,
            )
        };
        let text = self.state.take_guest_string(result.value);
        match result.status {
            ARIA_STATUS_OK => {
                image.set_readout(text);
                Ok(())
            }
            ARIA_STATUS_PANIC => {
                let reason = format!("scope {} panicked: {text}", self.id);
                self.state.mark_faulted(ctx, reason.clone());
                Err(reason)
            }
            _ => Err(text),
        }
    }
}
