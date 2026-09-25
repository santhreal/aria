//! One pool source rendered on its own, with no timeline behind it.
//!
//! Pool previews use the session renderer's direct single-source entry. The
//! real project and [`MediaId`] therefore feed the same decoder, still, proxy,
//! colour, and orientation caches as timeline playback without allocating a
//! scratch timeline.

use aria_models::{MediaId, ModelError, RationalTime};
use aria_render::RgbaImage;

use super::Engine;
use crate::error::EngineError;

impl Engine {
    /// Render pool source `media` at `source_tick` (a tick in the media's own
    /// frame rate), scaled to fit within `max_width`×`max_height`.
    ///
    /// The tick is clamped into the source: a scrub past the end shows the last
    /// frame instead of failing. A still ignores it. An audio-only source has
    /// no picture and is refused.
    pub fn media_frame_fit(
        &mut self,
        media: MediaId,
        source_tick: i64,
        max_width: u32,
        max_height: u32,
    ) -> Result<RgbaImage, EngineError> {
        let source = self
            .project
            .media(media)
            .ok_or(ModelError::UnknownMedia(media))?;
        if source.is_audio_only() {
            return Err(EngineError::Refused(
                "an audio-only source has no frame to show".into(),
            ));
        }
        let rate = source.frame_rate;
        let tick = if source.is_image {
            0
        } else {
            source_tick.clamp(0, (source.duration.value - 1).max(0))
        };

        Ok(self.renderer.render_media_frame_fit(
            &self.project,
            media,
            RationalTime::new(tick, rate),
            max_width,
            max_height,
        )?)
    }
}
