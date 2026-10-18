//! CPU-evaluated effects and generators (`RGBA16F` callback path).
//!
//! A plugin effect that cannot be written as WGSL implements [`CpuEffect`].
//! When an effect chain reaches a CPU pass, the compositor reads the current
//! offscreen texture into a reused staging buffer, converts it to premultiplied
//! `RGBA16F` (`[u16; 4]` IEEE-754 binary16 per pixel), invokes
//! [`CpuEffect::process`] off the GPU queue, and uploads the written output
//! into the next offscreen slot. A generator (`spec.generator == true`)
//! receives a transparent frame of the output dimensions.

use std::sync::Arc;

use aria_models::{
    EFFECT_CURVE_LUT_SAMPLES, EFFECT_GRADIENT_LUT_SAMPLES, EffectCurvePoint, EffectGradientStop,
    EffectInstance, EffectParamKind, EffectSpec,
};

use crate::registered_passes::EffectDefinition;

/// IEEE-754 binary16 representation of `1.0`.
pub const F16_ONE: u16 = 0x3c00;

/// Convert an IEEE-754 binary16 (`f16`) bit pattern to `f32`.
#[inline]
pub const fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits & 0x8000) as u32) << 16;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let mant = (bits & 0x03ff) as u32;
    if exp == 0 {
        if mant == 0 {
            return f32::from_bits(sign);
        }
        let shift = mant.leading_zeros() - 21;
        let mant = (mant << shift) & 0x03ff;
        let exp = (127 - 15 + 1) - shift;
        f32::from_bits(sign | (exp << 23) | (mant << 13))
    } else if exp == 0x1f {
        f32::from_bits(sign | 0x7f80_0000 | (mant << 13))
    } else {
        f32::from_bits(sign | ((exp + (127 - 15)) << 23) | (mant << 13))
    }
}

/// Convert an `f32` to an IEEE-754 binary16 (`f16`) bit pattern (round to
/// nearest, ties to even).
#[inline]
pub const fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x007f_ffff;
    if exp == 0xff {
        return if mant == 0 {
            sign | 0x7c00
        } else {
            sign | 0x7e00 | ((mant >> 13) as u16)
        };
    }
    let half_exp = exp - 127 + 15;
    if half_exp >= 0x1f {
        return sign | 0x7c00;
    }
    if half_exp <= 0 {
        if half_exp < -10 {
            return sign;
        }
        let full_mant = mant | 0x0080_0000;
        let shift = (14 - half_exp) as u32;
        let round_bit = 1u32 << (shift - 1);
        let mask = round_bit - 1;
        let mut half_mant = (full_mant >> shift) as u16;
        if (full_mant & round_bit) != 0 && ((full_mant & mask) != 0 || (half_mant & 1) != 0) {
            half_mant += 1;
        }
        return sign | half_mant;
    }
    let mut half_mant = (mant >> 13) as u16;
    if (mant & 0x1000) != 0 && ((mant & 0x0fff) != 0 || (half_mant & 1) != 0) {
        half_mant += 1;
    }
    sign | (((half_exp as u16) << 10) + half_mant)
}

/// Precomputed `u8` (`0..=255`) to normalized `f16` (`0.0..=1.0`) table.
pub const U8_TO_F16: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut i = 0usize;
    while i < 256 {
        table[i] = f32_to_f16((i as f32) / 255.0);
        i += 1;
    }
    table
};

/// Convert a normalized `f16` channel (`0.0..=1.0`) to `u8` (`0..=255`).
#[inline]
pub fn f16_to_u8(bits: u16) -> u8 {
    let v = f16_to_f32(bits);
    if v.is_nan() || v <= 0.0 {
        0
    } else if v >= 1.0 {
        255
    } else {
        (v * 255.0 + 0.5) as u8
    }
}

/// Input frame handed to [`CpuEffect::process`]: premultiplied `RGBA16F` pixels
/// stored as four IEEE-754 binary16 `u16` channels (`[R, G, B, A]`) per pixel.
#[derive(Debug, Clone, Copy)]
pub struct CpuFrameIn<'a> {
    data: &'a [u16],
    width: u32,
    height: u32,
    row_stride: usize,
}

