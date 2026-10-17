//! A row's reset value: that every property a clip offers has one, that it is
//! the kind the row declares, and that the model takes it back.
//!
//! The class closed is "a reset that is inert or refused". The reset value is
//! declared once per `ClipParam` variant, in the registry, so a variant cannot
//! compile without one; these tests hold what it declares. A default that is
//! missing, of the wrong kind, or outside the range the model validates would
//! draw a reset button that does nothing or reports an error. The rows are read
//! from `clip_params_for` on every dressed clip and on a clip carrying every
//! effect the catalog holds, so a new property, axis member or effect slot is
//! covered without being named here.
//!
//! Not caught: whether a declared default is the value the surface that creates
//! the block starts it at. That is a fact about the creating surface.

use super::*;
use crate::effects::EffectInstance;

/// Every dressed clip, and a clip with one of each catalog effect on it.
fn clips() -> Vec<Clip> {
    let mut clips = all_dressed();
    let mut effects = fully_dressed_clip();
    effects.effects = crate::effect_catalog()
        .into_iter()
        .map(|spec| EffectInstance::new(spec.id.clone()))
        .collect();
    clips.push(effects);
    clips
}

fn kind_of(value: ParamValue) -> ParamKind {
    match value {
        ParamValue::Scalar(_) => ParamKind::Scalar,
        ParamValue::Vec2(_) => ParamKind::Vec2,
        ParamValue::Vec3(_) => ParamKind::Vec3,
        ParamValue::Color(_) => ParamKind::Color,
        ParamValue::Rect(_) => ParamKind::Rect,
    }
}

#[test]
fn every_offered_row_resets_to_a_value_the_model_takes_back() {
    let mut checked = 0usize;
    let mut effect_rows = 0usize;
    for clip in clips() {
        for spec in clip_params_for(&clip) {
            let Some(value) = spec.default_value(&clip) else {
                panic!("{} is offered and has nothing to reset to", spec.id);
            };
            assert_eq!(
                kind_of(value),
                spec.kind,
                "{} resets to {value:?}, which is not the {:?} it declares",
                spec.id,
                spec.kind
            );
            if let Err(err) = spec.param.validate_value(value) {
                panic!(
                    "{} resets to {value:?}, which the model refuses: {err}",
                    spec.id
                );
            }
            let mut written = clip.clone();
            if let Err(err) = spec.param.set_constant(&mut written, value) {
                panic!(
                    "{} cannot be set to its reset value {value:?}: {err}",
                    spec.id
                );
            }
            assert_eq!(
                spec.sample(&written, 0),
                Some(value),
                "{} set to its reset value reads back as something else",
                spec.id
            );
            if matches!(spec.param, ClipParam::Effect { .. }) {
                effect_rows += 1;
            }
            checked += 1;
        }
    }
    assert!(
        checked > clip_param_catalog().len(),
        "only {checked} rows were offered, fewer than the static catalog holds, so \
         the dressing reaches less than it claims"
    );
    assert!(
        effect_rows > 0,
        "no effect slot was offered, so none was checked"
    );
}

/// A row whose block is absent answers no reset value, so a surface cannot
/// offer a reset for a property it does not draw.
#[test]
fn a_row_that_is_not_offered_has_no_reset_value() {
    let bare = Clip::generated(
        Generator::SolidColor {
            rgba: [255, 255, 255, 255],
        },
        tr(0, 48),
    );
    let mut absent = 0usize;
    for spec in clip_param_catalog() {
        if spec.available(&bare) {
            continue;
        }
        absent += 1;
        assert_eq!(
            spec.default_value(&bare),
            None,
            "{} is not offered on a bare clip and still answers a reset value",
            spec.id
        );
    }
    assert!(
        absent > 0,
        "every row is offered on a bare clip, so nothing was checked"
    );
}

/// A reset value is the block as it is first made, not whatever the clip holds:
/// moving a property and reading its reset value back gives the same answer.
#[test]
fn a_reset_value_does_not_follow_the_value_the_clip_holds() {
    for clip in clips() {
        for spec in clip_params_for(&clip) {
            let Some(value) = spec.default_value(&clip) else {
                continue;
            };
            let moved = match value {
                ParamValue::Scalar(v) => {
                    let range = spec.range;
                    let nudged = v + 0.25;
                    let inside = range.max.is_none_or(|max| nudged <= max);
                    let other = if inside { nudged } else { v - 0.25 };
                    ParamValue::Scalar(other)
                }
                _ => continue,
            };
            let mut written = clip.clone();
            if spec.param.set_constant(&mut written, moved).is_err() {
                continue;
            }
            assert_eq!(
                spec.default_value(&written),
                Some(value),
                "{} reset value moved with the clip's value",
                spec.id
            );
        }
    }
}
