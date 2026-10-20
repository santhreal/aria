//! CPU-evaluated effects and generators: the frames and parameter readers
//! [`CpuEffect::process`] receives, and binary16 conversion.

use crate::{
    EFFECT_CURVE_LUT_SAMPLES, EFFECT_GRADIENT_LUT_SAMPLES, EffectParamKind, EffectParamSpec,
    EffectSpec, Host,
};

/// IEEE-754 binary16 (`f16`) conversion helpers for [`CpuFrameIn`] and [`CpuFrameOut`].
pub mod f16 {
    /// IEEE-754 binary16 representation of `1.0`.
    pub const ONE: u16 = 0x3c00;

    /// Convert an IEEE-754 binary16 (`f16`) bit pattern to `f32`.
    #[inline]
    pub const fn to_f32(bits: u16) -> f32 {
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
    pub const fn from_f32(value: f32) -> u16 {
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
}

/// Input frame and evaluated parameters passed to [`CpuEffect::process`].
///
/// Pixels are packed premultiplied `RGBA16F` (`[u16; 4]` IEEE-754 binary16 bit
/// patterns per pixel, top row first, with no row padding).
#[derive(Debug, Clone, Copy)]
pub struct CpuFrameIn<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: &'a [u16],
    pub(crate) params: &'a [f32],
    pub(crate) params_json: &'a str,
    pub(crate) spec: Option<&'a EffectSpec>,
    pub(crate) time: f32,
}

impl<'a> CpuFrameIn<'a> {
    /// Frame width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Frame height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Packed premultiplied `RGBA16F` channel values (`width * height * 4`
    /// IEEE-754 binary16 `u16` elements, top row first).
    pub fn pixels(&self) -> &'a [u16] {
        self.pixels
    }

    /// Read pixel `(x, y)` as raw binary16 `[r, g, b, a]`, or `[0, 0, 0, 0]` out of bounds.
    pub fn pixel(&self, x: u32, y: u32) -> [u16; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// Read pixel `(x, y)` converted from `RGBA16F` to `f32` `[r, g, b, a]`, or
    /// `[0.0, 0.0, 0.0, 0.0]` out of bounds.
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 4] {
        let [r, g, b, a] = self.pixel(x, y);
        [
            f16::to_f32(r),
            f16::to_f32(g),
            f16::to_f32(b),
            f16::to_f32(a),
        ]
    }

