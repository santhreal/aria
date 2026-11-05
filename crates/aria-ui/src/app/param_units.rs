//! The units a property is printed, dragged and typed in.
//!
//! The model stores each property in the units its math wants: an opacity and
//! a scale as fractions, a gain as the multiplier the mixer applies. A row
//! shows some of them in other units: a percent, a decibel. This module is the
//! single definition of that mapping. The Inspector's rows, Effect Controls,
//! the timeline's property rows and the clip line's drag readout all print
//! through [`Units`], so a property reads the same number on every surface,
//! and a typed value is read in the units the row printed.
//!
//! A row moves the stored value through [`Scale::moved`], and the model still
//! clamps what lands: the units are the row's, the bound is the model's.

use aria_analysis::{GAIN_FLOOR_DB, db_from_linear, linear_from_db};
use aria_models::{
    ClipParam, EffectParamDisplay, EffectParamKind, EffectParamSpec, MaskParam, ParamRange,
    ShapeParam, StyleParam, TextParam,
};

use super::clip_audio::SILENT_DB;
use super::value_scrub::{ValueKind, write_number};
use crate::types::Taper;

/// What a gain row prints at silence, which has no decibel value. A readout
/// printing `-inf` would print a number its field could not take back.
pub const SILENT: &str = "Silent";

/// How a stored value maps to the number a row shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scale {
    /// The stored value times this factor: 100 for a fraction shown as a
    /// percent.
    Linear(f32),
    /// A linear gain shown in decibels. A fixed step in the multiplier is
    /// inaudible at the top of a fader and a cliff near silence; a step in
    /// decibels is the same change at any level. Silence shows one decibel
    /// under the floor, so a step up from it lands on the floor, and any level
    /// under the floor mutes.
    Gain,
}

impl Scale {
    /// `stored` in the units a row shows.
    pub fn shown(self, stored: f32) -> f32 {
        match self {
            Self::Linear(factor) => stored * factor,
            Self::Gain => db_from_linear(stored).unwrap_or(SILENT_DB),
        }
    }

    /// The stored value a row showing `shown` holds.
    pub fn stored(self, shown: f32) -> f32 {
        match self {
            Self::Linear(factor) => shown / factor,
            Self::Gain if shown < GAIN_FLOOR_DB => 0.0,
            Self::Gain => linear_from_db(shown),
        }
    }

    /// `stored` moved by `delta` in the units a row shows.
    pub fn moved(self, stored: f32, delta: f32) -> f32 {
        match self {
            // Added in stored units, so a value moved by nothing stays
            // exactly where it was rather than taking a round trip through
            // the factor.
            Self::Linear(factor) => stored + delta / factor,
            Self::Gain => self.stored(self.shown(stored) + delta),
        }
    }

    /// The change in the stored value that moves `stored` by `delta` in the
    /// units a row shows, for a write that adds to the value it finds.
    pub fn stored_delta(self, stored: f32, delta: f32) -> f32 {
        match self {
            Self::Linear(factor) => delta / factor,
            Self::Gain => self.moved(stored, delta) - stored,
        }
    }

    /// The travel a property of this scale bounded by `range` stands on: even
    /// between the bounds, or for a gain the mixer fader's taper, even in
    /// decibels from [`GAIN_FLOOR_DB`] up to the top of `range`. A volume row's
    /// track and a clip's volume line share it. `None` when `range` leaves the
    /// travel no end.
    pub fn taper(self, range: &ParamRange) -> Option<Taper> {
        match self {
            Self::Linear(_) => Taper::between(range),
            Self::Gain => range.max.map(|max| Taper::Gain { max }),
        }
    }
}

/// A property's number as a row shows it: its scale, the digits it is
/// printed and typed with, and what one unit of drag moves it by, in shown
/// units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Number {
    pub scale: Scale,
    pub decimals: u8,
    pub drag: f32,
}

impl Number {
    /// How a field opens this number and reads it back.
    pub fn kind(self) -> ValueKind {
        ValueKind::Number {
            decimals: self.decimals,
        }
    }
}

/// How one property is printed, dragged and typed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Units<'a> {
    pub number: Number,
    /// What follows the number. A symbol (`%`, `°`) is printed against it, a
    /// unit name (`dB`, `px`) after a space.
    pub suffix: &'a str,
}

/// The suffixes printed against the number rather than after a space.
const SYMBOLS: [&str; 2] = ["%", "°"];

