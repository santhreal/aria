//! Character boundaries and words for the Type tool's caret: which boundary a
//! caret index draws on, the boundary nearest a point, the line above or below,
//! and the word edges the Ctrl+arrow keys move to. Pure arithmetic over canvas
//! pixels and character counts, testable without a window.

use crate::app::selected_detail::TitleCaret;

/// The byte offset of character `index` in `text`, its length past the end.
pub(in crate::app) fn byte_at(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(at, _)| at)
}

/// The boundary a caret at `index` is drawn on: the last stop for it, which is
/// the start of the next line at a wrap, or the nearest stop before it.
pub(super) fn caret_stop(carets: &[TitleCaret], index: usize) -> Option<&TitleCaret> {
    carets
        .iter()
        .rev()
        .find(|stop| stop.index == index)
        .or_else(|| {
            carets
                .iter()
                .filter(|stop| stop.index <= index)
                .max_by_key(|stop| stop.index)
        })
}

/// The boundary on the line above (`down` false) or below the caret at
/// `index`, nearest across. `None` when there is no such line.
pub(super) fn vertical(carets: &[TitleCaret], index: usize, down: bool) -> Option<usize> {
    let from = caret_stop(carets, index)?;
    let line = if down {
        from.line.checked_add(1)?
    } else {
        from.line.checked_sub(1)?
    };
    let middle = midpoint(from);
    carets
        .iter()
        .filter(|stop| stop.line == line)
        .min_by(|a, b| length(midpoint(a), middle).total_cmp(&length(midpoint(b), middle)))
        .map(|stop| stop.index)
}

/// The boundary nearest `point` and its distance, both in canvas pixels.
pub(super) fn nearest(carets: &[TitleCaret], point: [f32; 2]) -> Option<(&TitleCaret, f32)> {
    carets
        .iter()
        .map(|caret| (caret, segment_distance(point, caret.top, caret.bottom)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

pub(super) fn midpoint(caret: &TitleCaret) -> [f32; 2] {
    [
        0.5 * (caret.top[0] + caret.bottom[0]),
        0.5 * (caret.top[1] + caret.bottom[1]),
    ]
}

pub(super) fn length(a: [f32; 2], b: [f32; 2]) -> f32 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// The distance from `point` to the segment `a`..`b`.
pub(super) fn segment_distance(point: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let span = dx * dx + dy * dy;
    let t = if span > 0.0 {
        (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    length(point, [a[0] + t * dx, a[1] + t * dy])
}

/// The start of the word before character `index`.
pub(in crate::app) fn word_left(text: &str, index: usize) -> usize {
    let mut chars = text[..byte_at(text, index)].chars().rev().peekable();
    let mut moved = 0;
    while chars.next_if(|ch| ch.is_whitespace()).is_some() {
        moved += 1;
    }
    while chars.next_if(|ch| !ch.is_whitespace()).is_some() {
        moved += 1;
    }
    index - moved
}

/// The start of the word after character `index`.
pub(in crate::app) fn word_right(text: &str, index: usize) -> usize {
    let mut chars = text[byte_at(text, index)..].chars().peekable();
    let mut moved = 0;
    while chars.next_if(|ch| !ch.is_whitespace()).is_some() {
        moved += 1;
    }
    while chars.next_if(|ch| ch.is_whitespace()).is_some() {
        moved += 1;
    }
    index + moved
}
