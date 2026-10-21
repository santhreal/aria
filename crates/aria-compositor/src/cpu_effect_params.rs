//! The parameter values a [`CpuEffect`](crate::CpuEffect) reads for one frame,
//! resolved by name or slot from the registered parameter layout.

use aria_models::{
    EFFECT_CURVE_LUT_SAMPLES, EFFECT_GRADIENT_LUT_SAMPLES, EffectCurvePoint, EffectGradientStop,
    EffectInstance, EffectParamKind,
};

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

/// Evaluated parameter values for one frame of a [`CpuEffect`](crate::CpuEffect).
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

#[cfg(test)]
mod tests {
    use super::*;

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
