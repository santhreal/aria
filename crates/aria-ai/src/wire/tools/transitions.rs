//! Transition tools: the junction where a clip meets the next one.


tool_group! {
    "add_transition" => AddTransition(AddTransition),
        "Add a transition at the cut where a clip meets the next clip on its track. A video, text or sticker lane takes: crossfade, dip_to_black, dip_to_white, wipe_left, wipe_right, wipe_up, wipe_down, slide. An audio lane takes: constant_power, constant_gain, exponential_fade. The clip must butt directly against a following clip. Not valid on effect, filter or adjustment lanes.";
    "remove_transition" => RemoveTransition(RemoveTransition),
        "Remove the transition at a clip's right cut. See describe_project for which clips carry one.";
    "set_transition" => SetTransition(SetTransition),
        "Set the duration in seconds of the transition at a clip's right cut (centered on the cut).";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "add_transition",
        args: |f| serde_json::json!({ "clip": f.clip, "transition": "wipe_left" }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::AddTransition {
            clip: f.clip_id(f.clip),
            transition_id: "wipe_left".into(),
        }),
        // The last clip on a lane has no cut to dissolve into.
        invalid: |f| serde_json::json!({ "clip": f.next, "transition": "wipe_left" }),
        refusal: "has no clip butting against its right edge",
    },
    ToolCase {
        tool: "remove_transition",
        args: |f| serde_json::json!({ "clip": f.clip }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::RemoveTransition {
            clip: f.clip_id(f.clip),
        }),
        invalid: |f| serde_json::json!({ "clip": f.next }),
        refusal: "has no transition at its right cut",
    },
    ToolCase {
        tool: "set_transition",
        args: |f| serde_json::json!({ "clip": f.clip, "seconds": 0.5 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetTransition {
            clip: f.clip_id(f.clip),
            duration: 12,
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "seconds": 0.0 }),
        refusal: "transition duration must be positive",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn transition_tools_lower_remap_and_refuse() {
        cases::check_group("transitions", super::CASES);
    }
}
