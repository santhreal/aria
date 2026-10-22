//! Raw RGBA stream media encoder (`example.raw-rgba`).

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use crate::{EncoderAudio, EncoderConfig, EncoderFrame, Instance, NEXT_INSTANCE, fs_write};

pub(crate) struct EncoderState {
    path: String,
    config: EncoderConfig,
    frames: u64,
    rgba_bytes: usize,
    audio_samples: usize,
}

pub(crate) static ENCODER_INSTANCES: Mutex<Option<HashMap<u64, EncoderState>>> = Mutex::new(None);

pub(crate) fn encoder_open(
    encoder_id: String,
    path: String,
    config: EncoderConfig,
) -> Result<Instance, String> {
    if encoder_id != "example.raw-rgba" {
        return Err(format!("unknown encoder: {encoder_id}"));
    }
    if path.ends_with("trap_open.rgba") {
        panic!("explicit encoder-open trap");
    }
    if path.ends_with("err_open.rgba") {
        return Err("encoder refused to open".to_owned());
    }
    let id = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let mut guard = ENCODER_INSTANCES
        .lock()
        .map_err(|_| "encoder instance lock poisoned".to_owned())?;
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(
        id,
        EncoderState {
            path,
            config,
            frames: 0,
            rgba_bytes: 0,
            audio_samples: 0,
        },
    );
    Ok(id)
}

pub(crate) fn encoder_push_frame(instance: Instance, frame: EncoderFrame) -> Result<(), String> {
    let mut guard = ENCODER_INSTANCES
        .lock()
        .map_err(|_| "encoder instance lock poisoned".to_owned())?;
    let Some(state) = guard.as_mut().and_then(|map| map.get_mut(&instance)) else {
        return Err(format!("unknown encoder instance: {instance}"));
    };
    if state.path.ends_with("trap_push.rgba") {
        panic!("explicit encoder-push trap");
    }
    if state.path.ends_with("unpermitted_fs.rgba") {
        fs_write("/etc/hosts", b"forbidden")?;
    }
    state.frames += 1;
    state.rgba_bytes += frame.rgba.len();
    Ok(())
}

pub(crate) fn encoder_push_audio(instance: Instance, audio: EncoderAudio) -> Result<(), String> {
    let mut guard = ENCODER_INSTANCES
        .lock()
        .map_err(|_| "encoder instance lock poisoned".to_owned())?;
    let Some(state) = guard.as_mut().and_then(|map| map.get_mut(&instance)) else {
        return Err(format!("unknown encoder instance: {instance}"));
    };
    if state.path.ends_with("trap_audio.rgba") {
        panic!("explicit encoder-audio trap");
    }
    state.audio_samples += audio.samples.len();
    Ok(())
}

pub(crate) fn encoder_finish(instance: Instance) -> Result<(), String> {
    let mut guard = ENCODER_INSTANCES
        .lock()
        .map_err(|_| "encoder instance lock poisoned".to_owned())?;
    let Some(state) = guard.as_mut().and_then(|map| map.remove(&instance)) else {
        return Err(format!("unknown encoder instance: {instance}"));
    };
    drop(guard);
    if state.path.ends_with("trap_finish.rgba") {
        panic!("explicit encoder-finish trap");
    }
    let summary = format!(
        "RGBA1 {}x{}@{}/{}:frames={}:bytes={}:samples={}\n",
        state.config.width,
        state.config.height,
        state.config.fps_num,
        state.config.fps_den,
        state.frames,
        state.rgba_bytes,
        state.audio_samples
    );
    fs_write(&state.path, summary.as_bytes())
}

pub(crate) fn encoder_drop(instance: Instance) -> Result<(), String> {
    let mut guard = ENCODER_INSTANCES
        .lock()
        .map_err(|_| "encoder instance lock poisoned".to_owned())?;
    if let Some(map) = guard.as_mut() {
        map.remove(&instance);
    }
    Ok(())
}
