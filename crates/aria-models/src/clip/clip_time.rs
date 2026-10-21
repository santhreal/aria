//! Mapping between source, clip and timeline time, and the markers a clip
//! holds in clip time.

use super::Clip;
use crate::error::ModelError;
use crate::time::{RationalTime, TimeRange};

impl Clip {
    /// The absolute timeline tick a source tick shows at, or `None` when that
    /// source tick is outside the clip's visible window.
    ///
    /// The one owner of the source-to-timeline mapping: the fraction of the
    /// source window a position sits at becomes the same fraction of the
    /// timeline span (exact for constant speed — the window maps linearly onto
    /// the span — and a close approximation for speed ramps). `reversed` mirrors
    /// the window. Pure; `O(1)`.
    ///
    /// Anything measured against the media — a beat grid, a detected span, an
    /// onset — is stored in source time so it rides trims and splits, and comes
    /// back through here.
    pub fn timeline_tick_for_source(&self, source_tick: i64) -> Option<i64> {
        let source = self.source_range()?;
        let dur = source.duration.value;
        if dur <= 0 {
            return None;
        }
        let first = source.start.value;
        if source_tick < first || source_tick >= first + dur {
            return None;
        }
        let offset = source_tick - first;
        // Integer fast path: 1:1 speed without curve
        if self.timeline.duration.value == dur && !self.has_speed_curve() {
            let tl_offset = if self.reversed { dur - offset } else { offset };
            return Some(self.timeline.start.value + tl_offset);
        }
        let mut frac = offset as f64 / dur as f64;
        if self.reversed {
            frac = 1.0 - frac;
        }
        Some(
            self.timeline.start.value + (frac * self.timeline.duration.value as f64).round() as i64,
        )
    }

    /// The timeline span a source span shows at, clamped to the clip's visible
    /// window, or `None` when the two do not overlap or the result is empty.
    ///
    /// Same mapping as [`Clip::timeline_tick_for_source`], for a measured span
    /// rather than a point: a detected silence, a voice segment, a scene. Both
    /// ends are exclusive-safe — a span that runs past the window is cut at it
    /// rather than dropped, because the part inside is what the clip shows.
    pub fn timeline_span_for_source(&self, start: i64, end: i64) -> Option<(i64, i64)> {
        let source = self.source_range()?;
        let dur = source.duration.value;
        if dur <= 0 || end <= start {
            return None;
        }
        let first = source.start.value;
        let low = start.max(first);
        let high = end.min(first + dur);
        if high <= low {
            return None;
        }
        let origin = self.timeline.start.value;
        // Integer fast path: 1:1 speed without curve
        if self.timeline.duration.value == dur && !self.has_speed_curve() {
            let (from, to) = if self.reversed {
                (origin + dur - (high - first), origin + dur - (low - first))
            } else {
                (origin + (low - first), origin + (high - first))
            };
            return (to > from).then_some((from, to));
        }
        let fraction = |tick: i64| (tick - first) as f64 / dur as f64;
        let (mut from, mut to) = (fraction(low), fraction(high));
        if self.reversed {
            (from, to) = (1.0 - to, 1.0 - from);
        }
        let span = self.timeline.duration.value as f64;
        let (from, to) = (
            origin + (from * span).round() as i64,
            origin + (to * span).round() as i64,
        );
        (to > from).then_some((from, to))
    }

    /// Absolute timeline ticks (at the clip's timeline rate) for every detected
    /// beat that falls within the clip's visible source window. Beats left
    /// behind by a trim or a split are skipped. Pure; `O(beats)`.
    pub fn beat_timeline_ticks(&self) -> Vec<i64> {
        let mut out: Vec<i64> = self
            .beats
            .iter()
            .filter_map(|beat| self.timeline_tick_for_source(*beat))
            .collect();
        out.sort_unstable();
        out
    }

    /// The id of the first contribution this clip holds that no catalog lists:
    /// its clip type, then its video effects and audio effects in chain order.
    /// A plugin that is missing, failed, or disabled drops its registrations,
    /// so this is the id the clip's missing-plugin badge states. The clip keeps
    /// the id, its parameters, and its keyframes unchanged.
    pub fn missing_plugin_id(&self) -> Option<&str> {
        if let Some(id) = self.clip_type.as_deref()
            && crate::clip_type_spec(id).is_none()
        {
            return Some(id);
        }
        self.effects
            .iter()
            .find(|fx| crate::effect_spec(&fx.effect_id).is_none())
            .map(|fx| fx.effect_id.as_str())
            .or_else(|| {
                self.audio_effects
                    .iter()
                    .find(|fx| crate::audio_effect_spec(&fx.effect_id).is_none())
                    .map(|fx| fx.effect_id.as_str())
            })
    }

    /// Markers attached to this clip, in `(tick, id)` order.
    pub fn markers(&self) -> &[crate::timeline::Marker] {
        &self.markers
    }

    /// Find a clip marker on this clip by id.
    pub fn marker(&self, id: crate::ids::MarkerId) -> Option<&crate::timeline::Marker> {
        self.markers.iter().find(|m| m.id == id)
    }

