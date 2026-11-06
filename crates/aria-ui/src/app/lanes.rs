//! Lane display options: height, and whether a lane draws waveforms or
//! filmstrips.
//!
//! These live in the project, not in the session, because a timeline layout is
//! part of the sequence as built: a tall dialogue lane with waveforms
//! on and a short B-roll lane with strips off must come back the way it was
//! left. One command carries all three so a gesture writes only what it
//! changed, and the height clamp lives in the engine action that writes it.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, TrackDisplay, TrackId};
use gpui::SharedString;

use crate::theme::Theme;

use super::AppState;

/// Default timeline panel height before the window's first layout frame lands.
pub(crate) const DEFAULT_PANEL_H: f32 = 320.0;

/// Height of a collapsed lane: enough to read the cut pattern, not enough for
/// a clip label.
pub(crate) const COLLAPSED_H: f32 = 20.0;

/// Height of the timeline header: two 24 px rows over the outline column plus
/// the 1 px rule under the header.
const RULER_H: f32 = 2.0 * Theme::ICON_HIT + 1.0;

/// Height of a lane group that has no lanes: one empty-lane strip.
const EMPTY_SIDE_H: f32 = Theme::ROW_HEIGHT;

/// Height of the draggable A/V divider between the video and audio lane groups.
const AV_DIVIDER_H: f32 = 6.0;

/// Height of the timeline footer: one 24 px control row plus the 1 px rule above it.
const OVERVIEW_H: f32 = Theme::ICON_HIT + 1.0;

/// The smallest share of the stack either side takes while both have lanes.
pub(crate) const MIN_SHARE: f32 = 0.15;

/// Height the video and audio lane groups share inside a timeline panel
/// `panel_h` tall, with or without the graph editor band open.
pub(crate) fn stack_height(panel_h: f32, graph_open: bool) -> f32 {
    let graph_h = if graph_open {
        crate::app::property_rows::GRAPH_EDITOR_H
    } else {
        0.0
    };
    (panel_h - RULER_H - 2.0 * Theme::SPACE_SM - AV_DIVIDER_H - graph_h - OVERVIEW_H).max(0.0)
}

/// A side's height in the stack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Side {
    /// This fraction of the stack.
    Share(f32),
    /// The stack less the other side's empty-lane strip.
    Rest,
    /// The empty-lane strip.
    Empty,
}

/// The video and audio sides for the lanes on each and the A/V `split`.
pub(crate) fn sides(video_lanes: usize, audio_lanes: usize, split: f32) -> (Side, Side) {
    match (video_lanes, audio_lanes) {
        (1.., 0) => (Side::Rest, Side::Empty),
        (0, 1..) => (Side::Empty, Side::Rest),
        _ => {
            let video = split.clamp(MIN_SHARE, 1.0 - MIN_SHARE);
            (Side::Share(video), Side::Share(1.0 - video))
        }
    }
}

