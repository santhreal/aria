//! Audio effect and analyzer callbacks: creating and running processor
//! instances, and opening, feeding, and finishing analyzer runs.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;

use aria_plugin_abi::{AriaAudioBlock, AriaAudioParam, AriaInstanceResult, AriaResult, AriaStr};

use crate::{AudioBlock, AudioParam, Plugin};

use super::{PluginRuntime, panic_message};

fn byte_ranges_overlap(a_addr: usize, a_bytes: usize, b_addr: usize, b_bytes: usize) -> bool {
    if a_bytes == 0 || b_bytes == 0 {
        return false;
    }
    let a_end = a_addr.saturating_add(a_bytes);
    let b_end = b_addr.saturating_add(b_bytes);
    a_addr < b_end && b_addr < a_end
}

impl<P: Plugin> PluginRuntime<P> {
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
            } else if samples_ptr.is_null()
                || !samples_ptr.is_aligned()
                || samples_len
                    .checked_mul(std::mem::size_of::<f32>())
                    .is_none()
            {
                return Err("analyzer_push received null or misaligned samples".to_owned());
            } else {
                // SAFETY: `samples_ptr` is non-null, aligned, and covers `samples_len` valid `f32` elements.
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

    /// Create an instance of audio effect `effect_id` at `sample_rate` through
    /// [`Plugin::audio_create`] and return its instance id.
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
            if block_ptr.is_null() || !block_ptr.is_aligned() {
                return Err("audio_process received null or misaligned block".to_owned());
            }
            // SAFETY: `block_ptr` is non-null, aligned, and valid for `audio_process`.
            let raw_block = unsafe { &mut *block_ptr };
            let frames = raw_block.frames;
            let channel_bytes = frames
                .checked_mul(std::mem::size_of::<f32>())
                .ok_or_else(|| "audio_process frame count overflows".to_owned())?;
            if frames > 0 {
                if raw_block.left.is_null()
                    || raw_block.right.is_null()
                    || !raw_block.left.is_aligned()
                    || !raw_block.right.is_aligned()
                {
                    return Err(
                        "audio_process received null or misaligned channel buffer".to_owned()
                    );
                }
                if byte_ranges_overlap(
                    raw_block.left as usize,
                    channel_bytes,
                    raw_block.right as usize,
                    channel_bytes,
                ) {
                    return Err(
                        "audio_process received overlapping left and right channels".to_owned()
                    );
                }
            }
            let sidechain = if raw_block.has_sidechain != 0
                && !raw_block.sidechain_left.is_null()
                && !raw_block.sidechain_right.is_null()
            {
                if frames > 0 {
                    if !raw_block.sidechain_left.is_aligned()
                        || !raw_block.sidechain_right.is_aligned()
                    {
                        return Err("audio_process received misaligned sidechain buffer".to_owned());
                    }
                    let sc_overlaps = byte_ranges_overlap(
                        raw_block.sidechain_left as usize,
                        channel_bytes,
                        raw_block.left as usize,
                        channel_bytes,
                    ) || byte_ranges_overlap(
                        raw_block.sidechain_left as usize,
                        channel_bytes,
                        raw_block.right as usize,
                        channel_bytes,
                    ) || byte_ranges_overlap(
                        raw_block.sidechain_right as usize,
                        channel_bytes,
                        raw_block.left as usize,
                        channel_bytes,
                    ) || byte_ranges_overlap(
                        raw_block.sidechain_right as usize,
                        channel_bytes,
                        raw_block.right as usize,
                        channel_bytes,
                    );
                    if sc_overlaps {
                        return Err("audio_process sidechain overlaps channel output".to_owned());
                    }
                }
                // SAFETY: Sidechain channels are non-null, aligned, and non-overlapping with outputs.
                let sc_l = if frames == 0 {
                    &[][..]
                } else {
                    unsafe { std::slice::from_raw_parts(raw_block.sidechain_left, frames) }
                };
                // SAFETY: Sidechain right channel is non-null, aligned, and non-overlapping with outputs.
                let sc_r = if frames == 0 {
                    &[][..]
                } else {
                    unsafe { std::slice::from_raw_parts(raw_block.sidechain_right, frames) }
                };
                Some((sc_l, sc_r))
            } else {
                None
            };

            let raw_params = if params_len == 0 {
                &[][..]
            } else if params_ptr.is_null()
                || !params_ptr.is_aligned()
                || params_len
                    .checked_mul(std::mem::size_of::<AriaAudioParam>())
                    .is_none()
            {
                return Err("audio_process received null or misaligned params".to_owned());
            } else {
                // SAFETY: `params_ptr` is non-null, aligned, and covers `params_len` elements.
                unsafe { std::slice::from_raw_parts(params_ptr, params_len) }
            };

            let decode_per_frame = |raw: &AriaAudioParam| -> Result<Option<&[f32]>, String> {
                if raw.per_frame_len == 0 {
                    return Ok(None);
                }
                let pf_bytes = raw
                    .per_frame_len
                    .checked_mul(std::mem::size_of::<f32>())
                    .ok_or_else(|| "audio_process per_frame_len overflows".to_owned())?;
                if raw.per_frame.is_null() || !raw.per_frame.is_aligned() {
                    return Err(
                        "audio_process received null or misaligned per_frame buffer".to_owned()
                    );
                }
                if frames > 0
                    && (byte_ranges_overlap(
                        raw.per_frame as usize,
                        pf_bytes,
                        raw_block.left as usize,
                        channel_bytes,
                    ) || byte_ranges_overlap(
                        raw.per_frame as usize,
                        pf_bytes,
                        raw_block.right as usize,
                        channel_bytes,
                    ))
                {
                    return Err("audio_process per_frame buffer overlaps channel output".to_owned());
                }
                // SAFETY: `per_frame` is non-null, aligned, and non-overlapping with mutable outputs.
                Ok(Some(unsafe {
                    std::slice::from_raw_parts(raw.per_frame, raw.per_frame_len)
                }))
            };

            let mut stack_params = [AudioParam {
                constant: 0.0,
                per_frame: None,
            }; 32];
            let mut heap_params = Vec::new();
            let params: &[AudioParam<'_>] = if raw_params.len() <= stack_params.len() {
                for (slot, raw) in stack_params.iter_mut().zip(raw_params.iter()) {
                    *slot = AudioParam {
                        constant: raw.constant,
                        per_frame: decode_per_frame(raw)?,
                    };
                }
                &stack_params[..raw_params.len()]
            } else {
                heap_params.reserve(raw_params.len());
                for raw in raw_params {
                    heap_params.push(AudioParam {
                        constant: raw.constant,
                        per_frame: decode_per_frame(raw)?,
                    });
                }
                &heap_params
            };

            // SAFETY: `left` and `right` are validated non-null, aligned, and non-overlapping above.
            let left = if frames == 0 {
                &mut [][..]
            } else {
                unsafe { std::slice::from_raw_parts_mut(raw_block.left, frames) }
            };
            // SAFETY: `right` is validated non-null, aligned, and non-overlapping above.
            let right = if frames == 0 {
                &mut [][..]
            } else {
                unsafe { std::slice::from_raw_parts_mut(raw_block.right, frames) }
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

    /// Reset audio instance `instance`.
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

    /// Drop audio instance `instance`.
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
}
