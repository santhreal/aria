//! Audio processor calls into a native plugin: creating an instance, processing
//! a block, resetting it, and dropping it.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use aria_plugin_abi::{AriaAudioBlock, AriaAudioParam, AriaStr};

use crate::context::PluginContext;

use super::{HostCallContext, NativePluginState};

impl NativePluginState {
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
}
