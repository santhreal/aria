//! Parameter-driver expression languages and the values they evaluate to.

use aria_plugin_abi::{
    ARIA_PARAM_COLOR, ARIA_PARAM_RECT, ARIA_PARAM_SCALAR, ARIA_PARAM_VEC2, ARIA_PARAM_VEC3,
    AriaParamValue,
};
use serde::{Deserialize, Serialize};

use crate::Host;

/// Value a clip property holds at one frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamValue {
    /// One value.
    Scalar(f32),
    /// `[x, y]`.
    Vec2([f32; 2]),
    /// `[x, y, z]`.
    Vec3([f32; 3]),
    /// RGBA8 `[r, g, b, a]`.
    Color([u8; 4]),
    /// `[x, y, width, height]`: the left edge, the top edge, the width, and the
    /// height.
    Rect([f32; 4]),
}

impl ParamValue {
    pub(crate) fn from_raw(raw: AriaParamValue) -> Result<Self, String> {
        match raw.kind {
            ARIA_PARAM_SCALAR => Ok(Self::Scalar(raw.x)),
            ARIA_PARAM_VEC2 => Ok(Self::Vec2([raw.x, raw.y])),
            ARIA_PARAM_VEC3 => Ok(Self::Vec3([raw.x, raw.y, raw.z])),
            ARIA_PARAM_COLOR => Ok(Self::Color([raw.r, raw.g, raw.b, raw.a])),
            ARIA_PARAM_RECT => Ok(Self::Rect([raw.x, raw.y, raw.z, raw.w])),
            other => Err(format!("unknown AriaParamValue kind {other}")),
        }
    }

    pub(crate) fn to_raw(self) -> AriaParamValue {
        match self {
            Self::Scalar(x) => AriaParamValue {
                kind: ARIA_PARAM_SCALAR,
                x,
                ..AriaParamValue::default()
            },
            Self::Vec2([x, y]) => AriaParamValue {
                kind: ARIA_PARAM_VEC2,
                x,
                y,
                ..AriaParamValue::default()
            },
            Self::Vec3([x, y, z]) => AriaParamValue {
                kind: ARIA_PARAM_VEC3,
                x,
                y,
                z,
                ..AriaParamValue::default()
            },
            Self::Color([r, g, b, a]) => AriaParamValue {
                kind: ARIA_PARAM_COLOR,
                r,
                g,
                b,
                a,
                ..AriaParamValue::default()
            },
            Self::Rect([x, y, z, w]) => AriaParamValue {
                kind: ARIA_PARAM_RECT,
                x,
                y,
                z,
                w,
                ..AriaParamValue::default()
            },
        }
    }
}

/// Frame variables passed to [`DriverLanguage::evaluate`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriverVars {
    /// Sequence seconds being evaluated.
    pub time: f64,
    /// Seconds since the clip's first frame.
    pub clip_time: f64,
    /// Sequence frames per second.
    pub frame_rate: f64,
    /// Sequence length in seconds.
    pub duration: f64,
    /// Canvas `[width, height]` in pixels.
    pub canvas: [u32; 2],
    /// The property's keyframed value at `time`.
    pub value: ParamValue,
    /// A number fixed per clip and property, for seeded noise.
    pub seed: u64,
}

/// Specification of a parameter-driver expression language registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverLanguageSpec {
    /// ASCII letters, digits, `.`, `-` and `_`. A second registration in one
    /// host is refused.
    pub id: String,
    /// The language's display name.
    pub name: String,
}

impl DriverLanguageSpec {
    /// Build a driver language specification with `id` and display `name`.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

/// Parameter-driver expression evaluator registered with [`Host::register_driver_language`].
pub trait DriverLanguage: Send + Sync + 'static {
    /// Evaluate `expr` for `vars` and return the resulting property value.
    fn evaluate(&self, host: &Host, expr: &str, vars: &DriverVars) -> Result<ParamValue, String>;
}

impl<F> DriverLanguage for F
where
    F: Fn(&Host, &str, &DriverVars) -> Result<ParamValue, String> + Send + Sync + 'static,
{
    fn evaluate(&self, host: &Host, expr: &str, vars: &DriverVars) -> Result<ParamValue, String> {
        self(host, expr, vars)
    }
}