    /// Flattened `f32` parameter values in catalog order.
    pub fn params(&self) -> &'a [f32] {
        self.params
    }

    /// The effect instance being rendered, serialized as JSON, or empty when
    /// there is none.
    pub fn params_json(&self) -> &'a str {
        self.params_json
    }

    /// Flattened parameter float at `index`, if present.
    pub fn param_at(&self, index: usize) -> Option<f32> {
        self.params.get(index).copied()
    }

    fn param_slot(&self, name: &str) -> Option<(&'a EffectParamSpec, usize)> {
        let spec = self.spec?;
        let mut offset = 0usize;
        for p in &spec.params {
            if p.name == name {
                return Some((p, offset));
            }
            offset += p.flattened_floats();
        }
        None
    }

    /// Evaluated scalar or angle parameter `name`.
    pub fn scalar(&self, name: &str) -> Option<f32> {
        let (p, offset) = self.param_slot(name)?;
        if !matches!(
            p.kind,
            EffectParamKind::Scalar
                | EffectParamKind::Angle
                | EffectParamKind::Int
                | EffectParamKind::Choice
        ) {
            return None;
        }
        self.params.get(offset).copied().or(Some(p.default))
    }

    /// Evaluated angle parameter `name` in degrees.
    pub fn angle(&self, name: &str) -> Option<f32> {
        let (p, offset) = self.param_slot(name)?;
        if !matches!(p.kind, EffectParamKind::Angle | EffectParamKind::Scalar) {
            return None;
        }
        self.params.get(offset).copied().or(Some(p.default))
    }

    /// Evaluated integer parameter `name`.
    pub fn int(&self, name: &str) -> Option<i32> {
        let (p, offset) = self.param_slot(name)?;
        if !matches!(p.kind, EffectParamKind::Int | EffectParamKind::Scalar) {
            return None;
        }
        self.params
            .get(offset)
            .map(|v| v.round() as i32)
            .or(Some(p.default_int))
    }

    /// Evaluated boolean parameter `name`.
    pub fn boolean(&self, name: &str) -> Option<bool> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Bool {
            return None;
        }
        self.params
            .get(offset)
            .map(|v| *v >= 0.5)
            .or(Some(p.default_bool))
    }

    /// Evaluated choice parameter `name` as its option id.
    pub fn choice(&self, name: &str) -> Option<String> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Choice {
            return None;
        }
        if let Some(idx) = self.params.get(offset).map(|v| v.round().max(0.0) as usize)
            && let Some(opt) = p.choices.get(idx)
        {
            return Some(opt.id.clone());
        }
        if !p.default_choice.is_empty() {
            Some(p.default_choice.clone())
        } else {
            p.choices.first().map(|c| c.id.clone())
        }
    }

    /// Evaluated 2-d vector or size parameter `name`.
    pub fn vec2(&self, name: &str) -> Option<[f32; 2]> {
        let (p, offset) = self.param_slot(name)?;
        if !matches!(p.kind, EffectParamKind::Vec2 | EffectParamKind::Size) {
            return None;
        }
        let s = self.params.get(offset..offset + 2)?;
        Some([s[0], s[1]])
    }

    /// Evaluated 2-d size parameter `name`.
    pub fn size(&self, name: &str) -> Option<[f32; 2]> {
        self.vec2(name)
    }

    /// Evaluated 3-d point parameter `name`.
    pub fn point3(&self, name: &str) -> Option<[f32; 3]> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Point3 {
            return None;
        }
        let s = self.params.get(offset..offset + 3)?;
        Some([s[0], s[1], s[2]])
    }

    /// Evaluated linear RGBA colour parameter `name`.
    pub fn color(&self, name: &str) -> Option<[f32; 4]> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Color {
            return None;
        }
        let s = self.params.get(offset..offset + 4)?;
        Some([s[0], s[1], s[2], s[3]])
    }

    /// Evaluated 64-sample curve lookup table for parameter `name`.
    pub fn curve_lut(&self, name: &str) -> Option<[f32; EFFECT_CURVE_LUT_SAMPLES]> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Curve {
            return None;
        }
        let s = self.params.get(offset..offset + EFFECT_CURVE_LUT_SAMPLES)?;
        let mut out = [0.0f32; EFFECT_CURVE_LUT_SAMPLES];
        out.copy_from_slice(s);
        Some(out)
    }

    /// Evaluated 64-stop RGBA gradient lookup table for parameter `name`.
    pub fn gradient_lut(&self, name: &str) -> Option<[[f32; 4]; EFFECT_GRADIENT_LUT_SAMPLES]> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Gradient {
            return None;
        }
        let s = self
            .params
            .get(offset..offset + EFFECT_GRADIENT_LUT_SAMPLES * 4)?;
        let mut out = [[0.0f32; 4]; EFFECT_GRADIENT_LUT_SAMPLES];
        for (dst, chunk) in out.iter_mut().zip(s.as_chunks::<4>().0) {
            *dst = *chunk;
        }
        Some(out)
    }

    /// Evaluated text, font, or file parameter `name`: the instance's value, or
    /// the parameter's `default_text` when the instance sets none.
    pub fn text(&self, name: &str) -> Option<String> {
        let (p, _) = self.param_slot(name)?;
        if !matches!(
            p.kind,
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File
        ) {
            return None;
        }
        match self.instance_value("text_params", name) {
            Some(serde_json::Value::String(text)) => Some(text),
            _ => Some(p.default_text.clone()),
        }
    }

    /// Evaluated font family parameter `name`.
    pub fn font(&self, name: &str) -> Option<String> {
        self.text(name)
    }

    /// Evaluated file path parameter `name`.
    pub fn file(&self, name: &str) -> Option<String> {
        self.text(name)
    }

    /// The id of the clip layer parameter `name` references, or `None` when the
    /// parameter references no clip or is not a `Layer` parameter.
    pub fn layer(&self, name: &str) -> Option<u64> {
        let (p, _) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Layer {
            return None;
        }
        self.instance_value("layer_params", name)?.as_u64()
    }

    /// The value the effect instance in [`Self::params_json`] stores for
    /// parameter `name` in its map `field`, which the host writes as a list of
    /// `[name, value]` pairs.
    fn instance_value(&self, field: &str, name: &str) -> Option<serde_json::Value> {
        if self.params_json.is_empty() {
            return None;
        }
        let mut instance: serde_json::Value = serde_json::from_str(self.params_json).ok()?;
        let serde_json::Value::Array(pairs) = instance.get_mut(field)?.take() else {
            return None;
        };
        pairs.into_iter().find_map(|pair| match pair {
            serde_json::Value::Array(mut entry) if entry.len() == 2 && entry[0] == name => {
                entry.pop()
            }
            _ => None,
        })
    }

    /// Clip-relative time in seconds.
    pub fn time(&self) -> f32 {
        self.time
    }
}