    /// The clip's own time window: its media/multicam/sequence source range, or
    /// `0..timeline.duration` at the timeline rate for a generated clip.
    pub fn clip_time_range(&self) -> TimeRange {
        self.source_range().unwrap_or_else(|| {
            TimeRange::at_rate(0, self.timeline.duration.value, self.timeline.start.rate)
        })
    }

    /// Map an absolute timeline tick inside this clip to the clip's own time
    /// coordinate (`source` time for media/multicam/sequence clips, or
    /// clip-local `0..duration` for generated clips).
    pub fn clip_time_at_timeline_tick(&self, timeline_tick: i64) -> Option<RationalTime> {
        if timeline_tick < self.timeline.start.value || timeline_tick >= self.timeline.end_tick() {
            return None;
        }
        if self.source_range().is_some() {
            self.source_time_at(RationalTime::new(timeline_tick, self.timeline.start.rate))
                .ok()
                .flatten()
        } else {
            Some(RationalTime::new(
                timeline_tick - self.timeline.start.value,
                self.timeline.start.rate,
            ))
        }
    }

    /// Map a clip-time tick to an absolute timeline tick, or `None` if it lies
    /// outside the clip's active window.
    pub fn timeline_tick_for_clip_time(&self, clip_tick: i64) -> Option<i64> {
        if self.source_range().is_some() {
            self.timeline_tick_for_source(clip_tick)
        } else if (0..self.timeline.duration.value).contains(&clip_tick) {
            Some(self.timeline.start.value + clip_tick)
        } else {
            None
        }
    }

    /// Map a clip-time span `[start, end)` to an absolute timeline span
    /// `(tl_start, tl_end)`, clamped to the clip's visible window.
    pub fn timeline_span_for_clip_time(&self, start: i64, end: i64) -> Option<(i64, i64)> {
        if self.source_range().is_some() {
            self.timeline_span_for_source(start, end)
        } else {
            let dur = self.timeline.duration.value;
            if dur <= 0 || end <= start {
                return None;
            }
            let low = start.max(0);
            let high = end.min(dur);
            (high > low).then_some((
                self.timeline.start.value + low,
                self.timeline.start.value + high,
            ))
        }
    }

    /// Insert a clip marker in `(tick, id)` order, validating that its position
    /// and duration lie within [`Self::clip_time_range`].
    pub fn add_marker(
        &mut self,
        mut marker: crate::timeline::Marker,
    ) -> Result<crate::ids::MarkerId, ModelError> {
        if !marker.tick.rate.is_valid() || !marker.duration.rate.is_valid() {
            return Err(ModelError::InvalidRange);
        }
        if marker.duration.value < 0 {
            return Err(ModelError::InvalidParam(format!(
                "marker duration must be non-negative (got {})",
                marker.duration.value
            )));
        }
        if self.marker(marker.id).is_some() {
            return Err(ModelError::InvalidRange);
        }
        let window = self.clip_time_range();
        marker.tick = crate::time::resample(marker.tick, window.start.rate);
        marker.duration = crate::time::resample(marker.duration, window.start.rate);
        let start = window.start.value;
        let end = window.end_tick();
        if marker.tick.value < start || marker.tick.value >= end {
            return Err(ModelError::InvalidParam(format!(
                "clip marker tick ({}) is outside clip source range [{start}..{end})",
                marker.tick.value
            )));
        }
        let marker_end = marker.tick.value + marker.duration.value;
        if marker_end > end {
            return Err(ModelError::InvalidParam(format!(
                "clip marker range end ({marker_end}) exceeds clip source end ({end})"
            )));
        }
        let id = marker.id;
        let at = self
            .markers
            .partition_point(|m| (m.tick.value, m.id) <= (marker.tick.value, marker.id));
        self.markers.insert(at, marker);
        Ok(id)
    }

    /// Remove a clip marker by id, returning it for undo capture.
    pub fn remove_marker(&mut self, id: crate::ids::MarkerId) -> Option<crate::timeline::Marker> {
        let index = self.markers.iter().position(|m| m.id == id)?;
        Some(self.markers.remove(index))
    }

    /// Update all fields of a clip marker in place, validating `duration >= 0`
    /// and that `tick + duration` stays within [`Self::clip_time_range`].
    #[allow(clippy::too_many_arguments)]
    pub fn edit_marker(
        &mut self,
        id: crate::ids::MarkerId,
        tick: RationalTime,
        duration: RationalTime,
        name: String,
        color: crate::timeline::MarkerColor,
        comment: String,
        kind: crate::timeline::MarkerKind,
    ) -> Result<(), ModelError> {
        let mut marker = self
            .remove_marker(id)
            .ok_or(ModelError::UnknownMarker(id))?;
        let before = marker.clone();
        marker.tick = tick;
        marker.duration = duration;
        marker.name = name;
        marker.color = color;
        marker.comment = comment;
        marker.kind = kind;
        match self.add_marker(marker) {
            Ok(_) => Ok(()),
            Err(e) => {
                let restored = self.add_marker(before);
                debug_assert!(restored.is_ok(), "re-inserting the removed clip marker");
                Err(e)
            }
        }
    }
}
