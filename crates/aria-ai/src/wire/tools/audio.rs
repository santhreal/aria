//! Audio tools: what a clip sounds like and what it counts as in the mix.


tool_group! {
    "set_clip_audio" => SetClipAudio(SetClipAudio),
        "Set a clip's volume (0.0 mutes, 1.0 unchanged, 2.0 doubles) and/or fade-in/fade-out durations in seconds. Omitted fields keep their current value. A video clip keeps its own sound, so target it directly.";
    "set_denoise" => SetDenoise(SetDenoise),
        "Turn noise reduction on or off for a media clip: runs its audio through a speech-preserving denoiser that suppresses steady background noise (hum, hiss, air-conditioning, room tone) while keeping voice. Use on clips with a constant background drone. A video clip keeps its own sound, so target it directly. Not valid for generated clips.";
    "set_audio_role" => SetAudioRole(SetAudioRole),
        "Tag or untag what an audio-lane clip is: music, sfx, voiceover, or extracted. Pass null for role to clear the tag. Audio-track clips only.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "set_clip_audio",
        args: |f| serde_json::json!({ "clip": f.clip, "volume": 0.5, "fade_in": 1.0 }),
        // An omitted fade keeps the clip's current one rather than zeroing it.
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipAudio {
            clip: f.clip_id(f.clip),
            volume: Some(0.5),
            fade_in: f.at(1.0),
            fade_out: f.at(0.0),
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "volume": 20.0 }),
        refusal: "volume must be between 0 (mute) and 10",
    },
    ToolCase {
        tool: "set_denoise",
        args: |f| serde_json::json!({ "clip": f.clip, "denoise": true }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetClipDenoise {
            clip: f.clip_id(f.clip),
            denoise: true,
        }),
        invalid: |f| serde_json::json!({ "clip": f.title, "denoise": true }),
        refusal: "set_denoise only works on media",
    },
    ToolCase {
        tool: "set_audio_role",
        args: |f| serde_json::json!({ "clip": f.sound, "role": "music" }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetAudioRole {
            clip: f.clip_id(f.sound),
            role: Some(aria_models::AudioRole::Music),
        }),
        // A picture-lane clip has no mix role, even though it carries sound.
        invalid: |f| serde_json::json!({ "clip": f.clip, "role": "music" }),
        refusal: "set_audio_role only works on audio clips",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn audio_tools_lower_remap_and_refuse() {
        cases::check_group("audio", super::CASES);
    }
}
