//! Keyframe tools: the animation curve of any animatable clip property.


tool_group! {
    "set_param_keyframe" => SetParamKeyframe(SetParamKeyframe),
        "Add or replace a keyframe on any animatable clip property at a timeline position in seconds. The `param` selector supports transform properties (position, anchor_point, scale, rotation, opacity), crop, volume, pan (−1 left … +1 right), speed, an effect `{effect:{index,param}}`, generated-shape `{shape:{param}}`, generated-text `{text:{param}}`, color-look `{look:{param}}`, and style `{style:{param}}` properties. Use `value` for scalars (including uniform scale), `position` for position/anchor_point/per-axis scale and other vec2 params, `rgba` for colors, and `rect` as [x,y,w,h] for crop. For curved position motion paths, pass optional `tangent_out` / `tangent_in` as canvas-fraction offsets from the keyframe value (After Effects–style bezier handles; position only). Effect names and valid properties are visible in describe_project.";
    "remove_param_keyframe" => RemoveParamKeyframe(RemoveParamKeyframe),
        "Remove the keyframe at a timeline position (seconds) on any animatable clip property. Use the same `param` selector as set_param_keyframe. Removing the last keyframe freezes the property at that value.";
    "set_param_constant" => SetParamConstant(SetParamConstant),
        "Set any animatable clip property to a fixed value and remove all its keyframes (stops its animation). Use the same `param` selector as set_param_keyframe, with `value` for scalars (including uniform scale), `position` for position/anchor_point/per-axis scale and other vec2 params, `rgba` for colors, or `rect` as [x,y,w,h] for crop.";
    "apply_easing_preset" => ApplyEasingPreset(ApplyEasingPreset),
        "Replace the outgoing keyframe segment at from_tick with a multi-keyframe bounce_out / elastic_out / back_out approximation. Requires a keyframe at from_tick and a following keyframe. Scalar and vec2 params only (not colors or crop). One undoable edit.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "set_param_keyframe",
        args: |f| {
            serde_json::json!({
                "clip": f.title,
                "param": "opacity",
                "at": 3.0,
                "value": 0.5,
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetParamKeyframe {
            clip: f.clip_id(f.title),
            param: aria_models::ClipParam::Opacity,
            at: f.at(3.0),
            value: aria_models::ParamValue::Scalar(0.5),
            easing: aria_models::Easing::Linear,
            tangents: None,
        }),
        // A keyframe outside the clip would animate nothing; the refusal
        // names where the clip sits.
        invalid: |f| {
            serde_json::json!({
                "clip": f.title,
                "param": "opacity",
                "at": 30.0,
                "value": 0.5,
            })
        },
        refusal: "is outside clip",
    },
    ToolCase {
        tool: "remove_param_keyframe",
        args: |f| serde_json::json!({ "clip": f.title, "param": "rotation", "at": 3.0 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::RemoveParamKeyframe {
            clip: f.clip_id(f.title),
            param: aria_models::ClipParam::Rotation,
            at: f.at(3.0),
        }),
        // An effect selector resolves against the clip's own chain.
        invalid: |f| {
            serde_json::json!({
                "clip": f.title,
                "param": { "effect": { "index": 5, "param": "radius" } },
                "at": 3.0,
            })
        },
        refusal: "has no effect at index 5 (chain length 0)",
    },
    ToolCase {
        tool: "set_param_constant",
        args: |f| serde_json::json!({ "clip": f.title, "param": "rotation", "value": 15.0 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetParamConstant {
            clip: f.clip_id(f.title),
            param: aria_models::ClipParam::Rotation,
            value: aria_models::ParamValue::Scalar(15.0),
        }),
        // A vec2 property sent as a scalar is the common model mistake; the
        // refusal names the argument that carries it.
        invalid: |f| serde_json::json!({ "clip": f.title, "param": "position", "value": 0.5 }),
        refusal: "need the 'position' argument as [x, y]",
    },
    ToolCase {
        tool: "apply_easing_preset",
        args: |f| {
            serde_json::json!({
                "clip": f.title,
                "param": "opacity",
                "from_tick": 3.0,
                "preset": "bounce_out",
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::ApplyEasingPreset {
            clip: f.clip_id(f.title),
            param: aria_models::ClipParam::Opacity,
            at: f.at(3.0),
            preset: aria_models::PiecewiseEasingPreset::BounceOut,
        }),
        invalid: |f| {
            serde_json::json!({
                "clip": f.title,
                "param": "crop",
                "from_tick": 3.0,
                "preset": "bounce_out",
            })
        },
        refusal: "supports scalar/vec2 transform",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn keyframe_tools_lower_remap_and_refuse() {
        cases::check_group("animation", super::CASES);
    }
}
