//! Magnet snapping for clip drag / trim.
//!
//! A magnet has an identity, not just a tick: the operator has to be able to see
//! what the clip locked onto, otherwise a snap that fired on the wrong edge
//! looks like a drag that missed. One owner for the candidate set's shape, the
//! precedence between two magnets at the same distance, and the name shown.
//!
//! The set is built once per projection into a [`SnapIndex`] sorted by tick, and
//! a drag frame binary-searches it. Before that every mouse move rebuilt every
//! clip edge and every beat in the sequence to answer one question about one
//! tick.

/// Default snap threshold in screen pixels (PLAN 4.4).
pub const SNAP_THRESHOLD_PX: f32 = 8.0;

/// What a magnet locked onto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    SequenceStart,
    SequenceEnd,
    Playhead,
    MarkIn,
    MarkOut,
    Marker,
    Beat,
    ClipStart,
    ClipEnd,
    /// Another keyframe on the clip a keyframe drag belongs to.
    Keyframe,
}

impl SnapKind {
    /// The name shown beside the guide line. One screen definition.
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::SequenceStart => "start",
            SnapKind::SequenceEnd => "sequence end",
            SnapKind::Playhead => "playhead",
            SnapKind::MarkIn => "mark in",
            SnapKind::MarkOut => "mark out",
            SnapKind::Marker => "marker",
            SnapKind::Beat => "beat",
            SnapKind::ClipStart => "clip start",
            SnapKind::ClipEnd => "clip end",
            SnapKind::Keyframe => "keyframe",
        }
    }

    /// Precedence when two magnets are the same distance away.
    ///
    /// This is the order the candidates were built in, made explicit: the
    /// playhead wins over the sequence bounds, those over the marks, and a
    /// clip's own edges lose to all of them. Ranking is data because the index
    /// is sorted by tick, so build order no longer answers it.
    pub fn rank(self) -> u8 {
        match self {
            SnapKind::Playhead => 0,
            SnapKind::SequenceStart => 1,
            SnapKind::SequenceEnd => 2,
            SnapKind::MarkIn => 3,
            SnapKind::MarkOut => 4,
            SnapKind::Marker => 5,
            SnapKind::ClipStart => 6,
            SnapKind::ClipEnd => 7,
            SnapKind::Beat => 8,
            SnapKind::Keyframe => 9,
        }
    }

    /// Every kind, so a test can enumerate them rather than list them.
    pub const ALL: [SnapKind; 10] = [
        SnapKind::SequenceStart,
        SnapKind::SequenceEnd,
        SnapKind::Playhead,
        SnapKind::MarkIn,
        SnapKind::MarkOut,
        SnapKind::Marker,
        SnapKind::Beat,
        SnapKind::ClipStart,
        SnapKind::ClipEnd,
        SnapKind::Keyframe,
    ];
}

/// A tick a drag may magnet to, with the reason it is a magnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapCandidate {
    pub tick: i64,
    pub kind: SnapKind,
    /// The clip this magnet belongs to, when it is a clip's own edge or beat.
    ///
    /// A drag must not magnet to the clip it is dragging, and the index is built
    /// once per projection rather than once per mouse move, so the clip being
    /// moved is skipped when the index is read.
    pub clip: Option<u64>,
}

impl SnapCandidate {
    pub fn new(tick: i64, kind: SnapKind) -> Self {
        Self {
            tick,
            kind,
            clip: None,
        }
    }

    /// A magnet that belongs to a clip: its edges and its beats.
    pub fn of_clip(tick: i64, kind: SnapKind, clip: u64) -> Self {
        Self {
            tick,
            kind,
            clip: Some(clip),
        }
    }

    /// A name for this exact magnet hit.
    ///
    /// The guide is animated, and an animation is keyed by element id: two
    /// different magnets have to be two different elements, or moving from one
    /// to the next slides a line that was already on screen instead of
    /// confirming a new snap. Built from the label, which is unique per kind,
    /// and the tick, which is what changed. A key rather than a string: a drag
    /// draws a guide on every mouse move, and the text form allocated each time.
    pub fn guide_key(&self) -> crate::motion::MotionKey {
        crate::motion::MotionKey::new("snap", self.kind.label()).with(self.tick)
    }
}

/// Every magnet in the sequence, sorted once.
///
/// Built when a projection is adopted, because that is when clip edges, beats,
/// markers and the sequence bounds change. The three magnets that move without a
/// new projection — the playhead and the two marks — are handed to
/// [`SnapIndex::nearest`] instead, which is three compares rather than a rebuild
/// of every clip's edges and beats on every mouse move of a drag.
#[derive(Debug, Clone)]
pub struct SnapIndex {
    rows: std::sync::Arc<[SnapCandidate]>,
}

impl Default for SnapIndex {
    /// No magnets: what a session holds before its first projection.
    fn default() -> Self {
        Self {
            rows: Vec::new().into(),
        }
    }
}