/// What a stored property measures, which sets how a row prints it and how far
/// one unit of drag moves it.
///
/// A model bound is a validation limit, not a travel: a mask may sit ten frames
/// off center and a title may be 4096 pixels tall. Only a property whose bounds
/// are its travel, an amount from none to all, steps by a share of them. Every
/// other measure steps by one printed unit whatever the bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Measure {
    /// A linear gain, printed in decibels, half a decibel per unit of drag.
    Gain,
    /// A multiplier or a share of a whole, such as an opacity or a scale: a
    /// percent, whole inside two bounds and to a tenth past an open one, one
    /// percent per unit of drag.
    Ratio,
    /// A place or an extent as a fraction of the frame or of the picture: a
    /// percent to a tenth, a tenth of a percent per unit of drag. Two pixels
    /// of drag are one unit, so the pointer moves a value about one pixel of
    /// a 1920-pixel frame per pixel it travels.
    Fraction,
    /// An angle: degrees to a tenth, one degree per unit of drag.
    Degrees,
    /// A length in reference pixels: to a tenth, one pixel per unit of drag.
    Pixels,
    /// An amount printed as stored, crossing its bounds in a hundred units of
    /// drag.
    Amount,
}

impl Measure {
    /// What `param` measures. Every property and every member of a property
    /// axis is named, so a new one does not compile until it has a measure.
    /// A look property is an amount on the travel its slider shape sets, and
    /// an effect parameter prints through [`Units::of_effect`].
    fn of(param: ClipParam) -> Self {
        match param {
            ClipParam::Volume => Self::Gain,
            ClipParam::Opacity
            | ClipParam::EffectOpacity { .. }
            | ClipParam::Scale
            | ClipParam::Speed => Self::Ratio,
            ClipParam::Position | ClipParam::AnchorPoint | ClipParam::Crop => Self::Fraction,
            ClipParam::Rotation => Self::Degrees,
            ClipParam::Pan | ClipParam::Effect { .. } | ClipParam::Look { .. } => Self::Amount,
            ClipParam::Mask { param, .. } => match param {
                MaskParam::Center | MaskParam::Size => Self::Fraction,
                MaskParam::Rotation => Self::Degrees,
                MaskParam::Opacity => Self::Ratio,
                MaskParam::Expansion => Self::Pixels,
                MaskParam::Feather | MaskParam::Roundness => Self::Amount,
            },
            ClipParam::Shape { param } => match param {
                ShapeParam::Width
                | ShapeParam::Height
                | ShapeParam::CornerRadius
                | ShapeParam::StrokeWidth => Self::Pixels,
                ShapeParam::InnerRatio | ShapeParam::Fill | ShapeParam::StrokeColor => Self::Amount,
            },
            ClipParam::Text { param } => match param {
                TextParam::Size
                | TextParam::LetterSpacing
                | TextParam::StrokeWidth
                | TextParam::ShadowDistance => Self::Pixels,
                TextParam::LineSpacing => Self::Ratio,
                TextParam::ShadowBlur
                | TextParam::BackgroundRadius
                | TextParam::Fill
                | TextParam::StrokeColor
                | TextParam::ShadowColor
                | TextParam::BackgroundColor => Self::Amount,
            },
            ClipParam::Style { param } => match param {
                StyleParam::ShadowOffset
                | StyleParam::ShadowBlur
                | StyleParam::GlowRadius
                | StyleParam::OutlineWidth
                | StyleParam::BackgroundPadding
                | StyleParam::BackgroundRadius => Self::Pixels,
                StyleParam::GlowIntensity
                | StyleParam::ShadowColor
                | StyleParam::GlowColor
                | StyleParam::OutlineColor
                | StyleParam::BackgroundColor => Self::Amount,
            },
        }
    }
}

impl Units<'static> {
    /// The units of a level: decibels against unity to a tenth, signed, half a
    /// decibel per unit of drag, and [`SILENT`] at zero. A clip's volume row
    /// and a mixer fader print through it.
    pub const GAIN: Self = Self {
        number: Number {
            scale: Scale::Gain,
            decimals: 1,
            drag: 0.5,
        },
        suffix: "dB",
    };

    /// The units of a clip's speed, stored as a multiplier: a percent to a
    /// tenth, one percent per unit of drag. The Speed tab, the Speed/Duration
    /// dialog, the clip's badge and the Rate Stretch readout print through it.
    pub const SPEED: Self = Self {
        number: Number {
            scale: Scale::Linear(100.0),
            decimals: 1,
            drag: 1.0,
        },
        suffix: "%",
    };

    /// The units of a keyframe's temporal influence, stored as a fraction of
    /// its segment: a percent to a tenth, one percent per unit of drag. The
    /// Keyframe Velocity dialog and the Graph Editor's handle readouts print
    /// through it.
    pub const INFLUENCE: Self = Self {
        number: Number {
            scale: Scale::Linear(100.0),
            decimals: 1,
            drag: 1.0,
        },
        suffix: "%",
    };

    /// The units of catalog property `param`, bounded by `range`, as what it
    /// measures prints and drags it: a percent, a degree, a reference pixel, a
    /// decibel, or an amount. An amount prints two digits inside two bounds
    /// and one past an open one.
    pub fn of(param: ClipParam, range: &ParamRange) -> Self {
        let span = range.span();
        let digits = |bounded: u8, open: u8| if span.is_some() { bounded } else { open };
        match Measure::of(param) {
            Measure::Gain => Self::GAIN,
            Measure::Ratio => stepped(100.0, digits(0, 1), "%", 1.0),
            Measure::Fraction => stepped(100.0, 1, "%", 0.1),
            Measure::Degrees => stepped(1.0, 1, "°", 1.0),
            Measure::Pixels => stepped(1.0, 1, "", 1.0),
            Measure::Amount => linear(1.0, digits(2, 1), "", span),
        }
    }
}

