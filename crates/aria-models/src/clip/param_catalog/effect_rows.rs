//! Effect instance parameters as catalog rows.
//!
//! An effect's parameters are already animatable: `ClipParam::Effect { effect,
//! param }` addresses a slot on one of the clip's effects, and the keyframe
//! commands write to the effect instance's own typed param maps. What was
//! missing is the catalog side. [`super::clip_param_catalog`] is a static list
//! of the properties every clip declares, and an effect's parameters are neither
//! static nor per-clip-uniform: they exist once per effect on the clip, in the
//! order the effect declares them.
//!
//! So the rows are built per clip. Every surface that enumerates properties —
//! the keyframe strip, the inspector's keyframe block, the projected clip band —
//! calls [`clip_params_for`] and gets the shipped rows the clip can reach plus
//! one row per effect parameter, in effect order.
//!
//! This is also the plugin story. A plugin contributes effects through
//! `EffectService`; its parameters come back through `effect_spec`, so they
//! reach the strip by the same route as a shipped effect's, and no second
//! property mechanism exists to keep in step.
//!
//! An unset parameter is the interesting case: the model stores nothing until
//! someone writes a value, and the catalog default is the value the renderer
//! uses meanwhile. Such a row reports that default and reports no keyframes, so
//! a first keyframe pins what is already on screen rather than snapping to zero.

use crate::clip::{Clip, ClipParam, ParamValue};
use crate::effects::{EffectInstance, EffectParamKind, EffectParamSpec, EffectSpec};

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
    let Some(pspec) = spec.param_at(param as usize) else {
        return ParamRoute::Absent;
    };
    match pspec.kind {
        EffectParamKind::Scalar => match instance.params.get(&pspec.name) {
            Some(p) => ParamRoute::Scalar(p),
            None => ParamRoute::Const(ParamValue::Scalar(pspec.default)),
        },
        EffectParamKind::Vec2 => match instance.vec2_params.get(&pspec.name) {
            Some(p) => ParamRoute::Vec2(p),
            None => ParamRoute::Const(ParamValue::Vec2(pspec.default_vec2)),
        },
        EffectParamKind::Color => match instance.color_params.get(&pspec.name) {
            Some(p) => ParamRoute::Color(p),
            None => ParamRoute::Const(ParamValue::Color(pspec.default_color)),
        },
    }
}

fn kind_of(kind: EffectParamKind) -> ParamKind {
    match kind {
        EffectParamKind::Scalar => ParamKind::Scalar,
        EffectParamKind::Vec2 => ParamKind::Vec2,
        EffectParamKind::Color => ParamKind::Color,
    }
}

/// The range a write is validated against. A colour is validated component-wise
/// by its own type, so it declares none.
fn range_of(pspec: &EffectParamSpec) -> ParamRange {
    match pspec.kind {
        EffectParamKind::Color => ParamRange::ANY,
        _ => ParamRange::between(pspec.min, pspec.max),
    }
}

fn row(index: u32, spec: &EffectSpec, slot: u32, pspec: &EffectParamSpec) -> ParamSpec {
    ParamSpec {
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
        kind: kind_of(pspec.kind),
        scope: ParamScope::Visual,
        range: range_of(pspec),
        route: Route::Effect {
            effect: index,
            param: slot,
        },
    }
}

/// One row per parameter of one effect instance, in the order the effect
/// declares them. Empty when the effect id is not in any catalog, which is what
/// an unmounted plugin's effect looks like: the clip keeps the instance, and
/// nothing offers keyframes on parameters no one can describe.
fn rows_for(index: u32, instance: &EffectInstance) -> Vec<ParamSpec> {
    let Some(spec) = crate::effect_spec(&instance.effect_id) else {
        return Vec::new();
    };
    spec.params
        .iter()
        .enumerate()
        .map(|(slot, pspec)| row(index, spec, slot as u32, pspec))
        .collect()
}

/// Call `visit` for every animatable property of `clip` without collecting a
/// catalog vector.
///
/// Static rows are borrowed from the process catalog. Dynamic effect rows are
/// built one at a time and remain valid for the duration of the call.
pub fn visit_clip_params(clip: &Clip, mut visit: impl FnMut(&ParamSpec)) {
    for spec in super::clip_param_catalog()
        .iter()
        .filter(|spec| spec.available(clip))
    {
        visit(spec);
    }
    for (index, instance) in clip.effects.iter().enumerate() {
        let Some(spec) = crate::effect_spec(&instance.effect_id) else {
            continue;
        };
        for (slot, pspec) in spec.params.iter().enumerate() {
            let dynamic = row(index as u32, spec, slot as u32, pspec);
            visit(&dynamic);
        }
    }
}

/// Every animatable property of `clip`: the shipped rows it can reach, then one
/// row per effect parameter.
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
    fn scalar_effect() -> (&'static EffectSpec, usize) {
        crate::effect_catalog()
            .iter()
            .find_map(|spec| {
                spec.params
                    .iter()
                    .position(|p| p.kind == EffectParamKind::Scalar)
                    .map(|slot| (spec, slot))
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
            assert_eq!(row.kind, kind_of(pspec.kind));
            assert_eq!(row.range, range_of(pspec));
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
