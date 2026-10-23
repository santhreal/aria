// --- Mask -------------------------------------------------------------------

use std::sync::LazyLock;

use aria_core::count::counted;
use serde::{Deserialize, Serialize};

use crate::clip::{MaskParam, ParamKind, ParamRange, ParamValue, ShapePathPoint};
use crate::error::ModelError;
use crate::param::Param;

use super::{default_zero_param, is_false, is_zero_param, validate_unit_param};

/// Mask shapes (CapCut mask panel). Serialized by snake_case id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskKind {
    Linear,
    Mirror,
    Circle,
    Rectangle,
    Heart,
    Star,
    Path,
}

impl MaskKind {
    /// Every shape this build knows, in catalog order. Adding a variant without
    /// listing it here is a compile error in [`MaskKind::id`] and a red test in
    /// `look::tests`, so a new shape cannot ship unlabelled or unbrowsable.
    pub const ALL: [MaskKind; 7] = [
        MaskKind::Linear,
        MaskKind::Mirror,
        MaskKind::Circle,
        MaskKind::Rectangle,
        MaskKind::Heart,
        MaskKind::Star,
        MaskKind::Path,
    ];

    /// Stable wire/catalog id (the serde name).
    pub const fn id(self) -> &'static str {
        match self {
            MaskKind::Linear => "linear",
            MaskKind::Mirror => "mirror",
            MaskKind::Circle => "circle",
            MaskKind::Rectangle => "rectangle",
            MaskKind::Heart => "heart",
            MaskKind::Star => "star",
            MaskKind::Path => "path",
        }
    }

    /// The kind for a catalog id; `None` for an id this build does not know.
    /// Enumerated from the catalog so a new shape resolves without an edit here.
    pub fn from_id(id: &str) -> Option<Self> {
        MASKS
            .iter()
            .map(|spec| spec.kind)
            .find(|kind| kind.id() == id)
    }
}

/// How a mask combines with the masks above it on the same clip.
///
/// A clip's masks apply in order. The stack starts empty, or full when the
/// first mask whose mode is not [`MaskMode::None`] subtracts, intersects, or
/// darkens. Each mask then replaces the running coverage with its mode's
/// result, mixed by the mask's opacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    /// The mask cuts nothing.
    None,
    /// The coverage gains the mask.
    #[default]
    Add,
    /// The coverage loses the mask.
    Subtract,
    /// The coverage keeps only what the mask also covers.
    Intersect,
    /// The larger of the coverage and the mask.
    Lighten,
    /// The smaller of the coverage and the mask.
    Darken,
    /// What the coverage or the mask covers, but not both.
    Difference,
}

impl MaskMode {
    /// Every mode, in menu order.
    pub const ALL: [MaskMode; 7] = [
        MaskMode::None,
        MaskMode::Add,
        MaskMode::Subtract,
        MaskMode::Intersect,
        MaskMode::Lighten,
        MaskMode::Darken,
        MaskMode::Difference,
    ];

    /// Stable wire id (the serde name).
    pub const fn id(self) -> &'static str {
        match self {
            MaskMode::None => "none",
            MaskMode::Add => "add",
            MaskMode::Subtract => "subtract",
            MaskMode::Intersect => "intersect",
            MaskMode::Lighten => "lighten",
            MaskMode::Darken => "darken",
            MaskMode::Difference => "difference",
        }
    }

    /// Display label.
    pub const fn label(self) -> &'static str {
        match self {
            MaskMode::None => "None",
            MaskMode::Add => "Add",
            MaskMode::Subtract => "Subtract",
            MaskMode::Intersect => "Intersect",
            MaskMode::Lighten => "Lighten",
            MaskMode::Darken => "Darken",
            MaskMode::Difference => "Difference",
        }
    }

    /// The mode for a wire id; `None` for an id this build does not know.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_add(&self) -> bool {
        *self == MaskMode::Add
    }
}

/// Most masks one clip may carry.
pub const MAX_CLIP_MASKS: usize = 32;

/// Largest mask expansion, in reference pixels, either way.
pub const MAX_MASK_EXPANSION: f32 = 4096.0;

/// Current serialized format version for [`MaskKind::Path`] vertex payloads.
pub const MASK_PATH_VERSION: u32 = 1;

/// Maximum vertices on a single path mask.
pub const MAX_MASK_PATH_POINTS: usize = 1024;

const fn default_mask_path_version() -> u32 {
    MASK_PATH_VERSION
}

const fn is_default_mask_path_version(v: &u32) -> bool {
    *v == MASK_PATH_VERSION
}