impl<'a> CpuFrameIn<'a> {
    /// Wrap `data` as a `width`×`height` `RGBA16F` input frame with
    /// `row_stride` `u16` elements per row.
    ///
    /// Panics if `row_stride < width as usize * 4` or `data` is shorter than
    /// `height as usize * row_stride`.
    pub fn new(data: &'a [u16], width: u32, height: u32, row_stride: usize) -> Self {
        let row_elems = (width as usize) * 4;
        assert!(
            row_stride >= row_elems,
            "row_stride ({row_stride}) must be at least width * 4 ({row_elems})"
        );
        let needed = (height as usize) * row_stride;
        assert!(
            data.len() >= needed,
            "data length ({}) is shorter than height * row_stride ({needed})",
            data.len()
        );
        Self {
            data,
            width,
            height,
            row_stride,
        }
    }

    /// Frame width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Frame height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row stride in `u16` elements (`>= width * 4`).
    #[inline]
    pub fn row_stride(&self) -> usize {
        self.row_stride
    }

    /// Underlying `u16` slice containing `height * row_stride` elements.
    #[inline]
    pub fn data(&self) -> &'a [u16] {
        self.data
    }

    /// The `width * 4` active `u16` channel values of row `y`.
    #[inline]
    pub fn row(&self, y: u32) -> &'a [u16] {
        assert!(
            y < self.height,
            "row {y} out of bounds for height {}",
            self.height
        );
        let start = (y as usize) * self.row_stride;
        let len = (self.width as usize) * 4;
        &self.data[start..start + len]
    }

    /// Raw `RGBA16F` (`[u16; 4]`) pixel at `(x, y)`.
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [u16; 4] {
        assert!(
            x < self.width,
            "x {x} out of bounds for width {}",
            self.width
        );
        let row = self.row(y);
        let idx = (x as usize) * 4;
        [row[idx], row[idx + 1], row[idx + 2], row[idx + 3]]
    }

    /// Pixel at `(x, y)` converted from `RGBA16F` to `f32` `[R, G, B, A]`.
    #[inline]
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 4] {
        let [r, g, b, a] = self.pixel(x, y);
        [f16_to_f32(r), f16_to_f32(g), f16_to_f32(b), f16_to_f32(a)]
    }
}

/// Output frame handed to [`CpuEffect::process`]: premultiplied `RGBA16F`
/// pixels stored as four IEEE-754 binary16 `u16` channels (`[R, G, B, A]`) per
/// pixel, of the same dimensions as the input frame.
#[derive(Debug)]
pub struct CpuFrameOut<'a> {
    data: &'a mut [u16],
    width: u32,
    height: u32,
    row_stride: usize,
}

impl<'a> CpuFrameOut<'a> {
    /// Wrap `data` as a mutable `width`×`height` `RGBA16F` output frame with
    /// `row_stride` `u16` elements per row.
    ///
    /// Panics if `row_stride < width as usize * 4` or `data` is shorter than
    /// `height as usize * row_stride`.
    pub fn new(data: &'a mut [u16], width: u32, height: u32, row_stride: usize) -> Self {
        let row_elems = (width as usize) * 4;
        assert!(
            row_stride >= row_elems,
            "row_stride ({row_stride}) must be at least width * 4 ({row_elems})"
        );
        let needed = (height as usize) * row_stride;
        assert!(
            data.len() >= needed,
            "data length ({}) is shorter than height * row_stride ({needed})",
            data.len()
        );
        Self {
            data,
            width,
            height,
            row_stride,
        }
    }

    /// Frame width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Frame height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row stride in `u16` elements (`>= width * 4`).
    #[inline]
    pub fn row_stride(&self) -> usize {
        self.row_stride
    }

    /// Immutable view of the underlying `u16` slice.
    #[inline]
    pub fn data(&self) -> &[u16] {
        self.data
    }

    /// Mutable view of the underlying `u16` slice.
    #[inline]
    pub fn data_mut(&mut self) -> &mut [u16] {
        self.data
    }

    /// The `width * 4` active `u16` channel values of row `y`.
    #[inline]
    pub fn row(&self, y: u32) -> &[u16] {
        assert!(
            y < self.height,
            "row {y} out of bounds for height {}",
            self.height
        );
        let start = (y as usize) * self.row_stride;
        let len = (self.width as usize) * 4;
        &self.data[start..start + len]
    }

    /// Mutable slice of the `width * 4` active `u16` channel values of row `y`.
    #[inline]
    pub fn row_mut(&mut self, y: u32) -> &mut [u16] {
        assert!(
            y < self.height,
            "row {y} out of bounds for height {}",
            self.height
        );
        let start = (y as usize) * self.row_stride;
        let len = (self.width as usize) * 4;
        &mut self.data[start..start + len]
    }

