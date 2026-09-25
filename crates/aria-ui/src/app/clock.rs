//! The playhead clock as a typed field.
//!
//! The tool strip's SMPTE readout is the fastest way to say where the playhead
//! should be, so it is a text box like any other: clicking it seeds a draft from
//! the current position, keys route to it through the field table, Enter seeks,
//! and anything that is not a timecode leaves the playhead alone. A refusal
//! restores the displayed value rather than clearing it, because a mistyped
//! digit should not lose the operator's place.

use super::AppState;

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

    pub fn clock_focused(&self) -> bool {
        self.clock_draft.is_some()
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
        let rate = self.store.sequence.frame_rate;
        self.clock_display =
            crate::timecode::format_ticks(self.store.playhead_ticks, rate.num, rate.den).into();
    }

    /// Digits and the two separators only: a timecode has no other characters,
    /// so a stray letter never reaches the draft.
    pub fn clock_push(&mut self, ch: char) {
        if !(ch.is_ascii_digit() || ch == ':' || ch == ';') {
            return;
        }
        if let Some(draft) = self.clock_draft.as_mut() {
            if draft.len() < 16 {
                draft.push(ch);
            }
        }
    }

    pub fn clock_pop(&mut self) {
        if let Some(draft) = self.clock_draft.as_mut() {
            draft.pop();
        }
    }

    /// Enter: seek, or refuse and put the previous value back.
    pub fn commit_clock(&mut self) {
        let Some(draft) = self.clock_draft.take() else {
            return;
        };
        let rate = self.store.sequence.frame_rate;
        match crate::timecode::parse_ticks(&draft, rate.num, rate.den) {
            Some(frame) => {
                self.set_playhead(frame);
                self.status_message = Some(format!(
                    "Playhead {}",
                    crate::timecode::format_ticks(self.store.playhead_ticks, rate.num, rate.den)
                ));
            }
            None => {
                self.report_error(format!(
                    "{draft} is not a timecode. Type frames, or HH:MM:SS:FF."
                ));
            }
        }
    }
}
