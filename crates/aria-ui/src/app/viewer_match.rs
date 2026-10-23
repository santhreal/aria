//! A Source tab opened from a timeline clip.
//!
//! Double-clicking a media clip opens its take in a Source tab at the clip's
//! frame, with the clip's source window as the tab's In and Out. Setting In or
//! Out in that tab trims the clip (a match-frame edit): a new In moves the
//! clip's head, a new Out its tail, and every frame that stays keeps its place
//! on the timeline. Linked companions trim with it, as they do on the timeline.
//!
//! The tab's marks are read back from the clip on every new projection, so an
//! undo, a trim on the timeline, or a refused trim leaves the tab showing the
//! window the clip holds.

use aria_commands::Command;
use aria_models::{ClipId, ClipSource, RationalTime, SequenceId, TimeRange, resample};

use super::AppState;
use super::pool_preview::SourceView;

/// The take a clip shows, its source window, and one source frame of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipFrame {
    pub media: u64,
    /// Source tick, in the take's own rate.
    pub tick: i64,
    /// The clip's source window, `[In, Out)`.
    pub window: (i64, i64),
}

impl AppState {
    /// Double-click on a timeline clip: a nest opens its sequence, a media clip
    /// opens its take in a linked Source tab, and a clip with no take behind it
    /// opens its property rows.
    pub fn double_click_clip(&mut self, clip: u64) {
        let Some((nested, media)) = self
            .ui_clip_ref(ClipId::from_raw(clip))
            .map(|ui| (ui.nested_sequence, ui.media_id))
        else {
            return;
        };
        if let Some(sequence) = nested {
            if let Err(err) = self.open_sequence(Some(SequenceId::from_raw(sequence))) {
                self.report_error(err);
            }
        } else if media.is_some() {
            self.open_clip_source(clip);
        } else {
            self.toggle_expanded(clip);
        }
    }

    /// The take `clip` shows on the open timeline, its source window, and the
    /// source frame at timeline tick `at`: the frame the sequence shows there,
    /// through the clip's speed, ramp, and direction. Outside the clip, the
    /// frame at its first tick. `None` for a clip with no take behind it.
    pub(crate) fn clip_frame(&self, clip: ClipId, at: i64) -> Option<ClipFrame> {
        let engine = self.engine.lock();
        let clip = engine.project().timeline().clip(clip)?;
        let ClipSource::Media { media, source } = &clip.content else {
            return None;
        };
        let inside = clip.timeline.start.value <= at && at < clip.timeline.end_tick();
        let at = if inside {
            at
        } else {
            clip.timeline.start.value
        };
        let tick = clip
            .source_time_at(RationalTime::new(at, clip.timeline.start.rate))
            .ok()
            .flatten()?
            .value;
        Some(ClipFrame {
            media: media.raw(),
            tick,
            window: (source.start.value, source.end_tick()),
        })
    }

    /// Open `clip`'s take in a Source tab linked to the clip, at the frame the
    /// sequence shows under the playhead when the playhead is on the clip, and
    /// at the clip's first frame otherwise. A tab already linked to the clip is
    /// focused and moved to that frame. Stops sequence playback, since the
    /// transport keys now reach the source. Returns whether a tab has the keys.
    pub fn open_clip_source(&mut self, clip: u64) -> bool {
        let Some(frame) = self.clip_frame(ClipId::from_raw(clip), self.playhead.value) else {
            self.status_message = Some("That clip has no source to open".into());
            return false;
        };
        let Some((name, duration_ticks)) = self.viewable_source(frame.media) else {
            return false;
        };
        if self.playing {
            self.toggle_playback();
        }
        let (mark_in, mark_out) = frame.window;
        self.sources.open(SourceView {
            media: frame.media,
            name: name.clone(),
            duration_ticks,
            tick: frame.tick,
            mark_in: Some(mark_in),
            mark_out: Some(mark_out),
            clip: Some(clip),
        });
        if let Some(view) = self.sources.focused_mut() {
            view.tick = frame.tick.clamp(0, duration_ticks.saturating_sub(1).max(0));
        }
        self.status_message = Some(format!("{name}: In and Out trim the clip"));
        true
    }

    /// Trim the clip the focused Source tab is linked to, so its source window
    /// becomes `[start, end)`, as one undo step. A refusal reports on the
    /// status line and leaves the clip and the tab's marks as they were.
    pub(crate) fn trim_linked_source(&mut self, start: i64, end: i64) {
        let Some(clip) = self.sources.focused().and_then(|view| view.clip) else {
            return;
        };
        let clip = ClipId::from_raw(clip);
        let timeline = match self.linked_trim(clip, start, end) {
            Ok(range) => range,
            Err(reason) => {
                self.status_message = Some(reason.into());
                return;
            }
        };
        match self.apply_command(Command::Edit(self.trim_command(clip, timeline))) {
            Ok(_) => self.status_message = Some("Trimmed the clip to the source In and Out".into()),
            Err(err) => self.report_error(err),
        }
    }

    /// The timeline range that gives `clip` the source window `[start, end)`.
    ///
    /// The head moves by the change in In and the tail by the change in Out,
    /// converted from the take's rate to the sequence's and through the clip's
    /// speed, so the frames that stay keep their timeline positions. A reversed
    /// clip shows its window backward: In moves its tail and Out its head.
    fn linked_trim(&self, clip: ClipId, start: i64, end: i64) -> Result<TimeRange, &'static str> {
        if end <= start {
            return Err("In must come before Out");
        }
        let engine = self.engine.lock();
        let Some(clip) = engine.project().timeline().clip(clip) else {
            return Err("That clip is no longer on the timeline");
        };
        let ClipSource::Media { source, .. } = &clip.content else {
            return Err("That clip has no source window to trim");
        };
        if clip.freeze_frame {
            return Err("A freeze frame shows one frame; trim it on the timeline");
        }
        if clip.has_speed_curve() {
            return Err("A speed ramp maps its source unevenly; trim it on the timeline");
        }
        let rate = clip.timeline.start.rate;
        let to_timeline = |ticks: i64| {
            clip.unscale_by_speed(resample(RationalTime::new(ticks, source.start.rate), rate).value)
        };
        let head = to_timeline(start - source.start.value);
        let tail = to_timeline(end - source.end_tick());
        let (head, tail) = if clip.reversed {
            (-tail, -head)
        } else {
            (head, tail)
        };
        let from = clip.timeline.start.value + head;
        let to = clip.timeline.end_tick() + tail;
        if from < 0 {
            return Err("The clip would start before the sequence does");
        }
        if to <= from {
            return Err("In must come before Out");
        }
        Ok(TimeRange::at_rate(from, to - from, rate))
    }
}

#[cfg(test)]
#[path = "viewer_match_tests.rs"]
mod tests;