    /// Raw `RGBA16F` (`[u16; 4]`) pixel at `(x, y)`.
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [u16; 4] {
        assert!(
            x < self.width,
            "x {x} out of bounds for width {}",
            self.width
        );
        let row = self.row(y);
        let idx = (x as usize) * 4;
        [row[idx], row[idx + 1], row[idx + 2], row[idx + 3]]
    }

    /// Pixel at `(x, y)` converted from `RGBA16F` to `f32` `[R, G, B, A]`.
    #[inline]
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 4] {
        let [r, g, b, a] = self.pixel(x, y);
        [f16_to_f32(r), f16_to_f32(g), f16_to_f32(b), f16_to_f32(a)]
    }

    /// Write raw `RGBA16F` (`[u16; 4]`) channels to `(x, y)`.
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u16; 4]) {
        assert!(
            x < self.width,
            "x {x} out of bounds for width {}",
            self.width
        );
        let row = self.row_mut(y);
        let idx = (x as usize) * 4;
        row[idx..idx + 4].copy_from_slice(&rgba);
    }

    /// Convert `rgba` (`f32` `[R, G, B, A]`) to `RGBA16F` and write to `(x, y)`.
    #[inline]
    pub fn set_pixel_f32(&mut self, x: u32, y: u32, rgba: [f32; 4]) {
        self.set_pixel(
            x,
            y,
            [
                f32_to_f16(rgba[0]),
                f32_to_f16(rgba[1]),
                f32_to_f16(rgba[2]),
                f32_to_f16(rgba[3]),
            ],
        );
    }
}

/// Metadata for one parameter slot of a registered effect.
#[derive(Debug, Clone)]
pub(crate) struct RegisteredParam {
    pub name: Box<str>,
    pub kind: EffectParamKind,
    pub offset: usize,
    pub choices: Box<[Box<str>]>,
    pub default_text: Box<str>,
    pub default_curve: Box<[EffectCurvePoint]>,
    pub default_gradient: Box<[EffectGradientStop]>,
}

impl Default for RegisteredParam {
    fn default() -> Self {
        Self {
            name: "".into(),
            kind: EffectParamKind::Scalar,
            offset: 0,
            choices: Box::new([]),
            default_text: "".into(),
            default_curve: Box::new([]),
            default_gradient: Box::new([]),
        }
    }
}

/// Evaluated parameter values for one frame of a [`CpuEffect`].
#[derive(Debug, Clone, Copy)]
pub struct EffectParams<'a> {
    specs: &'a [RegisteredParam],
    values: &'a [f32],
    instance: Option<&'a EffectInstance>,
}

impl<'a> EffectParams<'a> {
    pub(crate) fn with_instance(
        specs: &'a [RegisteredParam],
        values: &'a [f32],
        instance: Option<&'a EffectInstance>,
    ) -> Self {
        Self {
            specs,
            values,
            instance,
        }
    }

