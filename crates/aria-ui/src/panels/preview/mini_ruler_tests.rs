//! The Viewer strip's markers paint in work bounded by the strip's width.
//!
//! The class closed here is a strip whose drawing grows with the number of
//! markers: one element and one quad per marker, which a project with hundreds
//! of markers pays for on every frame the Viewer is built or replayed. The runs
//! are checked against the drawing they replace, column by column: each pixel
//! column shows the colour of the last point marker under it, and no column is
//! painted twice. Not caught: range markers, which still paint one quad each.

use std::collections::BTreeMap;

use aria_models::{MarkerColor, MarkerKind};

use super::{flag_runs, fraction};
use crate::types::UiMarker;

fn marker(id: u64, tick: i64, duration_ticks: i64, color: MarkerColor) -> UiMarker {
    UiMarker {
        id,
        tick,
        duration_ticks,
        name: "M".into(),
        color,
        comment: "".into(),
        kind: MarkerKind::Comment,
        clip_id: None,
        clip_tick: None,
        clip_duration_ticks: None,
        clip_rate: None,
    }
}

/// The columns `flag_runs` paints, each with its colour. Fails on a column
/// painted twice.
fn drawn(
    markers: &[UiMarker],
    last: i64,
    left: f32,
    width: f32,
) -> (usize, BTreeMap<i32, MarkerColor>) {
    let mut runs = 0;
    let mut columns = BTreeMap::new();
    flag_runs(markers, last, left, width, |x, w, color| {
        runs += 1;
        assert_eq!(x.fract(), 0.0, "a run starts on a pixel column");
        for column in x as i32..(x + w) as i32 {
            assert!(
                columns.insert(column, color).is_none(),
                "column {column} painted twice"
            );
        }
    });
    (runs, columns)
}

/// What one flag per marker showed: the colour of the last point marker in
/// each column.
fn expected(markers: &[UiMarker], last: i64, left: f32, width: f32) -> BTreeMap<i32, MarkerColor> {
    let mut columns = BTreeMap::new();
    for marker in markers.iter().filter(|marker| marker.duration_ticks <= 0) {
        let column = (left + fraction(marker.tick, last) * width).floor() as i32;
        columns.insert(column, marker.color);
    }
    columns
}

#[test]
fn many_markers_paint_one_run_per_colour_change_and_show_the_top_flag() {
    let (last, left, width) = (30_000, 37.0, 412.0);
    // Three markers to a column, cycling through the palette, with a range
    // marker among them that paints elsewhere.
    let mut markers: Vec<UiMarker> = (0..5_000)
        .map(|index| {
            marker(
                index,
                index as i64 * 6,
                0,
                MarkerColor::cycle(index as usize),
            )
        })
        .collect();
    markers.insert(2_000, marker(9_999, 12_000, 600, MarkerColor::Red));
    let (runs, columns) = drawn(&markers, last, left, width);
    assert_eq!(columns, expected(&markers, last, left, width));
    assert!(
        runs <= width as usize + 1,
        "{runs} runs for a strip {width} px wide"
    );
}

#[test]
fn neighbouring_columns_of_one_colour_paint_as_one_run() {
    let (last, left, width) = (1_000, 0.0, 100.0);
    let markers: Vec<UiMarker> = (0..1_000)
        .map(|tick| marker(tick as u64, tick, 0, MarkerColor::Teal))
        .collect();
    let (runs, columns) = drawn(&markers, last, left, width);
    assert_eq!(columns, expected(&markers, last, left, width));
    assert_eq!(runs, 1, "one colour across touching columns is one quad");
}

#[test]
fn a_column_change_of_colour_starts_a_run_and_a_gap_ends_one() {
    // A strip as wide in pixels as it is long in ticks, at a length whose
    // fractions are exact in binary, so each tick is its own column.
    let (last, left, width) = (128, 0.0, 128.0);
    let markers = [
        marker(1, 10, 0, MarkerColor::Blue),
        marker(2, 11, 0, MarkerColor::Blue),
        marker(3, 12, 0, MarkerColor::Green),
        marker(4, 12, 0, MarkerColor::Blue),
        marker(5, 20, 0, MarkerColor::Blue),
    ];
    let mut runs = Vec::new();
    flag_runs(&markers, last, left, width, |x, w, color| {
        runs.push((x, w, color))
    });
    assert_eq!(
        runs,
        vec![
            (10.0, 3.0, MarkerColor::Blue),
            (20.0, 1.0, MarkerColor::Blue)
        ],
        "the last marker in column 12 is Blue, so 10..=12 is one run, and 20 stands alone"
    );
}

#[test]
fn no_point_markers_paint_no_runs() {
    let markers = [marker(1, 10, 40, MarkerColor::Pink)];
    let (runs, columns) = drawn(&markers, 100, 0.0, 100.0);
    assert_eq!((runs, columns.len()), (0, 0));
    let (runs, _) = drawn(&[], 100, 0.0, 100.0);
    assert_eq!(runs, 0);
}
