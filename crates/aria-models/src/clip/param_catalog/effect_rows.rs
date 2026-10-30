//! Effect instance parameters as catalog rows.
//!
//! `ClipParam::Effect { effect, param }` addresses a slot on one of the clip's
//! effects, and the keyframe commands write to the effect instance's own typed
//! param maps. [`super::clip_param_catalog`] lists the properties every clip
//! declares; an effect's parameters exist once per effect on the clip, in the
//! order the effect declares them, so their rows are built per clip. Every
//! surface that enumerates properties, the keyframe strip, the inspector's
//! keyframe block and the projected clip band, calls [`clip_params_for`] and
//! receives the shipped rows the clip can reach plus one row per effect
//! parameter, in effect order.
//!
//! A plugin contributes effects through `EffectService`, and its parameters
//! return through `effect_spec`, so they reach the strip by the same route as a
//! shipped effect's.
//!
//! The model stores nothing for a parameter until a value is written, and the
//! renderer uses the catalog default meanwhile. Such a row reports that default
//! and no keyframes, so a first keyframe pins what is already on screen.

use crate::clip::{Clip, ClipParam, ParamValue};
use crate::effects::{EffectInstance, EffectParamKind, EffectParamSpec, EffectSpec};
use crate::param::Easing;

use super::{ParamKind, ParamRange, ParamRoute, ParamScope, ParamSpec, Route};

/// Where one effect parameter lives on `clip`, or `Absent` when the effect index
/// or the parameter slot does not exist.
pub(crate) fn effect_route(clip: &Clip, effect: u32, param: u32) -> ParamRoute<'_> {
    let Some(instance) = clip.effects.get(effect as usize) else {
        return ParamRoute::Absent;
    };
    let Some(spec) = crate::effect_spec(&instance.effect_id) else {
        return ParamRoute::Absent;
    };
    spec.param_at(param as usize)
        .map_or(ParamRoute::Absent, |pspec| slot_route(instance, pspec))
}

/// Where the parameter `pspec` describes lives on `instance`: its stored curve,
/// else its catalog default, or `Absent` on a kind no curve carries.
fn slot_route<'a>(instance: &'a EffectInstance, pspec: &EffectParamSpec) -> ParamRoute<'a> {
    let Some(default) = pspec.default_param_value() else {
        return ParamRoute::Absent;
    };
    let name = &pspec.name;
    let stored = match pspec.kind {
        EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => {
            instance.params.get(name).map(ParamRoute::Scalar)
        }
        EffectParamKind::Vec2 | EffectParamKind::Size => {
            instance.vec2_params.get(name).map(ParamRoute::Vec2)
        }
        EffectParamKind::Point3 => instance.vec3_params.get(name).map(ParamRoute::Vec3),
        EffectParamKind::Color => instance.color_params.get(name).map(ParamRoute::Color),
        EffectParamKind::Bool
        | EffectParamKind::Choice
        | EffectParamKind::Curve
        | EffectParamKind::Gradient
        | EffectParamKind::Text
        | EffectParamKind::Font
        | EffectParamKind::File
        | EffectParamKind::Layer => None,
    };
    stored.unwrap_or(ParamRoute::Const(default))
}

/// The value a reset returns one effect parameter to: its catalog default.
/// `None` when the effect index or the slot does not exist, and on a kind no
/// curve carries.
pub(crate) fn effect_default(clip: &Clip, effect: u32, param: u32) -> Option<ParamValue> {
    let instance = clip.effects.get(effect as usize)?;
    crate::effect_spec(&instance.effect_id)?
        .param_at(param as usize)?
        .default_param_value()
}

fn kind_of(kind: EffectParamKind) -> Option<ParamKind> {
    match kind {
        EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => {
            Some(ParamKind::Scalar)
        }
        EffectParamKind::Vec2 | EffectParamKind::Size => Some(ParamKind::Vec2),
        EffectParamKind::Point3 => Some(ParamKind::Vec3),
        EffectParamKind::Color => Some(ParamKind::Color),
        EffectParamKind::Bool
        | EffectParamKind::Choice
        | EffectParamKind::Curve
        | EffectParamKind::Gradient
        | EffectParamKind::Text
        | EffectParamKind::Font
        | EffectParamKind::File
        | EffectParamKind::Layer => None,
    }
}

