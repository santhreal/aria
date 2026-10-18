//! The accent families an [`aria_settings::ThemeChoice`] selects.
//!
//! Each family states its colours per appearance in OKLCH, with lightness and
//! chroma tuned for that appearance's surfaces. A hue swap on one shared colour
//! keeps neither perceived lightness nor contrast, so no family is derived
//! from another. The contrast rules every family meets are tested in
//! `theme_accent_tests.rs`.

use super::*;
use aria_settings::ThemeChoice;

/// The colours one accent family paints in one appearance.
pub(super) struct AccentTokens {
    pub accent: Hsla,
    pub accent_strong: Hsla,
    pub on_accent: Hsla,
    pub caret: Hsla,
    pub selection: Hsla,
    pub playhead: Hsla,
}

/// One family in one appearance: accent, strong fill, and caret as
/// `(l, c, h)`, the selection as `(l, c, h, alpha)`, and a playhead colour
/// when the playhead does not follow the accent.
struct Family {
    accent: (f32, f32, f32),
    strong: (f32, f32, f32),
    caret: (f32, f32, f32),
    selection: (f32, f32, f32, f32),
    playhead: Option<(f32, f32, f32)>,
}

const fn family(
    accent: (f32, f32, f32),
    strong: (f32, f32, f32),
    caret: (f32, f32, f32),
    selection: (f32, f32, f32, f32),
) -> Family {
    Family {
        accent,
        strong,
        caret,
        selection,
        playhead: None,
    }
}

fn family_for(choice: ThemeChoice, appearance: Appearance) -> Family {
    use Appearance::{Dark, Light};
    match (choice, appearance) {
        (ThemeChoice::Blue, Dark) => family(
            (0.71, 0.15, 253.0),
            (0.62, 0.17, 255.0),
            (0.80, 0.11, 253.0),
            (0.62, 0.15, 253.0, 0.35),
        ),
        (ThemeChoice::Blue, Light) => family(
            (0.47, 0.17, 257.0),
            (0.41, 0.17, 258.0),
            (0.45, 0.17, 257.0),
            (0.65, 0.14, 253.0, 0.28),
        ),
        (ThemeChoice::Ember, Dark) => family(
            (0.74, 0.15, 52.0),
            (0.66, 0.17, 45.0),
            (0.82, 0.11, 55.0),
            (0.66, 0.14, 52.0, 0.32),
        ),
        (ThemeChoice::Ember, Light) => family(
            (0.49, 0.15, 42.0),
            (0.43, 0.14, 40.0),
            (0.47, 0.15, 42.0),
            (0.72, 0.12, 55.0, 0.28),
        ),
        (ThemeChoice::Teal, Dark) => family(
            (0.77, 0.12, 190.0),
            (0.68, 0.12, 192.0),
            (0.84, 0.09, 190.0),
            (0.66, 0.11, 190.0, 0.32),
        ),
        (ThemeChoice::Teal, Light) => family(
            (0.45, 0.09, 202.0),
            (0.39, 0.08, 204.0),
            (0.43, 0.09, 202.0),
            (0.72, 0.09, 195.0, 0.28),
        ),
        // No hue: the accent is the neutral ramp's brightest step, and only
        // the playhead carries colour.
        (ThemeChoice::Graphite, Dark) => Family {
            playhead: Some((0.66, 0.21, 27.0)),
            ..family(
                (0.93, 0.0, 0.0),
                (0.82, 0.0, 0.0),
                (0.95, 0.0, 0.0),
                (0.80, 0.0, 0.0, 0.22),
            )
        },
        (ThemeChoice::Graphite, Light) => Family {
            playhead: Some((0.57, 0.21, 27.0)),
            ..family(
                (0.26, 0.0, 0.0),
                (0.20, 0.0, 0.0),
                (0.22, 0.0, 0.0),
                (0.40, 0.0, 0.0, 0.18),
            )
        },
    }
}

fn paint((l, c, h): (f32, f32, f32)) -> Hsla {
    oklch(l, c, h)
}

/// The accent colours `choice` paints in `appearance`.
pub(super) fn accent_tokens(choice: ThemeChoice, appearance: Appearance) -> AccentTokens {
    let family = family_for(choice, appearance);
    let accent = paint(family.accent);
    let (l, c, h, alpha) = family.selection;
    AccentTokens {
        accent,
        accent_strong: paint(family.strong),
        on_accent: Theme::ink_on(accent),
        caret: paint(family.caret),
        selection: oklch(l, c, h).opacity(alpha),
        playhead: family.playhead.map_or(accent, paint),
    }
}

impl Theme {
    /// This theme with the accent family of `choice` for its appearance.
    ///
    /// The neutrals, track tints, and status colours stay as they are; the
    /// accent, its strong fill and ink, the caret, the selection, and the
    /// playhead move together.
    pub fn with_choice(mut self, choice: ThemeChoice) -> Self {
        let tokens = accent_tokens(choice, self.appearance);
        self.accent = tokens.accent;
        self.accent_strong = tokens.accent_strong;
        self.on_accent = tokens.on_accent;
        self.caret = tokens.caret;
        self.selection = tokens.selection;
        self.playhead = tokens.playhead;
        self
    }
}

#[cfg(test)]
#[path = "theme_accent_tests.rs"]
mod tests;
