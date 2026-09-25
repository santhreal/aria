//! Effect tools: the per-clip effect chain and its parameters.


tool_group! {
    "add_effect" => AddEffect(AddEffect),
        "Add a visual effect to a clip's effect chain. Available effects: gaussian_blur (param 'radius'), vignette (param 'amount'). Effects render on the placed layer, in chain order. Not valid on audio tracks.";
    "remove_effect" => RemoveEffect(RemoveEffect),
        "Remove an effect from a clip's chain by its index (0 = first). See describe_project for a clip's current effects.";
    "move_effect" => MoveEffect(MoveEffect),
        "Reorder a clip's effect chain. Both from_index and to_index address the current pre-move chain; to_index is the effect's final index. See describe_project for the current order.";
    "set_effect_param" => SetEffectParam(SetEffectParam),
        "Set a parameter of an effect on a clip. Use `value` for scalars (e.g. gaussian_blur 'radius'), `position` for vec2 params (e.g. color_overlay 'offset'), and `rgba` for color params (e.g. duotone 'shadow_color'). Use describe_project to see effect indices and current params.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "add_effect",
        args: |f| serde_json::json!({ "clip": f.clip, "effect": "vignette" }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::AddEffect {
            clip: f.clip_id(f.clip),
            effect_id: "vignette".into(),
        }),
        // An unknown id comes back with the catalog, so the model can pick.
        invalid: |f| serde_json::json!({ "clip": f.clip, "effect": "kaleidoscope" }),
        refusal: "unknown effect 'kaleidoscope'",
    },
    ToolCase {
        tool: "remove_effect",
        args: |f| serde_json::json!({ "clip": f.clip, "index": 1 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::RemoveEffect {
            clip: f.clip_id(f.clip),
            index: 1,
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "index": 9 }),
        refusal: "has no effect at index 9 (chain length 2)",
    },
    ToolCase {
        tool: "move_effect",
        args: |f| serde_json::json!({ "clip": f.clip, "from_index": 0, "to_index": 1 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::MoveEffect {
            clip: f.clip_id(f.clip),
            from_index: 0,
            to_index: 1,
        }),
        // A move onto its own index is refused rather than applied as a
        // no-op edit that would still cost an undo step.
        invalid: |f| serde_json::json!({ "clip": f.clip, "from_index": 1, "to_index": 1 }),
        refusal: "is already at index 1",
    },
    ToolCase {
        tool: "set_effect_param",
        args: |f| {
            serde_json::json!({
                "clip": f.clip,
                "index": 0,
                "param": "radius",
                "value": 8.0,
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetEffectParam {
            clip: f.clip_id(f.clip),
            index: 0,
            param: 0,
            value: 8.0,
        }),
        invalid: |f| {
            serde_json::json!({
                "clip": f.clip,
                "index": 0,
                "param": "radius",
                "value": 999.0,
            })
        },
        refusal: "radius must be between 0 and 64",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn effect_tools_lower_remap_and_refuse() {
        cases::check_group("effects", super::CASES);
    }
}
