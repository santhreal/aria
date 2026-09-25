//! System: 4 helpers from [`super`].
//!
//! Split out of `theme.rs` to stay under the file ceiling. A pure
//! move. Each item is `pub(super)` so the parent and its other children can
//! reach it by name; nothing else changed.

use super::*;

pub(super) fn system_sans() -> &'static str {
    if cfg!(target_os = "macos") {
        "Helvetica"
    } else if cfg!(target_os = "windows") {
        "Segoe UI"
    } else {
        "DejaVu Sans"
    }
}

pub(super) fn system_mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

/// Move one colour to a new hue, keeping its saturation, lightness, and alpha.
///
/// The accent family is tuned for contrast against both appearances; a theme choice is
/// allowed to change what colour it is, not how visible it is.
pub(super) fn retint(color: Hsla, hue_degrees: f32) -> Hsla {
    Hsla {
        h: (hue_degrees / 360.0).rem_euclid(1.0),
        ..color
    }
}

pub(super) fn gamma_encode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}
