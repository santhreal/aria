//! Moving through what a detector found: the filters and the two seeks.
//!
//! Helpers are `pub(super)` so `analysis.rs` and its sibling modules call them
//! by name.

use super::*;

impl AppState {
    pub fn set_analysis_kind_filter(&mut self, kind: Option<AnalysisKind>) {
        self.analysis.kind_filter = kind;
        if let Err(err) = self.refresh_moments() {
            self.report_error(err);
        }
    }

    pub fn set_analysis_minimum_confidence(&mut self, minimum: f32) {
        self.analysis.minimum_confidence = minimum.clamp(0.0, 1.0);
        if let Err(err) = self.refresh_moments() {
            self.report_error(err);
        }
    }

    /// Where the selected clip shows each listed moment of `kind`, or of every
    /// kind for `None`, in timeline ticks, sorted and deduplicated.
    ///
    /// Placed from the stored seconds through the clip as the projection
    /// holds it now, so a move, a trim or a retime since the rows were read
    /// lands each moment where the clip plays it. Empty when the selection is
    /// not the media the rows were read for.
    pub(crate) fn moments_on_selection(&self, kind: Option<MomentKind>) -> Vec<(i64, i64)> {
        let Some(detail) = self.first_selected_detail() else {
            return Vec::new();
        };
        let media = self.selected_ui_clip_ref().and_then(|clip| clip.media_id);
        if media.is_none() || media != self.analysis.media_id {
            return Vec::new();
        }
        let clip = &detail.model;
        let mut placed: Vec<(i64, i64)> = self
            .analysis
            .moments
            .iter()
            .filter(|moment| {
                kind.as_ref()
                    .is_none_or(|kind| moment.kind == kind.as_str())
            })
            .filter_map(|moment| moment_on(clip, moment.seconds))
            .collect();
        placed.sort_unstable();
        placed.dedup();
        placed
    }

    /// Move the playhead to the next moment after it, if any.
    pub fn seek_next_moment(&mut self) {
        let now = self.store.playhead_ticks;
        let next = self
            .moments_on_selection(None)
            .into_iter()
            .map(|(start, _)| start)
            .find(|tick| *tick > now);
        if let Some(tick) = next {
            self.set_playhead(tick);
        }
    }

    /// Move the playhead to the previous moment before it, if any.
    pub fn seek_previous_moment(&mut self) {
        let now = self.store.playhead_ticks;
        let previous = self
            .moments_on_selection(None)
            .into_iter()
            .map(|(start, _)| start)
            .filter(|tick| *tick < now)
            .max();
        if let Some(tick) = previous {
            self.set_playhead(tick);
        }
    }

    /// Ticks of one moment kind that fall strictly inside the selected clip.
    pub(super) fn moment_ticks_in_selection(&self, kind: MomentKind) -> Vec<i64> {
        let Some(clip) = self.selected_ui_clip_ref() else {
            return Vec::new();
        };
        let (first, last) = (clip.timeline_start_ticks, clip.timeline_end_ticks);
        let mut ticks: Vec<i64> = self
            .moments_on_selection(Some(kind))
            .into_iter()
            .map(|(start, _)| start)
            .filter(|tick| *tick > first && *tick < last)
            .collect();
        ticks.dedup();
        ticks
    }

    /// Half-open spans of one moment kind inside the selected clip: the part
    /// of each that its source window shows.
    pub(super) fn moment_spans_in_selection(&self, kind: MomentKind) -> Vec<(i64, i64)> {
        self.moments_on_selection(Some(kind))
            .into_iter()
            .filter(|(start, end)| end > start)
            .collect()
    }

    /// The clips a detector's edit acts on: the selected clip first, then each
    /// selected clip linked to it. With linked selection on, a click selects a
    /// picture and its sound together and both are cut; a clip selected alone
    /// is cut alone, and a clip on another lane under the same frame is never
    /// touched.
    pub(super) fn analysis_targets(&self) -> Vec<aria_models::ClipId> {
        let Some(primary) = self.selected_ui_clip_ref() else {
            return Vec::new();
        };
        let (id, link) = (primary.id, primary.linked_to);
        let mut targets = vec![aria_models::ClipId::from_raw(id)];
        if link.is_some() {
            targets.extend(self.selection.iter().copied().filter(|clip| {
                clip.raw() != id
                    && self
                        .ui_clip_ref(*clip)
                        .is_some_and(|ui| ui.linked_to == link)
            }));
        }
        targets
    }

    /// The unlocked clip on `lane` that starts at `start` and ends at or
    /// before `end`: the piece of a target that a removal at `[start, end)`
    /// takes.
    pub(super) fn clip_on_lane_within(
        &self,
        lane: u64,
        start: i64,
        end: i64,
    ) -> Option<aria_models::ClipId> {
        self.store
            .sequence
            .tracks
            .iter()
            .filter(|track| track.id == lane)
            .flat_map(|track| track.clips.iter())
            .find(|clip| {
                !clip.locked && clip.timeline_start_ticks == start && clip.timeline_end_ticks <= end
            })
            .map(|clip| aria_models::ClipId::from_raw(clip.id))
    }
}
