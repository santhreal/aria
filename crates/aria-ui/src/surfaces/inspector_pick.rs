//! The picture blocks' picks: every chip and button in the inspector that sets
//! one thing on the selected clip.
//!
//! A stepper row is generated from the property catalog, so the numbers in a
//! block were already declared. The picks were not: a filter chip, a mask shape,
//! a key colour, a matte source, a style toggle, a blend mode, a stabilization
//! level, the flips and the clears were each a `div()` with its call sealed in a
//! gpui listener. A dozen of the model's commands were reachable only through
//! those, so nothing could enumerate them and a chip wired to the wrong setter
//! would not have turned anything red.
//!
//! A pick is the payload plus what it means, and one function applies it. The
//! panels build payloads; this file says what each one does. The other blocks
//! (audio, animation, effects, keyframes, speed, template, text, transition) are
//! still closures, and are the next slices of ledger row 140.

use aria_engine::EngineError;
use aria_models::{BlendMode, MaskKind, MatteSource, StabilizeLevel};

use crate::app::AppState;

/// Keying presets: the two screen colours a shoot actually uses, plus blue's
/// paler variant. A picker widget is a separate concern.
///
/// The palette lives here rather than in the block that draws it, so a census
/// can press every swatch instead of one representative.
pub const KEY_COLORS: &[(&str, &str, [u8; 3])] = &[
    ("chroma-green", "Green", [0, 177, 64]),
    ("chroma-blue", "Blue", [0, 71, 187]),
    ("chroma-cyan", "Cyan", [0, 174, 239]),
];

/// Style colours worth one click. A full picker is a separate concern.
pub const STYLE_COLORS: &[(&str, &str, [u8; 4])] = &[
    ("black", "Black", [0, 0, 0, 255]),
    ("white", "White", [255, 255, 255, 255]),
    ("half", "Half black", [0, 0, 0, 128]),
];

/// One thing an operator picks in a picture block.
pub enum Pick {
    /// A chip in the filter catalog.
    Filter(String),
    ClearFilter,
    /// A mask shape from the shape row.
    Mask(MaskKind),
    ToggleMaskInvert,
    ClearMask,
    /// A key colour from the swatch row.
    Chroma([u8; 3]),
    ClearChroma,
    /// Which channel of the layer above cuts this clip.
    Matte(MatteSource),
    ToggleMatteInvert,
    ClearMatte,
    /// One layer-style block, on or off.
    Style(&'static str),
    /// A swatch inside one layer-style block.
    StyleColor {
        style: &'static str,
        rgba: [u8; 4],
    },
    ClearStyles,
    ClearLut,
    /// How this clip composites onto what is under it.
    Blend(BlendMode),
    /// A stabilization level, or none.
    Stabilize(Option<StabilizeLevel>),
    ClearGrade,
    ToggleDenoise,
    FlipHorizontal,
    FlipVertical,
    ClearCrop,
    ToggleMotionBlur,
}

impl Pick {
    /// What the operator picked, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Filter(_) => "a filter from the catalog",
            Self::ClearFilter => "the filter cleared",
            Self::Mask(_) => "a mask shape",
            Self::ToggleMaskInvert => "the mask inverted",
            Self::ClearMask => "the mask cleared",
            Self::Chroma(_) => "a key colour",
            Self::ClearChroma => "the key cleared",
            Self::Matte(_) => "a matte source",
            Self::ToggleMatteInvert => "the matte inverted",
            Self::ClearMatte => "the matte cleared",
            Self::Style(_) => "a layer style turned on or off",
            Self::StyleColor { .. } => "a layer style's colour",
            Self::ClearStyles => "every layer style cleared",
            Self::ClearLut => "the LUT removed",
            Self::Blend(_) => "a blend mode",
            Self::Stabilize(_) => "a stabilization level",
            Self::ClearGrade => "the grade reset",
            Self::ToggleDenoise => "noise reduction toggled",
            Self::FlipHorizontal => "the picture flipped left to right",
            Self::FlipVertical => "the picture flipped top to bottom",
            Self::ClearCrop => "the crop reset",
            Self::ToggleMotionBlur => "motion blur toggled",
        }
    }
}

/// Apply one pick to the selected clip.
///
/// Every arm is inert with nothing selected, because that is the rule the
/// setters carry: a pick made while the selection is being replaced is a state
/// the window can be in, and it is not an error.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    match pick {
        Pick::Filter(id) => state.set_selected_filter(id),
        Pick::ClearFilter => state.clear_selected_filter(),
        Pick::Mask(kind) => state.set_selected_mask_kind(kind),
        Pick::ToggleMaskInvert => state.toggle_selected_mask_invert(),
        Pick::ClearMask => state.clear_selected_mask(),
        Pick::Chroma(rgb) => state.set_selected_chroma_color(rgb),
        Pick::ClearChroma => state.clear_selected_chroma(),
        Pick::Matte(source) => state.set_selected_matte_source(source),
        Pick::ToggleMatteInvert => state.toggle_selected_matte_invert(),
        Pick::ClearMatte => state.clear_selected_matte(),
        Pick::Style(id) => state.toggle_selected_style(id),
        Pick::StyleColor { style, rgba } => state.set_selected_style_color(style, rgba),
        Pick::ClearStyles => state.clear_selected_styles(),
        Pick::ClearLut => state.clear_selected_lut(),
        Pick::Blend(mode) => state.set_selected_blend_mode(mode),
        Pick::Stabilize(level) => state.set_selected_stabilize(level),
        Pick::ClearGrade => state.clear_selected_adjustments(),
        Pick::ToggleDenoise => state.toggle_selected_denoise(),
        Pick::FlipHorizontal => state.toggle_selected_flip_h(),
        Pick::FlipVertical => state.toggle_selected_flip_v(),
        Pick::ClearCrop => state.clear_selected_crop(),
        Pick::ToggleMotionBlur => state.toggle_selected_motion_blur(),
    }
}
