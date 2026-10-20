//! Adjustment layers: a lane bar that grades and filters everything under it.
//!
//! The model has held `TrackKind::Adjustment` and `Generator::Adjustment` since
//! the look work, and the resolver turns such a bar into a canvas pass, but no
//! surface could create one. The span is taken from what the operator has
//! already said: the marked range, else the selection, else the whole sequence —
//! the same order of preference the export sheet uses, so one gesture does not
//! mean two different ranges in two places.
//!
//! Placement is refused on overlap by the model, so a second layer over an
//! occupied range stacks onto a fresh lane rather than surfacing an error the
//! operator cannot act on.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{Generator, TimeRange, TrackId, TrackKind};

use super::AppState;

impl AppState {
    /// Place an adjustment layer over the marked range, the selection, or the
    /// whole sequence, on the topmost adjustment lane that is free there.
    pub fn add_adjustment_layer(&mut self) -> Result<TrackId, EngineError> {
        let (start, duration) = self.adjustment_span();
        let track = match self.free_adjustment_lane(start, start + duration) {
            Some(id) => id,
            None => self.add_lane_of_kind(TrackKind::Adjustment, None)?,
        };
        let rate = self.playhead.rate;
        self.apply_command(Command::Edit(EditCommand::AddGenerated {
            track,
            generator: Generator::Adjustment,
            timeline: TimeRange::at_rate(start, duration, rate),
        }))?;
        self.status_message = Some("Added adjustment layer".into());
        Ok(track)
    }

    /// The range a new adjustment layer covers, in timeline ticks.
    ///
    /// A marked range is an explicit statement and wins. A selection is the next
    /// most explicit. With neither, the layer covers the sequence, and an empty
    /// sequence gets the same five-second default a dropped generator gets, so
    /// the bar is long enough to grab.
    fn adjustment_span(&self) -> (i64, i64) {
        if let Some((from, to)) = self.marked_range() {
            return (from.max(0), (to - from).max(1));
        }
        let selected: Vec<(i64, i64)> = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|lane| lane.clips.iter())
            .filter(|clip| clip.selected)
            .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
            .collect();
        if let (Some(from), Some(to)) = (
            selected.iter().map(|span| span.0).min(),
            selected.iter().map(|span| span.1).max(),
        ) && to > from
        {
            return (from.max(0), to - from);
        }
        let duration = self.store.sequence.duration_ticks;
        if duration > 0 {
            return (0, duration);
        }
        let rate = self.playhead.rate;
        (
            0,
            ((5 * i64::from(rate.num.max(1))) / i64::from(rate.den.max(1))).max(1),
        )
    }

    /// The topmost adjustment lane with nothing across `[from, to)`.
    fn free_adjustment_lane(&self, from: i64, to: i64) -> Option<TrackId> {
        self.store
            .sequence
            .tracks
            .iter()
            .find(|lane| {
                lane.kind == crate::types::UiTrackKind::Adjustment
                    && !lane.clips.iter().any(|clip| {
                        clip.timeline_start_ticks < to && clip.timeline_end_ticks > from
                    })
            })
            .map(|lane| TrackId::from_raw(lane.id))
    }
}
