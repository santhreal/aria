//! A title typed into in the Viewer with the Type tool.
//!
//! A press on a title places the caret at the character boundary nearest the
//! pointer, a drag selects, and Shift+press extends the selection. The
//! boundaries are the projected detail's [`TitleCaret`]s, which the renderer
//! measures from the layout it places the glyphs from, so the caret sits
//! between the glyphs drawn, through kerning, tracking, wrapping and per-range
//! sizes.
//!
//! Keys reach the edit through the `aria.field.title` row of
//! [`crate::keys::fields`]: typing replaces the selection, Backspace and Delete
//! remove it or one character, and the arrows, Home and End move the caret,
//! with Shift extending the selection. Each burst of typing is one undo step:
//! a keystroke that lands right after the previous one folds into it, and a
//! caret move ends the burst.
//!
//! The edit is live only while the Type tool is on and the title it names is
//! selected. A stale edit is inert rather than cleared, so changing tool or
//! selection needs no hook here.

use std::ops::Range;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, ClipSource, Generator};

use crate::app::AppState;
use crate::app::plugin_overlays::CanvasMap;
use crate::app::selected_detail::TitleCaret;
use crate::keys::fields::CaretKey;

#[path = "text_caret_geometry.rs"]
pub(in crate::app) mod geometry;
use geometry::*;

/// How far outside a title's line, in window pixels, a press still lands on
/// the title rather than creating a new one.
const REACH: f32 = 6.0;

/// The caret and selection in a title being typed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextEdit {
    /// The title.
    pub clip: ClipId,
    /// Characters of the title's text before the caret.
    pub caret: usize,
    /// The fixed end of the selection; the caret when nothing is selected.
    pub anchor: usize,
    /// Whether the press that placed the caret is held: a move selects.
    pub dragging: bool,
    /// The history depth the last typed edit left, so the next keystroke of
    /// the same burst folds into that undo step.
    pub burst: Option<usize>,
}

impl TextEdit {
    /// A caret at `index` in `clip`, with nothing selected.
    pub fn at(clip: ClipId, index: usize) -> Self {
        Self {
            clip,
            caret: index,
            anchor: index,
            dragging: false,
            burst: None,
        }
    }

    /// The selected characters, start first. Empty when nothing is selected.
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.caret)..self.anchor.max(self.caret)
    }
}

/// The caret and the selection as the Viewer draws them, in window pixels.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrawnTextCaret {
    /// The caret, from the top of its line to the bottom.
    pub caret: Option<[[f32; 2]; 2]>,
    /// One quad per selected character cell, corners in drawing order.
    pub selection: Vec<[[f32; 2]; 4]>,
}

impl AppState {
    /// The edit in flight, when the Type tool is on and its title is selected.
    fn text_edit(&self) -> Option<&TextEdit> {
        let edit = self.tool_session.text_edit.as_ref()?;
        (self.tool.types() && self.selected_clip() == Some(edit.clip) && self.selected_is_title())
            .then_some(edit)
    }

    /// Whether a title is being typed into in the Viewer.
    pub fn text_editing(&self) -> bool {
        self.text_edit().is_some()
    }

    /// Whether the press that placed the caret is held.
    pub fn text_dragging(&self) -> bool {
        self.text_edit().is_some_and(|edit| edit.dragging)
    }

    /// The selection in the title being typed into, `(anchor, caret)` in
    /// characters. `(0, 0)` with no edit live.
    pub fn text_selection(&self) -> (usize, usize) {
        self.text_edit()
            .map_or((0, 0), |edit| (edit.anchor, edit.caret))
    }

    /// The selected characters of the title being typed into, `None` with no
    /// edit live or nothing selected. The Character panel styles this range.
    pub fn text_style_range(&self) -> Option<Range<usize>> {
        self.text_edit()
            .map(TextEdit::range)
            .filter(|range| !range.is_empty())
    }

    /// The selected title's text.
    fn title_text(&self) -> Option<&str> {
        match &self.selected_model()?.content {
            ClipSource::Generated(Generator::Text { content, .. }) => Some(content),
            _ => None,
        }
    }

