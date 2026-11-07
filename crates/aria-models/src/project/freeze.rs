//! Whether a clip can hold the frame at a timeline tick.
//!
//! [`Project::freeze_check`] is the single definition of the freeze frame
//! rules a clip and its track must meet: the engine's freeze edit applies it
//! before its first mutation, and the Freeze Frame row reads it to state why
//! the row is disabled.

use std::collections::HashMap;

use crate::clip::{ClipSource, CutRule};
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

/// The freeze frame rules of one clip at every tick, read once from the clip,
/// its track and the clips linked to them: [`Project::freeze_check`] applies
/// it, and a projection holds it so a surface can ask at any tick without the
/// project.
#[derive(Debug, Clone, PartialEq)]
pub struct FreezeRule {
    /// The extent and links of a clip that can hold a frame, or the refusal
    /// that holds at every tick.
    reach: Result<FreezeReach, FreezeRefusal>,
}

#[derive(Debug, Clone, PartialEq)]
struct FreezeReach {
    start: i64,
    end: i64,
    /// An entrance or combo animation plays over the first tick.
    entrance: bool,
    /// An exit or combo animation plays over the last tick.
    exit: bool,
    /// The clip shares its link with another clip.
    linked: bool,
    /// For each linked clip on the track, the ticks at which the freeze
    /// pushes it along and leaves a partner behind: `(after, through]`, or
    /// every tick through `through` while a partner sits on another track.
    splits: Vec<(Option<i64>, i64)>,
    cut: CutRule,
}

impl FreezeRule {
    /// Whether the clip can hold its frame at timeline tick `at`, checked in
    /// the order the refusals are declared, then, for a tick inside the clip,
    /// the [`ModelError::Split`] refusals of the cut the freeze makes there.
    pub fn check(&self, at: i64) -> Result<(), ModelError> {
        let refuse = |why| Err(ModelError::Freeze(why));
        let reach = match &self.reach {
            Ok(reach) => reach,
            Err(why) => return refuse(*why),
        };
        if at < reach.start || at > reach.end {
            return refuse(FreezeRefusal::OffClip);
        }
        if at == reach.start && reach.entrance {
            return refuse(FreezeRefusal::Entrance);
        }
        if at == reach.end && reach.exit {
            return refuse(FreezeRefusal::Exit);
        }
        if reach.linked {
            return refuse(FreezeRefusal::Linked);
        }
        let splits = reach
            .splits
            .iter()
            .any(|&(after, through)| at <= through && after.is_none_or(|after| after < at));
        if splits {
            return refuse(FreezeRefusal::SplitsLink);
        }
        if reach.start < at && at < reach.end {
            reach.cut.check(at - reach.start, reach.end - reach.start)?;
        }
        Ok(())
    }
}

impl Project {
    /// Whether `clip_id` can hold its frame at timeline tick `at`: the
    /// [`FreezeRule`] of the clip, applied at `at`.
    pub fn freeze_check(&self, clip_id: ClipId, at: i64) -> Result<(), ModelError> {
        self.freeze_rule(clip_id)?.check(at)
    }

    /// The freeze frame rules of `clip_id` at every tick.
    ///
    /// Reads the clip, its track, and every clip sharing a link with the clip
    /// or with another clip on its track, since a freeze pushes the later
    /// clips along.
    pub fn freeze_rule(&self, clip_id: ClipId) -> Result<FreezeRule, ModelError> {
        let held = |why| Ok(FreezeRule { reach: Err(why) });
        let timeline = self.timeline();
        let track_id = timeline
            .track_of(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let track = timeline.require_track(track_id)?;
        if track.kind != TrackKind::Video {
            return held(FreezeRefusal::NotOnVideoTrack);
        }
        if track.locked {
            return held(FreezeRefusal::TrackLocked);
        }
        let clip = timeline
            .clip(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let ClipSource::Media { media, .. } = &clip.content else {
            return held(FreezeRefusal::NotMedia);
        };
        if clip.freeze_frame {
            return held(FreezeRefusal::AlreadyFrozen);
        }
        let source = self.media(*media).ok_or(ModelError::UnknownMedia(*media))?;
        if source.kind() != MediaKind::Video {
            return held(FreezeRefusal::NoVideo);
        }
        let start = clip.timeline.start.value;
        let end = clip.timeline.end()?.value;
        // Each link's members: the earliest start, whether one sits off the
        // clip's track, and how many there are.
        let mut links: HashMap<LinkId, (i64, bool, usize)> = HashMap::new();
        for lane in timeline.tracks_ordered() {
            for (member, link) in lane.clips().filter_map(|m| Some((m, m.link?))) {
                let link = links.entry(link).or_insert((i64::MAX, false, 0));
                link.0 = link.0.min(member.timeline.start.value);
                link.1 |= lane.id != track_id;
                link.2 += 1;
            }
        }
        let splits = track
            .clips()
            .filter_map(|shifted| {
                let &(earliest, elsewhere, _) = links.get(&shifted.link?)?;
                Some((
                    (!elsewhere).then_some(earliest),
                    shifted.timeline.start.value,
                ))
            })
            .collect();
        Ok(FreezeRule {
            reach: Ok(FreezeReach {
                start,
                end,
                entrance: clip.animation_in.is_some() || clip.animation_combo.is_some(),
                exit: clip.animation_out.is_some() || clip.animation_combo.is_some(),
                linked: clip
                    .link
                    .and_then(|link| links.get(&link))
                    .is_some_and(|&(_, _, members)| members > 1),
                splits,
                cut: CutRule::of(clip, timeline.frame_rate),
            }),
        })
    }
}

#[cfg(test)]
#[path = "tests_freeze.rs"]
mod tests;
