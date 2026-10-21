//! Encoder calls into a native plugin: opening an instance, pushing video
//! frames and audio, finishing the stream, and dropping it.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::panic::resume_unwind;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use aria_plugin_abi::{AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame, AriaStr};

use crate::context::PluginContext;

use super::{HostCallContext, NativePluginState};

impl NativePluginState {
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
}
