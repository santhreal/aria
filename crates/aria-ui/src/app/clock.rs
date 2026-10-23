//! The playhead clock as a typed field.
//!
//! The SMPTE readout at the left of the Viewer's control row is a text box
//! like any other: clicking it seeds a draft from the current position, keys
//! route to it through the field table, Enter seeks, and anything that is not
//! a timecode leaves the playhead alone. A refusal restores the displayed value
//! rather than clearing it, so a mistyped digit keeps the playhead's place.
//!
//! A digit or a keypad `+`/`-` pressed over the Timeline or the Program view
//! opens the field with that character, so a timecode is typed without a click.
//! A leading sign makes the typed duration relative: it moves the selected
//! clips, trims the selected edge, or moves the playhead when nothing is
//! selected.

use aria_commands::Command;
use aria_core::count::counted;
use aria_engine::EngineError;
use aria_models::{ClipId, RationalTime, TrackId};

use super::AppState;

/// A playhead field: the one in the Viewer's control row, or the 18 px one in
/// the timeline header. Both read and type the same draft; the site is the one
/// drawn with the caret.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClockSite {
    #[default]
    Viewer,
    Timeline,
}

impl AppState {
    /// Take or release the clock's focus. Taking it seeds the draft from where
    /// the playhead is now, so a partial edit starts from the truth on screen.
    pub fn focus_clock(&mut self, focused: bool) {
        if focused {
            self.clock_draft = Some(self.clock_display.to_string());
        } else {
            self.clock_draft = None;
        }
    }

    /// Open the field from a key pressed over the Timeline or the Program
    /// view. `key` is the gpui key name: a digit, or the keypad's `add` and
    /// `subtract`. Returns whether the key started the draft; any other key, a
    /// Source tab in the Viewer, or a field already open leaves it to the
    /// bindings.
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
            crate::keys::KeyRoute::Viewer if self.sources.focused().is_none() => ClockSite::Viewer,
            _ => return false,
        };
        if self.clock_focused() {
            return false;
        }
        self.clock_site = site;
        self.clock_draft = Some(first.to_string());
        true
    }

    pub fn clock_focused(&self) -> bool {
        self.clock_draft.is_some()
    }

    /// Whether the field at `site` holds the draft, and the text it shows:
    /// the draft while it does, the playhead otherwise.
    pub fn clock_field(&self, site: ClockSite) -> (bool, gpui::SharedString) {
        if self.clock_focused() && self.clock_site == site {
            (true, self.clock_text())
        } else {
            (false, self.clock_display.clone())
        }
    }

    /// The text the field shows: the draft while it is being typed, the playhead
    /// otherwise. One owner, so the strip cannot disagree with the field.
    pub fn clock_text(&self) -> gpui::SharedString {
        self.clock_draft
            .as_deref()
            .map(gpui::SharedString::from)
            .unwrap_or_else(|| self.clock_display.clone())
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

    /// Digits, the separators `:`, `;` and `.`, and the feet mark `+` after a
    /// digit, plus a `+` or `-` as the first character: a time has no other
    /// characters, so a stray letter never reaches the draft.
    pub fn clock_push(&mut self, ch: char) {
        let Some(draft) = self.clock_draft.as_mut() else {
            return;
        };
        let sign = matches!(ch, '+' | '-') && draft.is_empty();
        let feet = ch == '+' && draft.bytes().any(|b| b.is_ascii_digit());
        if !(sign || feet || ch.is_ascii_digit() || matches!(ch, ':' | ';' | '.')) {
            return;
        }
        if draft.len() < 16 {
            draft.push(ch);
        }
    }

    pub fn clock_pop(&mut self) {
        if let Some(draft) = self.clock_draft.as_mut() {
            draft.pop();
        }
    }

    /// Enter: seek, move by a signed duration, or refuse and put the previous
    /// value back.
    pub fn commit_clock(&mut self) {
        let Some(draft) = self.clock_draft.take() else {
            return;
        };
        let rate = self.store.sequence.frame_rate;
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