    /// Underlying [`EffectInstance`] when available.
    #[inline]
    pub fn instance(&self) -> Option<&'a EffectInstance> {
        self.instance
    }

    /// Number of declared parameters on the effect.
    #[inline]
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Whether the effect declares no parameters.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// All flattened float values in catalog order.
    #[inline]
    pub fn values(&self) -> &'a [f32] {
        self.values
    }

    /// Flattened float at `flat_index`, or `None`.
    #[inline]
    pub fn get(&self, flat_index: usize) -> Option<f32> {
        self.values.get(flat_index).copied()
    }

    /// Scalar (or angle) parameter `name`, or `None` if not present or not scalar/angle.
    pub fn scalar(&self, name: &str) -> Option<f32> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.scalar_at(index)
    }

    /// Scalar (or angle) parameter at slot `index`, or `None` if out of bounds or not scalar/angle.
    pub fn scalar_at(&self, index: usize) -> Option<f32> {
        let spec = self.specs.get(index)?;
        if !matches!(spec.kind, EffectParamKind::Scalar | EffectParamKind::Angle) {
            return None;
        }
        self.values.get(spec.offset).copied()
    }

    /// Angle parameter `name` in degrees, or `None` if not present or not an angle/scalar.
    pub fn angle(&self, name: &str) -> Option<f32> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.angle_at(index)
    }

    /// Angle parameter at slot `index` in degrees, or `None`.
    pub fn angle_at(&self, index: usize) -> Option<f32> {
        let spec = self.specs.get(index)?;
        if !matches!(spec.kind, EffectParamKind::Angle | EffectParamKind::Scalar) {
            return None;
        }
        self.values.get(spec.offset).copied()
    }

    /// Integer parameter `name`, or `None` if not present or not `Int`.
    pub fn int(&self, name: &str) -> Option<i32> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.int_at(index)
    }

    /// Integer parameter at slot `index`, or `None` if out of bounds or not `Int`.
    pub fn int_at(&self, index: usize) -> Option<i32> {
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Int {
            return None;
        }
        self.values.get(spec.offset).map(|v| v.round() as i32)
    }

    /// Boolean parameter `name`, or `None` if not present or not `Bool`.
    pub fn boolean(&self, name: &str) -> Option<bool> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.bool_at(index)
    }

    /// Boolean parameter `name`, or `None` if not present or not `Bool`.
    pub fn bool_val(&self, name: &str) -> Option<bool> {
        self.boolean(name)
    }

    /// Boolean parameter at slot `index`, or `None` if out of bounds or not `Bool`.
    pub fn bool_at(&self, index: usize) -> Option<bool> {
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Bool {
            return None;
        }
        self.values.get(spec.offset).map(|&v| v >= 0.5)
    }

    /// Zero-based selected option index of choice parameter `name`, or `None`.
    pub fn choice_index(&self, name: &str) -> Option<usize> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.choice_index_at(index)
    }

    /// Zero-based selected option index of choice parameter at slot `index`, or `None`.
    pub fn choice_index_at(&self, index: usize) -> Option<usize> {
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Choice {
            return None;
        }
        self.values
            .get(spec.offset)
            .map(|v| v.round().max(0.0) as usize)
    }

    /// Selected option value string of choice parameter `name`, or `None`.
    pub fn choice(&self, name: &str) -> Option<&'a str> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.choice_at(index)
    }

    /// Selected option value string of choice parameter at slot `index`, or `None`.
    pub fn choice_at(&self, index: usize) -> Option<&'a str> {
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Choice {
            return None;
        }
        if let Some(fx) = self.instance
            && let Some(val) = fx.choice_params.get(&*spec.name)
            && spec.choices.iter().any(|c| &**c == val)
        {
            return Some(val.as_str());
        }
        let choice_idx = self.choice_index_at(index)?;
        spec.choices.get(choice_idx).map(|s| &**s)
    }

    /// Vec2 (or size) parameter `name`, or `None` if not present or not vec2/size.
    pub fn vec2(&self, name: &str) -> Option<[f32; 2]> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.vec2_at(index)
    }

    /// Vec2 (or size) parameter at slot `index`, or `None` if out of bounds or not vec2/size.
    pub fn vec2_at(&self, index: usize) -> Option<[f32; 2]> {
        let spec = self.specs.get(index)?;
        if !matches!(spec.kind, EffectParamKind::Vec2 | EffectParamKind::Size) {
            return None;
        }
        let slice = self.values.get(spec.offset..spec.offset + 2)?;
        Some([slice[0], slice[1]])
    }

    /// Size parameter `name` (`[width, height]`), or `None`.
    pub fn size(&self, name: &str) -> Option<[f32; 2]> {
        self.vec2(name)
    }

    /// Size parameter at slot `index` (`[width, height]`), or `None`.
    pub fn size_at(&self, index: usize) -> Option<[f32; 2]> {
        self.vec2_at(index)
    }

    /// 3D point parameter `name` (`[x, y, z]`), or `None` if not present or not `Point3`.
    pub fn point3(&self, name: &str) -> Option<[f32; 3]> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.point3_at(index)
    }

    /// Alias for [`Self::point3`].
    pub fn vec3(&self, name: &str) -> Option<[f32; 3]> {
        self.point3(name)
    }

    /// 3D point parameter at slot `index` (`[x, y, z]`), or `None`.
    pub fn point3_at(&self, index: usize) -> Option<[f32; 3]> {
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Point3 {
            return None;
        }
        let slice = self.values.get(spec.offset..spec.offset + 3)?;
        Some([slice[0], slice[1], slice[2]])
    }

    /// Colour parameter `name` as linear RGBA `[r, g, b, a]`, or `None` if not
    /// present or not a colour.
    pub fn color(&self, name: &str) -> Option<[f32; 4]> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.color_at(index)
    }

    /// Colour parameter at slot `index` as linear RGBA `[r, g, b, a]`, or
    /// `None` if out of bounds or not a colour.
    pub fn color_at(&self, index: usize) -> Option<[f32; 4]> {
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Color {
            return None;
        }
        let slice = self.values.get(spec.offset..spec.offset + 4)?;
        Some([slice[0], slice[1], slice[2], slice[3]])
    }

    /// 64-sample LUT for curve parameter `name`, or `None`.
    pub fn curve_lut(&self, name: &str) -> Option<&'a [f32; EFFECT_CURVE_LUT_SAMPLES]> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.curve_lut_at(index)
    }

    /// 64-sample LUT for curve parameter at slot `index`, or `None`.
    pub fn curve_lut_at(&self, index: usize) -> Option<&'a [f32; EFFECT_CURVE_LUT_SAMPLES]> {
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Curve {
            return None;
        }
        self.values
            .get(spec.offset..spec.offset + EFFECT_CURVE_LUT_SAMPLES)?
            .try_into()
            .ok()
    }

    /// Sample curve parameter `name` at `x` in `0.0..=1.0`.
    pub fn sample_curve(&self, name: &str, x: f32) -> Option<f32> {
        let lut = self.curve_lut(name)?;
        let u = x.clamp(0.0, 1.0) * ((EFFECT_CURVE_LUT_SAMPLES - 1) as f32);
        let i = (u.floor() as usize).min(EFFECT_CURVE_LUT_SAMPLES - 2);
        let f = u - (i as f32);
        Some(lut[i] + (lut[i + 1] - lut[i]) * f)
    }

    /// Control points of curve parameter `name`, or `None`.
    pub fn curve_points(&self, name: &str) -> Option<&'a [EffectCurvePoint]> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Curve {
            return None;
        }
        if let Some(fx) = self.instance
            && let Some(points) = fx.curve_params.get(&*spec.name)
        {
            return Some(points.as_slice());
        }
        Some(&spec.default_curve)
    }

    /// 64-stop RGBA LUT (256 floats) for gradient parameter `name`, or `None`.
    pub fn gradient_lut(&self, name: &str) -> Option<&'a [f32; EFFECT_GRADIENT_LUT_SAMPLES * 4]> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.gradient_lut_at(index)
    }

    /// 64-stop RGBA LUT (256 floats) for gradient parameter at slot `index`, or `None`.
    pub fn gradient_lut_at(
        &self,
        index: usize,
    ) -> Option<&'a [f32; EFFECT_GRADIENT_LUT_SAMPLES * 4]> {
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Gradient {
            return None;
        }
        self.values
            .get(spec.offset..spec.offset + EFFECT_GRADIENT_LUT_SAMPLES * 4)?
            .try_into()
            .ok()
    }

    /// Sample gradient parameter `name` at `t` in `0.0..=1.0`.
    pub fn sample_gradient(&self, name: &str, t: f32) -> Option<[f32; 4]> {
        let lut = self.gradient_lut(name)?;
        let u = t.clamp(0.0, 1.0) * ((EFFECT_GRADIENT_LUT_SAMPLES - 1) as f32);
        let i = (u.floor() as usize).min(EFFECT_GRADIENT_LUT_SAMPLES - 2);
        let f = u - (i as f32);
        let b0 = i * 4;
        let b1 = b0 + 4;
        Some([
            lut[b0] + (lut[b1] - lut[b0]) * f,
            lut[b0 + 1] + (lut[b1 + 1] - lut[b0 + 1]) * f,
            lut[b0 + 2] + (lut[b1 + 2] - lut[b0 + 2]) * f,
            lut[b0 + 3] + (lut[b1 + 3] - lut[b0 + 3]) * f,
        ])
    }

    /// Stops of gradient parameter `name`, or `None`.
    pub fn gradient_stops(&self, name: &str) -> Option<&'a [EffectGradientStop]> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Gradient {
            return None;
        }
        if let Some(fx) = self.instance
            && let Some(stops) = fx.gradient_params.get(&*spec.name)
        {
            return Some(stops.as_slice());
        }
        Some(&spec.default_gradient)
    }

    /// Text, font, or file parameter `name`, or `None`.
    pub fn text(&self, name: &str) -> Option<&'a str> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.text_at(index)
    }

    /// Text, font, or file parameter at slot `index`, or `None`.
    pub fn text_at(&self, index: usize) -> Option<&'a str> {
        let spec = self.specs.get(index)?;
        if !matches!(
            spec.kind,
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File
        ) {
            return None;
        }
        if let Some(fx) = self.instance
            && let Some(val) = fx.text_params.get(&*spec.name)
        {
            return Some(val.as_str());
        }
        Some(&spec.default_text)
    }

    /// Font family parameter `name`, or `None`.
    pub fn font(&self, name: &str) -> Option<&'a str> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Font {
            return None;
        }
        self.text_at(index)
    }

    /// File path parameter `name`, or `None`.
    pub fn file(&self, name: &str) -> Option<&'a str> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::File {
            return None;
        }
        self.text_at(index)
    }

    /// Layer reference parameter `name` (`Some(Some(clip_id))` when set,
    /// `Some(None)` when unset, `None` if `name` is not a `Layer` parameter).
    pub fn layer(&self, name: &str) -> Option<Option<u64>> {
        let index = self.specs.iter().position(|p| &*p.name == name)?;
        self.layer_at(index)
    }

    /// Layer reference parameter at slot `index`.
    pub fn layer_at(&self, index: usize) -> Option<Option<u64>> {
        let spec = self.specs.get(index)?;
        if spec.kind != EffectParamKind::Layer {
            return None;
        }
        Some(
            self.instance
                .and_then(|fx| fx.layer_params.get(&*spec.name).copied()),
        )
    }
}

