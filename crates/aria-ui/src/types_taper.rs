//! How a control places a value along its travel: the line a clip bar draws a
//! property as, and the value track under an Effect Controls row.

use aria_models::ParamRange;

/// How a value maps to a position along a control's travel, `0.0` at its
/// start and `1.0` at its end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Taper {
    /// Evenly from `min` at the start to `max` at the end.
    Linear { min: f32, max: f32 },
    /// A gain, on the travel from [`aria_analysis::GAIN_FLOOR_DB`] at the
    /// start to the gain `max` at the end, even in decibels: the travel a mixer
    /// fader stands on. Unity stands where 0 dB falls between the two.
    Gain { max: f32 },
}

impl Taper {
    /// The even travel between the bounds of `range`, when it declares both
    /// and they are apart.
    pub fn between(range: &ParamRange) -> Option<Self> {
        range
            .bounds()
            .filter(|(min, max)| max > min)
            .map(|(min, max)| Self::Linear { min, max })
    }

    /// Where `value` stands along the travel. A linear travel with no length
    /// holds every value at its middle.
    pub fn fraction(self, value: f32) -> f32 {
        match self {
            Self::Linear { min, max } => {
                let span = max - min;
                if span.abs() <= f32::EPSILON {
                    0.5
                } else {
                    ((value - min) / span).clamp(0.0, 1.0)
                }
            }
            Self::Gain { max } => aria_analysis::gain_travel(value, max),
        }
    }

    /// The value standing at `fraction` of the travel.
    pub fn value(self, fraction: f32) -> f32 {
        match self {
            Self::Linear { min, max } => min + (max - min) * fraction.clamp(0.0, 1.0),
            Self::Gain { max } => aria_analysis::gain_at_travel(fraction, max),
        }
    }
}
