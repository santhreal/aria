//! Sound-only export: the same mix, written to a file with no picture in it.
//!
//! [`crate::export`] renders frames and pushes the mix alongside them. A sound
//! intermediate has no frames to render, so it has its own loop — but the same
//! mixer, the same block boundaries, the same transactional publication and the
//! same observer contract, because a sound export is an export.
//!
//! Blocks are still cut on video frame boundaries even though nothing is being
//! composited. That is what keeps a bake sample-exact against the video path:
//! [`sample_boundary`] is the arithmetic both loops use to decide which samples
//! belong to frame `n`, so a clip rendered as sound holds the samples it would
//! have carried inside a picture export of the same span.

use std::path::Path;

use aria_core::{AudioEncoderConfig, RationalTime};
use aria_models::Project;

use crate::error::{EmptyExport, RenderError};
use crate::export::{ExportObserver, ExportSettings, export_frame_count};
use crate::export_audio::{
    EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, ExportAudioMixer, sample_boundary,
};
use crate::transactional_file::TransactionalFile;

/// The sample format a sound export writes, for a caller building a config or a
/// label without reaching into the encoder crate.
pub fn export_audio_config() -> AudioEncoderConfig {
    AudioEncoderConfig::new(EXPORT_AUDIO_RATE, EXPORT_AUDIO_CHANNELS)
}

/// Mix `project` and write it to a sound file at `path`, returning the number of
/// frames the mix covers.
///
/// The frame count rather than a sample count so a job's progress and its
/// completion line read the same as a picture export's, and so a caller that
/// does not know which kind of clip it baked does not have to care.
///
/// Refuses a sequence or a range with no frames in it, the same way
/// [`crate::export_to_file_observed`] does, and refuses a silent mix: with no
/// picture to carry, a silent file holds nothing at all. Publication is
/// transactional, so a cancelled or failed export leaves an existing
/// destination byte-for-byte unchanged.
pub fn export_audio_to_file_observed(
    project: &Project,
    path: &Path,
    settings: ExportSettings,
    observer: ExportObserver<'_>,
) -> Result<u64, RenderError> {
    let frames = export_frame_count(project, settings)?;
    if !settings.audio {
        return Err(RenderError::Unsupported(
            "a sound export with the audio switched off would write an empty file".to_string(),
        ));
    }
    let Some(mut mix) = ExportAudioMixer::for_project(project)? else {
        return Err(RenderError::Empty(EmptyExport::Silence));
    };

    let rate = settings.frame_rate;
    let (start, _) = settings.frame_span(project);
    let audio_rate = aria_core::Rational::new(EXPORT_AUDIO_RATE as i32, 1);
    let offset = sample_boundary(start, rate.num, rate.den);

    let staging = TransactionalFile::new(path)?;
    let mut sink = aria_encoder::open_audio_encoder(staging.staging_path(), export_audio_config())?;
    let mut block = Vec::new();
    for i in 0..frames {
        if !observer(i, frames) {
            return Err(RenderError::Cancelled);
        }
        let from = sample_boundary(start + i as i64, rate.num, rate.den);
        let to = sample_boundary(start + i as i64 + 1, rate.num, rate.den);
        let count = (to - from).max(0) as usize;
        block.resize(count * EXPORT_AUDIO_CHANNELS as usize, 0.0);
        mix.mix_into(from, &mut block)?;
        sink.push_audio(&block, RationalTime::new(from - offset, audio_rate))?;
    }
    sink.finish()?;
    drop(sink);
    staging.publish()?;
    observer(frames, frames);
    Ok(frames)
}