fn deserialize_mask_path_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = u32::deserialize(deserializer)?;
    if v != MASK_PATH_VERSION {
        return Err(serde::de::Error::custom(format!(
            "unsupported mask path version {v} (expected {MASK_PATH_VERSION})"
        )));
    }
    Ok(v)
}

/// A shaped alpha mask over a clip's content, one entry of the clip's mask
/// stack ([`crate::Clip::masks`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub kind: MaskKind,
    /// Edge softness, `0` (hard) … `1` (fully feathered).
    #[serde(default = "default_zero_param", skip_serializing_if = "is_zero_param")]
    pub feather: Param<f32>,
    /// Mask center offset from the layer's center, as a fraction of the layer's
    /// size per axis (`[0,0]` = centered, `[0.5,0]` = right edge). May exceed ±1.
    #[serde(
        default = "default_center_param",
        skip_serializing_if = "is_default_center"
    )]
    pub center: Param<[f32; 2]>,
    /// Mask size as a fraction of the layer's size per axis. `[1,1]` covers the
    /// layer exactly (legacy behavior).
    #[serde(
        default = "default_size_param",
        skip_serializing_if = "is_default_size"
    )]
    pub size: Param<[f32; 2]>,
    /// Mask rotation in degrees, clockwise, about the mask center.
    #[serde(default = "default_zero_param", skip_serializing_if = "is_zero_param")]
    pub rotation: Param<f32>,
    /// Rectangle corner rounding, `0` (sharp) … `1` (fully round). Ignored by
    /// other kinds.
    #[serde(default = "default_zero_param", skip_serializing_if = "is_zero_param")]
    pub roundness: Param<f32>,
    /// How this mask combines with the masks above it.
    #[serde(default, skip_serializing_if = "MaskMode::is_add")]
    pub mode: MaskMode,
    /// How much of this mask's result replaces the stack above it, `0` … `1`.
    #[serde(
        default = "default_opacity_param",
        skip_serializing_if = "is_full_opacity"
    )]
    pub opacity: Param<f32>,
    /// Edge offset in reference pixels: positive grows the mask outward,
    /// negative shrinks it. Scales with the clip like a stroke width.
    #[serde(default = "default_zero_param", skip_serializing_if = "is_zero_param")]
    pub expansion: Param<f32>,
    /// Keep the outside instead of the inside.
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
    /// Bezier path vertices in normalized layer fractions (`0..1` across the
    /// layer quad) when `kind == MaskKind::Path`: one path, or keyframes of
    /// whole paths. [`Mask::path_at`] reads the path at a time.
    #[serde(default = "no_path", skip_serializing_if = "is_no_path")]
    pub path: Param<Vec<ShapePathPoint>>,
    /// Whether `path` is closed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub path_closed: bool,
    /// Format version for `path` payloads (`MASK_PATH_VERSION`).
    #[serde(
        default = "default_mask_path_version",
        skip_serializing_if = "is_default_mask_path_version",
        deserialize_with = "deserialize_mask_path_version"
    )]
    pub path_version: u32,
}

fn no_path() -> Param<Vec<ShapePathPoint>> {
    Param::Constant(Vec::new())
}

fn is_no_path(path: &Param<Vec<ShapePathPoint>>) -> bool {
    matches!(path, Param::Constant(points) if points.is_empty())
}

fn default_center_param() -> Param<[f32; 2]> {
    Param::Constant([0.0, 0.0])
}

fn is_default_center(p: &Param<[f32; 2]>) -> bool {
    p.constant() == Some([0.0, 0.0])
}

fn default_size_param() -> Param<[f32; 2]> {
    Param::Constant([1.0, 1.0])
}

fn is_default_size(p: &Param<[f32; 2]>) -> bool {
    p.constant() == Some([1.0, 1.0])
}

fn validate_center_value(value: [f32; 2]) -> Result<(), ModelError> {
    if value[0].is_finite()
        && value[1].is_finite()
        && value[0].abs() <= 10.0
        && value[1].abs() <= 10.0
    {
        Ok(())
    } else {
        Err(ModelError::InvalidParam(format!(
            "mask center = [{}, {}] must be finite and within ±10",
            value[0], value[1]
        )))
    }
}

fn validate_size_value(value: [f32; 2]) -> Result<(), ModelError> {
    if value[0].is_finite() && value[1].is_finite() && value[0] > 0.0 && value[1] > 0.0 {
        Ok(())
    } else {
        Err(ModelError::InvalidParam(format!(
            "mask size = [{}, {}] must be finite and > 0",
            value[0], value[1]
        )))
    }
}

