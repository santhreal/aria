//! The playhead clock as a typed field.
//!
//! The SMPTE readout at the left of the Viewer's control row is a text box
//! like any other: clicking it seeds a draft from the tab in front of the
//! Viewer, all of it selected so a typed timecode replaces it, keys route to
//! it through the field table, Enter seeks that tab, and anything that is not
//! a timecode leaves it alone. A refusal restores the displayed value rather
//! than clearing it, so a mistyped digit keeps the tab's place.
//!
//! A digit or a keypad `+`/`-` pressed over the Timeline or the Viewer opens
//! the field with that character, so a timecode is typed without a click. A
//! leading sign makes the typed duration relative: on a Source tab it moves the
//! take, and on Program or the Timeline it moves the selected clips, trims the
//! selected edge, or moves the playhead when nothing is selected.

use aria_commands::Command;
use aria_core::count::counted;
use aria_engine::EngineError;
use aria_models::{ClipId, RationalTime, TrackId};

use super::AppState;
use super::line_text::{LineRule, LineText};

/// What the clock takes: digits, the separators `:`, `;` and `.`, the feet
/// mark `+` after a digit, and a `+` or `-` ahead of everything else. A time
/// has no other characters, so a stray letter never reaches the draft.
pub(crate) const CLOCK_RULE: LineRule = LineRule::chars(16).admitting(|before, ch| {
    let sign = matches!(ch, '+' | '-') && before.is_empty();
    let feet = ch == '+' && before.bytes().any(|b| b.is_ascii_digit());
    sign || feet || ch.is_ascii_digit() || matches!(ch, ':' | ';' | '.')
});

/// A playhead field: the one in the Viewer's control row, which follows the
/// tab in front, or the 18 px one in the timeline header, which follows the
/// sequence playhead. Both type the same draft; the site is the one drawn with
/// the caret.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClockSite {
    #[default]
    Viewer,
    Timeline,
}

impl AppState {
    /// Take or release the clock's focus. Taking it seeds the draft from the
    /// time shown at `self.clock_site`, selected: a typed time replaces it, and
    /// an arrow key keeps it for a partial edit that starts from the value on
    /// screen.
    pub fn focus_clock(&mut self, focused: bool) {
        if focused {
            let seed = self.clock_idle_text(self.clock_site).to_string();
            self.clock_draft = Some(LineText::selecting_all(seed));
        } else {
            self.clock_draft = None;
            self.clock_site = ClockSite::Viewer;
        }
    }

    /// Open the field from a key pressed over the Timeline or the Viewer. `key`
    /// is the gpui key name: a digit, or the keypad's `add` and `subtract`.
    /// Returns whether the key started the draft; any other key, or a field
    /// already open, is left to the bindings.
    pub(crate) fn type_into_clock(&mut self, key: &str, shift: bool) -> bool {
        let first = match key {
            "add" => '+',
            "subtract" => '-',
            digit if !shift && digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() => {
                char::from(digit.as_bytes()[0])
            }
            _ => return false,
        };
        let site = match crate::keys::focus_route(self) {
            crate::keys::KeyRoute::Timeline => ClockSite::Timeline,
            crate::keys::KeyRoute::Viewer => ClockSite::Viewer,
            _ => return false,
        };
        if self.clock_focused() {
            return false;
        }
        self.clock_site = site;
        self.clock_draft = Some(LineText::new(first.to_string()));
        true
    }

    pub fn clock_focused(&self) -> bool {
        self.clock_draft.is_some()
    }

    /// Whether the field at `site` holds the draft, and the text it shows:
    /// the draft while it does, the site's time otherwise.
    pub fn clock_field(&self, site: ClockSite) -> (bool, gpui::SharedString) {
        if let Some(draft) = self
            .clock_draft
            .as_deref()
            .filter(|_| self.clock_site == site)
        {
            (true, gpui::SharedString::from(draft))
        } else {
            (false, self.clock_idle_text(site))
        }
    }

    /// The draft's `(anchor, caret)`, `None` while no clock field holds it.
    pub fn clock_selection(&self) -> Option<(usize, usize)> {
        self.clock_draft.as_ref().map(LineText::selection)
    }

    /// The text the Viewer field shows: the draft while a clock field is being
    /// typed, the tab in front otherwise.
    pub fn clock_text(&self) -> gpui::SharedString {
        self.clock_draft
            .as_deref()
            .map(gpui::SharedString::from)
            .unwrap_or_else(|| self.clock_idle_text(ClockSite::Viewer))
    }

    fn clock_idle_text(&self, site: ClockSite) -> gpui::SharedString {
        if site == ClockSite::Viewer
            && let Some(view) = self.sources.focused()
        {
            let (tick_rate, frame_rate) = self.source_clock_rates(view.media);
            let frame =
                aria_models::resample(RationalTime::new(view.tick, tick_rate), frame_rate).value;
            return crate::timecode::format_time(
                frame,
                frame_rate,
                self.store.sequence.time_display,
            )
            .into();
        }
        self.clock_display.clone()
    }

