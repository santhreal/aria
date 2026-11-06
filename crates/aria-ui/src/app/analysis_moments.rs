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

    /// How far the selected clip's timeline-to-source offset has moved since
    /// [`Self::refresh_moments`] projected [`AnalysisForm::moments`], or `None`
    /// when the selection no longer points at the media those rows came from.
    fn moment_shift(&self) -> Option<i64> {
        let Some(clip) = self.selected_ui_clip_ref() else {
            return self.analysis.media_id.is_none().then_some(0);
        };
        if let Some(media_id) = self.analysis.media_id
            && clip.media_id != Some(media_id)
        {
            return None;
        }
        if self.analysis.media_id.is_none() {
            return Some(0);
        }
        let offset = clip.timeline_start_ticks - clip.source_start_ticks;
        Some(offset - self.analysis.offset_ticks)
    }

    /// Move the playhead to the next moment after it, if any.
    pub fn seek_next_moment(&mut self) {
        let Some(shift) = self.moment_shift() else {
            return;
        };
        let now = self.store.playhead_ticks;
        let next = self
            .analysis
            .moments
            .iter()
            .map(|moment| moment.start_tick + shift)
            .filter(|tick| *tick > now)
            .min();
        if let Some(tick) = next {
            self.set_playhead(tick);
        }
    }

    /// Move the playhead to the previous moment before it, if any.
    pub fn seek_previous_moment(&mut self) {
        let Some(shift) = self.moment_shift() else {
            return;
        };
        let now = self.store.playhead_ticks;
        let previous = self
            .analysis
            .moments
            .iter()
            .map(|moment| moment.start_tick + shift)
            .filter(|tick| *tick < now)
            .max();
        if let Some(tick) = previous {
            self.set_playhead(tick);
        }
    }

    /// Ticks of one moment kind that fall strictly inside the selected clip.
    pub(super) fn moment_ticks_in_selection(&self, kind: MomentKind) -> Vec<i64> {
        let Some(clip) = self.selected_ui_clip() else {
            return Vec::new();
        };
        let Some(shift) = self.moment_shift() else {
            return Vec::new();
        };
        let mut ticks: Vec<i64> = self
            .analysis
            .moments
            .iter()
            .filter(|moment| moment.kind == kind.as_str())
            .map(|moment| moment.start_tick + shift)
            .filter(|tick| *tick > clip.timeline_start_ticks && *tick < clip.timeline_end_ticks)
            .collect();
        ticks.sort_unstable();
        ticks.dedup();
        ticks
    }

    /// Half-open spans of one moment kind overlapping the selected clip, clamped to it.
    pub(super) fn moment_spans_in_selection(&self, kind: MomentKind) -> Vec<(i64, i64)> {
        let Some(clip) = self.selected_ui_clip() else {
            return Vec::new();
        };
        let Some(shift) = self.moment_shift() else {
            return Vec::new();
        };
        let mut spans: Vec<(i64, i64)> = self
            .analysis
            .moments
            .iter()
            .filter(|moment| moment.kind == kind.as_str())
            .filter_map(|moment| {
                let start = (moment.start_tick + shift).max(clip.timeline_start_ticks);
                let end = (moment.end_tick + shift).min(clip.timeline_end_ticks);
                (end > start).then_some((start, end))
            })
            .collect();
        spans.sort_unstable();
        spans
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