impl<'a> Units<'a> {
    /// The units of effect or transition parameter `spec`, whose drag crosses
    /// its soft range in a hundred units.
    ///
    /// An integer is whole, one per unit of drag. A vector's axes print three
    /// digits, an angle is in degrees to a tenth, a parameter displayed as a
    /// percentage is a fraction shown as a whole percent, and any other number
    /// prints two digits. A declared unit follows every number but an angle's
    /// and a percentage's.
    pub fn of_effect(spec: &'a EffectParamSpec) -> Self {
        let span = Some(spec.soft_max.unwrap_or(spec.max) - spec.soft_min.unwrap_or(spec.min));
        let unit = spec.unit.as_deref().unwrap_or_default();
        match spec.kind {
            EffectParamKind::Int => Self {
                number: Number {
                    scale: Scale::Linear(1.0),
                    decimals: 0,
                    drag: 1.0,
                },
                suffix: unit,
            },
            EffectParamKind::Vec2 | EffectParamKind::Size | EffectParamKind::Point3 => {
                linear(1.0, 3, unit, span)
            }
            _ if spec.kind == EffectParamKind::Angle
                || spec.display == Some(EffectParamDisplay::AngleDial) =>
            {
                linear(1.0, 1, "°", span)
            }
            _ if spec.display == Some(EffectParamDisplay::Percentage) => {
                linear(100.0, 0, "%", span)
            }
            _ => linear(1.0, 2, unit, span),
        }
    }

    /// What a row prints for `stored`.
    pub fn print(&self, stored: f32) -> String {
        let mut out = String::new();
        self.print_into(&mut out, stored);
        out
    }

    /// What a row prints for the axes of a vector, one after another.
    pub fn print_all(&self, stored: &[f32]) -> String {
        let mut out = String::new();
        for (at, &axis) in stored.iter().enumerate() {
            if at > 0 {
                out.push_str(", ");
            }
            self.print_into(&mut out, axis);
        }
        out
    }

    fn print_into(&self, out: &mut String, stored: f32) {
        let decimals = self.number.decimals;
        match self.number.scale {
            // A level reads against unity, so it is signed.
            Scale::Gain => match db_from_linear(stored) {
                Some(db) => write_number(out, db, decimals, true),
                None => {
                    out.push_str(SILENT);
                    return;
                }
            },
            Scale::Linear(factor) => write_number(out, stored * factor, decimals, false),
        }
        match self.suffix {
            "" => {}
            symbol if SYMBOLS.contains(&symbol) => out.push_str(symbol),
            unit => {
                out.push(' ');
                out.push_str(unit);
            }
        }
    }
}

/// A linear scale of `factor` shown units per stored unit, printing
/// `decimals` digits. One unit of drag is a hundredth of `span`, the stored
/// range, and no finer than the digits printed, or one shown unit where the
/// range is open.
fn linear(factor: f32, decimals: u8, suffix: &str, span: Option<f32>) -> Units<'_> {
    let finest = 10f32.powi(-i32::from(decimals));
    let drag = match span {
        Some(span) if span > 0.0 => (span * factor / 100.0).max(finest),
        _ => 1.0,
    };
    stepped(factor, decimals, suffix, drag)
}

/// A linear scale of `factor` shown units per stored unit, printing
/// `decimals` digits, `drag` shown units per unit of drag.
fn stepped(factor: f32, decimals: u8, suffix: &str, drag: f32) -> Units<'_> {
    Units {
        number: Number {
            scale: Scale::Linear(factor),
            decimals,
            drag,
        },
        suffix,
    }
}

#[cfg(test)]
#[path = "param_units_tests.rs"]
mod tests;
