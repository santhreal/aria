//! The text of a one-line box, with its caret and selection.
//!
//! Every key-routed box but the title in the Viewer holds its text as a
//! [`LineText`]: the rename bar, the searches, the clocks, a value readout open
//! for typing, the dialog fields and the prompts. The arrows, Home and End move
//! the caret, Shift extends the selection, typed and pasted text replaces the
//! selection, and Ctrl+Z puts back the text the box held before its first edit.
//!
//! The text is reachable only through these methods, so the caret and the
//! anchor are always character boundaries inside it.

use std::ops::Range;

use super::clip_text::text_caret::geometry::{byte_at, word_left, word_right};
use crate::keys::fields::CaretKey;

/// How long a box's text may grow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    /// Characters: a name, a query, a prompt.
    Chars(usize),
    /// UTF-8 bytes: text sent across a boundary that limits its size.
    Bytes(usize),
}

/// What a box takes: how long its text may grow, and which characters.
#[derive(Debug, Clone, Copy)]
pub struct LineRule {
    pub cap: Cap,
    /// Whether `ch` may follow `before`, the text ahead of it once placed.
    /// Control characters are dropped before this is called.
    pub admits: fn(&str, char) -> bool,
}

impl LineRule {
    /// Any printable character, with no cap: a prompt, or a setting's value,
    /// whose length the receiver limits.
    pub const ANY: Self = Self::chars(usize::MAX);

    /// Any printable character, up to `max` characters.
    pub const fn chars(max: usize) -> Self {
        Self {
            cap: Cap::Chars(max),
            admits: any_char,
        }
    }

    /// Any printable character, up to `max` UTF-8 bytes.
    pub const fn bytes(max: usize) -> Self {
        Self {
            cap: Cap::Bytes(max),
            admits: any_char,
        }
    }

    /// This rule, taking only the characters `admits` accepts.
    pub const fn admitting(self, admits: fn(&str, char) -> bool) -> Self {
        Self { admits, ..self }
    }

    /// Whether `ch` fits after a text of `chars` characters and `bytes` bytes.
    fn fits(self, chars: usize, bytes: usize, ch: char) -> bool {
        match self.cap {
            Cap::Chars(max) => chars < max,
            Cap::Bytes(max) => bytes + ch.len_utf8() <= max,
        }
    }
}

fn any_char(_: &str, _: char) -> bool {
    true
}

/// One line of typed text, its caret, and its selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineText {
    text: String,
    /// The fixed end of the selection, in characters; the caret when nothing
    /// is selected.
    anchor: usize,
    /// Characters before the caret.
    caret: usize,
    /// The text before the first edit since the box was set, for an undo.
    undo: Option<String>,
    /// The text an undo replaced, for a redo.
    redo: Option<String>,
}