fn validate_rotation_value(value: f32) -> Result<(), ModelError> {
    if value.is_finite() && value.abs() <= 3600.0 {
        Ok(())
    } else {
        Err(ModelError::InvalidParam(format!(
            "mask rotation = {value} must be finite and within ±3600"
        )))
    }
}

fn default_opacity_param() -> Param<f32> {
    Param::Constant(1.0)
}

fn is_full_opacity(p: &Param<f32>) -> bool {
    p.constant() == Some(1.0)
}

fn validate_expansion_value(value: f32) -> Result<(), ModelError> {
    if value.is_finite() && value.abs() <= MAX_MASK_EXPANSION {
        Ok(())
    } else {
        Err(ModelError::InvalidParam(format!(
            "mask expansion = {value} must be finite and within ±{MAX_MASK_EXPANSION}"
        )))
    }
}

impl Mask {
    /// A hard, opaque, non-inverted mask of `kind` that adds to the stack.
    pub fn new(kind: MaskKind) -> Self {
        let (path, path_closed) = if kind == MaskKind::Path {
            (
                Param::Constant(vec![
                    ShapePathPoint::corner([0.25, 0.25]),
                    ShapePathPoint::corner([0.75, 0.25]),
                    ShapePathPoint::corner([0.75, 0.75]),
                    ShapePathPoint::corner([0.25, 0.75]),
                ]),
                true,
            )
        } else {
            (no_path(), false)
        };
        Self {
            kind,
            feather: Param::Constant(0.0),
            center: Param::Constant([0.0, 0.0]),
            size: Param::Constant([1.0, 1.0]),
            rotation: Param::Constant(0.0),
            roundness: Param::Constant(0.0),
            mode: MaskMode::Add,
            opacity: Param::Constant(1.0),
            expansion: Param::Constant(0.0),
            invert: false,
            path,
            path_closed,
            path_version: MASK_PATH_VERSION,
        }
    }

    /// A path mask with explicit `points` and `closed` state.
    pub fn from_path(points: Vec<ShapePathPoint>, closed: bool) -> Self {
        Self {
            path: Param::Constant(points),
            path_closed: closed,
            ..Self::new(MaskKind::Path)
        }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        if self.path_version != MASK_PATH_VERSION {
            return Err(ModelError::InvalidParam(format!(
                "unsupported mask path version {} (expected {MASK_PATH_VERSION})",
                self.path_version
            )));
        }
        validate_unit_param("mask feather", &self.feather)?;
        self.center.validate_shape()?;
        self.center
            .for_each_value(|value| validate_center_value(*value))?;
        self.size.validate_shape()?;
        self.size
            .for_each_value(|value| validate_size_value(*value))?;
        self.rotation.validate_shape()?;
        self.rotation
            .for_each_value(|value| validate_rotation_value(*value))?;
        validate_unit_param("mask roundness", &self.roundness)?;
        validate_unit_param("mask opacity", &self.opacity)?;
        self.expansion.validate_shape()?;
        self.expansion
            .for_each_value(|value| validate_expansion_value(*value))?;
        self.path.validate_shape()?;
        self.path
            .for_each_value(|points| validate_path_points(self.kind, points))
    }
}

/// The vertex checks [`Mask::validate`] holds every path of a mask to, the
/// path itself or each keyframe's.
fn validate_path_points(kind: MaskKind, points: &[ShapePathPoint]) -> Result<(), ModelError> {
    if kind == MaskKind::Path && points.is_empty() {
        return Err(ModelError::InvalidParam(
            "path mask requires at least 1 vertex".into(),
        ));
    }
    if points.len() > MAX_MASK_PATH_POINTS {
        return Err(ModelError::InvalidParam(format!(
            "path mask has {} vertices (max {MAX_MASK_PATH_POINTS})",
            points.len()
        )));
    }
    for pt in points {
        for v in [pt.anchor, pt.handle_in, pt.handle_out] {
            if !v[0].is_finite() || !v[1].is_finite() || v[0].abs() > 10.0 || v[1].abs() > 10.0 {
                return Err(ModelError::InvalidParam(format!(
                    "mask path coordinate [{}, {}] must be finite and within ±10",
                    v[0], v[1]
                )));
            }
        }
    }
    Ok(())
}

#[path = "mask_path.rs"]
mod path_keys;

/// The mask at `index` in a clip's stack.
pub(crate) fn mask_mut(masks: &mut [Mask], index: u32) -> Result<&mut Mask, ModelError> {
    let count = masks.len();
    masks.get_mut(index as usize).ok_or_else(|| {
        ModelError::InvalidParam(format!(
            "mask {index} does not exist; the clip has {}",
            counted(count, "mask", "masks")
        ))
    })
}