/// A video effect or generator evaluated on the CPU.
///
/// The input and output frames hold premultiplied `RGBA16F` pixels (four
/// IEEE-754 binary16 `u16` channels `[R, G, B, A]` per pixel). For a generator
/// (`spec.generator == true`), `input` is a transparent frame of the output
/// dimensions.
///
/// Returning `Err(reason)` leaves the frame as passthrough for that frame and
/// reports a recoverable plugin fault. A panic is caught, leaves the frame as
/// passthrough, and fails the plugin for the session.
pub trait CpuEffect: Send + Sync {
    /// Process `input` into `output` at clip-local `time` (in seconds) with
    /// evaluated `params`.
    fn process(
        &self,
        input: CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
        params: &EffectParams<'_>,
        time: f32,
    ) -> Result<(), String>;
}

impl<F> CpuEffect for F
where
    F: Fn(CpuFrameIn<'_>, &mut CpuFrameOut<'_>, &EffectParams<'_>, f32) -> Result<(), String>
        + Send
        + Sync,
{
    fn process(
        &self,
        input: CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
        params: &EffectParams<'_>,
        time: f32,
    ) -> Result<(), String> {
        self(input, output, params, time)
    }
}

/// An effect or generator a plugin renders on the CPU.
#[derive(Clone)]
pub struct CpuEffectDefinition {
    /// The catalog entry: a namespaced id, a label, and the parameters in the
    /// order they flatten into [`EffectParams`].
    pub spec: EffectSpec,
    /// The callback that processes each frame.
    pub effect: Arc<dyn CpuEffect>,
}

impl std::fmt::Debug for CpuEffectDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuEffectDefinition")
            .field("spec", &self.spec)
            .finish_non_exhaustive()
    }
}

