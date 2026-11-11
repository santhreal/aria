//! WHY: closes the class of a text bound stated as a float artifact, as `text
//! size must be from 0.00000011920929 to 4096` was, or stated two ways: a whole
//! style ([`TextStyle::validate`]) and one animated parameter
//! ([`Generator::set_text_param_constant`]) each kept a copy of every bound.
//! Every scalar [`TextParam`] is driven past both sides of its bound through
//! both paths, which must agree on the verdict and the words. A new
//! [`TextParam`] fails to compile in [`field`] until it has a row.
//!
//! What this does not catch: a bound with no [`TextParam`], such as a style
//! run's size, which shares [`validate_text_size`] but is not driven here.

use super::*;
use crate::Generator;
use crate::clip::clip_param::TextParam;
use crate::clip::param_value::ParamValue;

/// A style with every optional block, so each parameter applies.
fn full_style() -> TextStyle {
    TextStyle {
        stroke: Some(TextStroke::default()),
        background: Some(TextBackground::default()),
        shadow: Some(TextShadow::default()),
        ..TextStyle::default()
    }
}

/// The style field `param` animates, or `None` for a color.
fn field(style: &mut TextStyle, param: TextParam) -> Option<&mut Param<f32>> {
    match param {
        TextParam::Size => Some(&mut style.size),
        TextParam::LetterSpacing => Some(&mut style.letter_spacing),
        TextParam::LineSpacing => Some(&mut style.line_spacing),
        TextParam::StrokeWidth => style.stroke.as_mut().map(|stroke| &mut stroke.width),
        TextParam::ShadowBlur => style.shadow.as_mut().map(|shadow| &mut shadow.blur),
        TextParam::ShadowDistance => style.shadow.as_mut().map(|shadow| &mut shadow.distance),
        TextParam::BackgroundRadius => style
            .background
            .as_mut()
            .map(|background| &mut background.radius),
        TextParam::Fill
        | TextParam::StrokeColor
        | TextParam::ShadowColor
        | TextParam::BackgroundColor => None,
    }
}

/// The verdict on `value` for `param` through the style check.
fn through_style(param: TextParam, value: f32) -> Result<(), String> {
    let mut style = full_style();
    *field(&mut style, param).expect("a scalar parameter has a field") = Param::Constant(value);
    style.validate().map_err(|err| err.to_string())
}

/// The verdict on `value` for `param` through the parameter edit.
fn through_param(param: TextParam, value: f32) -> Result<(), String> {
    let mut generator = Generator::Text {
        content: "Title".into(),
        style: full_style(),
    };
    generator
        .set_text_param_constant(param, ParamValue::Scalar(value))
        .map_err(|err| err.to_string())
}

/// A number written with four or more decimals: a float printed raw.
fn float_artifact(message: &str) -> bool {
    let bytes = message.as_bytes();
    bytes
        .windows(5)
        .any(|window| window[0] == b'.' && window[1..].iter().all(u8::is_ascii_digit))
}

#[test]
fn a_text_bound_reads_one_way_through_the_style_and_the_parameter() {
    let mut scalars = 0;
    for &param in TextParam::ALL {
        if field(&mut full_style(), param).is_none() {
            continue;
        }
        scalars += 1;
        for value in [-1.0e9, -1.0, 0.0, 1.0e9, f32::NAN, f32::INFINITY] {
            let style = through_style(param, value);
            let edit = through_param(param, value);
            assert_eq!(style, edit, "{param:?} at {value}: the two checks disagree");
            let far = !value.is_finite() || value.abs() > 4096.0;
            assert!(!far || style.is_err(), "{param:?} took {value}");
            if let Err(message) = style {
                assert!(
                    !float_artifact(&message),
                    "{param:?} at {value} states a raw float: {message}"
                );
            }
        }
    }
    assert_eq!(
        scalars, 7,
        "a scalar text parameter was added or lost a field"
    );
}

/// A size and a line spacing of zero draw nothing, so both are rejected and
/// the bound reads as positive, the way a shape size does.
#[test]
fn a_zero_size_or_line_spacing_is_rejected_as_not_positive() {
    for (param, words) in [
        (
            TextParam::Size,
            "invalid parameter: text size must be positive and at most 4096 reference px",
        ),
        (
            TextParam::LineSpacing,
            "invalid parameter: text line spacing must be positive and at most 100",
        ),
    ] {
        assert_eq!(
            through_param(param, 0.0),
            Err(words.to_string()),
            "{param:?}"
        );
        assert_eq!(
            through_param(param, 1.0e-9),
            Ok(()),
            "{param:?} a hair above zero"
        );
    }
}