impl SnapIndex {
    /// Sort by tick, then by precedence. Sorted by tick is what makes the query
    /// a binary search; precedence second is what makes an equal distance
    /// resolve the way a single pass over the unsorted list resolved it.
    pub fn build(mut rows: Vec<SnapCandidate>) -> Self {
        rows.sort_by(|a, b| {
            a.tick
                .cmp(&b.tick)
                .then_with(|| a.kind.rank().cmp(&b.kind.rank()))
        });
        Self { rows: rows.into() }
    }

    /// The magnets themselves, for a test or a report that wants to see them.
    pub fn rows(&self) -> &[SnapCandidate] {
        &self.rows
    }

    /// Snap `desired` to the nearest magnet within `threshold_ticks`.
    ///
    /// `moving` is the clip being dragged, whose own edges and beats are not
    /// magnets for it. `extra` carries the magnets that are not in the index.
    ///
    /// Returns `(snapped_or_original, Some(candidate))` when a magnet fires. The
    /// nearest wins; an equal distance goes to the lower rank, and two magnets
    /// of the same kind at the same distance either side go to the later tick.
    pub fn nearest(
        &self,
        desired: i64,
        threshold_ticks: i64,
        moving: Option<u64>,
        extra: &[SnapCandidate],
    ) -> (i64, Option<SnapCandidate>) {
        let threshold = threshold_ticks.max(0);
        let mut best: Option<(i64, u8, SnapCandidate)> = None;
        for &candidate in extra {
            let dist = desired.saturating_sub(candidate.tick).abs();
            if dist <= threshold {
                keep(&mut best, dist, candidate);
            }
        }
        // Everything within the threshold is contiguous in tick order, so the
        // walk out of the search point stops at the first row past it.
        let start = self.rows.partition_point(|row| row.tick < desired);
        for row in &self.rows[start..] {
            let dist = row.tick.saturating_sub(desired);
            if dist > threshold {
                break;
            }
            if row.clip.is_some() && row.clip == moving {
                continue;
            }
            keep(&mut best, dist, *row);
        }
        for row in self.rows[..start].iter().rev() {
            let dist = desired.saturating_sub(row.tick);
            if dist > threshold {
                break;
            }
            if row.clip.is_some() && row.clip == moving {
                continue;
            }
            keep(&mut best, dist, *row);
        }
        match best {
            Some((_, _, candidate)) => (candidate.tick, Some(candidate)),
            None => (desired, None),
        }
    }
}

/// Keep `candidate` when it beats what is held: nearer, or as near with a lower
/// rank. Equal on both leaves the held one, which is the one the walk saw first.
fn keep(best: &mut Option<(i64, u8, SnapCandidate)>, dist: i64, candidate: SnapCandidate) {
    let rank = candidate.kind.rank();
    let better = match best {
        None => true,
        Some((held_dist, held_rank, _)) => {
            dist < *held_dist || (dist == *held_dist && rank < *held_rank)
        }
    };
    if better {
        *best = Some((dist, rank, candidate));
    }
}

/// Convert an 8px magnet into ticks at the current pixels-per-second / fps.
pub fn threshold_ticks(pps: f32, fps: f32) -> i64 {
    let pps = pps.max(1.0);
    let fps = fps.max(1.0);
    ((SNAP_THRESHOLD_PX / pps) * fps).round().max(1.0) as i64
}