/// Output frame buffer written by [`CpuEffect::process`].
///
/// Pixels are packed premultiplied `RGBA16F` (`[u16; 4]` IEEE-754 binary16 bit
/// patterns per pixel, top row first, with no row padding).
#[derive(Debug)]
pub struct CpuFrameOut<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: &'a mut [u16],
}

impl<'a> CpuFrameOut<'a> {
    /// Frame width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Frame height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Packed premultiplied `RGBA16F` channel values (`width * height * 4`
    /// IEEE-754 binary16 `u16` elements, top row first).
    pub fn pixels(&self) -> &[u16] {
        self.pixels
    }

    /// Mutable packed premultiplied `RGBA16F` channel values (`width * height * 4`
    /// IEEE-754 binary16 `u16` elements, top row first).
    pub fn pixels_mut(&mut self) -> &mut [u16] {
        self.pixels
    }

    /// Write raw binary16 `[r, g, b, a]` at `(x, y)` when inside the frame.
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u16; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        self.pixels[i..i + 4].copy_from_slice(&rgba);
    }

    /// Convert `[r, g, b, a]` from `f32` to `RGBA16F` and write it at `(x, y)`
    /// when inside the frame.
    pub fn set_pixel_f32(&mut self, x: u32, y: u32, rgba: [f32; 4]) {
        self.set_pixel(
            x,
            y,
            [
                f16::from_f32(rgba[0]),
                f16::from_f32(rgba[1]),
                f16::from_f32(rgba[2]),
                f16::from_f32(rgba[3]),
            ],
        );
    }

    /// Fill every pixel of the output frame with raw binary16 `rgba`.
    pub fn fill(&mut self, rgba: [u16; 4]) {
        for px in self.pixels.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&rgba);
        }
    }

    /// Convert `rgba` from `f32` to `RGBA16F` and fill every pixel of the output frame.
    pub fn fill_f32(&mut self, rgba: [f32; 4]) {
        self.fill([
            f16::from_f32(rgba[0]),
            f16::from_f32(rgba[1]),
            f16::from_f32(rgba[2]),
            f16::from_f32(rgba[3]),
        ]);
    }
}

/// CPU-evaluated visual effect or generator registered with
/// [`Host::register_cpu_effect`] or [`Host::register_generator`].
pub trait CpuEffect: Send + Sync + 'static {
    /// Process `input` into `output`.
    fn process(
        &self,
        host: &Host,
        input: &CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
    ) -> Result<(), String>;
}

impl<F> CpuEffect for F
where
    F: Fn(&Host, &CpuFrameIn<'_>, &mut CpuFrameOut<'_>) -> Result<(), String>
        + Send
        + Sync
        + 'static,
{
    fn process(
        &self,
        host: &Host,
        input: &CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
    ) -> Result<(), String> {
        self(host, input, output)
    }
}
