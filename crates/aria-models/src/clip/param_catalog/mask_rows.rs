//! Mask properties as catalog rows.
//!
//! A clip carries a stack of masks, so a mask property exists once per mask:
//! `ClipParam::Mask { mask, param }` addresses one by the mask's index. The
//! rows are built per clip, like effect rows, and a mask contributes a row only
//! for a property its shape draws with: roundness on a rectangle, and size on
//! every shape but a path, which keeps its own extent.

use crate::clip::{Clip, ClipParam, MaskParam};
use crate::look::Mask;

use super::{ParamRoute, ParamScope, ParamSpec, Route};

/// Where one mask property lives on `clip`, or `Absent` when the mask index
/// does not exist.
pub(crate) fn mask_route(clip: &Clip, mask: u32, param: MaskParam) -> ParamRoute<'_> {
    let Some(mask) = clip.masks.get(mask as usize) else {
        return ParamRoute::Absent;
    };
    match param {
        MaskParam::Feather => ParamRoute::Scalar(&mask.feather),
        MaskParam::Center => ParamRoute::Vec2(&mask.center),
        MaskParam::Size => ParamRoute::Vec2(&mask.size),
        MaskParam::Rotation => ParamRoute::Scalar(&mask.rotation),
        MaskParam::Roundness => ParamRoute::Scalar(&mask.roundness),
        MaskParam::Opacity => ParamRoute::Scalar(&mask.opacity),
        MaskParam::Expansion => ParamRoute::Scalar(&mask.expansion),
    }
}

/// The catalog group of the rows of the mask at `index` in its clip's stack.
pub fn mask_param_group(index: u32) -> String {
    format!("mask-{index}")
}

/// The mask property a row id addresses: the inverse of the `mask-<index>-<id>`
/// ids the rows carry, with the index in decimal and no leading zero. `None`
/// for any other string. Whether the mask exists is a question for the clip.
pub(crate) fn mask_row_param(id: &str) -> Option<ClipParam> {
    let (index, property) = id.strip_prefix("mask-")?.split_once('-')?;
    let canonical = index == "0"
        || (!index.starts_with('0') && index.bytes().all(|byte| byte.is_ascii_digit()));
    if !canonical {
        return None;
    }
    let param = MaskParam::ALL
        .iter()
        .copied()
        .find(|param| param.id() == property)?;
    Some(ClipParam::Mask {
        mask: index.parse().ok()?,
        param,
    })
}

pub(super) fn row(index: u32, param: MaskParam) -> ParamSpec {
    ParamSpec {
        // The index is part of the id and the group: two masks on one clip are
        // two sets of rows, and a surface addresses them apart.
        id: format!("mask-{index}-{}", param.id()).into(),
        label: format!("Mask {} {}", index + 1, param.label()).into(),
        group: mask_param_group(index).into(),
        param: ClipParam::Mask { mask: index, param },
        kind: param.kind(),
        scope: ParamScope::Visual,
        range: param.range(),
        route: Route::Mask { mask: index, param },
    }
}

/// One row per property `mask` draws with, in [`MaskParam`] order, for the
/// mask at `index` in its clip's stack.
pub(super) fn rows_of(index: u32, mask: &Mask) -> impl Iterator<Item = ParamSpec> + '_ {
    MaskParam::ALL
        .iter()
        .filter(|param| param.applies_to(mask.kind))
        .map(move |&param| row(index, param))
}