    /// The text of the title being typed into, empty with no edit live.
    pub fn edited_text(&self) -> String {
        self.text_edit()
            .and_then(|_| self.title_text())
            .map(str::to_owned)
            .unwrap_or_default()
    }

    /// The selected title's text selected in the Viewer, `None` with nothing
    /// selected.
    pub fn edited_selection_text(&self) -> Option<String> {
        let range = self.text_style_range()?;
        let text = self.title_text()?;
        Some(text[byte_at(text, range.start)..byte_at(text, range.end)].to_owned())
    }

    fn title_len(&self) -> usize {
        self.title_text().map_or(0, |text| text.chars().count())
    }

    /// The selected title's character boundaries at the playhead.
    fn title_carets(&self) -> &[TitleCaret] {
        self.selected
            .as_ref()
            .and_then(|detail| detail.carets.as_deref())
            .unwrap_or(&[])
    }

    /// Canvas pixels to window pixels and back for the Viewer as last drawn.
    fn title_map(&self) -> Option<CanvasMap> {
        CanvasMap::new(self.overlay_frame.canvas?, self.canvas_size())
    }

    /// The boundary nearest a window point, wherever it is.
    fn nearest_caret(&self, point: [f32; 2]) -> Option<usize> {
        let at = self.title_map()?.to_canvas(point);
        nearest(self.title_carets(), at).map(|(caret, _)| caret.index)
    }

    /// The boundary nearest a window point that lands on the selected title:
    /// within half a line of a boundary, and [`REACH`] beyond.
    fn caret_near(&self, point: [f32; 2]) -> Option<usize> {
        let map = self.title_map()?;
        let (caret, distance) = nearest(self.title_carets(), map.to_canvas(point))?;
        let half = 0.5 * length(caret.top, caret.bottom);
        let reach = REACH / map.scale()[0].max(f32::EPSILON);
        (distance <= half + reach).then_some(caret.index)
    }

    /// A Type tool press at a window point inside the picture.
    ///
    /// With Shift on a title being typed into, the selection extends to the
    /// boundary under the pointer. On the selected title, or on any title
    /// drawn there, the caret goes to the nearest boundary and the press starts
    /// a selection drag. Anywhere else a new title is created there, with its
    /// placeholder text selected so typing replaces it.
    ///
    /// Returns whether anything changed.
    pub fn press_type_at(&mut self, point: [f32; 2], extend: bool) -> bool {
        if extend
            && let Some(edit) = self.text_edit().copied()
            && let Some(index) = self.nearest_caret(point)
        {
            self.tool_session.text_edit = Some(TextEdit {
                caret: index,
                dragging: true,
                burst: None,
                ..edit
            });
            return true;
        }
        if self.selected_is_title()
            && let Some(clip) = self.selected_clip()
            && let Some(index) = self.caret_near(point)
        {
            self.start_text_edit(clip, index);
            return true;
        }
        let title = self.clip_at_canvas_point(point).filter(|&clip| {
            self.engine.lock().project().clip(clip).is_some_and(|held| {
                matches!(held.content, ClipSource::Generated(Generator::Text { .. }))
            })
        });
        if let Some(clip) = title {
            if self.selected_clip() != Some(clip) {
                self.select_clip(clip);
            }
            let index = self
                .nearest_caret(point)
                .unwrap_or_else(|| self.title_len());
            self.start_text_edit(clip, index);
            return true;
        }
        match self.create_text_at(point) {
            Ok(_) => true,
            Err(err) => {
                self.report_error(&err);
                false
            }
        }
    }

    fn start_text_edit(&mut self, clip: ClipId, index: usize) {
        self.tool_session.text_edit = Some(TextEdit {
            dragging: true,
            ..TextEdit::at(clip, index)
        });
    }

