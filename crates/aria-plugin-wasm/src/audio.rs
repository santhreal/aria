//! Audio effect factory and zero-allocation block processor for WASM plugins.

use std::sync::Arc;

use aria_plugin::{
    AUDIO_HOST_BLOCK, AudioBlock, AudioProcessor, AudioProcessorFactory, ParamSignal, PluginContext,
};

use crate::bindings;
use crate::host::WasmPluginShared;
#[derive(Clone)]
pub(crate) struct WasmAudioFactory {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    effect_id: String,
    param_count: usize,
}

impl WasmAudioFactory {
    pub(crate) fn new(
        shared: Arc<WasmPluginShared>,
        ctx: PluginContext,
        effect_id: String,
        param_count: usize,
    ) -> Self {
        Self {
            shared,
            ctx,
            effect_id,
            param_count,
        }
    }
}

impl AudioProcessorFactory for WasmAudioFactory {
    fn create(&self, sample_rate: u32) -> Box<dyn AudioProcessor> {
        match self
            .shared
            .call_audio_create(&self.ctx, &self.effect_id, sample_rate)
        {
            Ok(instance) => Box::new(WasmAudioProcessor::new(
                Arc::clone(&self.shared),
                self.ctx.clone(),
                self.param_count,
                instance,
            )),
            Err(err) => {
                panic!("the processor faulted: {err}");
            }
        }
    }
}

pub(crate) struct WasmAudioProcessor {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    param_count: usize,
    instance: Option<u64>,
    scratch_params: Vec<bindings::AudioParam>,
    scratch_block: bindings::AudioBlock,
}

impl WasmAudioProcessor {
    fn new(
        shared: Arc<WasmPluginShared>,
        ctx: PluginContext,
        param_count: usize,
        instance: u64,
    ) -> Self {
        let mut scratch_params = Vec::with_capacity(param_count);
        for _ in 0..param_count {
            scratch_params.push(bindings::AudioParam {
                constant: 0.0,
                per_frame: Vec::with_capacity(AUDIO_HOST_BLOCK),
            });
        }
        let scratch_block = bindings::AudioBlock {
            left: Vec::with_capacity(AUDIO_HOST_BLOCK),
            right: Vec::with_capacity(AUDIO_HOST_BLOCK),
            position: 0,
            sample_rate: 48_000,
            sidechain_left: Vec::with_capacity(AUDIO_HOST_BLOCK),
            sidechain_right: Vec::with_capacity(AUDIO_HOST_BLOCK),
        };
        Self {
            shared,
            ctx,
            param_count,
            instance: Some(instance),
            scratch_params,
            scratch_block,
        }
    }
}

impl AudioProcessor for WasmAudioProcessor {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let Some(instance) = self.instance else {
            return;
        };

        let sample_rate = block.sample_rate().max(1);
        let position = block.position();

        for slot in 0..self.param_count {
            let entry = &mut self.scratch_params[slot];
            entry.per_frame.clear();
            match block.param(slot) {
                Some(ParamSignal::Constant(v)) => {
                    entry.constant = v;
                }
                Some(ParamSignal::PerFrame(values)) => {
                    entry.constant = values.first().copied().unwrap_or(0.0);
                    entry.per_frame.extend_from_slice(values);
                }
                None => {
                    entry.constant = 0.0;
                }
            }
        }

        self.scratch_block.sidechain_left.clear();
        self.scratch_block.sidechain_right.clear();
        if let Some((sc_l, sc_r)) = block.sidechain() {
            self.scratch_block.sidechain_left.extend_from_slice(sc_l);
            self.scratch_block.sidechain_right.extend_from_slice(sc_r);
        }

        {
            let (left, right) = block.channels();
            self.scratch_block.left.clear();
            self.scratch_block.left.extend_from_slice(left);
            self.scratch_block.right.clear();
            self.scratch_block.right.extend_from_slice(right);
        }
        self.scratch_block.position = position;
        self.scratch_block.sample_rate = sample_rate;

        match self.shared.call_audio_process(
            &self.ctx,
            instance,
            &self.scratch_params,
            &self.scratch_block,
        ) {
            Ok(stereo) => {
                let (left, right) = block.channels();
                let copy_l = left.len().min(stereo.left.len());
                let copy_r = right.len().min(stereo.right.len());
                left[..copy_l].copy_from_slice(&stereo.left[..copy_l]);
                right[..copy_r].copy_from_slice(&stereo.right[..copy_r]);
            }
            Err(err) => {
                self.instance = None;
                panic!("the processor faulted: {err}");
            }
        }
    }

    fn reset(&mut self) {
        if let Some(instance) = self.instance
            && let Err(err) = self.shared.call_audio_reset(&self.ctx, instance)
        {
            self.instance = None;
            panic!("the processor faulted: {err}");
        }
    }
}

impl Drop for WasmAudioProcessor {
    fn drop(&mut self) {
        if let Some(instance) = self.instance.take() {
            let _ = self.shared.call_audio_drop(&self.ctx, instance);
        }
    }
}