impl EffectParamSpec {
    /// The range a write to this parameter is held to: the integer bounds of
    /// an integer, the float bounds of any other number. A colour is held
    /// component-wise by its own type, so it declares none.
    pub fn range(&self) -> ParamRange {
        match self.kind {
            EffectParamKind::Color => ParamRange::ANY,
            EffectParamKind::Int => ParamRange::between(self.int_min as f32, self.int_max as f32),
            _ => ParamRange::between(self.min, self.max),
        }
    }
}

/// The bounds of one effect parameter on `clip`, or none when the effect index
/// or the parameter slot does not exist.
pub(crate) fn effect_range(clip: &Clip, effect: u32, param: u32) -> ParamRange {
    clip.effects
        .get(effect as usize)
        .and_then(|instance| crate::effect_spec(&instance.effect_id))
        .and_then(|spec| spec.param_at(param as usize).map(EffectParamSpec::range))
        .unwrap_or(ParamRange::ANY)
}

fn row(index: u32, spec: &EffectSpec, slot: u32, pspec: &EffectParamSpec) -> Option<ParamSpec> {
    let kind = kind_of(pspec.kind)?;
    Some(ParamSpec {
        // The index is part of the id: two instances of the same effect on one
        // clip are two rows, and a surface addresses them apart.
        id: format!("effect-{index}-{}", pspec.name).into(),
        label: format!("{} {}", spec.label, pspec.label).into(),
        // One group per instance, so a panel drawing an effect's rows does not
        // have to work out which slots belong to which copy of it.
        group: format!("effect-{index}").into(),
        param: ClipParam::Effect {
            effect: index,
            param: slot,
        },
        kind,
        scope: ParamScope::Visual,
        range: pspec.range(),
        route: Route::Effect {
            effect: index,
            param: slot,
        },
    })
}

/// One row per keyframeable parameter of one effect instance, in the order the
/// effect declares them. Empty when the effect id is not in any catalog.
fn rows_for(index: u32, instance: &EffectInstance) -> Vec<ParamSpec> {
    let Some(spec) = crate::effect_spec(&instance.effect_id) else {
        return Vec::new();
    };
    spec.params
        .iter()
        .enumerate()
        .filter(|(_, pspec)| instance.is_param_visible(pspec, &spec, 0.0))
        .filter_map(|(slot, pspec)| row(index, &spec, slot as u32, pspec))
        .collect()
}

/// Call `visit` for every animatable property of `clip` without collecting a
/// catalog vector.
///
/// Static rows are borrowed from the process catalog. Dynamic mask and effect
/// rows are built one at a time and remain valid for the duration of the call.
pub fn visit_clip_params(clip: &Clip, mut visit: impl FnMut(&ParamSpec)) {
    for spec in super::clip_param_catalog()
        .iter()
        .filter(|spec| spec.available(clip))
    {
        visit(spec);
    }
    for (index, mask) in clip.masks.iter().enumerate() {
        for dynamic in super::mask_rows::rows_of(index as u32, mask) {
            visit(&dynamic);
        }
    }
    for (index, instance) in clip.effects.iter().enumerate() {
        let Some(spec) = crate::effect_spec(&instance.effect_id) else {
            continue;
        };
        for (slot, pspec) in spec.params.iter().enumerate() {
            if !instance.is_param_visible(pspec, &spec, 0.0) {
                continue;
            }
            if let Some(dynamic) = row(index as u32, &spec, slot as u32, pspec) {
                visit(&dynamic);
            }
        }
    }
}