    /// Move the held caret to the boundary nearest a window point, selecting
    /// from where the press landed. Returns whether the caret moved.
    pub fn drag_text_caret(&mut self, point: [f32; 2]) -> bool {
        if !self.text_dragging() {
            return false;
        }
        let Some(index) = self.nearest_caret(point) else {
            return false;
        };
        match self.tool_session.text_edit.as_mut() {
            Some(edit) if edit.caret != index => {
                edit.caret = index;
                true
            }
            _ => false,
        }
    }

    /// Release the press that placed the caret.
    pub fn end_text_drag(&mut self) {
        if let Some(edit) = self.tool_session.text_edit.as_mut() {
            edit.dragging = false;
        }
    }

    /// Stop typing into the title. The title stays selected.
    pub fn end_text_edit(&mut self) {
        self.tool_session.text_edit = None;
    }

    /// Replace characters `range` of the title being typed into with `text`,
    /// as one keystroke of a typing burst. Control characters other than a
    /// line break are dropped.
    pub fn replace_title_text(&mut self, range: Range<usize>, text: &str) {
        let text: String = text
            .chars()
            .filter(|ch| *ch == '\n' || !ch.is_control())
            .collect();
        if let Err(err) = self.edit_title(range, &text) {
            self.report_error(&err);
        }
    }

    /// Type one character over the selection.
    pub fn title_push(&mut self, ch: char) {
        let range = self.text_edit().map(TextEdit::range);
        if let Some(range) = range {
            self.replace_title_text(range, ch.encode_utf8(&mut [0; 4]));
        }
    }

    /// Backspace: the selection, or the character before the caret.
    pub fn title_backspace(&mut self) {
        let Some(edit) = self.text_edit().copied() else {
            return;
        };
        let range = if edit.anchor == edit.caret {
            self.step(edit.caret, false)..edit.caret
        } else {
            edit.range()
        };
        self.replace_title_text(range, "");
    }

    /// Move, select, delete forward, or step history in the title being typed
    /// into. `extend` is Shift held: the caret moves and the anchor stays.
    pub fn title_caret_key(&mut self, key: CaretKey, extend: bool) {
        let Some(edit) = self.text_edit().copied() else {
            return;
        };
        match key {
            CaretKey::Delete => {
                let range = if edit.anchor == edit.caret {
                    edit.caret..self.step(edit.caret, true)
                } else {
                    edit.range()
                };
                self.replace_title_text(range, "");
            }
            CaretKey::Undo | CaretKey::Redo => {
                if key == CaretKey::Undo {
                    self.undo();
                } else {
                    self.redo();
                }
                self.clamp_text_edit();
            }
            _ => {
                let (anchor, caret) = self.moved(&edit, key, extend);
                if let Some(held) = self.tool_session.text_edit.as_mut() {
                    held.anchor = anchor;
                    held.caret = caret;
                    held.burst = None;
                }
            }
        }
    }

    /// Hold the caret inside the title after its text changed under it, and
    /// drop the edit when the title went away.
    fn clamp_text_edit(&mut self) {
        if !self.selected_is_title() {
            self.tool_session.text_edit = None;
            return;
        }
        let len = self.title_len();
        if let Some(edit) = self.tool_session.text_edit.as_mut() {
            edit.caret = edit.caret.min(len);
            edit.anchor = edit.anchor.min(len);
            edit.burst = None;
        }
    }

    /// The boundary one stop before (`forward` false) or after `index`. A
    /// boundary the layout does not draw, inside a cluster, is skipped.
    fn step(&self, index: usize, forward: bool) -> usize {
        let carets = self.title_carets().iter().map(|caret| caret.index);
        let len = self.title_len();
        let next = if forward {
            carets.filter(|&at| at > index).min()
        } else {
            carets.filter(|&at| at < index).max()
        };
        next.unwrap_or(if forward {
            (index + 1).min(len)
        } else {
            index.saturating_sub(1)
        })
        .min(len)
    }

