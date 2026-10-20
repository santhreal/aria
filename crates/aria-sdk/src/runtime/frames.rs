//! Frame callbacks: scope measurement, CPU effect and generator processing,
//! and driver expression evaluation.

use std::ffi::c_void;

use aria_plugin_abi::{AriaCpuEffectCall, AriaDriverEvalCall, AriaResult, AriaScopeCall, AriaStr};

use crate::{CpuFrameIn, CpuFrameOut, DriverVars, ParamValue, Plugin, ScopeFrame, ScopeImage};

use super::PluginRuntime;

impl<P: Plugin> PluginRuntime<P> {
    /// Measure the frame in `call` with scope `scope_id`, drawing into the
    /// call's image and returning the readout line.
    ///
    /// # Safety
    ///
    /// `call` is null or points to an [`AriaScopeCall`] whose `frame` holds
    /// `frame_len` readable bytes and whose `image` holds `image_len` writable
    /// bytes, none of them aliased, for the duration of the call.
    #[allow(unsafe_code)]
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
}
