//! Guest media decoder entrypoints dispatching to [`crate::y4m`].

use crate::{DecodeError, DecodedFrame, Instance, MediaProbe, OpenedVideo, y4m};

pub(crate) fn decoder_probe(decoder_id: String, path: String) -> Result<MediaProbe, DecodeError> {
    match decoder_id.as_str() {
        y4m::DECODER_ID => y4m::probe(&path),
        other => Err(DecodeError::Unsupported(format!(
            "unknown decoder: {other}"
        ))),
    }
}

pub(crate) fn decoder_open_video(
    decoder_id: String,
    path: String,
) -> Result<OpenedVideo, DecodeError> {
    match decoder_id.as_str() {
        y4m::DECODER_ID => y4m::open(&path),
        other => Err(DecodeError::Unsupported(format!(
            "unknown decoder: {other}"
        ))),
    }
}

pub(crate) fn decoder_seek(instance: Instance, pts: i64) -> Result<(), String> {
    y4m::seek(instance, pts)
}

pub(crate) fn decoder_next_frame(instance: Instance) -> Result<Option<DecodedFrame>, String> {
    y4m::next_frame(instance)
}

pub(crate) fn decoder_open_audio(
    _decoder_id: String,
    path: String,
    _sample_rate: u32,
    _channels: u16,
) -> Result<Instance, DecodeError> {
    Err(DecodeError::Unsupported(format!("{path} has no audio")))
}

pub(crate) fn decoder_read_audio(
    instance: Instance,
    _max_samples: u32,
) -> Result<Vec<f32>, String> {
    Err(format!("unknown audio stream: {instance}"))
}

pub(crate) fn decoder_seek_audio(instance: Instance, _frame: i64) -> Result<(), String> {
    Err(format!("unknown audio stream: {instance}"))
}

pub(crate) fn decoder_close(instance: Instance) {
    y4m::close(instance);
}
