//! Caret stops against the layout the glyphs are drawn from.
//!
//! Closes the class "a caret drawn where the text is not": a stop missing for
//! a character boundary, a stop inside a multi-byte character, stops out of
//! order along a line, a line with no stop, and a stop space that does not
//! line up with the bitmap the compositor draws. Each case skips on a host
//! with no fonts, as the rest of the crate's shaping tests do. It does not
//! cover right-to-left runs, whose stops lead at the far edge; no shipped
//! font set guarantees an RTL face on every host.

use super::*;
use crate::{TextRenderer, TextRun, TextStyle};

fn renderer() -> Option<TextRenderer> {
    let renderer = TextRenderer::new();
    (renderer.font_count() > 0).then_some(renderer)
}

fn bytes_on(stops: &[CaretStop], line: usize) -> Vec<usize> {
    stops
        .iter()
        .filter(|stop| stop.line == line)
        .map(|stop| stop.byte)
        .collect()
}

#[test]
fn every_character_boundary_has_one_stop_in_order() {
    let Some(mut text) = renderer() else {
        return;
    };
    let stops = text.carets("Abc", &TextStyle::new(48.0));
    assert_eq!(bytes_on(&stops, 0), vec![0, 1, 2, 3]);
    for pair in stops.windows(2) {
        assert!(
            pair[1].x > pair[0].x,
            "stops run left to right along the line: {stops:?}"
        );
    }
    let extent = text.shape("Abc", &TextStyle::new(48.0)).extent;
    let last = stops.last().expect("a stop").x;
    assert!(
        (last - extent.0 as f32).abs() < 8.0,
        "the end stop sits at the ink's right edge: {last} against {}",
        extent.0
    );
}

#[test]
fn a_stop_never_falls_inside_a_character() {
    let Some(mut text) = renderer() else {
        return;
    };
    let content = "é€x";
    let stops = text.carets(content, &TextStyle::new(48.0));
    let bytes = bytes_on(&stops, 0);
    assert_eq!(bytes, vec![0, 2, 5, 6]);
    assert!(bytes.iter().all(|&byte| content.is_char_boundary(byte)));
}

#[test]
fn each_paragraph_is_its_own_line_and_an_empty_one_still_has_a_stop() {
    let Some(mut text) = renderer() else {
        return;
    };
    let stops = text.carets("ab\n\ncd", &TextStyle::new(40.0));
    assert_eq!(bytes_on(&stops, 0), vec![0, 1, 2]);
    assert_eq!(bytes_on(&stops, 1), vec![3]);
    assert_eq!(bytes_on(&stops, 2), vec![4, 5, 6]);
    let top = |line: usize| {
        stops
            .iter()
            .find(|stop| stop.line == line)
            .expect("a stop on the line")
    };
    assert!(top(1).top >= top(0).bottom - 0.5, "lines stack downward");
    assert!(top(2).top >= top(1).bottom - 0.5, "lines stack downward");
}

#[test]
fn a_point_picks_the_nearest_boundary_on_its_own_line() {
    let Some(mut text) = renderer() else {
        return;
    };
    let stops = text.carets("ab\ncd", &TextStyle::new(40.0));
    let on = |byte: usize| {
        *stops
            .iter()
            .find(|stop| stop.byte == byte)
            .expect("the stop")
    };
    let (one, two) = (on(1), on(2));
    let mid_y = (one.top + one.bottom) * 0.5;
    // Just right of the midpoint between `a|b` and `ab|` lands on `ab|`.
    let x = (one.x + two.x) * 0.5 + 1.0;
    assert_eq!(caret_at(&stops, [x, mid_y]).map(|stop| stop.byte), Some(2));
    let x = (one.x + two.x) * 0.5 - 1.0;
    assert_eq!(caret_at(&stops, [x, mid_y]).map(|stop| stop.byte), Some(1));
    // The same x one line down is on the second paragraph.
    let four = on(4);
    let low_y = (four.top + four.bottom) * 0.5;
    assert_eq!(
        caret_at(&stops, [four.x, low_y]).map(|stop| stop.byte),
        Some(4)
    );
    // Far below every line still answers with the last line.
    assert_eq!(
        caret_at(&stops, [four.x, low_y + 500.0]).map(|stop| stop.line),
        Some(1)
    );
    assert!(caret_at(&[], [0.0, 0.0]).is_none());
}