impl LineText {
    /// `text` with the caret after it.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let end = text.chars().count();
        Self {
            text,
            anchor: end,
            caret: end,
            undo: None,
            redo: None,
        }
    }

    /// `text`, all of it selected: typing replaces it, and an arrow key keeps
    /// it and puts the caret at that end.
    pub fn selecting_all(text: impl Into<String>) -> Self {
        Self {
            anchor: 0,
            ..Self::new(text)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn into_string(self) -> String {
        self.text
    }

    /// Replace the whole text and put the caret after it. Not an edit: the
    /// undo forgets what the box held.
    pub fn set(&mut self, text: impl Into<String>) {
        *self = Self::new(text);
    }

    /// Empty the box, keeping its buffer.
    pub fn clear(&mut self) {
        self.text.clear();
        self.anchor = 0;
        self.caret = 0;
        self.undo = None;
        self.redo = None;
    }

    /// The length in characters.
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    /// `(anchor, caret)` in characters.
    pub fn selection(&self) -> (usize, usize) {
        (self.anchor, self.caret)
    }

    /// The selection a box draws: `Some((anchor, caret))` while the box holds
    /// focus, `None` while it does not.
    pub fn editing(&self, focused: bool) -> Option<(usize, usize)> {
        focused.then(|| self.selection())
    }

    /// The selected text, `None` with nothing selected.
    pub fn selected(&self) -> Option<&str> {
        let (low, high) = self.span();
        (low < high).then(|| &self.text[byte_at(&self.text, low)..byte_at(&self.text, high)])
    }

    /// The selection's ends in order.
    fn span(&self) -> (usize, usize) {
        (self.anchor.min(self.caret), self.anchor.max(self.caret))
    }

    /// Type `text` over the selection, or at the caret.
    pub fn type_text(&mut self, text: &str, rule: LineRule) -> bool {
        let (low, high) = self.span();
        self.replace(low..high, text, rule)
    }

    /// Replace characters `range` with what `rule` admits of `text`, and put
    /// the caret after it: a keystroke, a paste, or an IME commit. Control
    /// characters are dropped. Text the rule refuses whole leaves the box as
    /// it was, so a letter typed over a selected timecode does not clear it.
    ///
    /// Returns whether the text changed.
    pub fn replace(&mut self, range: Range<usize>, text: &str, rule: LineRule) -> bool {
        if text.is_empty() {
            return self.remove(range);
        }
        let count = self.char_count();
        let end = range.end.min(count);
        let start = range.start.min(end);
        let (from, to) = (byte_at(&self.text, start), byte_at(&self.text, end));
        // Nothing ahead of the insertion point changes, so the first character
        // that would land is tested against exactly what it lands after.
        let (kept_chars, kept_bytes) = (count - (end - start), self.text.len() - (to - from));
        let before = &self.text[..from];
        let lands = text.chars().any(|ch| {
            !ch.is_control() && rule.fits(kept_chars, kept_bytes, ch) && (rule.admits)(before, ch)
        });
        if !lands {
            return false;
        }
        self.remember();
        let tail = self.text.split_off(to);
        self.text.truncate(from);
        let mut chars = kept_chars;
        let mut placed = 0;
        for ch in text.chars().filter(|ch| !ch.is_control()) {
            if rule.fits(chars, self.text.len() + tail.len(), ch) && (rule.admits)(&self.text, ch) {
                self.text.push(ch);
                chars += 1;
                placed += 1;
            }
        }
        self.text.push_str(&tail);
        self.caret = start + placed;
        self.anchor = self.caret;
        true
    }

    /// Backspace: the selection, or the character before the caret.
    pub fn backspace(&mut self) -> bool {
        let (low, high) = self.span();
        if low == high {
            self.remove(low.saturating_sub(1)..low)
        } else {
            self.remove(low..high)
        }
    }

    /// Move, select, delete forward, select all, undo, or redo. `extend` is
    /// Shift held: the caret moves and the anchor stays.
    ///
    /// Returns whether the text changed.
    pub fn key(&mut self, key: CaretKey, extend: bool) -> bool {
        let count = self.char_count();
        let (low, high) = self.span();
        let collapse = !extend && low != high;
        let to = match key {
            CaretKey::Delete if low == high => return self.remove(low..(low + 1).min(count)),
            CaretKey::Delete => return self.remove(low..high),
            CaretKey::SelectAll => {
                self.anchor = 0;
                self.caret = count;
                return false;
            }
            CaretKey::Undo => return self.undo(),
            CaretKey::Redo => return self.redo(),
            CaretKey::Left if collapse => low,
            CaretKey::Right if collapse => high,
            CaretKey::Left => self.caret.saturating_sub(1),
            CaretKey::Right => (self.caret + 1).min(count),
            CaretKey::WordLeft => word_left(&self.text, self.caret),
            CaretKey::WordRight => word_right(&self.text, self.caret),
            // One line: Up and Down go to its ends.
            CaretKey::LineStart | CaretKey::TextStart | CaretKey::Up => 0,
            CaretKey::LineEnd | CaretKey::TextEnd | CaretKey::Down => count,
        };
        self.caret = to;
        if !extend {
            self.anchor = to;
        }
        false
    }

    /// Delete characters `range` and put the caret where they were.
    fn remove(&mut self, range: Range<usize>) -> bool {
        let count = self.char_count();
        let end = range.end.min(count);
        let start = range.start.min(end);
        if start == end {
            return false;
        }
        self.remember();
        let span = byte_at(&self.text, start)..byte_at(&self.text, end);
        self.text.replace_range(span, "");
        self.anchor = start;
        self.caret = start;
        true
    }

    /// Keep the text an undo returns to, before the first edit since the box
    /// was set or undone. Any edit ends the redo.
    fn remember(&mut self) {
        if self.undo.is_none() {
            self.undo = Some(self.text.clone());
        }
        self.redo = None;
    }

    /// Put back the text the box held before its first edit, selected, and
    /// keep the typed text for a redo.
    fn undo(&mut self) -> bool {
        let Some(before) = self.undo.take_if(|before| *before != self.text) else {
            return false;
        };
        let typed = std::mem::replace(&mut self.text, before);
        self.anchor = 0;
        self.caret = self.char_count();
        self.redo = Some(typed);
        true
    }

    /// Put back the text an undo replaced, with the caret after it.
    fn redo(&mut self) -> bool {
        let Some(typed) = self.redo.take() else {
            return false;
        };
        self.undo = Some(std::mem::replace(&mut self.text, typed));
        self.caret = self.char_count();
        self.anchor = self.caret;
        true
    }
}

impl std::ops::Deref for LineText {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for LineText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl From<String> for LineText {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<&str> for LineText {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl PartialEq<str> for LineText {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl PartialEq<&str> for LineText {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}
