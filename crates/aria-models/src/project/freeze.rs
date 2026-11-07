//! Whether a clip can hold the frame at a timeline tick.
//!
//! [`Project::freeze_check`] is the single definition of the freeze frame
//! rules a clip and its track must meet: the engine's freeze edit applies it
//! before its first mutation, and the Freeze Frame row reads it to state why
//! the row is disabled.

use crate::clip::{ClipSource, split_speed_curve, validate_split_render_continuity};
use crate::error::ModelError;
use crate::ids::{ClipId, LinkId};
use crate::media::MediaKind;
use crate::track::TrackKind;

use super::Project;

/// Why a clip cannot hold the frame at a tick, read from the clip, its track
/// and the clips linked to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeRefusal {
    /// The clip is not on a video track.
    NotOnVideoTrack,
    /// The clip's track is locked.
    TrackLocked,
    /// The clip is generated, so it has no media frame to hold.
    NotMedia,
    /// The clip is already a freeze frame.
    AlreadyFrozen,
    /// The clip's media has no video frames.
    NoVideo,
    /// The tick lies outside the clip.
    OffClip,
    /// The tick is the clip's first, which an entrance animation plays over.
    Entrance,
    /// The tick is the clip's last, which an exit animation plays over.
    Exit,
    /// The clip shares its link with another clip.
    Linked,
    /// A linked clip after the tick has a partner the freeze would leave behind.
    SplitsLink,
}

impl FreezeRefusal {
    /// Every refusal, in declaration order.
    pub const ALL: [FreezeRefusal; 10] = [
        Self::NotOnVideoTrack,
        Self::TrackLocked,
        Self::NotMedia,
        Self::AlreadyFrozen,
        Self::NoVideo,
        Self::OffClip,
        Self::Entrance,
        Self::Exit,
        Self::Linked,
        Self::SplitsLink,
    ];
}

impl std::fmt::Display for FreezeRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotOnVideoTrack => "freeze frame requires a clip on a video track",
            Self::TrackLocked => "the clip's video track is locked",
            Self::NotMedia => "freeze frame requires a media-backed clip",
            Self::AlreadyFrozen => "the clip is already a freeze frame",
            Self::NoVideo => "the clip does not reference video media",
            Self::OffClip => "the freeze point lies outside the clip",
            Self::Entrance => {
                "cannot freeze at a clip start with an active entrance/combo animation"
            }
            Self::Exit => "cannot freeze at a clip end with an active exit/combo animation",
            Self::Linked => {
                "the clip is linked to another clip; unlink the group before freezing a frame"
            }
            Self::SplitsLink => {
                "freezing would desynchronize a linked clip after the freeze point; unlink the \
                 group first"
            }
        })
    }
}

impl Project {
    /// Whether `clip_id` can hold its frame at timeline tick `at`, checked in
    /// the order the refusals are declared, then, for a tick inside the clip,
    /// the [`ModelError::Split`] refusals of the cut the freeze makes there.
    ///
    /// Reads the clip, its track, and every clip sharing a link with the clip
    /// or with a clip at or after `at` on its track, since the freeze pushes
    /// those later clips along.
    pub fn freeze_check(&self, clip_id: ClipId, at: i64) -> Result<(), ModelError> {
        let refuse = |why| Err(ModelError::Freeze(why));
        let timeline = self.timeline();
        let track_id = timeline
            .track_of(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let track = timeline.require_track(track_id)?;
        if track.kind != TrackKind::Video {
            return refuse(FreezeRefusal::NotOnVideoTrack);
        }
        if track.locked {
            return refuse(FreezeRefusal::TrackLocked);
        }
        let clip = timeline
            .clip(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let ClipSource::Media { media, .. } = &clip.content else {
            return refuse(FreezeRefusal::NotMedia);
        };
        if clip.freeze_frame {
            return refuse(FreezeRefusal::AlreadyFrozen);
        }
        let source = self.media(*media).ok_or(ModelError::UnknownMedia(*media))?;
        if source.kind() != MediaKind::Video {
            return refuse(FreezeRefusal::NoVideo);
        }
        let start = clip.timeline.start.value;
        let end = clip.timeline.end()?.value;
        if at < start || at > end {
            return refuse(FreezeRefusal::OffClip);
        }
        if at == start && (clip.animation_in.is_some() || clip.animation_combo.is_some()) {
            return refuse(FreezeRefusal::Entrance);
        }
        if at == end && (clip.animation_out.is_some() || clip.animation_combo.is_some()) {
            return refuse(FreezeRefusal::Exit);
        }
        let members = |link: LinkId| {
            timeline
                .tracks_ordered()
                .flat_map(|lane| lane.clips())
                .filter(move |member| member.link == Some(link))
        };
        if clip
            .link
            .is_some_and(|link| members(link).any(|member| member.id != clip_id))
        {
            return refuse(FreezeRefusal::Linked);
        }
        let splits = track
            .clips()
            .filter(|shifted| shifted.timeline.start.value >= at)
            .filter_map(|shifted| shifted.link)
            .any(|link| {
                members(link).any(|member| {
                    timeline.track_of(member.id) != Some(track_id)
                        || member.timeline.start.value < at
                })
            });
        if splits {
            return refuse(FreezeRefusal::SplitsLink);
        }
        if start < at && at < end {
            let rate = timeline.frame_rate;
            validate_split_render_continuity(clip, at - start, end - at, rate)?;
            let fraction = (at - start) as f64 / clip.timeline.duration.value as f64;
            split_speed_curve(&clip.speed_curve, fraction)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests_freeze.rs"]
mod tests;
