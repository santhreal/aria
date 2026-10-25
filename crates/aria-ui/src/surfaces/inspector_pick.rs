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
//! panels build payloads; this file says what each one does.
//!
//! With several clips selected a pick reaches the other selected clips of the
//! drawn clip's kind that read what the drawn clip reads through it
//! ([`Pick::peers`]), in one undo step.

use aria_engine::EngineError;
use aria_models::{BlendMode, ClipId, MaskKind, MaskMode, MatteSource, StabilizeLevel};

use crate::app::AppState;
use crate::app::clip_styles::style_enabled;
use crate::types::UiStyles;

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

/// One pick in a picture block.
#[derive(Clone)]
pub enum Pick {
    /// A chip in the filter catalog.
    Filter(String),
    ClearFilter,
    /// A mask shape from the shape row, added to the bottom of the stack.
    Mask(MaskKind),
    /// How the mask at `mask` combines with the masks above it.
    MaskMode {
        mask: u32,
        mode: MaskMode,
    },
    /// The mask at this index, inverted.
    ToggleMaskInvert(u32),
    /// The mask at `from`, moved to `to`.
    MoveMask {
        from: u32,
        to: u32,
    },
    /// The mask at this index, removed.
    RemoveMask(u32),
    /// A key colour from the swatch row.
    Chroma([u8; 3]),
    ClearChroma,
    /// Which channel of the layer above cuts this clip.
    Matte(MatteSource),
    ToggleMatteInvert,
    ClearMatte,
    /// One layer-style block, on or off.
    Style(&'static str),
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
    /// Detach the clip from its plugin clip type.
    DetachClipType,
}

impl Pick {
    /// The picked control, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Filter(_) => "a filter from the catalog",
            Self::ClearFilter => "the filter cleared",
            Self::Mask(_) => "a mask added",
            Self::MaskMode { .. } => "a mask's mode",
            Self::ToggleMaskInvert(_) => "a mask inverted",
            Self::MoveMask { .. } => "a mask moved in the stack",
            Self::RemoveMask(_) => "a mask removed",
            Self::Chroma(_) => "a key color",
            Self::ClearChroma => "the key cleared",
            Self::Matte(_) => "a matte source",
            Self::ToggleMatteInvert => "the matte inverted",
            Self::ClearMatte => "the matte cleared",
            Self::Style(_) => "a layer style turned on or off",
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
            Self::DetachClipType => "the clip detached from its clip type",
        }
    }

    /// The other selected clips this pick reaches besides the drawn one: those
    /// of its kind that read what it reads through the pick. A value reaches
    /// every one of them; a switch reaches those it reads alike on, so every
    /// clip ends where the drawn one does; a clear reaches those holding
    /// something to clear; a mask edit reaches those with a mask at that place
    /// in the stack.
    pub fn peers(&self, state: &AppState) -> Vec<ClipId> {
        match self {
            Self::Filter(_)
            | Self::Mask(_)
            | Self::Chroma(_)
            | Self::Matte(_)
            | Self::Blend(_)
            | Self::Stabilize(_)
            | Self::ClearGrade => state.peers_reading(|_| ()),
            Self::ClearFilter => state.peers_reading(|clip| clip.filter.is_some()),
            Self::MaskMode { mask, .. } | Self::RemoveMask(mask) => {
                state.peers_reading(|clip| clip.masks.len() > *mask as usize)
            }
            Self::ToggleMaskInvert(mask) => {
                state.peers_reading(|clip| clip.masks.get(*mask as usize).map(|it| it.invert))
            }
            Self::MoveMask { from, to } => {
                state.peers_reading(|clip| clip.masks.len() > (*from).max(*to) as usize)
            }
            Self::ClearChroma => state.peers_reading(|clip| clip.chroma.is_some()),
            Self::ToggleMatteInvert => {
                state.peers_reading(|clip| clip.matte.map(|matte| matte.invert))
            }
            Self::ClearMatte => state.peers_reading(|clip| clip.matte.is_some()),
            Self::Style(style) => state.peers_reading(|clip| style_enabled(&clip.styles, style)),
            Self::ClearStyles => state.peers_reading(|clip| clip.styles != UiStyles::default()),
            Self::ClearLut => state.peers_reading(|clip| clip.lut.is_some()),
            Self::ToggleDenoise => state.peers_reading(|clip| clip.denoise),
            Self::FlipHorizontal => state.peers_reading(|clip| clip.flip_h),
            Self::FlipVertical => state.peers_reading(|clip| clip.flip_v),
            Self::ClearCrop => state.peers_reading(|clip| {
                clip.crop != aria_models::CropRect::FULL || clip.flip_h || clip.flip_v
            }),
            Self::ToggleMotionBlur => state.peers_reading(|clip| clip.motion_blur.enabled),
            Self::DetachClipType => {
                state.peers_reading(|clip| clip.clip_type.as_ref().map(|kind| kind.id.clone()))
            }
        }
    }
}

/// Apply one pick to the selected clip and to the other selected clips it
/// reaches ([`Pick::peers`]), as one undo step.
///
/// Every arm is inert with nothing selected, because that is the rule the
/// setters carry: a pick made while the selection is being replaced is a state
/// the window can be in, and it is not an error.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    let peers = pick.peers(state);
    state.edit_each(peers, |state| one(state, &pick))
}

/// One pick on the selected clip alone.
fn one(state: &mut AppState, pick: &Pick) -> Result<(), EngineError> {
    match pick {
        Pick::Filter(id) => state.set_selected_filter(id.clone()),
        Pick::ClearFilter => state.clear_selected_filter(),
        Pick::Mask(kind) => state.add_selected_mask(*kind),
        Pick::MaskMode { mask, mode } => state.set_selected_mask_mode(*mask, *mode),
        Pick::ToggleMaskInvert(mask) => state.toggle_selected_mask_invert(*mask),
        Pick::MoveMask { from, to } => state.move_selected_mask(*from, *to),
        Pick::RemoveMask(mask) => state.remove_selected_mask(*mask),
        Pick::Chroma(rgb) => state.set_selected_chroma_color(*rgb),
        Pick::ClearChroma => state.clear_selected_chroma(),
        Pick::Matte(source) => state.set_selected_matte_source(*source),
        Pick::ToggleMatteInvert => state.toggle_selected_matte_invert(),
        Pick::ClearMatte => state.clear_selected_matte(),
        Pick::Style(id) => state.toggle_selected_style(id),
        Pick::ClearStyles => state.clear_selected_styles(),
        Pick::ClearLut => state.clear_selected_lut(),
        Pick::Blend(mode) => state.set_selected_blend_mode(*mode),
        Pick::Stabilize(level) => state.set_selected_stabilize(*level),
        Pick::ClearGrade => state.clear_selected_adjustments(),
        Pick::ToggleDenoise => state.toggle_selected_denoise(),
        Pick::FlipHorizontal => state.toggle_selected_flip_h(),
        Pick::FlipVertical => state.toggle_selected_flip_v(),
        Pick::ClearCrop => state.clear_selected_crop(),
        Pick::ToggleMotionBlur => state.toggle_selected_motion_blur(),
        Pick::DetachClipType => state.detach_selected_clip_type(),
    }
}
