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

use std::fmt::Write as _;

use aria_analysis::{db_from_linear, linear_from_db};
use aria_models::{ClipParam, EffectParamDisplay, EffectParamKind, EffectParamSpec, ParamRange};

use super::clip_audio::{SILENT_DB, VOLUME_FLOOR_DB};
use super::value_scrub::ValueKind;

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
            Self::Gain if shown < VOLUME_FLOOR_DB => 0.0,
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

impl Units<'static> {
    /// The units of catalog property `param`, bounded by `range`.
    ///
    /// An opacity and a scale are fractions shown as percents, whole inside a
    /// bounded range and to a tenth in an open one; a rotation is in degrees
    /// to a tenth; a gain is in decibels to a tenth, half a decibel per unit of
    /// drag. Every other property is shown as stored, to two digits inside a
    /// bounded range and to one in an open range, which here counts pixels.
    pub fn of(param: ClipParam, range: &ParamRange) -> Self {
        let span = range.span();
        let digits = |bounded: u8, open: u8| if span.is_some() { bounded } else { open };
        match param {
            ClipParam::Volume => Self {
                number: Number {
                    scale: Scale::Gain,
                    decimals: 1,
                    drag: 0.5,
                },
                suffix: "dB",
            },
            ClipParam::Opacity | ClipParam::Scale => linear(100.0, digits(0, 1), "%", span),
            ClipParam::Rotation => linear(1.0, 1, "°", span),
            _ => linear(1.0, digits(2, 1), "", span),
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
        let digits = usize::from(self.number.decimals);
        // `fmt::Write` for a `String` cannot fail.
        let _ = match self.number.scale {
            // A level reads against unity, so it is signed.
            Scale::Gain => match db_from_linear(stored) {
                Some(db) => write!(out, "{db:+.digits$}"),
                None => {
                    out.push_str(SILENT);
                    return;
                }
            },
            Scale::Linear(factor) => write!(out, "{:.digits$}", stored * factor),
        };
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
    Units {
        number: Number {
            scale: Scale::Linear(factor),
            decimals,
            drag,
        },
        suffix,
    }
}
