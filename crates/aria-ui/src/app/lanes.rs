//! Lane display options: height, and whether a lane draws waveforms or
//! filmstrips.
//!
//! These live in the project, not in the session, because a timeline layout is
//! part of the sequence the operator built: a tall dialogue lane with waveforms
//! on and a short B-roll lane with strips off must come back the way it was
//! left. One command carries all three so a gesture writes only what it
//! changed, and the height clamp lives in the engine action that writes it.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{TrackDisplay, TrackId};

use super::AppState;

impl AppState {
    /// Resize a lane. Any pixel value is accepted; the engine clamps it into
    /// [`TrackDisplay::HEIGHT`], so a drag never has to stop at the limit.
    pub fn set_lane_height(&mut self, track: u64, height: f32) -> Result<(), EngineError> {
        let height = height.round().max(0.0) as u32;
        self.set_lane_display(track, Some(height), None, None, None)
    }

    /// Turn waveform drawing on or off for one lane.
    pub fn set_lane_waveform(&mut self, track: u64, waveform: bool) -> Result<(), EngineError> {
        self.set_lane_display(track, None, Some(waveform), None, None)
    }

    /// Turn filmstrip drawing on or off for one lane.
    pub fn set_lane_filmstrip(&mut self, track: u64, filmstrip: bool) -> Result<(), EngineError> {
        self.set_lane_display(track, None, None, Some(filmstrip), None)
    }

    /// Collapse a lane to a summary strip, or expand it again. The height is
    /// kept either way, so expanding restores the lane the operator sized.
    pub fn set_lane_collapsed(&mut self, track: u64, collapsed: bool) -> Result<(), EngineError> {
        self.set_lane_display(track, None, None, None, Some(collapsed))
    }

    /// Flip the lane between collapsed and expanded. The chevron in the header
    /// is this and nothing else.
    pub fn toggle_lane_collapsed(&mut self, track: u64) -> Result<(), EngineError> {
        let collapsed = self.lane_display(track).collapsed;
        self.set_lane_collapsed(track, !collapsed)
    }

    fn set_lane_display(
        &mut self,
        track: u64,
        height: Option<u32>,
        waveform: Option<bool>,
        filmstrip: Option<bool>,
        collapsed: Option<bool>,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetTrackDisplay {
            track: TrackId::from_raw(track),
            height,
            waveform,
            filmstrip,
            collapsed,
        }))?;
        Ok(())
    }

    /// The lane's current display options as projected, or the shipped defaults
    /// when the id names no lane.
    pub fn lane_display(&self, track: u64) -> TrackDisplay {
        self.store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.id == track)
            .map(|lane| TrackDisplay {
                height: lane.height.round().max(0.0) as u32,
                waveform: lane.waveform,
                filmstrip: lane.filmstrip,
                collapsed: lane.collapsed,
            })
            .unwrap_or_default()
    }

    /// Step a lane's height to the next preset in [`TrackDisplay::PRESETS`] in the
    /// requested direction (`taller = true` steps up, `false` steps down).
    ///
    /// A lane between presets steps to the nearest preset in the scroll direction.
    /// A lane already at or beyond the outer preset in that direction is left
    /// unchanged without recording an undo step.
    pub fn step_lane_height_preset(&mut self, track: u64, taller: bool) -> Result<(), EngineError> {
        let current = self.lane_display(track).height;
        let next = if taller {
            TrackDisplay::PRESETS
                .iter()
                .map(|&(_, h)| h)
                .find(|&h| h > current)
        } else {
            TrackDisplay::PRESETS
                .iter()
                .rev()
                .map(|&(_, h)| h)
                .find(|&h| h < current)
        };
        if let Some(height) = next {
            self.set_lane_height(track, height as f32)?;
        }
        Ok(())
    }

    /// Record the height a lane resize would land on. Clamped here so the
    /// preview never shows a height the engine would refuse.
    pub fn set_lane_preview(&mut self, track: u64, height: f32) {
        let height = TrackDisplay::clamp_height(height.round().max(0.0) as u32) as f32;
        self.lane_resize = Some((track, height));
    }

    /// The previewed height for `track`, if that is the lane being resized.
    pub fn lane_preview(&self, track: u64) -> Option<f32> {
        self.lane_resize
            .and_then(|(id, height)| (id == track).then_some(height))
    }

    /// End the gesture, yielding the height to commit.
    pub fn take_lane_preview(&mut self) -> Option<f32> {
        self.lane_resize.take().map(|(_, height)| height)
    }

    /// Set the height fraction `[0.15, 0.85]` allocated to the video lane group
    /// above the horizontal A/V divider.
    pub fn set_timeline_av_split(&mut self, ratio: f32) {
        self.timeline_av_split = ratio.clamp(0.15, 0.85);
    }

    /// Scroll the video lane group vertically, clamped to `[0.0, max_scroll]`.
    pub fn scroll_video_lanes(&mut self, delta_y: f32, max_scroll: f32) {
        self.video_scroll_y = (self.video_scroll_y + delta_y).clamp(0.0, max_scroll.max(0.0));
    }

    /// Scroll the audio lane group vertically, clamped to `[0.0, max_scroll]`.
    pub fn scroll_audio_lanes(&mut self, delta_y: f32, max_scroll: f32) {
        self.audio_scroll_y = (self.audio_scroll_y + delta_y).clamp(0.0, max_scroll.max(0.0));
    }

    /// Number of visual lanes seated above the A/V divider.
    pub fn visual_lane_count(&self) -> usize {
        self.store
            .sequence
            .tracks
            .iter()
            .take_while(|lane| {
                !matches!(
                    lane.kind,
                    crate::types::UiTrackKind::Audio | crate::types::UiTrackKind::Submix
                )
            })
            .count()
    }

    /// Ordinal badge for the lane at `row`: video lanes number upward from the
    /// divider (`V1`, `V2`, `V3`, ...), while audio lanes number downward from
    /// the divider (`A1`, `A2`, `A3`, ...).
    pub fn lane_badge_at(&self, row: usize) -> String {
        lane_badge(&self.store.sequence.tracks, row)
    }
}

/// Compute the ordinal badge (`V1`, `V2`, ..., `A1`, `A2`, ...) for `tracks[row]`.
pub(super) fn lane_badge(tracks: &[crate::types::UiTrack], row: usize) -> String {
    use crate::types::UiTrackKind;
    let Some(target) = tracks.get(row) else {
        return String::new();
    };
    let visual_count = tracks
        .iter()
        .take_while(|lane| !matches!(lane.kind, UiTrackKind::Audio | UiTrackKind::Submix))
        .count();
    let prefix = match target.kind {
        UiTrackKind::Video => "V",
        UiTrackKind::Audio => "A",
        UiTrackKind::Text => "T",
        UiTrackKind::Generator => "G",
        UiTrackKind::Adjustment => "ADJ",
        UiTrackKind::Submix => "SUB",
    };
    let ordinal = if row < visual_count {
        tracks[row..visual_count]
            .iter()
            .filter(|lane| lane.kind == target.kind)
            .count()
    } else {
        tracks[visual_count..=row]
            .iter()
            .filter(|lane| lane.kind == target.kind)
            .count()
    }
    .max(1);
    format!("{prefix}{ordinal}")
}
