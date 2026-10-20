//! Render and replace, at the project level: which file stands in for a clip,
//! and putting the clip back.
//!
//! The placement rules live here and the mapping lives on the clip
//! ([`Clip::baked`], [`Clip::unbaked`]). What this file adds is what only a
//! project knows: that the rendered file is in the pool, that it runs at the
//! sequence rate, and that it is long enough to cover the clip it stands in
//! for. All three are checked before the swap, because a bake that half-applied
//! would leave a clip decoding a file that cannot answer for its own span.

use crate::clip::Clip;
use crate::error::ModelError;
use crate::ids::{ClipId, MediaId};
use crate::time::TimeRange;
use crate::timeline::Timeline;
use crate::track::TrackKind;
use crate::track_catalog::BakeMedium;

use super::Project;

impl Project {
    /// What a render of `clip` would write, or why one cannot stand in for it.
    ///
    /// Two questions with one answer, because they are always asked together:
    /// the lane decides which medium an intermediate is, and the clip decides
    /// whether a render is worth making at all. The lane comes first — what a
    /// clip sits on decides whether a file can stand in for it, before what is
    /// on the clip decides whether one is worth having.
    ///
    /// Both callers read it here: the pass that renders the clip, and the
    /// assistant tool that asks for a render before one is started. A tool that
    /// checked a shorter list would accept calls the render then refused,
    /// minutes later, with a job bar on screen.
    pub fn bake_medium(&self, clip_id: ClipId) -> Result<BakeMedium, ModelError> {
        let kind = self
            .lane_kind(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let medium = BakeMedium::for_lane(kind)?;
        self.require_clip(clip_id)?.bakeable()?;
        Ok(medium)
    }

    /// The kind of lane holding `clip`, wherever in the project it lives.
    ///
    /// Mirrors [`Project::clip`]'s search rather than reading the open
    /// timeline: a clip inside a nest is still a clip an id names, and a render
    /// of it is still a render of that lane's medium.
    fn lane_kind(&self, clip_id: ClipId) -> Option<TrackKind> {
        let find = |timeline: &Timeline| {
            timeline
                .track_of(clip_id)
                .and_then(|track| timeline.track(track))
                .map(|track| track.kind)
        };
        find(self.timeline())
            .or_else(|| find(self.root_timeline()))
            .or_else(|| {
                self.sequences()
                    .find_map(|sequence| find(&sequence.timeline))
            })
    }

    /// Replace `clip_id`'s content and look with a decode of `media`, keeping
    /// what was replaced so [`Self::restore_baked_clip`] can put it back.
    ///
    /// `media` must be a pool item that covers the clip's whole extent from its
    /// first frame: the intermediate is that span rendered one-to-one, which is
    /// the invariant the restore reads back through.
    ///
    /// What "one-to-one" counts in is the file's own tick rate. A picture
    /// intermediate is written at the sequence rate, so a file at another rate
    /// is refused rather than resampled. A sound intermediate has no frames at
    /// all and counts audio ticks, so its length is compared as time.
    pub fn bake_clip(&mut self, clip_id: ClipId, media: MediaId) -> Result<(), ModelError> {
        let rate = self.timeline().frame_rate;
        let rendered = self.media(media).ok_or(ModelError::UnknownMedia(media))?;
        if !rendered.is_audio_only() && rendered.frame_rate != rate {
            return Err(ModelError::RateMismatch {
                expected: rate,
                got: rendered.frame_rate,
            });
        }
        let media_rate = rendered.frame_rate;
        let available = rendered.duration.value;
        let clip = self.timeline().require_clip(clip_id)?;
        let span = crate::time::resample(clip.timeline.duration, media_rate).value;
        if available < span {
            return Err(ModelError::InvalidParam(format!(
                "the rendered file covers {available} of the clip's {span} ticks"
            )));
        }
        let baked = clip.baked(media, TimeRange::at_rate(0, span, media_rate))?;
        self.replace_clip(clip_id, baked)
    }

    /// Put back what a bake replaced, over whatever span the baked clip now
    /// covers.
    pub fn restore_baked_clip(&mut self, clip_id: ClipId) -> Result<(), ModelError> {
        let restored = self.timeline().require_clip(clip_id)?.unbaked()?;
        self.replace_clip(clip_id, restored)
    }

    /// Swap one clip for a rewritten copy of itself, in place.
    fn replace_clip(&mut self, clip_id: ClipId, next: Clip) -> Result<(), ModelError> {
        let slot = self.timeline_mut().require_clip_mut(clip_id)?;
        *slot = next;
        Ok(())
    }
}
