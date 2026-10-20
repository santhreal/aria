//! The hand-built controls, declared as rows the panels draw from.
//!
//! A palette row, a menu row and a key chord are table entries, so a test can
//! walk them and check that the command each claims to send is a command
//! something sends. Every other pressable control used to be a `div()`
//! with its action sealed inside a gpui closure: a lane header toggle, a context
//! menu row, a mixer strip control, an inspector block. Nothing could enumerate
//! those, so about forty operator-reachable commands sat outside every
//! measurement, and a control that was deleted or wired to the wrong call would
//! not have turned anything red.
//!
//! So a drawn control is a row here, and the panel builds the control *from* the
//! row. Two things follow. A control is added by adding a row, which is one place
//! rather than one closure buried in a render function. And `app::drawn_census`
//! can press every row and report what it asked for, which is what puts these
//! surfaces inside the forward reach census (`app::operator_surface_tests`)
//! instead of beside it as prose.
//!
//! This module owns declarations only. It draws nothing, so it names no panel and
//! no gpui element: the direction runs from the panel to the registry, never
//! back.

pub mod align_pick;
pub mod analysis_pick;
pub mod bin_strip;
pub mod clip_pick;
pub mod context_menu;
pub mod effects_rack;
pub mod home_pick;
pub mod insert_rack;
pub mod inspector_pick;
pub mod keyframe_rows;
pub mod labels;
pub mod lane_header;
pub mod markers_pick;
pub mod mixer_strip;
pub mod overlay_gesture;
pub mod param_units;
pub mod pool_drop;
pub mod pool_pick;
pub mod ramp_gesture;
pub mod settings_pick;
pub mod sidebar_pick;
pub mod source_pick;
pub mod timeline_gesture;

#[cfg(test)]
#[path = "surfaces/census_tests.rs"]
mod census_tests;

/// What a control draws, lit and unlit.
pub enum Glyph {
    /// One icon lit, another unlit.
    Icons { on: &'static str, off: &'static str },
    /// A text stand-in, for a control the icon set has no glyph for yet. Both
    /// spellings are registered in `docs/icons.md`.
    Text { on: &'static str, off: &'static str },
    /// The lane chevron, which turns rather than swapping glyphs, so the header
    /// shows the lane closing.
    Chevron,
    /// The letter of the source channel the lane takes, `V` or `A`, so a patch
    /// reads as the channel it carries.
    Channel,
}
