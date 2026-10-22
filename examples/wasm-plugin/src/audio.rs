//! Stereo gain audio effect and clip peak audio analyzer.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use serde_json::json;

use crate::{AudioBlock, AudioParam, Instance, NEXT_INSTANCE, StereoBuffers};

pub(crate) struct GainInstance {
    sample_rate: u32,
    frames_processed: u64,
}

pub(crate) static AUDIO_INSTANCES: Mutex<Option<HashMap<u64, GainInstance>>> = Mutex::new(None);

pub(crate) struct AnalyzerInstance {
    channels: u16,
    frames: i64,
    peaks: Vec<i64>,
}

pub(crate) static ANALYZERS: Mutex<Option<HashMap<u64, AnalyzerInstance>>> = Mutex::new(None);

pub(crate) fn audio_create(effect_id: String, sample_rate: u32) -> Result<Instance, String> {
    if effect_id != "example.gain" {
        return Err(format!("unknown audio effect: {effect_id}"));
    }
    let id = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let mut guard = AUDIO_INSTANCES
        .lock()
        .map_err(|_| "audio instance lock poisoned".to_owned())?;
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(
        id,
        GainInstance {
            sample_rate,
            frames_processed: 0,
        },
    );
    Ok(id)
}

pub(crate) fn audio_process(
    instance: Instance,
    params: Vec<AudioParam>,
    block: AudioBlock,
) -> Result<StereoBuffers, String> {
    let mut guard = AUDIO_INSTANCES
        .lock()
        .map_err(|_| "audio instance lock poisoned".to_owned())?;
    let Some(state) = guard.as_mut().and_then(|map| map.get_mut(&instance)) else {
        return Err(format!("unknown audio instance: {instance}"));
    };
    let _ = state.sample_rate;

    let frames = block.left.len().min(block.right.len());
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);

    let base_gain = params.first().map_or(1.0, |p| p.constant);
    if base_gain <= -2.0 {
        panic!("the processor faulted");
    }
    if base_gain < 0.0 {
        let mut acc = 1u64;
        for _ in 0..25_000_000u64 {
            acc = acc.wrapping_mul(6364136223846793005).wrapping_add(1);
            std::hint::black_box(acc);
        }
    }

    for i in 0..frames {
        let g = match params.first() {
            Some(param) => param.per_frame.get(i).copied().unwrap_or(param.constant),
            None => 1.0,
        };
        left.push(block.left[i] * g);
        right.push(block.right[i] * g);
    }
    state.frames_processed += frames as u64;

    Ok(StereoBuffers { left, right })
}

pub(crate) fn audio_reset(instance: Instance) -> Result<(), String> {
    let mut guard = AUDIO_INSTANCES
        .lock()
        .map_err(|_| "audio instance lock poisoned".to_owned())?;
    let Some(state) = guard.as_mut().and_then(|map| map.get_mut(&instance)) else {
        return Err(format!("unknown audio instance: {instance}"));
    };
    state.frames_processed = 0;
    Ok(())
}

pub(crate) fn audio_drop(instance: Instance) -> Result<(), String> {
    let mut guard = AUDIO_INSTANCES
        .lock()
        .map_err(|_| "audio instance lock poisoned".to_owned())?;
    if let Some(map) = guard.as_mut() {
        map.remove(&instance);
    }
    Ok(())
}

pub(crate) fn analyzer_open(
    id: String,
    _sample_rate: u32,
    channels: u16,
) -> Result<Instance, String> {
    if id != "example.wasm-peaks" {
        return Err(format!("unknown analyzer {id}"));
    }
    let instance = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    ANALYZERS
        .lock()
        .map_err(|_| "analyzer lock poisoned".to_owned())?
        .get_or_insert_with(HashMap::new)
        .insert(
            instance,
            AnalyzerInstance {
                channels,
                frames: 0,
                peaks: Vec::new(),
            },
        );
    Ok(instance)
}

pub(crate) fn analyzer_push(instance: Instance, samples: Vec<f32>) -> Result<(), String> {
    let mut guard = ANALYZERS
        .lock()
        .map_err(|_| "analyzer lock poisoned".to_owned())?;
    let state = guard
        .as_mut()
        .and_then(|m| m.get_mut(&instance))
        .ok_or_else(|| format!("unknown analyzer instance {instance}"))?;
    let ch = usize::from(state.channels.max(1));
    for frame in samples.chunks_exact(ch) {
        if frame[0].is_nan() {
            panic!("explicit analyzer trap");
        }
        if frame[0].abs() >= 0.5 {
            state.peaks.push(state.frames);
        }
        state.frames += 1;
    }
    Ok(())
}

pub(crate) fn analyzer_finish(instance: Instance) -> Result<String, String> {
    let mut guard = ANALYZERS
        .lock()
        .map_err(|_| "analyzer lock poisoned".to_owned())?;
    let state = guard
        .as_mut()
        .and_then(|m| m.remove(&instance))
        .ok_or_else(|| format!("unknown analyzer instance {instance}"))?;
    Ok(json!({
        "beats": state.peaks,
        "markers": [{
            "frame": state.frames,
            "duration_frames": 0,
            "name": "WasmEnd",
            "color": "teal"
        }]
    })
    .to_string())
}

pub(crate) fn analyzer_drop(instance: Instance) {
    if let Ok(mut guard) = ANALYZERS.lock()
        && let Some(map) = guard.as_mut()
    {
        map.remove(&instance);
    }
}
