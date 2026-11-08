//! Whether a clip can hold the frame at a timeline tick.
//!
//! [`Project::freeze_check`] is the single definition of the freeze frame
//! rules a clip and its track must meet: the engine's freeze edit applies it
//! before its first mutation, and the Freeze Frame row reads it to state why
//! the row is disabled.

use std::collections::BTreeMap;

use crate::clip::{ClipSource, CutRule};
use crate::error::ModelError;
use crate::ids::{ClipId, LinkId, TrackId};
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
    /// The freeze would move the clip out of step with a clip linked to it.
    Linked,
    /// The freeze would move another linked clip out of step with its
    /// partner.
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
                "the freeze would move the clip out of step with a clip linked to it; unlink \
                 the group, or unlock its lanes and set their sync lock, before freezing a frame"
            }
            Self::SplitsLink => {
                "the freeze would move a linked clip out of step with its partner; unlink the \
                 group, or unlock its lanes and set their sync lock, before freezing a frame"
            }
        })
    }
}

/// The freeze frame rules of one clip at every tick, read once from the clip,
/// its track, the lanes that ride along with it and the clips linked to them:
/// [`Project::freeze_check`] applies it, and a projection holds it so a
/// surface can ask at any tick without the project.
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
    /// Every link of two or more clips with a member on a lane the freeze
    /// moves, the clip's own first.
    links: Vec<LinkSpans>,
    cut: CutRule,
    /// Each clip on a lane that rides along, which the freeze cuts where it
    /// lies across the tick: its first tick, its end, and its cut rule, in
    /// stack order.
    rider_cuts: Vec<(i64, i64, CutRule)>,
}

/// One link's members: each one's first tick, its end, and whether its lane
/// moves with the freeze.
#[derive(Debug, Clone, PartialEq)]
struct LinkSpans {
    /// The link is the frozen clip's own.
    own: bool,
    members: Vec<(i64, i64, bool)>,
}

impl LinkSpans {
    /// Whether a freeze at `at` moves part of the link out of step with the
    /// rest. The freeze cuts every moving lane at `at` and moves what follows
    /// by the hold. A member that starts at or after `at` on a moving lane
    /// moves whole with the link, while one that starts before `at`, or lies on
    /// a pinned lane, keeps the link where it was. And the part after `at` of a
    /// member on a moving lane moves away from a member on a pinned lane that
    /// plays beside it.
    fn pulled_apart(&self, at: i64) -> bool {
        let members = &self.members;
        let carried = members
            .iter()
            .any(|&(start, _, rides)| rides && start >= at);
        let left = members
            .iter()
            .any(|&(start, _, rides)| !rides || start < at);
        let beside = members.iter().any(|&(start, end, rides)| {
            rides
                && end > at
                && members
                    .iter()
                    .any(|&(other_start, other_end, other_rides)| {
                        !other_rides && start.max(at) < other_end && other_start < end
                    })
        });
        (carried && left) || beside
    }
}

impl FreezeRule {
    /// Whether the clip can hold its frame at timeline tick `at`, checked in
    /// the order the refusals are declared, then, for a tick inside a clip the
    /// freeze cuts, the [`ModelError::Split`] refusals of that cut: the clip's
    /// own, then each riding lane's in stack order, as the edit makes them.
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
        if let Some(link) = reach.links.iter().find(|link| link.pulled_apart(at)) {
            return refuse(if link.own {
                FreezeRefusal::Linked
            } else {
                FreezeRefusal::SplitsLink
            });
        }
        if reach.start < at && at < reach.end {
            reach.cut.check(at - reach.start, reach.end - reach.start)?;
        }
        for (start, end, cut) in &reach.rider_cuts {
            if *start < at && at < *end {
                cut.check(at - start, end - start)?;
            }
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
    /// Reads the clip, its track, the lanes that ride along with a ripple on
    /// it ([`Timeline::rider_lanes`](crate::Timeline::rider_lanes)), and every
    /// link with a member on one of those lanes, since a freeze opens its hold
    /// on all of them.
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
        let moving: Vec<TrackId> = std::iter::once(track_id)
            .chain(timeline.rider_lanes(&[track_id]))
            .collect();
        let mut members: BTreeMap<LinkId, Vec<(i64, i64, bool)>> = BTreeMap::new();
        let mut rider_cuts = Vec::new();
        for lane in timeline.tracks_ordered() {
            let rides = moving.contains(&lane.id);
            for member in lane.clips() {
                let span = (member.timeline.start.value, member.timeline.end()?.value);
                if rides && lane.id != track_id {
                    rider_cuts.push((span.0, span.1, CutRule::of(member, timeline.frame_rate)));
                }
                if let Some(link) = member.link {
                    members
                        .entry(link)
                        .or_default()
                        .push((span.0, span.1, rides));
                }
            }
        }
        let mut links: Vec<LinkSpans> = members
            .into_iter()
            .filter(|(_, members)| members.len() > 1 && members.iter().any(|member| member.2))
            .map(|(link, members)| LinkSpans {
                own: clip.link == Some(link),
                members,
            })
            .collect();
        links.sort_by_key(|link| !link.own);
        Ok(FreezeRule {
            reach: Ok(FreezeReach {
                start,
                end,
                entrance: clip.animation_in.is_some() || clip.animation_combo.is_some(),
                exit: clip.animation_out.is_some() || clip.animation_combo.is_some(),
                links,
                cut: CutRule::of(clip, timeline.frame_rate),
                rider_cuts,
            }),
        })
    }
}

#[cfg(test)]
#[path = "tests_freeze.rs"]
mod tests;
