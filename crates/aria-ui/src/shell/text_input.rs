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
//! Every box reports its selection and replaces the range asked for through
//! its own row, which keeps the box's own rules: typed text goes over the
//! selection, and a preedit run is marked where the caret is.

use std::ops::Range;

use gpui::{Bounds, Context, EntityInputHandler, Pixels, UTF16Selection, Window};

use super::Shell;
use crate::keys::fields::{self, TextField};

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

/// How many characters of `text` lie before the UTF-16 offset `offset`.
fn chars_before_utf16(text: &str, offset: usize) -> usize {
    text[..byte_of_utf16(text, offset)].chars().count()
}

/// The UTF-16 offset of character `index` in `text`, clamped to its end.
fn utf16_of_char(text: &str, index: usize) -> usize {
    text.chars().take(index).map(char::len_utf16).sum()
}

impl Shell {
    /// The focused box, its text, and its `(anchor, caret)` in characters.
    fn focused_field(&self, cx: &Context<Self>) -> Option<(TextField, String, (usize, usize))> {
        let state = self.state.read(cx);
        let field = fields::focused(state)?;
        Some((
            field,
            field.text(state).into_owned(),
            field.selection(state),
        ))
    }

    /// The focused box's text.
    fn field_text(&self, cx: &Context<Self>) -> Option<String> {
        self.focused_field(cx).map(|(_, text, _)| text)
    }

    /// Replace `range` (UTF-16 offsets) with `text` in the focused box. `None`
    /// means the run the IME marked, and with nothing marked, the selection.
    ///
    /// Returns how many characters the box took, which is the run a preedit
    /// marks: a box that refused a character (the clock refuses letters, a
    /// name stops at its cap) is never asked to replace one it does not hold.
    fn replace_field_text(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> usize {
        let Some((field, current, (anchor, caret))) = self.focused_field(cx) else {
            return 0;
        };
        let range = match (range, self.ime_marked) {
            (Some(range), _) => {
                let start = chars_before_utf16(&current, range.start);
                let end = chars_before_utf16(&current, range.end);
                start.min(end)..start.max(end)
            }
            (None, Some(marked)) => caret.saturating_sub(marked)..caret,
            (None, None) => anchor.min(caret)..anchor.max(caret),
        };
        let start = range.start;
        let taken = self.state.update(cx, |state, _| {
            if field.replace(state, range, text) {
                field.selection(state).1.saturating_sub(start)
            } else {
                0
            }
        });
        if field.redraws() {
            crate::app::spawn_preview(self.state.clone(), cx);
        }
        cx.notify();
        taken
    }

    /// The focused box's `(anchor, caret)` as UTF-16 offsets, and its text.
    fn caret_utf16(&self, cx: &Context<Self>) -> Option<(usize, usize, String)> {
        let (_, text, (anchor, at)) = self.focused_field(cx)?;
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
        let (anchor, at, _) = self.caret_utf16(cx)?;
        Some(UTF16Selection {
            range: anchor.min(at)..anchor.max(at),
            reversed: at < anchor,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let marked = self.ime_marked?;
        // The marked run ends at the caret.
        let (_, end, text) = self.caret_utf16(cx)?;
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
        self.replace_field_text(range, text, cx);
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
        // What the box took is what may be replaced next.
        self.ime_marked = Some(self.replace_field_text(range, new_text, cx));
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