/// Call `visit` with every keyframe of every property [`visit_clip_params`]
/// offers on `clip`, as `(clip-local tick, easing)`: each row's keyframes in
/// tick order, and the rows in the order that function visits them.
///
/// What [`ParamSpec::visit_keyframes`] reports for each offered row, read with
/// one route lookup per property and one catalog lookup per effect, and
/// without building a mask or effect row.
pub fn visit_clip_keyframes(clip: &Clip, mut visit: impl FnMut(i64, Easing)) {
    // A row that is not available has no route or is out of scope, and
    // `visit_keyframes` reports nothing for either, so no row is filtered.
    for spec in super::clip_param_catalog() {
        spec.visit_keyframes(clip, &mut visit);
    }
    for mask in &clip.masks {
        for param in super::mask_rows::params_of(mask) {
            super::route_keyframes(super::mask_rows::property_route(mask, param), &mut visit);
        }
    }
    for instance in &clip.effects {
        let Some(spec) = crate::effect_spec(&instance.effect_id) else {
            continue;
        };
        for pspec in &spec.params {
            if instance.is_param_visible(pspec, &spec, 0.0) {
                super::route_keyframes(slot_route(instance, pspec), &mut visit);
            }
        }
    }
}

/// Every animatable property of `clip`: the shipped rows it can reach, then the
/// rows of each mask in stack order, then one row per effect parameter.
///
/// This is what a surface enumerates. `clip_param_catalog()` remains the list of
/// properties every clip declares, for callers that describe the model rather
/// than a clip.
pub fn clip_params_for(clip: &Clip) -> Vec<ParamSpec> {
    let mut rows: Vec<ParamSpec> = super::clip_param_catalog()
        .iter()
        .filter(|spec| spec.available(clip))
        .cloned()
        .collect();
    for (index, mask) in clip.masks.iter().enumerate() {
        rows.extend(super::mask_rows::rows_of(index as u32, mask));
    }
    for (index, instance) in clip.effects.iter().enumerate() {
        rows.extend(rows_for(index as u32, instance));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::{Clip, ClipSource, Generator};
    use crate::param::{Easing, Param};
    use crate::time::{Rational, TimeRange};

    fn tr(start: i64, duration: i64) -> TimeRange {
        TimeRange::at_rate(start, duration, Rational::FPS_24)
    }

    fn solid_with(effects: Vec<EffectInstance>) -> Clip {
        let mut clip = Clip::generated(
            Generator::SolidColor {
                rgba: [255, 255, 255, 255],
            },
            tr(0, 48),
        );
        clip.effects = effects;
        clip
    }

    /// The first shipped effect that declares at least one scalar parameter.
    fn scalar_effect() -> (std::sync::Arc<EffectSpec>, usize) {
        crate::shipped_effect_catalog()
            .iter()
            .find_map(|spec| {
                spec.params
                    .iter()
                    .position(|p| p.kind == EffectParamKind::Scalar)
                    .map(|slot| (spec.clone(), slot))
            })
            .expect("the shipped catalog must declare a scalar effect parameter")
    }

    #[test]
    fn an_effect_contributes_one_row_per_parameter() {
        let (spec, _) = scalar_effect();
        let clip = solid_with(vec![EffectInstance::new(&spec.id)]);
        let rows = clip_params_for(&clip);
        let mine: Vec<_> = rows
            .iter()
            .filter(|row| matches!(row.param, ClipParam::Effect { effect: 0, .. }))
            .collect();
        assert_eq!(
            mine.len(),
            spec.params.len(),
            "{} declares {} parameters and contributed {}",
            spec.id,
            spec.params.len(),
            mine.len()
        );
        for (slot, pspec) in spec.params.iter().enumerate() {
            let row = mine
                .iter()
                .find(|row| {
                    row.param
                        == ClipParam::Effect {
                            effect: 0,
                            param: slot as u32,
                        }
                })
                .unwrap_or_else(|| panic!("slot {slot} is missing"));
            assert_eq!(row.id.as_ref(), format!("effect-0-{}", pspec.name));
            assert_eq!(row.kind, kind_of(pspec.kind).expect("keyframeable kind"));
            assert_eq!(row.range, pspec.range());
        }
    }

    #[test]
    fn two_instances_of_one_effect_are_two_addressable_rows() {
        let (spec, _) = scalar_effect();
        let clip = solid_with(vec![
            EffectInstance::new(&spec.id),
            EffectInstance::new(&spec.id),
        ]);
        let rows = clip_params_for(&clip);
        let first = rows
            .iter()
            .filter(|row| matches!(row.param, ClipParam::Effect { effect: 0, .. }))
            .count();
        let second = rows
            .iter()
            .filter(|row| matches!(row.param, ClipParam::Effect { effect: 1, .. }))
            .count();
        assert_eq!(first, spec.params.len());
        assert_eq!(second, spec.params.len());
        let ids: Vec<_> = rows.iter().map(|row| row.id.to_string()).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "row ids collided: {ids:?}");
    }

    #[test]
    fn an_unset_parameter_reads_its_catalog_default_and_has_no_keyframes() {
        let (spec, slot) = scalar_effect();
        let pspec = &spec.params[slot];
        let clip = solid_with(vec![EffectInstance::new(&spec.id)]);
        let rows = clip_params_for(&clip);
        let row = rows
            .iter()
            .find(|row| {
                row.param
                    == ClipParam::Effect {
                        effect: 0,
                        param: slot as u32,
                    }
            })
            .expect("the scalar slot must have a row");
        assert!(row.available(&clip));
        assert_eq!(
            row.sample(&clip, 0),
            Some(ParamValue::Scalar(pspec.default)),
            "an unset parameter must read the value the renderer is using"
        );
        assert!(row.keyframes(&clip).is_empty());
        assert!(row.easing_at(&clip, 0).is_none());
    }

    #[test]
    fn a_set_parameter_reports_its_own_keyframes() {
        let (spec, slot) = scalar_effect();
        let pspec = &spec.params[slot];
        let mut instance = EffectInstance::new(&spec.id);
        let mut param = Param::Constant(pspec.min);
        param.set_keyframe(0, pspec.min, Easing::Linear);
        param.set_keyframe(24, pspec.max, Easing::Linear);
        instance.params.insert(pspec.name.clone(), param);
        let clip = solid_with(vec![instance]);
        let rows = clip_params_for(&clip);
        let row = rows
            .iter()
            .find(|row| {
                row.param
                    == ClipParam::Effect {
                        effect: 0,
                        param: slot as u32,
                    }
            })
            .expect("the scalar slot must have a row");
        let marks: Vec<i64> = row.keyframes(&clip).into_iter().map(|(t, _)| t).collect();
        assert_eq!(marks, vec![0, 24]);
        assert_eq!(row.sample(&clip, 0), Some(ParamValue::Scalar(pspec.min)));
        assert_eq!(row.sample(&clip, 24), Some(ParamValue::Scalar(pspec.max)));
    }

    #[test]
    fn an_unknown_effect_id_contributes_no_rows() {
        // An unmounted plugin's effect: the instance survives in the project so
        // the work is not lost, and nothing offers keyframes on parameters no
        // catalog describes.
        let clip = solid_with(vec![EffectInstance::new("not-an-effect")]);
        let rows = clip_params_for(&clip);
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row.param, ClipParam::Effect { .. })),
            "an unknown effect must contribute nothing"
        );
        assert!(
            !rows.is_empty(),
            "the shipped rows must still be offered alongside it"
        );
    }

    #[test]
    fn a_missing_slot_is_absent_rather_than_a_panic() {
        let (spec, _) = scalar_effect();
        let clip = solid_with(vec![EffectInstance::new(&spec.id)]);
        // One past the last declared slot, and one past the last effect.
        assert!(matches!(
            effect_route(&clip, 0, spec.params.len() as u32),
            ParamRoute::Absent
        ));
        assert!(matches!(effect_route(&clip, 7, 0), ParamRoute::Absent));
    }

    #[test]
    fn a_clip_with_no_effects_offers_only_the_shipped_rows() {
        let clip = solid_with(Vec::new());
        let rows = clip_params_for(&clip);
        assert!(!rows.is_empty());
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row.param, ClipParam::Effect { .. }))
        );
        // Every offered row is reachable: `clip_params_for` filters, so a caller
        // does not have to.
        for row in &rows {
            assert!(
                row.available(&clip),
                "{} is offered but unreachable",
                row.id
            );
        }
        assert!(matches!(clip.content, ClipSource::Generated(_)));
    }
}