impl Side {
    /// The lane viewport this side gives its group in a stack `stack_h` tall.
    pub(crate) fn viewport(self, stack_h: f32) -> f32 {
        let stack_h = stack_h.max(0.0);
        match self {
            Side::Share(share) => stack_h * share,
            Side::Rest => (stack_h - EMPTY_SIDE_H).max(0.0),
            Side::Empty => EMPTY_SIDE_H,
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self == Side::Empty
    }
}

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
    /// kept either way, so expanding restores the lane at its set height.
    pub fn set_lane_collapsed(&mut self, track: u64, collapsed: bool) -> Result<(), EngineError> {
        self.set_lane_display(track, None, None, None, Some(collapsed))?;
        self.reveal_video_lane(track);
        Ok(())
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
        // A write of what the lane already shows, such as the checked height
        // row or a resize released where it started, records nothing.
        if self
            .store
            .sequence
            .tracks
            .iter()
            .any(|lane| lane.id == track)
        {
            let now = self.lane_display(track);
            let next = TrackDisplay {
                height: height.map_or(now.height, TrackDisplay::clamp_height),
                waveform: waveform.unwrap_or(now.waveform),
                filmstrip: filmstrip.unwrap_or(now.filmstrip),
                collapsed: collapsed.unwrap_or(now.collapsed),
            };
            if next == now {
                return Ok(());
            }
        }
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

    /// The height the video and audio lane groups share, and each side's
    /// allocation in it, for the current timeline frame, lane counts, A/V
    /// split, and graph editor state.
    pub(crate) fn timeline_sides(&self) -> (f32, Side, Side) {
        let panel_h = self.timeline_frame.map(|f| f[3]).unwrap_or(DEFAULT_PANEL_H);
        let stack_h = stack_height(panel_h, self.timeline_graph_open());
        let total_rows = self.store.sequence.tracks.len();
        let visual_rows = self.visual_lane_count();
        let (video_side, audio_side) = sides(
            visual_rows,
            total_rows.saturating_sub(visual_rows),
            self.timeline_av_split,
        );
        (stack_h, video_side, audio_side)
    }

    /// Viewport height (px) of the video lane side above the A/V divider.
    pub fn video_side_viewport(&self) -> f32 {
        let (stack_h, video_side, _) = self.timeline_sides();
        video_side.viewport(stack_h)
    }

    /// Drawn clip-band height (px) for `track` before motion easing:
    /// [`COLLAPSED_H`] while collapsed, otherwise the active resize preview or
    /// the track's stored height clamped to [`TrackDisplay::HEIGHT`].
    pub(crate) fn lane_clip_height(&self, track: &crate::types::UiTrack) -> f32 {
        if track.collapsed {
            COLLAPSED_H
        } else {
            self.lane_preview(track.id)
                .unwrap_or(track.height)
                .max(*TrackDisplay::HEIGHT.start() as f32)
        }
    }

    /// Total height (px) of `track` at rest: its clip band plus any open
    /// property outline rows under it.
    pub(crate) fn lane_total_height(&self, track: &crate::types::UiTrack) -> f32 {
        let clip_h = self.lane_clip_height(track);
        let outline_h = if track.collapsed || self.expanded_clips.is_empty() {
            0.0
        } else {
            self.track_outline_rows(track.id).len() as f32
                * crate::app::property_rows::PROPERTY_ROW_H
        };
        clip_h + outline_h
    }

    /// Adjust [`Self::video_scroll_y`] so the visual lane `track_id` stands
    /// inside the video side's viewport, with its top edge at the viewport top
    /// when the lane and its open property rows are taller than the side.
    pub fn reveal_video_lane(&mut self, track_id: u64) {
        let visual_rows = self.visual_lane_count();
        let Some(row) = self.store.sequence.tracks[..visual_rows]
            .iter()
            .position(|track| track.id == track_id)
        else {
            return;
        };
        let mut y_top = 0.0;
        let mut y_bottom = 0.0;
        let mut content_h = 0.0;
        for (idx, track) in self.store.sequence.tracks[..visual_rows].iter().enumerate() {
            let h = self.lane_total_height(track);
            if idx == row {
                y_top = content_h;
                y_bottom = content_h + h;
            }
            content_h += h;
        }
        let viewport_h = self.video_side_viewport();
        let max_scroll = (content_h - viewport_h).max(0.0);
        let scroll = self.video_scroll_y.clamp(0.0, max_scroll);
        let low = content_h - viewport_h - y_top;
        let high = content_h - y_bottom;
        self.video_scroll_y = scroll.min(high).max(low).clamp(0.0, max_scroll);
    }

    /// Adjust [`Self::video_scroll_y`] so the visual lane holding `clip_id`
    /// keeps its header and clip row inside the video side's viewport.
    pub fn reveal_clip_lane(&mut self, clip_id: u64) {
        let visual_rows = self.visual_lane_count();
        let Some(track_id) = self.store.sequence.tracks[..visual_rows]
            .iter()
            .find(|track| track.clips.iter().any(|clip| clip.id == clip_id))
            .map(|track| track.id)
        else {
            return;
        };
        self.reveal_video_lane(track_id);
    }

    /// Clamp [`Self::video_scroll_y`] to the current video side overflow.
    pub(crate) fn clamp_video_scroll(&mut self) {
        let visual_rows = self.visual_lane_count();
        let content_h: f32 = self.store.sequence.tracks[..visual_rows]
            .iter()
            .map(|track| self.lane_total_height(track))
            .sum();
        let max_scroll = (content_h - self.video_side_viewport()).max(0.0);
        self.video_scroll_y = self.video_scroll_y.clamp(0.0, max_scroll);
    }

    /// Scroll the video lane group `delta_y` px further up from the A/V
    /// divider, toward its top lane, clamped to `[0.0, max_scroll]`.
    pub fn scroll_video_lanes(&mut self, delta_y: f32, max_scroll: f32) {
        self.video_scroll_y = scrolled(self.video_scroll_y, delta_y, max_scroll);
    }

    /// Scroll the audio lane group `delta_y` px further down from the A/V
    /// divider, toward its last lane, clamped to `[0.0, max_scroll]`.
    pub fn scroll_audio_lanes(&mut self, delta_y: f32, max_scroll: f32) {
        self.audio_scroll_y = scrolled(self.audio_scroll_y, delta_y, max_scroll);
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

    /// `clip`'s lane badge and name, as in `V3 hevc-2160p60`. Clips cut from
    /// one file read the same by name; on different lanes the badge tells them
    /// apart. `None` when no lane holds `clip`.
    pub fn clip_lane_label(&self, clip: ClipId) -> Option<SharedString> {
        let tracks = &self.store.sequence.tracks;
        tracks.iter().enumerate().find_map(|(row, track)| {
            let ui = track.clips.iter().find(|ui| ui.id == clip.raw())?;
            Some(format!("{} {}", lane_badge(tracks, row), ui.name).into())
        })
    }
}

/// A lane side's scroll moved by `delta`, within `[0.0, max_scroll]`. A scroll
/// past `max_scroll`, kept from before a lane was removed or shortened, is
/// brought within it before `delta` applies, so the first wheel step moves the
/// lanes.
fn scrolled(scroll: f32, delta: f32, max_scroll: f32) -> f32 {
    let max_scroll = max_scroll.max(0.0);
    (scroll.min(max_scroll) + delta).clamp(0.0, max_scroll)
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
