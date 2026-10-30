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

    /// Move the playhead to the next moment after it, if any.
    pub fn seek_next_moment(&mut self) {
        let now = self.store.playhead_ticks;
        let next = self
            .analysis
            .moments
            .iter()
            .map(|moment| moment.start_tick)
            .filter(|tick| *tick > now)
            .min();
        if let Some(tick) = next {
            self.set_playhead(tick);
        }
    }

    /// Move the playhead to the previous moment before it, if any.
    pub fn seek_previous_moment(&mut self) {
        let now = self.store.playhead_ticks;
        let previous = self
            .analysis
            .moments
            .iter()
            .map(|moment| moment.start_tick)
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
        let mut ticks: Vec<i64> = self
            .analysis
            .moments
            .iter()
            .filter(|moment| moment.kind == kind.as_str())
            .map(|moment| moment.start_tick)
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
        let mut spans: Vec<(i64, i64)> = self
            .analysis
            .moments
            .iter()
            .filter(|moment| moment.kind == kind.as_str())
            .filter_map(|moment| {
                let start = moment.start_tick.max(clip.timeline_start_ticks);
                let end = moment.end_tick.min(clip.timeline_end_ticks);
                (end > start).then_some((start, end))
            })
            .collect();
        spans.sort_unstable();
        spans
    }

    /// The clip covering `tick` strictly inside its own range.
    pub(super) fn clip_covering(&self, tick: i64) -> Option<aria_models::ClipId> {
        self.store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|clip| clip.timeline_start_ticks < tick && clip.timeline_end_ticks > tick)
            .map(|clip| aria_models::ClipId::from_raw(clip.id))
    }

    /// The clip that starts exactly at `tick`.
    pub(super) fn clip_starting_at(&self, tick: i64) -> Option<aria_models::ClipId> {
        self.store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|clip| clip.timeline_start_ticks == tick)
            .map(|clip| aria_models::ClipId::from_raw(clip.id))
    }
}
