use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Mask shape kinds (CapCut mask presets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireMaskKind {
    Linear,
    Mirror,
    Circle,
    Rectangle,
    Heart,
    Star,
}

/// A shaped alpha mask on a media-backed visual clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WireMask {
    pub kind: WireMaskKind,
    /// Edge softness, 0 (hard) … 1 (fully feathered). Defaults to 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feather: Option<f64>,
    /// Keep the outside instead of the inside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invert: Option<bool>,
    /// Mask center offset from the layer center, as fractions of the layer
    /// size per axis (`[0,0]` = centered, `[0.5,0]` = right edge). Omitted
    /// uses the default (`[0,0]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f32; 2]>,
    /// Mask size as fractions of the layer size per axis (`[1,1]` covers the
    /// layer). Omitted uses the default (`[1,1]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
    /// Mask rotation in degrees, clockwise about the mask center. Omitted
    /// uses the default (`0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f32>,
    /// Rectangle corner rounding, `0` (sharp) … `1` (fully round). Ignored
    /// by other kinds. Omitted uses the default (`0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roundness: Option<f32>,
}

/// Set (or clear) a per-clip mask on a media-backed visual clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetClipMask {
    pub clip: u64,
    /// Mask to apply. `null` clears the mask.
    pub mask: Option<WireMask>,
}

/// Chroma key settings for green-screen style removal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WireChromaKey {
    /// Key color as `[red, green, blue]`, each 0-255.
    pub rgb: [u8; 3],
    /// Keying strength (tolerance), 0 … 1. Defaults to 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
    /// Shadow retention, 0 … 1. Defaults to 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<f64>,
}

/// Set (or clear) chroma keying on a media-backed visual clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetClipChroma {
    pub clip: u64,
    /// Chroma settings. `null` clears chroma key.
    pub chroma: Option<WireChromaKey>,
}

/// Which channel of the layer above a track matte reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireMatteSource {
    Alpha,
    Luma,
}

/// Track matte settings: the layer directly above cuts this clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WireTrackMatte {
    /// Channel read from the layer above.
    pub source: WireMatteSource,
    /// Keep where the matte is transparent (or black) instead of opaque.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invert: Option<bool>,
    /// Edge softness, 0 … 1. Defaults to 0, which keeps the matte's own edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub softness: Option<f64>,
}

/// Set (or clear) a track matte on a visual clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetClipMatte {
    pub clip: u64,
    /// Matte settings. `null` clears the track matte.
    pub matte: Option<WireTrackMatte>,
}

/// Stabilization strength presets (CapCut stabilize).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireStabilizeLevel {
    Recommended,
    Smooth,
    MaxSmooth,
}

/// Set (or clear) video stabilization on a media-backed video clip (not stills).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetClipStabilize {
    pub clip: u64,
    /// Stabilization preset. `null` clears stabilization.
    pub level: Option<WireStabilizeLevel>,
}

/// A color-grade filter preset and blend intensity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WireFilter {
    /// Catalog id (e.g. "vivid", "warm", "noir").
    pub id: String,
    /// Blend of the graded result over the original, 0 … 1. Defaults to 0.8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f64>,
}

/// Set (or clear) a color-grade filter preset on any visual clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetClipFilter {
    pub clip: u64,
    /// Filter preset. `null` clears the filter.
    pub filter: Option<WireFilter>,
}

/// How a visual clip composites over the stack below (CapCut "Blend").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireBlendMode {
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    Lighten,
    Screen,
    ColorDodge,
    Add,
    Overlay,
    SoftLight,
    HardLight,
    Difference,
    Exclusion,
}

/// Set how a clip composites over the stack below (CapCut "Blend").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetClipBlendMode {
    /// Target clip id.
    pub clip: u64,
    /// Blend mode id: normal, darken, multiply, color_burn, lighten,
    /// screen, color_dodge, add, overlay, soft_light, hard_light,
    /// difference, exclusion.
    pub mode: WireBlendMode,
}

/// Set per-clip transform motion blur (temporal supersampling). Plain values
/// — not animatable. Omitted `shutter_deg` / `samples` keep the clip's
/// current values (or model defaults when first enabling).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetMotionBlur {
    /// Target clip id.
    pub clip: u64,
    /// When false, supersampling is skipped entirely.
    pub enabled: bool,
    /// Shutter angle in degrees (`0..=720`). `360` = full frame interval.
    /// `0` disables even when enabled. Omit to keep the current value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shutter_deg: Option<f32>,
    /// Sub-frame sample count (`2..=32`; render clamps to `2..=16`). Omit to
    /// keep the current value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<u32>,
}