    /// Where a movement key leaves `(anchor, caret)`.
    fn moved(&self, edit: &TextEdit, key: CaretKey, extend: bool) -> (usize, usize) {
        let text = self.title_text().unwrap_or_default();
        let len = text.chars().count();
        let carets = self.title_carets();
        let collapse = !extend && edit.anchor != edit.caret;
        let line = caret_stop(carets, edit.caret).map(|stop| stop.line);
        let on_line = |line: Option<usize>| {
            carets
                .iter()
                .filter(move |stop| Some(stop.line) == line)
                .map(|stop| stop.index)
        };
        let caret = match key {
            CaretKey::SelectAll => return (0, len),
            CaretKey::Left if collapse => edit.range().start,
            CaretKey::Right if collapse => edit.range().end,
            CaretKey::Left => self.step(edit.caret, false),
            CaretKey::Right => self.step(edit.caret, true),
            CaretKey::WordLeft => word_left(text, edit.caret),
            CaretKey::WordRight => word_right(text, edit.caret),
            CaretKey::LineStart => on_line(line).min().unwrap_or(0),
            CaretKey::LineEnd => on_line(line).max().unwrap_or(len),
            CaretKey::TextStart => 0,
            CaretKey::TextEnd => len,
            CaretKey::Up => vertical(carets, edit.caret, false).unwrap_or(0),
            CaretKey::Down => vertical(carets, edit.caret, true).unwrap_or(len),
            CaretKey::Delete | CaretKey::Undo | CaretKey::Redo => edit.caret,
        }
        .min(len);
        (if extend { edit.anchor } else { caret }, caret)
    }

    /// Splice `insert` over characters `range` of the title being typed into,
    /// keeping its style runs on the characters they styled, and fold the
    /// edit into the burst before it.
    fn edit_title(&mut self, range: Range<usize>, insert: &str) -> Result<(), EngineError> {
        let Some(edit) = self.text_edit().copied() else {
            return Ok(());
        };
        let Some((clip, mut generator)) = self.selected_text_generator() else {
            return Ok(());
        };
        let Generator::Text { content, style } = &mut generator else {
            return Ok(());
        };
        let len = content.chars().count();
        let end = range.end.min(len);
        let start = range.start.min(end);
        if start == end && insert.is_empty() {
            return Ok(());
        }
        let span = byte_at(content, start)..byte_at(content, end);
        content.replace_range(span, insert);
        let inserted = insert.chars().count();
        style.adjust_runs_for_edit(start, end, inserted);
        self.apply_command(Command::Edit(EditCommand::SetGenerator { clip, generator }))?;
        let mut depth = self.engine.lock().history_depth();
        if edit.burst.is_some_and(|burst| burst + 1 == depth) {
            self.engine.lock().coalesce_last_two_undo_steps();
            self.sync_from_engine();
            depth -= 1;
        }
        if let Some(held) = self.tool_session.text_edit.as_mut() {
            held.caret = start + inserted;
            held.anchor = held.caret;
            held.burst = Some(depth);
        }
        Ok(())
    }

    /// The caret and the selection of the title being typed into, in window
    /// pixels. `None` with no edit live or no Viewer frame drawn.
    pub fn drawn_text_caret(&self) -> Option<DrawnTextCaret> {
        let edit = self.text_edit()?;
        let map = self.title_map()?;
        let carets = self.title_carets();
        let caret = caret_stop(carets, edit.caret)
            .map(|stop| [map.to_window(stop.top), map.to_window(stop.bottom)]);
        let range = edit.range();
        let selection = carets
            .windows(2)
            .filter(|pair| pair[0].line == pair[1].line)
            .filter(|pair| {
                let low = pair[0].index.min(pair[1].index);
                let high = pair[0].index.max(pair[1].index);
                low < high && range.start <= low && high <= range.end
            })
            .map(|pair| {
                [
                    map.to_window(pair[0].top),
                    map.to_window(pair[1].top),
                    map.to_window(pair[1].bottom),
                    map.to_window(pair[0].bottom),
                ]
            })
            .collect();
        Some(DrawnTextCaret { caret, selection })
    }
}

#[cfg(test)]
#[path = "text_caret_tests.rs"]
pub(in crate::app) mod tests;
