//! The plate a Markers row's controls stand on.
//!
//! The class closed here: a plate over a row painted in the row's translucent
//! fill, which composites the wash twice and reads as a darker box behind the
//! controls. Every row state is enumerated: rest and playhead rows, even and
//! odd bands, the hover fade from its first frames to the whole, in both
//! appearances, with the plate at the opacity the fade draws it at.
//!
//! Not covered: a draw that paints the plate from something other than
//! `row_fills`. `app/dock_panels_tests.rs` places the plate inside its row;
//! no test reads painted colors.

use super::row_fills;
use crate::theme::{Appearance, Theme, flatten};

/// The largest channel difference between two colors, in 0..1.
fn apart(a: gpui::Hsla, b: gpui::Hsla) -> f32 {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    [a.r - b.r, a.g - b.g, a.b - b.b]
        .into_iter()
        .map(f32::abs)
        .fold(0.0, f32::max)
}

#[test]
fn the_controls_plate_reads_as_its_row() {
    for appearance in [Appearance::Dark, Appearance::Light] {
        let theme = Theme::for_appearance(appearance);
        for at_playhead in [false, true] {
            for index in [0, 1] {
                for lit in [0.25, 0.5, 1.0] {
                    let (fill, plate) = row_fills(&theme, at_playhead, index, lit);
                    let row = flatten(fill, theme.bg);
                    let seen = flatten(plate.opacity(lit), row);
                    assert!(
                        apart(seen, row) < 1.0 / 255.0,
                        "{appearance:?} row (playhead {at_playhead}, index {index}, lit {lit}): \
                         the plate reads {seen:?} over a row of {row:?}"
                    );
                }
            }
        }
    }
}