impl CpuEffectDefinition {
    /// Build a CPU effect definition from a catalog `spec` and a callback.
    pub fn new(spec: EffectSpec, effect: impl CpuEffect + 'static) -> Self {
        Self {
            spec,
            effect: Arc::new(effect),
        }
    }
}

/// How a registered effect renders: WGSL fragment passes or a CPU callback.
#[derive(Debug, Clone)]
pub enum EffectRender {
    /// WGSL fragment passes, in dispatch order.
    Shader(EffectDefinition),
    /// A CPU callback.
    Cpu(CpuEffectDefinition),
}

impl EffectRender {
    /// The catalog spec of the effect.
    pub fn spec(&self) -> &EffectSpec {
        match self {
            Self::Shader(def) => &def.spec,
            Self::Cpu(def) => &def.spec,
        }
    }
}

impl From<EffectDefinition> for EffectRender {
    fn from(definition: EffectDefinition) -> Self {
        Self::Shader(definition)
    }
}

impl From<CpuEffectDefinition> for EffectRender {
    fn from(definition: CpuEffectDefinition) -> Self {
        Self::Cpu(definition)
    }
}

/// Why a registered effect or transition pass raised a fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectFaultKind {
    /// The callback returned `Err(message)`; the frame passes through and the
    /// plugin stays mounted.
    Failed(String),
    /// The callback panicked with `message`; the frame passes through and the
    /// plugin is failed for the session.
    Panicked(String),
    /// Rendering the effect took `took`, exceeding the sequence or preview
    /// frame `budget` (`1 / fps`); the plugin stays mounted and the fault is
    /// reported.
    MissedDeadline {
        budget: std::time::Duration,
        took: std::time::Duration,
    },
}

