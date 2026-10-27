//! How far a trim takes each edge of a clip before the model refuses it.
//!
//! A drag on a clip edge stops at the end of the take, at the clip next to
//! it, and at the sequence start. The reach is read off
//! [`Project::trim_clip`]'s own check, so a drag held inside it is never
//! refused at release.

use crate::error::ModelError;
use crate::ids::ClipId;
use crate::time::TimeRange;

use super::Project;

/// How far a search for a bound goes before it reads the edge as unbounded,
/// in timeline ticks. Small enough that a retimed clip's source arithmetic
/// cannot overflow at this distance.
const FAR: i64 = 1 << 30;

/// How far [`Project::trim_clip`] takes each edge of a clip with the other
/// edge held, in timeline ticks. `None` is an edge nothing bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimReach {
    /// The earliest and the latest start the head trims to.
    pub start: (Option<i64>, i64),
    /// The earliest and the latest end the tail trims to.
    pub end: (i64, Option<i64>),
}

impl TrimReach {
    /// `start` held inside the head's reach.
    pub fn head(&self, start: i64) -> i64 {
        let start = start.min(self.start.1);
        self.start.0.map_or(start, |earliest| start.max(earliest))
    }

    /// `end` held inside the tail's reach.
    pub fn tail(&self, end: i64) -> i64 {
        let end = end.max(self.end.0);
        self.end.1.map_or(end, |latest| end.min(latest))
    }
}

impl Project {
    /// How far each edge of `clip_id` trims with the other edge held.
    ///
    /// With `lane` false neither the clip's neighbors nor the sequence start
    /// bound it, only its source: the reach of a trim that moves the rest of
    /// the lane out of its way. With `linked` the reach is that of the linked
    /// trim, which gives every clip linked to `clip_id` the same edge deltas,
    /// so each of them bounds it.
    pub fn trim_reach(
        &self,
        clip_id: ClipId,
        lane: bool,
        linked: bool,
    ) -> Result<TrimReach, ModelError> {
        let clip = self.timeline().require_clip(clip_id)?;
        let (start, end) = (clip.timeline.start.value, clip.timeline.end_tick());
        let members = match clip.link {
            Some(link) if linked => self.linked_clips(link),
            Some(_) | None => vec![clip_id],
        };
        // Each member's reach as deltas of its own edges, intersected.
        let (mut head, mut shortest_head) = (None::<i64>, i64::MAX);
        let (mut shortest_tail, mut tail) = (i64::MIN, None::<i64>);
        for member in members {
            let other = self.timeline().require_clip(member)?;
            let (from, to) = (other.timeline.start.value, other.timeline.end_tick());
            let (earliest, latest) = self.own_reach(member, lane)?;
            if let Some(earliest) = earliest {
                let delta = earliest - from;
                head = Some(head.map_or(delta, |bound| bound.max(delta)));
            }
            shortest_head = shortest_head.min(to - 1 - from);
            shortest_tail = shortest_tail.max(from + 1 - to);
            if let Some(latest) = latest {
                let delta = latest - to;
                tail = Some(tail.map_or(delta, |bound| bound.min(delta)));
            }
        }
        Ok(TrimReach {
            start: (head.map(|delta| start + delta), start + shortest_head),
            end: (end + shortest_tail, tail.map(|delta| end + delta)),
        })
    }

    /// The earliest start and the latest end `clip_id` alone trims to, the
    /// other edge held. `None` is an edge nothing bounds.
    fn own_reach(
        &self,
        clip_id: ClipId,
        lane: bool,
    ) -> Result<(Option<i64>, Option<i64>), ModelError> {
        let clip = self.timeline().require_clip(clip_id)?;
        let rate = clip.timeline.start.rate;
        let (start, end) = (clip.timeline.start.value, clip.timeline.end_tick());
        let fits = |head: i64, tail: i64| {
            self.plan_trim(clip_id, TimeRange::at_rate(head, tail - head, rate), lane)
                .is_ok()
        };
        // The sequence start bounds the head through the trim check itself.
        let earliest = outward(start, -1, |head| fits(head, end));
        let wall = if lane {
            let track = self
                .timeline()
                .track_of(clip_id)
                .and_then(|track| self.timeline().track(track))
                .ok_or(ModelError::UnknownClip(clip_id))?;
            track
                .clips()
                .filter(|other| other.id != clip_id)
                .map(|other| other.timeline.start.value)
                .filter(|&from| from >= end)
                .min()
        } else {
            None
        };
        let latest = match wall {
            Some(wall) if fits(start, wall) => Some(wall),
            Some(wall) => Some(edge(wall, end, |tail| fits(start, tail))),
            None => outward(end, 1, |tail| fits(start, tail)),
        };
        Ok((earliest, latest))
    }
}

/// The value nearest `bad` that fits, between `good`, which fits, and `bad`,
/// which does not.
fn edge(mut bad: i64, mut good: i64, fits: impl Fn(i64) -> bool) -> i64 {
    while (good - bad).abs() > 1 {
        let mid = bad + (good - bad) / 2;
        if fits(mid) {
            good = mid;
        } else {
            bad = mid;
        }
    }
    good
}

/// The farthest value from `from`, which fits, in the direction of `sign`
/// that fits, found by doubling the step until a value does not. `None` when
/// [`FAR`] ticks out still fits.
fn outward(from: i64, sign: i64, fits: impl Fn(i64) -> bool) -> Option<i64> {
    let mut good = from;
    let mut step = 1;
    while step <= FAR {
        let probe = from.saturating_add(sign * step);
        if !fits(probe) {
            return Some(edge(probe, good, &fits));
        }
        good = probe;
        step *= 2;
    }
    None
}
