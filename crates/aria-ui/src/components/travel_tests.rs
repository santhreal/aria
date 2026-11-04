//! Every value track's rail stands off the surface it is drawn over.
//!
//! The class closed here: a rail drawn in a tone so near the panel's that a
//! track at its origin, which fills nothing, drew only its thumb, so a column
//! of grade sliders at zero read as a column of floating dots. The rail was the
//! splitter tone, about 1.1:1 off every surface in both appearances. [`Ink`]
//! is the one place a rail's color is set, and its fields are private, so every
//! track, the Effect Controls row, the mixer strip and the plugin slider, is
//! held to this: in both appearances and on every surface a panel or a popover
//! is drawn on, the rail at rest stands off the surface at least as far as a row
//! separator, the faintest rule drawn to be seen, and under the pointer farther
//! than at rest.
//!
//! Not covered: a theme file's own token values, which its author sets.

use super::Ink;
use crate::theme::{Theme, contrast_ratio, flatten};

#[test]
fn a_rail_stands_off_every_surface_at_least_as_far_as_a_row_separator() {
    let mut faults = Vec::new();
    for theme in [Theme::dark(), Theme::light()] {
        let rest = Ink::new(&theme, theme.accent, 0.0).rail;
        let lifted = Ink::new(&theme, theme.accent, 1.0).rail;
        for (name, surface) in [
            ("bg", theme.bg),
            ("surface", theme.surface),
            ("surface_header", theme.surface_header),
            ("surface_raised", theme.surface_raised),
            ("surface_card", theme.surface_card),
            ("surface_dialog", theme.surface_dialog),
            ("surface_overlay", theme.surface_overlay),
        ] {
            let seen = |ink| contrast_ratio(flatten(ink, surface), surface);
            let (rail, hover, rule) = (seen(rest), seen(lifted), seen(theme.hairline));
            if rail < rule {
                faults.push(format!(
                    "{:?} on {name}: the rail stands {rail:.3}:1 off, a row separator {rule:.3}:1",
                    theme.appearance
                ));
            }
            if hover <= rail {
                faults.push(format!(
                    "{:?} on {name}: the rail under the pointer stands {hover:.3}:1 off, at rest {rail:.3}:1",
                    theme.appearance
                ));
            }
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}