/// A fault raised by a registered effect or transition pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectFault {
    pub effect_id: String,
    pub kind: EffectFaultKind,
}

impl std::fmt::Display for EffectFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            EffectFaultKind::Failed(message) => {
                write!(f, "effect {} failed: {message}", self.effect_id)
            }
            EffectFaultKind::Panicked(message) => {
                write!(f, "effect {} panicked: {message}", self.effect_id)
            }
            EffectFaultKind::MissedDeadline { budget, took } => {
                write!(
                    f,
                    "effect {} took {:.1} ms, over the {:.1} ms frame budget",
                    self.effect_id,
                    took.as_secs_f64() * 1_000.0,
                    budget.as_secs_f64() * 1_000.0,
                )
            }
        }
    }
}

/// Receives faults raised by a registered effect or transition pass.
pub type EffectFaultSink = Arc<dyn Fn(&EffectFault) + Send + Sync>;

/// Receives the measured per-frame execution cost of a registered effect or
/// transition pass.
pub type EffectCostSink = Arc<dyn Fn(std::time::Duration) + Send + Sync>;

/// Run one registered [`CpuEffect`] pass on `input` (in `pool` slot `in_idx` or
/// outside `pool`), writing the output into the next offscreen slot and
/// returning `(out_idx, output_view)` or `None` on passthrough.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_cpu_effect<'a>(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &crate::effect_render::PassRegistry,
    pool: &'a crate::effect_render::OffscreenPool,
    input: &'a wgpu::TextureView,
    in_idx: usize,
    effect: &crate::passes::PassInstance,
    pass: &crate::registered_passes::RegisteredPass,
    width: u32,
    height: u32,
) -> Option<(usize, &'a wgpu::TextureView)> {
    if width == 0 || height == 0 || pass.is_removed() {
        return None;
    }
    let slots = pool.slots();
    if slots < 2 {
        return None;
    }
    let out_idx = (in_idx + 1) % slots;
    let src_texture = if let Some(slot) = pool.index_of(input) {
        pool.texture(slot)
    } else {
        crate::effect_render::run_grade_pass(
            device,
            encoder,
            registry,
            pool.passes(),
            input,
            pool.view(out_idx),
            crate::ColorGrade::default(),
        );
        pool.texture(out_idx)
    };
    let mut staging = pool.passes().take_cpu_staging(device, width, height);
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: src_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging.readback_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(staging.padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        size,
    );
    let next_encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let submitted = std::mem::replace(encoder, next_encoder).finish();
    let queue = pool.passes().queue();
    queue.submit(std::iter::once(submitted));

    staging
        .map_state
        .store(0, std::sync::atomic::Ordering::Release);
    let flag = Arc::clone(&staging.map_state);
    let slice = staging.readback_buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let code = if res.is_ok() { 1 } else { 2 };
        flag.store(code, std::sync::atomic::Ordering::Release);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    if staging.map_state.load(std::sync::atomic::Ordering::Acquire) != 1 {
        pool.passes().return_cpu_staging(staging);
        return None;
    }

    let frame_budget = pool.passes().frame_budget();
    let started = pass.tracks_cost(frame_budget).then(std::time::Instant::now);
    let row_bytes = (width as usize) * 4;
    let padded_row = staging.padded_bytes_per_row as usize;
    {
        let mapped = slice.get_mapped_range();
        for y in 0..(height as usize) {
            let src_row = &mapped[y * padded_row..y * padded_row + row_bytes];
            let dst_row = &mut staging.input_f16[y * row_bytes..(y + 1) * row_bytes];
            for (dst, &byte) in dst_row.iter_mut().zip(src_row.iter()) {
                *dst = U8_TO_F16[usize::from(byte)];
            }
        }
    }
    staging.readback_buffer.unmap();

    staging.output_f16.fill(0);
    let input_frame = CpuFrameIn::new(&staging.input_f16, width, height, row_bytes);
    let mut output_frame = CpuFrameOut::new(&mut staging.output_f16, width, height, row_bytes);
    let succeeded = pass.run_cpu_with_instance(
        input_frame,
        &mut output_frame,
        &effect.params,
        effect.instance.as_ref(),
        effect.time,
    );
    if !succeeded {
        pool.passes().return_cpu_staging(staging);
        return None;
    }

    for (dst, &half) in staging
        .upload_rgba8
        .iter_mut()
        .zip(staging.output_f16.iter())
    {
        *dst = f16_to_u8(half);
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: pool.texture(out_idx),
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &staging.upload_rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        size,
    );
    if let Some(started) = started {
        pass.record_cost(started.elapsed(), frame_budget);
    }
    pool.passes().return_cpu_staging(staging);
    Some((out_idx, pool.view(out_idx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: CPU effects read and write `RGBA16F` converted from and to `u8`
    /// channel values. If any `0..=255` byte fails to round-trip through `f16`
    /// exactly, a passthrough CPU effect shifts colors on every frame.
    #[test]
    fn every_u8_channel_round_trips_through_f16_exactly() {
        for byte in 0u8..=255 {
            let half = U8_TO_F16[usize::from(byte)];
            let back = f16_to_u8(half);
            assert_eq!(back, byte, "byte {byte} became {back} via f16 {half:#06x}");
        }
        assert_eq!(f32_to_f16(1.0), F16_ONE);
        assert_eq!(f16_to_f32(F16_ONE), 1.0);
        assert_eq!(f32_to_f16(0.0), 0);
        assert_eq!(f16_to_f32(0), 0.0);
    }

    #[test]
    fn frame_views_respect_row_stride_and_pixel_accessors() {
        // 2x2 frame with a padded stride of 12 u16s per row (active width is 8 u16s).
        let mut storage = vec![0u16; 24];
        {
            let mut out = CpuFrameOut::new(&mut storage, 2, 2, 12);
            assert_eq!(out.width(), 2);
            assert_eq!(out.height(), 2);
            assert_eq!(out.row_stride(), 12);
            out.set_pixel_f32(1, 0, [0.25, 0.5, 0.75, 1.0]);
            out.set_pixel(0, 1, [U8_TO_F16[10], U8_TO_F16[20], U8_TO_F16[30], F16_ONE]);
        }
        let input = CpuFrameIn::new(&storage, 2, 2, 12);
        assert_eq!(input.row(0).len(), 8);
        assert_eq!(input.row(1).len(), 8);
        assert_eq!(input.pixel_f32(1, 0), [0.25, 0.5, 0.75, 1.0]);
        assert_eq!(
            input.pixel(0, 1),
            [U8_TO_F16[10], U8_TO_F16[20], U8_TO_F16[30], F16_ONE]
        );
    }

    #[test]
    fn effect_params_resolve_by_name_and_slot_with_kind_checks() {
        let specs = [
            RegisteredParam {
                name: "gain".into(),
                kind: EffectParamKind::Scalar,
                offset: 0,
                ..Default::default()
            },
            RegisteredParam {
                name: "center".into(),
                kind: EffectParamKind::Vec2,
                offset: 1,
                ..Default::default()
            },
            RegisteredParam {
                name: "tint".into(),
                kind: EffectParamKind::Color,
                offset: 3,
                ..Default::default()
            },
        ];
        let values = [0.75, 10.0, 20.0, 0.1, 0.2, 0.3, 1.0];
        let params = EffectParams::with_instance(&specs, &values, None);
        assert_eq!(params.len(), 3);
        assert!(!params.is_empty());
        assert_eq!(params.scalar("gain"), Some(0.75));
        assert_eq!(params.scalar_at(0), Some(0.75));
        assert_eq!(params.vec2("center"), Some([10.0, 20.0]));
        assert_eq!(params.vec2_at(1), Some([10.0, 20.0]));
        assert_eq!(params.color("tint"), Some([0.1, 0.2, 0.3, 1.0]));
        assert_eq!(params.color_at(2), Some([0.1, 0.2, 0.3, 1.0]));
        assert_eq!(params.scalar("center"), None);
        assert_eq!(params.vec2("gain"), None);
        assert_eq!(params.color("gain"), None);
        assert_eq!(params.scalar("missing"), None);
        assert_eq!(params.scalar_at(9), None);
    }
}