    /// `(tick_rate, frame_rate)` for `media` in a Source tab: video takes use
    /// their own frame rate for both; stills, audio-only items, and unprojected
    /// ids display and parse in the sequence's frame rate.
    fn source_clock_rates(&self, media: u64) -> (aria_models::Rational, aria_models::Rational) {
        let seq_rate = self.store.sequence.frame_rate;
        let Some(item) = self.store.media.iter().find(|item| item.id == media) else {
            return (seq_rate, seq_rate);
        };
        if item.still || item.kind == crate::types::UiClipKind::Audio {
            (item.frame_rate, seq_rate)
        } else {
            (item.frame_rate, item.frame_rate)
        }
    }
    pub(crate) fn refresh_clock_text(&mut self) {
        self.clock_display = self.sequence_time(self.store.playhead_ticks).into();
    }

    /// `ticks` as the sequence's time display reads it: timecode, frames, or
    /// feet and frames.
    pub(crate) fn sequence_time(&self, ticks: i64) -> String {
        let sequence = &self.store.sequence;
        crate::timecode::format_time(ticks, sequence.frame_rate, sequence.time_display)
    }

    /// Enter: seek, move by a signed duration, or report an error and restore
    /// the previous value.
    pub fn commit_clock(&mut self) {
        let site = std::mem::take(&mut self.clock_site);
        let Some(draft) = self.clock_draft.take() else {
            return;
        };
        let source = (site == ClockSite::Viewer)
            .then(|| self.sources.focused())
            .flatten()
            .map(|view| (view.media, view.tick, view.duration_ticks));
        let (tick_rate, rate) = match source {
            Some((media, ..)) => self.source_clock_rates(media),
            None => (
                self.store.sequence.frame_rate,
                self.store.sequence.frame_rate,
            ),
        };
        let (sign, body) = match draft.as_bytes().first() {
            Some(b'+') => (1, &draft[1..]),
            Some(b'-') => (-1, &draft[1..]),
            _ => (0, draft.as_str()),
        };
        let Some(frames) = crate::timecode::parse_ticks(body, rate.num, rate.den) else {
            self.report_error(format!(
                "{draft} is not a time. Type frames, HH:MM:SS:FF or feet+frames, with a \
                 leading + or - to move by it."
            ));
            return;
        };
        if let Some((media, from, duration)) = source {
            let ticks = aria_models::resample(RationalTime::new(frames, rate), tick_rate).value;
            let to = if sign == 0 {
                ticks
            } else {
                from.saturating_add(sign * ticks)
            };
            let last = duration.saturating_sub(1).max(0);
            self.sources.stop();
            self.hover_pool_item(media, to.clamp(0, last));
            return;
        }
        if sign == 0 {
            self.set_playhead(frames);
            self.report_playhead();
            return;
        }
        let delta = sign * frames;
        if self.selected_edge.is_some() || !self.selection.is_empty() {
            if let Err(err) = self.move_selection_by(delta) {
                self.report_error(&err);
            }
            return;
        }
        self.set_playhead((self.store.playhead_ticks + delta).max(0));
        self.report_playhead();
    }

    fn report_playhead(&mut self) {
        self.status_message = Some(format!(
            "Playhead {}",
            self.sequence_time(self.store.playhead_ticks)
        ));
    }

    /// Move the selected clips by `delta` frames as one undo step, or trim the
    /// selected edge by it. Grouped companions, and with linked selection on
    /// linked ones, move with the clip that carries them, once. A move stops at the sequence start rather than
    /// squeezing the selection against it.
    pub(crate) fn move_selection_by(&mut self, delta: i64) -> Result<(), EngineError> {
        if self.selected_edge.is_some() {
            return self.trim_selected_edge(delta);
        }
        let mut moves: Vec<(ClipId, TrackId, i64)> = Vec::new();
        let mut earliest = i64::MAX;
        {
            let engine = self.engine.lock();
            let project = engine.project();
            let companions = if self.linked_selection {
                aria_models::Project::move_companions
            } else {
                aria_models::Project::group_companions
            };
            let mut covered: Vec<ClipId> = Vec::new();
            for &clip in &self.selection {
                if covered.contains(&clip) {
                    continue;
                }
                let (Some(placed), Some(track)) =
                    (project.clip(clip), project.timeline().track_of(clip))
                else {
                    continue;
                };
                for companion in companions(project, clip) {
                    if let Some(other) = project.clip(companion) {
                        earliest = earliest.min(other.start().value);
                    }
                    covered.push(companion);
                }
                moves.push((clip, track, placed.start().value));
            }
        }
        if moves.is_empty() {
            return Ok(());
        }
        let delta = delta.max(-earliest);
        if delta == 0 {
            self.status_message = Some("The selection is already at the sequence start".into());
            return Ok(());
        }
        // Lead with the clip on the side the move goes to, so a clip never
        // lands on a selected neighbour that has not moved yet.
        moves.sort_by_key(|&(_, _, start)| if delta > 0 { -start } else { start });
        let rate = self.playhead.rate;
        let count = moves.len();
        self.one_step(|state| {
            for (clip, to_track, start) in moves {
                state.apply_command(Command::Edit(state.move_command(
                    clip,
                    to_track,
                    RationalTime::new(start + delta, rate),
                )))?;
            }
            Ok(())
        })?;
        let way = if delta > 0 { "later" } else { "earlier" };
        self.status_message = Some(format!(
            "Moved {} {} {way}",
            counted(count, "clip", "clips"),
            counted(delta.unsigned_abs(), "frame", "frames")
        ));
        Ok(())
    }
}
