//! A clip drag in flight, read for the frame that draws it where its release
//! leaves it: a move, and an edge drag or a slide.

use super::*;

/// A clip drag in flight, read for the frame that draws it where its release
/// lands.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldMove {
    /// The clips the release moves, the held one first, as they are drawn
    /// before the drag. Behind a handle, so the frame that reads it copies
    /// a refcount rather than the clips.
    pub members: std::sync::Arc<[UiClip]>,
    /// Whether `members` was read with linked selection on.
    pub linked: bool,
    /// The lane the held clip lands on.
    pub track: u64,
    /// The tick the held clip lands at.
    pub start: i64,
    /// A ripple move, which inserts at `start` on `track`: its cards stay
    /// where they are and the insertion line is drawn there.
    pub ripple: bool,
}

impl HeldMove {
    /// Whether the release moves `clip` off where it is drawn.
    pub fn takes(&self, clip: u64) -> bool {
        !self.ripple && self.members.iter().any(|member| member.id == clip)
    }

    /// The cards the release leaves, each where it lands: the held clip on
    /// `track` at `start`, and each companion on its own lane by the same
    /// distance, stopped at the sequence start. None for a ripple move.
    pub fn cards(&self, rate: Rational) -> Vec<UiClip> {
        let Some(held) = self.members.first().filter(|_| !self.ripple) else {
            return Vec::new();
        };
        let delta = self.start - held.timeline_start_ticks;
        self.members
            .iter()
            .map(|member| {
                if member.id == held.id {
                    member.moved(self.track, delta, rate)
                } else {
                    let to = (member.timeline_start_ticks + delta).max(0);
                    member.moved(member.track_id, to - member.timeline_start_ticks, rate)
                }
            })
            .collect()
    }

    /// The tick a ripple move inserts at on lane `track`.
    pub fn inserts_at(&self, track: u64) -> Option<i64> {
        (self.ripple && self.track == track).then_some(self.start)
    }
}

/// An edge drag or a slide in flight, read for the frame that draws it where
/// its release leaves it.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldEdit {
    /// Each clip the release changes, as the release leaves it.
    pub cards: Vec<HeldCard>,
    /// The lane shifts the release sends, in the order it sends them.
    pub shifts: Vec<LaneShift>,
    /// The edge the drag holds. `None` for a slide.
    pub edge: Option<HeldEdge>,
}

impl HeldEdit {
    /// The card of `clip`, when the release changes it.
    pub fn card(&self, clip: u64) -> Option<&HeldCard> {
        self.cards.iter().find(|card| card.clip.id == clip)
    }

    /// Whether the release changes a clip on lane `track`.
    pub fn touches(&self, track: u64) -> bool {
        self.cards.iter().any(|card| card.clip.track_id == track)
            || self.shifts.iter().any(|shift| shift.lane == track)
    }

    /// The farthest the shifts move a clip on lane `track`. The lane's clips
    /// from this far before the view to this far after it are every clip
    /// they move into the view.
    pub fn reach(&self, track: u64) -> i64 {
        self.shifts
            .iter()
            .filter(|shift| shift.lane == track)
            .map(|shift| shift.by.abs())
            .sum()
    }

    /// What lane `track` draws inside the ticks `view`, in timeline order:
    /// `clips`, the lane's clips as the store holds them, each moved by the
    /// shifts, and each clip the release changes as its card. `rate` is the
    /// sequence's.
    pub fn lane(
        &self,
        track: u64,
        clips: &[UiClip],
        view: (i64, i64),
        rate: Rational,
    ) -> Vec<UiClip> {
        let shows = |start: i64, end: i64| end > view.0 && start < view.1;
        let mut drawn: Vec<UiClip> = clips
            .iter()
            .filter(|clip| self.card(clip.id).is_none())
            .filter_map(|clip| {
                let by = LaneShift::total(&self.shifts, track, clip.timeline_start_ticks);
                shows(clip.timeline_start_ticks + by, clip.timeline_end_ticks + by).then(|| {
                    if by == 0 {
                        clip.clone()
                    } else {
                        clip.moved(track, by, rate)
                    }
                })
            })
            .collect();
        drawn.extend(
            self.cards
                .iter()
                .map(|card| &card.clip)
                .filter(|clip| {
                    clip.track_id == track
                        && shows(clip.timeline_start_ticks, clip.timeline_end_ticks)
                })
                .cloned(),
        );
        drawn.sort_by_key(|clip| clip.timeline_start_ticks);
        drawn
    }
}

