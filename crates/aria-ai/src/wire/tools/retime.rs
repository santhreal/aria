//! Retiming tools: constant speed, speed ramps, and the pitch lock that
//! rides along with them.


tool_group! {
    "set_clip_speed" => SetClipSpeed(SetClipSpeed),
        "Change a media clip's playback speed (2.0 = double speed, 0.5 = slow motion) and/or play it in reverse. The clip's timeline length re-derives from the speed; its audio time-stretches to match (pitch preserved by default). Not valid for generated clips.";
    "set_speed_curve" => SetSpeedCurve(SetSpeedCurve),
        "Apply a CapCut-style speed ramp to a media clip so its speed varies across its length: preset 'ramp_up' (slow to fast), 'ramp_down' (fast to slow), 'montage' (fast/slow/fast), 'hero' (slow-mo on the action), or 'bullet' (fast/hard-slow/fast). Omit preset to clear the ramp. The clip's length re-derives from the ramp's average speed; its audio time-stretches along the ramp. Not valid for generated clips.";
    "set_clip_pitch" => SetClipPitch(SetClipPitch),
        "Lock or unlock a retimed media clip's pitch. preserve_pitch true (default) keeps the original pitch while time-stretching; false lets pitch follow speed (the chipmunk effect when sped up). Only affects a clip that is retimed (speed change, reverse, or ramp). Not valid for generated clips.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "set_clip_speed",
        args: |f| serde_json::json!({ "clip": f.clip, "speed": 2.0 }),
        // Speeds snap to hundredths so the engine keeps exact rational math.
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipSpeed {
            clip: f.clip_id(f.clip),
            speed: aria_models::Rational::new(2, 1),
            reversed: false,
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "speed": 0.0 }),
        refusal: "speed must be between 0.05 and 100",
    },
    ToolCase {
        tool: "set_speed_curve",
        args: |f| serde_json::json!({ "clip": f.clip, "preset": "ramp_up" }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetSpeedCurve {
            clip: f.clip_id(f.clip),
            curve: aria_models::speed_preset("ramp_up"),
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "preset": "warp" }),
        refusal: "unknown speed ramp preset 'warp'",
    },
    ToolCase {
        tool: "set_clip_pitch",
        args: |f| serde_json::json!({ "clip": f.clip, "preserve_pitch": false }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipPitch {
            clip: f.clip_id(f.clip),
            preserve_pitch: false,
        }),
        // A generated clip has no source audio to stretch.
        invalid: |f| serde_json::json!({ "clip": f.title, "preserve_pitch": false }),
        refusal: "set_clip_pitch only works on media",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn retime_tools_lower_remap_and_refuse() {
        cases::check_group("retime", super::CASES);
    }
}
