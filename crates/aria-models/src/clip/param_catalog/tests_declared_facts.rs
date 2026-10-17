//! What a row *declares* against what holds: its bounds against the bounds the
//! model enforces, and its group against the panels that draw rows by group.

use super::*;

#[test]
fn a_declared_range_is_the_range_the_model_enforces() {
    // Both directions matter. A row that claims a bound the model does not
    // enforce lets a surface clamp where nothing would have refused; a row that
    // leaves a side open where the model *does* refuse lets a stepper walk into
    // an error toast. So a declared side must be enforced at the edge, and an
    // undeclared side must accept a far value.
    const FAR: f32 = 1.0e6;
    let fixtures = [
        ("dressed media", dressed_media_project()),
        ("dressed title", generated_project(dressed_text_clip())),
        ("dressed shape", generated_project(dressed_shape_clip())),
    ];
    let mut checked = 0_usize;
    let mut wrong: Vec<String> = Vec::new();

    for (name, (mut project, clip)) in fixtures {
        let specs = {
            let model_clip = project.timeline().clip(clip).expect("clip");
            clip_params_for(model_clip)
        };
        for spec in specs {
            if spec.scope != ParamScope::Visual {
                continue;
            }
            // A vector row declares one range for both components, so the sweep
            // writes the same value to each: a bound the model enforces per
            // component is enforced when both carry it.
            let value_of: fn(f32) -> ParamValue = match spec.kind {
                ParamKind::Scalar => ParamValue::Scalar,
                ParamKind::Vec2 => |v| ParamValue::Vec2([v, v]),
                ParamKind::Vec3 => |v| ParamValue::Vec3([v, v, v]),
                _ => continue,
            };
            {
                let model_clip = project.timeline().clip(clip).expect("clip");
                if !spec.available(model_clip) {
                    continue;
                }
            }
            checked += 1;
            let write = |project: &mut crate::Project, value: f32| {
                project
                    .set_param_keyframe(
                        clip,
                        spec.param,
                        crate::RationalTime::new(0, FPS_24),
                        value_of(value),
                        Easing::Linear,
                        None,
                    )
                    .is_ok()
            };
            match spec.range.max {
                Some(max) => {
                    if write(&mut project, max + 1.0) {
                        wrong.push(format!(
                            "{name}: {} declares a max of {max}, the model accepted {}",
                            spec.id,
                            max + 1.0
                        ));
                    }
                    if !write(&mut project, max) {
                        wrong.push(format!(
                            "{name}: {} declares a max of {max}, the model refused it",
                            spec.id
                        ));
                    }
                }
                None => {
                    if !write(&mut project, FAR) {
                        wrong.push(format!(
                            "{name}: {} declares no maximum, the model refused {FAR}",
                            spec.id
                        ));
                    }
                }
            }
            match spec.range.min {
                Some(min) => {
                    if write(&mut project, min - 1.0) {
                        wrong.push(format!(
                            "{name}: {} declares a min of {min}, the model accepted {}",
                            spec.id,
                            min - 1.0
                        ));
                    }
                    if !write(&mut project, min) {
                        wrong.push(format!(
                            "{name}: {} declares a min of {min}, the model refused it",
                            spec.id
                        ));
                    }
                }
                None => {
                    if !write(&mut project, -FAR) {
                        wrong.push(format!(
                            "{name}: {} declares no minimum, the model refused {}",
                            spec.id, -FAR
                        ));
                    }
                }
            }
        }
    }

    assert!(checked > 20, "the sweep only reached {checked} rows");
    assert!(
        wrong.is_empty(),
        "{} rows declare a range the model does not enforce:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn every_row_declares_a_group_a_panel_knows() {
    // The group is what a panel draws its rows by. An unknown group means a row
    // exists that nothing renders, which is the defect this registry closes,
    // moved one level up.
    const GROUPS: &[&str] = &[
        "transform",
        "framing",
        "audio",
        "filter",
        "lut",
        "grade",
        "chroma",
        "matte",
        "shadow",
        "glow",
        "outline",
        "background",
        "text",
        "shape",
    ];
    for spec in clip_param_catalog() {
        assert!(
            GROUPS.contains(&spec.group.as_ref()),
            "{} declares group {:?}, which no panel owns",
            spec.id,
            spec.group
        );
    }
    // Every declared group is used: a group nobody claims is a panel that lost
    // its rows.
    for group in GROUPS {
        assert!(
            clip_param_catalog().iter().any(|spec| spec.group == *group),
            "no row belongs to group {group}"
        );
    }
}
