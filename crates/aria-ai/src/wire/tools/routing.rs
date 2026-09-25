//! Routing tools: the mixer. A lane's fader, where its sum goes, and the
//! copies of it other buses take.


tool_group! {
    "set_track_gain" => SetTrackGain(SetTrackGain),
        "Ride a lane's fader, as linear gain: 0 silences the lane, 1 is unity, 4 is +12 dB and is the maximum. Applies to audio lanes, video lanes carrying their own sound, and submix buses. To change one clip instead of the whole lane, use set_clip_audio.";
    "set_track_output" => SetTrackOutput(SetTrackOutput),
        "Point a lane's output at a submix bus, or pass null for to to send it straight to the master. Only a submix lane can receive it (add_track with kind 'submix' creates one), and a route that would feed a lane back into itself is refused. Moving the output is how a group of lanes comes under one fader.";
    "add_send" => AddSend(AddSend),
        "Add a send: a copy of a lane, at its own level, added to a submix bus — how a reverb or a parallel chain is fed while the lane still reaches the master. Set pre_fader true to take the copy before the lane's own fader, so riding the lane down does not thin the send. One send per lane per bus; use set_send to change an existing one.";
    "set_send" => SetSend(SetSend),
        "Change an existing send's level and/or whether it is taken pre-fader. The send is addressed by its index on the lane, counting from 0 in the order they were added — describe_project lists a lane's sends.";
    "remove_send" => RemoveSend(RemoveSend),
        "Drop a send off a lane, addressed by its index. The lane keeps its own output.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "set_track_gain",
        args: |f| serde_json::json!({ "track": f.audio, "gain": 0.5 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetTrackGain {
            track: f.track_id(f.audio),
            gain: 0.5,
        }),
        // A level is refused rather than clamped: the number in the mix is the
        // number that was asked for.
        invalid: |f| serde_json::json!({ "track": f.audio, "gain": 9.0 }),
        refusal: "gain 9 is outside 0..=4",
    },
    ToolCase {
        tool: "set_track_output",
        args: |f| serde_json::json!({ "track": f.audio, "to": f.bus }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetTrackOutput {
            track: f.track_id(f.audio),
            to: Some(f.track_id(f.bus)),
        }),
        // Only a bus lane sums other lanes; anything else is a lane with clips.
        invalid: |f| serde_json::json!({ "track": f.audio, "to": f.main }),
        refusal: "not a submix bus",
    },
    ToolCase {
        tool: "add_send",
        // The main lane carries its own sound and holds no send yet.
        args: |f| serde_json::json!({ "track": f.main, "to": f.bus, "pre_fader": true }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::AddSend {
            track: f.track_id(f.main),
            to: f.track_id(f.bus),
            level: 1.0,
            pre_fader: true,
        }),
        // A1 already sends to the bus: a second copy is a level, not a route.
        invalid: |f| serde_json::json!({ "track": f.audio, "to": f.bus }),
        refusal: "already sends to bus",
    },
    ToolCase {
        tool: "set_send",
        args: |f| serde_json::json!({ "track": f.audio, "index": 0, "level": 0.25 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetSend {
            track: f.track_id(f.audio),
            index: 0,
            level: Some(0.25),
            pre_fader: None,
        }),
        invalid: |f| serde_json::json!({ "track": f.audio, "index": 7, "level": 1.0 }),
        refusal: "has no send 7",
    },
    ToolCase {
        tool: "remove_send",
        args: |f| serde_json::json!({ "track": f.audio, "index": 0 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::RemoveSend {
            track: f.track_id(f.audio),
            index: 0,
        }),
        invalid: |f| serde_json::json!({ "track": f.main, "index": 0 }),
        refusal: "it has no sends",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn routing_tools_lower_remap_and_refuse() {
        cases::check_group("routing", super::CASES);
    }
}
