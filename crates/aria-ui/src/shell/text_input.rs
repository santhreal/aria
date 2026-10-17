//! The platform text-input handler.
//!
//! Every text box in the app is key-routed data (`keys::fields`): the key
//! handler takes its Backspace, Enter, Esc, arrows and paste, and this handler
//! takes its text. A plain keystroke arrives here as the character the layout
//! produced, so Shift+F is `F`, and a platform IME sends a run of text to
//! replace a range, after showing a preedit run it marked earlier. A Japanese or
//! Korean commit lands in whichever box holds focus without any box knowing an
//! IME exists.
//!
//! Most boxes append and pop, so a range is expressible only as a tail: the
//! caret is always at the end of the draft and nothing selects text with the
//! pointer. A replacement therefore pops the marked run and pushes the new one,
//! through each box's own `push`, which keeps that box's own rules.
//!
//! A box with a caret, the title typed into in the Viewer or the rename bar,
//! reports its selection and replaces the range asked for: typed text goes over
//! the selection, and a preedit run is marked where the caret is.

use std::ops::Range;

use gpui::{Bounds, Context, EntityInputHandler, Pixels, UTF16Selection, Window};

use super::Shell;
use crate::keys::fields;

/// Byte index of a UTF-16 offset, clamped to the string.
fn byte_of_utf16(text: &str, offset: usize) -> usize {
    let mut seen = 0usize;
    for (index, ch) in text.char_indices() {
        if seen >= offset {
            return index;
        }
        seen += ch.len_utf16();
    }
    text.len()
}

/// How many characters of `text` lie at or after the UTF-16 offset `from`.
fn chars_after_utf16(text: &str, from: usize) -> usize {
    text[byte_of_utf16(text, from)..].chars().count()
}

/// How many characters of `text` lie before the UTF-16 offset `offset`.
fn chars_before_utf16(text: &str, offset: usize) -> usize {
    text[..byte_of_utf16(text, offset)].chars().count()
}

/// The UTF-16 offset of character `index` in `text`, clamped to its end.
fn utf16_of_char(text: &str, index: usize) -> usize {
    text.chars().take(index).map(char::len_utf16).sum()
}

impl Shell {
    /// The text of the box holding focus, if any.
    fn field_text(&self, cx: &Context<Self>) -> Option<String> {
        let state = self.state.read(cx);
        fields::focused(state).map(|field| (field.text)(state))
    }

    /// Replace `range` (UTF-16 offsets) with `text`. A box without a caret
    /// replaces the tail from the range's start.
    ///
    /// `None` means "the range the IME already marked", and when nothing is
    /// marked the run is typed at the caret.
    fn replace_tail(&mut self, range: Option<Range<usize>>, text: &str, cx: &mut Context<Self>) {
        let Some(current) = self.field_text(cx) else {
            return;
        };
        if self.replace_at_caret(range.clone(), &current, text, cx) {
            return;
        }
        let drop = match range.map(|range| range.start) {
            Some(offset) => chars_after_utf16(&current, offset),
            None => self.ime_marked.unwrap_or(0),
        };
        self.state.update(cx, |state, _| {
            fields::truncate(state, drop);
            fields::insert(state, text);
        });
        cx.notify();
    }

    /// Replace `range` (UTF-16 offsets) in a box with a caret, or the marked
    /// run, or the selection when nothing is marked. `false` when the focused
    /// box has no caret.
    fn replace_at_caret(
        &mut self,
        range: Option<Range<usize>>,
        current: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let state = self.state.read(cx);
        let Some(caret) = fields::focused(state).and_then(|field| field.caret) else {
            return false;
        };
        let (anchor, at) = (caret.selection)(state);
        let range = match (range, self.ime_marked) {
            (Some(range), _) => {
                let start = chars_before_utf16(current, range.start);
                let end = chars_before_utf16(current, range.end);
                start.min(end)..start.max(end)
            }
            (None, Some(marked)) => at.saturating_sub(marked)..at,
            (None, None) => anchor.min(at)..anchor.max(at),
        };
        self.state
            .update(cx, |state, _| (caret.replace)(state, range, text));
        if caret.redraws {
            crate::app::spawn_preview(self.state.clone(), cx);
        }
        cx.notify();
        true
    }

    /// The focused box's caret as UTF-16 offsets `(anchor, caret)` and its
    /// text, `None` when the box has no caret.
    fn caret_utf16(&self, cx: &Context<Self>) -> Option<(usize, usize, String)> {
        let state = self.state.read(cx);
        let field = fields::focused(state)?;
        let (anchor, at) = (field.caret?.selection)(state);
        let text = (field.text)(state);
        Some((utf16_of_char(&text, anchor), utf16_of_char(&text, at), text))
    }
}

impl EntityInputHandler for Shell {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.field_text(cx)?;
        let start = byte_of_utf16(&text, range.start);
        let end = byte_of_utf16(&text, range.end).max(start);
        // The requested range is clamped to the draft, so the caller is told
        // what it actually got rather than being left to assume.
        let clamped = text[..start].encode_utf16().count()..text[..end].encode_utf16().count();
        if clamped != range {
            *adjusted = Some(clamped);
        }
        Some(text[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if let Some((anchor, at, _)) = self.caret_utf16(cx) {
            return Some(UTF16Selection {
                range: anchor.min(at)..anchor.max(at),
                reversed: at < anchor,
            });
        }
        let text = self.field_text(cx)?;
        let end = text.encode_utf16().count();
        // The caret is at the end: a box without one has no pointer selection
        // and no arrow-key caret, so a collapsed range there is the whole truth.
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let marked = self.ime_marked?;
        // The marked run ends at the caret in a box that has one, and at the
        // end of the text in every other box.
        let (text, end) = match self.caret_utf16(cx) {
            Some((_, at, text)) => (text, at),
            None => {
                let state = self.state.read(cx);
                let text = (fields::focused(state)?.text)(state);
                let end = text.encode_utf16().count();
                (text, end)
            }
        };
        let run: usize = text[..byte_of_utf16(&text, end)]
            .chars()
            .rev()
            .take(marked)
            .map(|ch| ch.len_utf16())
            .sum();
        Some(end.saturating_sub(run)..end)
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        // The preedit is accepted as typed; only the marking goes away.
        self.ime_marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_tail(range, text, cx);
        self.ime_marked = None;
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_tail(range, new_text, cx);
        // What the box actually took is what may be replaced next: a box that
        // refused a character (the clock refuses letters, a name stops at its
        // cap) must not be asked to pop one it never pushed.
        let landed = new_text.chars().filter(|ch| !ch.is_control()).count();
        let held = self
            .field_text(cx)
            .map(|text| text.chars().count())
            .unwrap_or(0);
        self.ime_marked = Some(landed.min(held));
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // A box draws its own text, and none of them reports a caret rectangle,
        // so the candidate window sits where the platform puts it by default.
        None
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        // Clicking into the middle of a draft is not a gesture the boxes have.
        None
    }

    fn text_length_utf16(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<usize> {
        self.field_text(cx).map(|text| text.encode_utf16().count())
    }

    fn accepts_text_input(&self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        fields::focused(self.state.read(cx)).is_some()
    }
}