/// Every magnet a projected sequence carries, sorted into an index.
///
/// One owner for what a magnet set contains: the editor calls this when it
/// adopts a projection, and the bench that records its cost calls the same
/// function, so a measured build and a shipped build cannot drift. The
/// playhead and the marks are not here — they move without a projection and
/// are passed to [`SnapIndex::nearest`] as `extra`.
pub fn index_for(store: &crate::types::UiEditorStore) -> SnapIndex {
    let mut rows = vec![
        SnapCandidate::new(0, SnapKind::SequenceStart),
        SnapCandidate::new(store.sequence.duration_ticks, SnapKind::SequenceEnd),
    ];
    for marker in store.sequence.markers.iter() {
        rows.push(SnapCandidate::new(marker.tick, SnapKind::Marker));
        if marker.duration_ticks > 0 {
            rows.push(SnapCandidate::new(
                marker.tick + marker.duration_ticks,
                SnapKind::Marker,
            ));
        }
    }
    // Beats live on the clip, not in the analysis cache: a reopened project
    // still snaps to them, and a trimmed clip offers only the beats it kept.
    for track in &store.sequence.tracks {
        for clip in &track.clips {
            rows.push(SnapCandidate::of_clip(
                clip.timeline_start_ticks,
                SnapKind::ClipStart,
                clip.id,
            ));
            rows.push(SnapCandidate::of_clip(
                clip.timeline_end_ticks,
                SnapKind::ClipEnd,
                clip.id,
            ));
            rows.extend(
                clip.beat_ticks
                    .iter()
                    .map(|tick| SnapCandidate::of_clip(*tick, SnapKind::Beat, clip.id)),
            );
        }
    }
    SnapIndex::build(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(tick: i64) -> SnapCandidate {
        SnapCandidate::new(tick, SnapKind::ClipStart)
    }

    fn index(rows: &[SnapCandidate]) -> SnapIndex {
        SnapIndex::build(rows.to_vec())
    }

    #[test]
    fn snaps_within_threshold() {
        let (tick, guide) = index(&[edge(0), edge(12), edge(48)]).nearest(10, 3, None, &[]);
        assert_eq!(tick, 12);
        assert_eq!(guide, Some(edge(12)));
    }

    #[test]
    fn leaves_distant_values() {
        let (tick, guide) = index(&[edge(0), edge(48)]).nearest(20, 3, None, &[]);
        assert_eq!(tick, 20);
        assert_eq!(guide, None);
    }

    #[test]
    fn the_guide_names_what_fired() {
        let rows = index(&[
            SnapCandidate::new(0, SnapKind::SequenceStart),
            SnapCandidate::new(41, SnapKind::ClipEnd),
        ]);
        let playhead = [SnapCandidate::new(40, SnapKind::Playhead)];
        let (_, guide) = rows.nearest(39, 3, None, &playhead);
        assert_eq!(guide.expect("fired").kind, SnapKind::Playhead);
    }

    #[test]
    fn a_tie_goes_to_the_higher_precedence_magnet() {
        let rows = index(&[SnapCandidate::new(44, SnapKind::Marker)]);
        let playhead = [SnapCandidate::new(40, SnapKind::Playhead)];
        let (tick, guide) = rows.nearest(42, 4, None, &playhead);
        assert_eq!(tick, 40);
        assert_eq!(guide.expect("fired").kind, SnapKind::Playhead);
    }

    /// Precedence is data, so it can be asserted rather than inferred from the
    /// order somebody happened to build the list in. Every kind ranks, no two
    /// share a rank, and the ranking is the order the old builder used.
    #[test]
    fn every_kind_ranks_and_no_two_share_a_rank() {
        let mut ranks: Vec<u8> = SnapKind::ALL.iter().map(|kind| kind.rank()).collect();
        let before = ranks.len();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), before, "two kinds share a precedence");
        assert!(
            SnapKind::Playhead.rank() < SnapKind::ClipStart.rank(),
            "a clip edge must not outrank the playhead"
        );
        assert!(
            SnapKind::Marker.rank() < SnapKind::Beat.rank(),
            "a beat must not outrank a marker the operator placed"
        );
    }

    /// The clip being dragged is not a magnet for itself, and another clip's
    /// edge at the same tick still is.
    #[test]
    fn a_drag_does_not_magnet_to_the_clip_it_is_moving() {
        let rows = index(&[
            SnapCandidate::of_clip(48, SnapKind::ClipStart, 7),
            SnapCandidate::of_clip(96, SnapKind::ClipStart, 9),
        ]);
        let (tick, guide) = rows.nearest(47, 3, Some(7), &[]);
        assert_eq!(tick, 47, "the dragged clip magneted to its own edge");
        assert_eq!(guide, None);

        let (tick, guide) = rows.nearest(95, 3, Some(7), &[]);
        assert_eq!(tick, 96);
        assert_eq!(guide.expect("fired").clip, Some(9));
    }

    /// A magnet past the threshold is never reached, however many magnets sit
    /// beyond it: the walk stops at the first row out of range in each
    /// direction, which is the whole point of sorting them.
    #[test]
    fn the_walk_stops_at_the_threshold() {
        let far: Vec<SnapCandidate> = (0..1_000).map(|n| edge(1_000 + n * 48)).collect();
        let rows = SnapIndex::build(far);
        assert_eq!(rows.rows().len(), 1_000);
        let (tick, guide) = rows.nearest(0, 8, None, &[]);
        assert_eq!(tick, 0);
        assert_eq!(guide, None);
        let (tick, _) = rows.nearest(1_002, 8, None, &[]);
        assert_eq!(tick, 1_000, "the nearest of a thousand magnets");
    }

    #[test]
    fn every_kind_names_itself() {
        for kind in SnapKind::ALL {
            let label = kind.label();
            assert!(!label.is_empty(), "{kind:?} has no label");
            assert_eq!(label, label.trim());
        }
        let mut labels: Vec<&str> = SnapKind::ALL.iter().map(|k| k.label()).collect();
        labels.sort_unstable();
        let before = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), before, "two kinds share a label");
    }

    /// The guide's animation is keyed by this id, so two magnets that are not
    /// the same hit must not share one: a repeated id would slide the line that
    /// is already on screen instead of confirming the new snap.
    #[test]
    fn a_guide_key_names_one_magnet_at_one_tick() {
        let mut ids: Vec<crate::motion::MotionKey> = SnapKind::ALL
            .iter()
            .flat_map(|kind| {
                [0, 1, 48].map(move |tick| SnapCandidate::new(tick, *kind).guide_key())
            })
            .collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "two magnets share a guide id");
        assert_eq!(
            SnapCandidate::new(48, SnapKind::Playhead).guide_key(),
            SnapCandidate::new(48, SnapKind::Playhead).guide_key(),
            "the same magnet at the same tick must keep its id"
        );
    }

    #[test]
    fn threshold_ticks_scales_with_zoom() {
        assert_eq!(threshold_ticks(80.0, 24.0), 2);
        assert!(threshold_ticks(8.0, 24.0) > threshold_ticks(80.0, 24.0));
    }
}
