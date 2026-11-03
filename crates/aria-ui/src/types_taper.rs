//! How a control places a value along its travel: the line a clip bar draws a
//! property as, the value track under an Effect Controls row, and a mixer
//! strip's pan and fader.

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

    /// Where a value's fill starts along the travel: where zero stands on a
    /// linear travel that runs from below zero to above it, and the travel's
    /// start on any other. So a centered pan or a zero rotation fills nothing,
    /// and a value either side of zero fills from zero to the value.
    pub fn origin(self) -> f32 {
        match self {
            Self::Linear { min, max } if min < 0.0 && max > 0.0 => self.fraction(0.0),
            Self::Linear { .. } | Self::Gain { .. } => 0.0,
        }
    }

    /// The stretch of the travel `value` covers: from [`Self::origin`] to
    /// where the value stands.
    pub fn reach(self, value: f32) -> Reach {
        Reach {
            origin: self.origin(),
            at: self.fraction(value),
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

/// The stretch of a control's travel a value covers, in fractions of the
/// travel: the fill runs between the two ends, and the thumb stands at
/// [`Reach::at`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reach {
    /// Where the fill starts.
    pub origin: f32,
    /// Where the value stands.
    pub at: f32,
}

impl Reach {
    /// The end of the fill nearer the travel's start.
    pub fn start(self) -> f32 {
        self.origin.min(self.at)
    }

    /// The fill's length along the travel.
    pub fn length(self) -> f32 {
        (self.at - self.origin).abs()
    }
}
