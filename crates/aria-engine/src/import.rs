//! Register a file in the media pool via native (FFmpeg-free) probing.
//!
//! Probing opens the platform decoder ([`aria_decoder::probe`]) just far
//! enough to read the source's dimensions, frame rate, and duration, then
//! builds a [`MediaSource`] for the project's pool. No frame cache and no
//! FFmpeg: the same native codec that decodes for preview/export reports the
//! metadata.

use std::path::Path;

use aria_decoder::probe;
use aria_models::{MediaSource, Project};
use tracing::debug;

use crate::error::EngineError;

/// Probe a media file and build a [`MediaSource`] describing it.
///
/// `path` should already exist and be canonical; callers resolve it before
/// probing so the pool stores a stable absolute path.
pub fn import_media(path: &Path) -> Result<MediaSource, EngineError> {
    let probed = probe(path)?;
    // Stills carry no timebase of their own: the pool applies the model's
    // convention (millisecond ticks, 5s default placement length).
    let mut media = if probed.is_image {
        MediaSource::image(path, probed.width, probed.height)
    } else {
        MediaSource::new(
            path,
            probed.width,
            probed.height,
            probed.frame_rate,
            probed.frame_count,
            probed.has_audio,
        )
    };
    // Carried rather than re-probed later: opening the file again to answer
    // "what codec is this" would cost an import's worth of work per pool row.
    media.video_codec = probed.video_codec;
    media.audio_codec = probed.audio_codec;

    debug!(
        path = %path.display(),
        width = media.width,
        height = media.height,
        frames = media.duration.value,
        has_audio = media.has_audio,
        video_codec = ?media.video_codec,
        audio_codec = ?media.audio_codec,
        "probed media"
    );

    Ok(media)
}

/// Correct the stored size of every video in `project` that a save from before
/// the probe applied the container's display rotation recorded
/// ([`Project::orient_media`] with the probe imports use). Returns how many
/// entries changed size.
pub fn orient_pool(project: &mut Project) -> usize {
    let changed = project.orient_media(|path| {
        probe(path)
            .ok()
            .filter(|probed| !probed.is_image)
            .map(|probed| (probed.width, probed.height))
    });
    debug!(changed, "oriented the media pool");
    changed
}
