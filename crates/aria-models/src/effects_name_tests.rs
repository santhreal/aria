//! An effect instance's name and its reset.
//!
//! The class closed here: a name the model stores and the inspector cannot
//! draw, and a reset that drops what is not a parameter. A name is validated
//! where every instance is, in `EffectInstance::validate`, so a project file,
//! a plugin and the assistant meet the same limit as the rename field. A reset
//! rebuilds the instance from the catalog and has to carry the three fields
//! that are not parameters across.
//!
//! Not covered: the inspector drawing the name, which `aria-ui` drives.

use super::*;

/// The first scalar slot of a shipped effect, and a value for it that is not
/// its default.
fn a_scalar_slot() -> (String, usize, f32) {
    shipped_effect_catalog()
        .iter()
        .find_map(|spec| {
            let slot = spec
                .params
                .iter()
                .position(|p| p.kind == EffectParamKind::Scalar && p.min < p.max)?;
            let param = &spec.params[slot];
            let value = if param.max == param.default {
                param.min
            } else {
                param.max
            };
            Some((spec.id.clone(), slot, value))
        })
        .expect("the catalog ships an effect with a scalar slot")
}

fn named(name: &str) -> EffectInstance {
    let (id, _, _) = a_scalar_slot();
    EffectInstance {
        name: Some(name.to_owned()),
        ..EffectInstance::new(id)
    }
}

#[test]
fn an_unnamed_instance_writes_no_name_and_a_named_one_reads_back() {
    let (id, _, _) = a_scalar_slot();
    let plain = serde_json::to_value(EffectInstance::new(id.clone())).expect("serializes");
    assert_eq!(plain, serde_json::json!({ "effect_id": id }));

    let old: EffectInstance =
        serde_json::from_value(serde_json::json!({ "effect_id": id })).expect("parses");
    assert_eq!(old.name, None, "a file from before names has no name");

    let given = named("Soft glow");
    let text = serde_json::to_string(&given).expect("serializes");
    let back: EffectInstance = serde_json::from_str(&text).expect("parses");
    assert_eq!(back, given);
    assert_eq!(back.validate().map_err(|err| err.to_string()), Ok(()));
}

#[test]
fn a_name_the_inspector_cannot_draw_is_refused() {
    let longest = "n".repeat(EFFECT_NAME_MAX_CHARS);
    let wide = "é".repeat(EFFECT_NAME_MAX_CHARS);
    for accepted in ["Soft glow", "2", longest.as_str(), wide.as_str()] {
        assert!(
            named(accepted).validate().is_ok(),
            "{accepted:?} is a name the field can type"
        );
    }
    let over = "n".repeat(EFFECT_NAME_MAX_CHARS + 1);
    let wide_over = format!("{wide}é");
    for refused in [
        "",
        "   ",
        " glow",
        "glow ",
        "tab\tname",
        "line\nbreak",
        "bell\u{7}",
        over.as_str(),
        wide_over.as_str(),
    ] {
        let err = named(refused)
            .validate()
            .expect_err("a name the model cannot draw is refused");
        assert!(
            err.to_string().contains("effect name"),
            "{refused:?} was refused for another reason: {err}"
        );
    }
}

#[test]
fn the_drawn_name_is_the_given_one_else_the_label_else_the_id() {
    let (id, _, _) = a_scalar_slot();
    let label = effect_spec(&id).expect("shipped").label.clone();
    assert_eq!(EffectInstance::new(id).display_name(), label);
    assert_eq!(named("Soft glow").display_name(), "Soft glow");
    let gone: EffectInstance =
        serde_json::from_value(serde_json::json!({ "effect_id": "no_such_effect" }))
            .expect("parses");
    assert_eq!(gone.display_name(), "no_such_effect");
}

#[test]
fn a_reset_keeps_the_name_the_switch_and_the_kind_and_nothing_else() {
    let (id, slot, value) = a_scalar_slot();
    let mut edited = named("Soft glow");
    edited.bypass = true;
    edited
        .set_param_constant(slot, value)
        .expect("the slot takes a value in range");
    assert_ne!(edited, named("Soft glow"), "the edit landed");

    let reset = edited.reset();
    assert_eq!(
        reset,
        EffectInstance {
            name: Some("Soft glow".to_owned()),
            bypass: true,
            ..EffectInstance::new(id)
        }
    );

    let keyed = {
        let mut keyed = named("Keyed");
        keyed
            .set_param_keyframe(slot, 0, value, Easing::Linear)
            .expect("the slot keys");
        keyed
    };
    assert!(keyed.reset().params.is_empty(), "a reset leaves a keyframe");

    // A generator whose plugin is gone keeps its flag, which is what draws
    // the clip as a missing generator rather than as an empty effect.
    let gone: EffectInstance = serde_json::from_value(
        serde_json::json!({ "effect_id": "no_such_generator", "generator": true }),
    )
    .expect("parses");
    assert!(gone.reset().generator);
}