impl MaskParam {
    /// Stable id, the tail of the mask row id `mask-<index>-<id>`.
    pub const fn id(self) -> &'static str {
        match self {
            MaskParam::Feather => "feather",
            MaskParam::Center => "center",
            MaskParam::Size => "size",
            MaskParam::Rotation => "rotation",
            MaskParam::Roundness => "roundness",
            MaskParam::Opacity => "opacity",
            MaskParam::Expansion => "expansion",
        }
    }

    /// Display label.
    pub const fn label(self) -> &'static str {
        match self {
            MaskParam::Feather => "Feather",
            MaskParam::Center => "Centre",
            MaskParam::Size => "Size",
            MaskParam::Rotation => "Rotation",
            MaskParam::Roundness => "Roundness",
            MaskParam::Opacity => "Opacity",
            MaskParam::Expansion => "Expansion",
        }
    }

    /// The value kind the property's keyframes carry.
    pub const fn kind(self) -> ParamKind {
        match self {
            MaskParam::Center | MaskParam::Size => ParamKind::Vec2,
            MaskParam::Feather
            | MaskParam::Rotation
            | MaskParam::Roundness
            | MaskParam::Opacity
            | MaskParam::Expansion => ParamKind::Scalar,
        }
    }

    /// The bounds [`Mask::validate`] holds the property to.
    pub const fn range(self) -> ParamRange {
        match self {
            MaskParam::Feather | MaskParam::Roundness | MaskParam::Opacity => {
                ParamRange::between(0.0, 1.0)
            }
            MaskParam::Center => ParamRange::between(-10.0, 10.0),
            MaskParam::Size => ParamRange::above(f32::EPSILON),
            MaskParam::Rotation => ParamRange::between(-3600.0, 3600.0),
            MaskParam::Expansion => ParamRange::between(-MAX_MASK_EXPANSION, MAX_MASK_EXPANSION),
        }
    }

    /// Whether a mask of `kind` draws differently when this property changes:
    /// roundness rounds only a rectangle, and a path keeps its own extent.
    pub const fn applies_to(self, kind: MaskKind) -> bool {
        match self {
            MaskParam::Roundness => matches!(kind, MaskKind::Rectangle),
            MaskParam::Size => !matches!(kind, MaskKind::Path),
            MaskParam::Feather
            | MaskParam::Center
            | MaskParam::Rotation
            | MaskParam::Opacity
            | MaskParam::Expansion => true,
        }
    }
}

/// Kind and range check for one mask property value (matches
/// [`Mask::validate`]).
pub(crate) fn validate_mask_value(param: MaskParam, value: ParamValue) -> Result<(), ModelError> {
    match param {
        MaskParam::Feather | MaskParam::Roundness | MaskParam::Opacity => {
            let v = value.scalar()?;
            if v.is_finite() && (0.0..=1.0).contains(&v) {
                Ok(())
            } else {
                Err(ModelError::InvalidParam(format!(
                    "mask {} = {v} is out of range [0, 1]",
                    param.id()
                )))
            }
        }
        MaskParam::Rotation => validate_rotation_value(value.scalar()?),
        MaskParam::Expansion => validate_expansion_value(value.scalar()?),
        MaskParam::Center => validate_center_value(value.vec2()?),
        MaskParam::Size => validate_size_value(value.vec2()?),
    }
}

/// One mask catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MaskSpec {
    pub kind: MaskKind,
    pub label: String,
}

/// Every mask shape, read once. Shipped set at `data/masks.toml`.
///
/// A label is a file's to choose; a shape is not. The renderer dispatches on
/// [`MaskKind`], so an override naming a shape this build cannot draw is refused
/// whole rather than leaving a button that draws nothing — and the shipped file
/// has to offer every kind, because a shape reachable only by hand-editing a
/// project file is a shape nobody finds.
static MASKS: LazyLock<Vec<MaskSpec>> = LazyLock::new(|| {
    crate::catalog::load(
        "masks",
        include_str!("../../data/masks.toml"),
        |entries: &[MaskSpec]| {
            crate::catalog::unique_ids(entries, |s| s.kind.id(), |s| &s.label)?;
            for kind in MaskKind::ALL {
                if !entries.iter().any(|spec| spec.kind == kind) {
                    return Err(format!("mask shape {} is not offered", kind.id()));
                }
            }
            Ok(())
        },
    )
});

/// Every mask shape (UI browsing order).
pub fn mask_catalog() -> &'static [MaskSpec] {
    &MASKS
}