#[test]
fn a_larger_run_pushes_the_stops_after_it() {
    let Some(mut text) = renderer() else {
        return;
    };
    let base = TextStyle::new(36.0);
    let plain = text.carets("AB", &base);
    let styled = text.carets(
        "AB",
        &base.clone().with_runs(vec![TextRun {
            range: 0..1,
            font_size: Some(96.0),
            ..TextRun::default()
        }]),
    );
    let width = |stops: &[CaretStop]| stops[1].x - stops[0].x;
    assert!(
        width(&styled) > width(&plain) * 1.8,
        "a 96 px A is wider than a 36 px one: {} against {}",
        width(&styled),
        width(&plain)
    );
}

#[test]
fn stops_line_up_with_the_painted_bitmap() {
    let Some(mut text) = renderer() else {
        return;
    };
    // A wide stroke grows the bitmap's margin, so a stop that ignored it would
    // sit left of the glyph it bounds.
    let style = TextStyle::new(64.0).with_stroke(crate::TextStroke {
        rgba: [0, 0, 0, 255],
        width: 12.0,
    });
    let image = text.rasterize("I", &style);
    let pad = raster_padding(&style) as f32;
    assert!(pad >= 12.0, "the stroke widens the margin: {pad}");
    let stops = text.carets("I", &style);
    let (left, right) = (stops[0].x + pad, stops[1].x + pad);
    let row = image.height / 2;
    let fill = (0..image.width)
        .filter(|&x| {
            let i = ((row * image.width + x) * 4) as usize;
            image.pixels[i] > 200 && image.pixels[i + 3] == 255
        })
        .collect::<Vec<_>>();
    let (first, last) = (
        *fill.first().expect("white fill on the middle row") as f32,
        *fill.last().expect("white fill") as f32,
    );
    assert!(
        left <= first + 1.0 && right >= last - 1.0,
        "the fill of `I` lies between its stops: {left}..{right} against {first}..{last}"
    );
}

/// Every stop is in the ink box's frame, the first one included: the same
/// glyph three times is three equal cells, the first starting at the pen
/// origin left of its ink rather than at the ink.
#[test]
fn a_repeated_glyph_gives_equal_cells_from_the_pen_origin() {
    let Some(mut text) = renderer() else {
        return;
    };
    let stops = text.carets("III", &TextStyle::new(96.0));
    assert_eq!(stops.len(), 4, "{stops:?}");
    let cells: Vec<f32> = stops.windows(2).map(|pair| pair[1].x - pair[0].x).collect();
    assert!(cells[0] > 1.0, "{cells:?}");
    assert!(
        cells.iter().all(|cell| (cell - cells[0]).abs() < 0.5),
        "unequal cells: {cells:?}"
    );
    assert!(
        stops[0].x < 0.0,
        "the pen starts left of the ink: {stops:?}"
    );
}

#[test]
fn carets_share_the_shaping_memo() {
    let Some(mut text) = renderer() else {
        return;
    };
    let style = TextStyle::new(30.0);
    let first = text.carets("memo", &style);
    let (shaped, _) = text.memo_sizes();
    assert_eq!(shaped, 1, "a caret miss shapes once and keeps the run");
    let _ = text.shape("memo", &style);
    assert_eq!(
        text.memo_sizes().0,
        1,
        "the shape is the one the caret took"
    );
    assert_eq!(text.carets("memo", &style), first);
}