/// A clip an edge drag or a slide changes, as the release leaves it.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldCard {
    pub clip: UiClip,
    /// Where the clip played its take before the drag.
    pub was: TakeSpan,
}

impl HeldCard {
    /// Where the frame of the take drawn `x` pixels into the card before the
    /// drag is drawn now, at `px_per_tick`. `None` for a clip with no take.
    pub fn moved_x(&self, x: f32, px_per_tick: f32) -> Option<f32> {
        self.was.carry(TakeSpan::of(&self.clip), x, px_per_tick)
    }
}

/// Where a clip plays its take: the ticks it spans on the timeline and the
/// source seconds it plays across them, from its head to its tail, or from
/// its tail to its head when `reversed`. The take maps onto the span in
/// proportion, as the model maps it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TakeSpan {
    pub ticks: (i64, i64),
    pub source: (f64, f64),
    pub reversed: bool,
}

impl TakeSpan {
    pub fn of(clip: &UiClip) -> Self {
        Self {
            ticks: (clip.timeline_start_ticks, clip.timeline_end_ticks),
            source: (clip.source_start, clip.source_end),
            reversed: clip.reversed,
        }
    }

    /// Where the frame a clip playing this span draws `x` pixels from its
    /// head is drawn by one playing `now`, at `px_per_tick`.
    pub fn carry(self, now: Self, x: f32, px_per_tick: f32) -> Option<f32> {
        let tick = self.ticks.0 as f64 + f64::from(x / px_per_tick);
        let at = now.tick_of(self.source_at(tick)?)?;
        Some(((at - now.ticks.0 as f64) * f64::from(px_per_tick)) as f32)
    }

    /// The span's length in ticks and the take's in seconds, `None` when
    /// either is empty.
    fn lengths(self) -> Option<(f64, f64)> {
        let span = (self.ticks.1 - self.ticks.0) as f64;
        let take = self.source.1 - self.source.0;
        (span > 0.0 && take > 0.0).then_some((span, take))
    }

    /// How far along the take `along` of the way along the span is.
    fn turned(self, along: f64) -> f64 {
        if self.reversed { 1.0 - along } else { along }
    }

    /// The source second drawn at timeline `tick`.
    fn source_at(self, tick: f64) -> Option<f64> {
        let (span, take) = self.lengths()?;
        Some(self.source.0 + self.turned((tick - self.ticks.0 as f64) / span) * take)
    }

    /// The timeline tick source second `source` is drawn at.
    fn tick_of(self, source: f64) -> Option<f64> {
        let (span, take) = self.lengths()?;
        Some(self.ticks.0 as f64 + self.turned((source - self.source.0) / take) * span)
    }
}

/// Every clip on `lane` that starts at or after `from` moves by `by`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneShift {
    pub lane: u64,
    pub from: i64,
    pub by: i64,
}

impl LaneShift {
    /// How far `shifts`, sent in order, move a clip on lane `track` that
    /// starts at `start`.
    pub fn total(shifts: &[Self], track: u64, start: i64) -> i64 {
        shifts
            .iter()
            .filter(|shift| shift.lane == track)
            .fold(0, |by, shift| {
                if start + by >= shift.from {
                    by + shift.by
                } else {
                    by
                }
            })
    }
}

/// The edge an edge drag holds: the head of `clip` when `left`, else its
/// tail. `stopped` while the drag's room holds the edge short of where the
/// pointer took it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldEdge {
    pub clip: u64,
    pub left: bool,
    pub stopped: bool,
}

#[cfg(test)]
#[path = "types_held_tests.rs"]
mod tests;