/// Drop shadow drawn from the layer's alpha. Lengths are reference pixels
/// (1080p baseline); color is RGBA 0–255.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireLayerShadow {
    pub rgba: [u8; 4],
    /// Offset in reference pixels (`+x` right, `+y` down).
    pub offset: [f32; 2],
    /// Blur radius in reference pixels (`>= 0`).
    pub blur: f32,
}

/// Soft glow bloom drawn from the layer's alpha.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireLayerGlow {
    pub rgba: [u8; 4],
    /// Glow radius in reference pixels (`>= 0`).
    pub radius: f32,
    /// Strength multiplier, `0` … `4`.
    pub intensity: f32,
}

/// Hard outline / stroke around the layer's alpha silhouette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireLayerOutline {
    pub rgba: [u8; 4],
    /// Outline width in reference pixels (`>= 0`).
    pub width: f32,
}

/// Solid plate behind the layer (padded AABB of the alpha bounds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireLayerBackground {
    pub rgba: [u8; 4],
    /// Padding around the alpha bounds in reference pixels (`>= 0`).
    pub padding: f32,
    /// Corner radius in reference pixels (`>= 0`).
    pub radius: f32,
}

/// Layer-quad styles (CapCut shadow/glow/outline/background) for any visual
/// clip — distinct from text glyph treatments. Lengths are reference pixels
/// (1080p baseline). Omitted blocks are removed; included blocks replace the
/// previous block with the given constant values. Animate individual fields
/// afterward with `set_param_keyframe` and style params.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireLayerStyles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<WireLayerShadow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<WireLayerGlow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<WireLayerOutline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<WireLayerBackground>,
}

/// Replace a visual clip's layer styles (shadow/glow/outline/background).
/// Pass an empty `styles` object to clear every block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetClipLayerStyles {
    /// Target clip id.
    pub clip: u64,
    /// Style blocks to install. Omitted blocks are removed.
    pub styles: WireLayerStyles,
}

macro_rules! define_set_clip_adjustments {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        /// Set manual color adjustments (CapCut adjust) on any visual clip.
        /// Omitted sliders keep their current value; all-neutral clears the
        /// grade.
        ///
        /// One argument per grade slider, expanded from the model's adjustment
        /// row list, so every slider a clip carries is reachable here and a
        /// slider cannot be named under two different words. A slider's range
        /// comes from its own row: signed sliders are `-1..=1`,
        /// one-directional sliders (`sharpness`, `vignette`) are `0..=1`, and
        /// the lift / gamma / gain wheels are `0..=4` around unity.
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct SetClipAdjustments {
            /// Target clip id.
            pub clip: u64,
            $(
                #[doc = $doc]
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $field: Option<f64>,
            )*
        }

        impl SetClipAdjustments {
            /// The sliders this call names, each paired with the catalog row it
            /// writes.
            ///
            /// The values are zipped with
            /// `ColorAdjustments::SLIDERS` in row order, which holds because
            /// both sides expand the same list.
            pub(crate) fn writes(
                &self,
            ) -> impl Iterator<Item = (&'static aria_models::AdjustSlider, f64)> {
                aria_models::ColorAdjustments::SLIDERS
                    .iter()
                    .zip([$(self.$field,)*])
                    .filter_map(|(slider, value)| value.map(|value| (slider, value)))
            }
        }
    };
}

aria_models::adjust_slider_rows!(define_set_clip_adjustments);

/// Animation slot (CapCut In / Out / Combo tabs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireAnimationSlot {
    In,
    Out,
    Combo,
}

/// Set (or clear) a look animation preset on any visual clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetClipAnimation {
    pub clip: u64,
    /// Which animation slot to set.
    pub slot: WireAnimationSlot,
    /// Catalog animation id (e.g. "fade_in", "pulse"). `null` clears the slot.
    pub animation: Option<String>,
    /// Playback rate of the entrance/exit window or combo period (`1` = catalog).
    /// Omit or `null` for the default. Range `0.25..=4`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    /// Magnitude of motion / opacity swing (`1` = catalog, `0` = none).
    /// Omit or `null` for the default. Range `0..=2`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f32>,
    /// Per-character stagger stretch (`1` = catalog). Text presets only.
    /// Omit or `null` for the default. Range `0..=2`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stagger: Option<f32>,
}

/// Audio role tags for clips on audio tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireAudioRole {
    Music,
    Sfx,
    Voiceover,
    Extracted,
}

/// Tag (or untag) what an audio-lane clip is (music / sfx / voiceover / extracted).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetAudioRole {
    pub clip: u64,
    /// Role to apply. `null` clears the tag.
    pub role: Option<WireAudioRole>,
}
