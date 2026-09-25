//! Clip tools: putting content on a lane, copying it, and the single-clip
//! edits (split, trim, move, remove).


tool_group! {
    "add_clip" => AddClip(AddClip),
        "Place a trimmed range of an imported media file on a video or audio track. Times are in seconds.";
    "extract_audio" => ExtractAudio(ExtractAudio),
        "Detach a video clip's embedded sound onto an existing unlocked audio track, preserving its exact placement and audio/retime settings. The track id is required: call add_track with kind audio first when needed, then pass the returned id. Keeping the target explicit lets planned track ids remap correctly during replay.";
    "duplicate_clip" => DuplicateClip(DuplicateClip),
        "Make a deep property-preserving copy of one clip at an explicit target track and start (timeline seconds). The copy gets a fresh unlinked clip id. This tool does not ripple clips or search for space; choose a non-overlapping destination explicitly.";
    "add_generated" => AddGenerated(AddGenerated),
        "Place a generated clip (text title, solid color, or shape) on a matching track. Times are in seconds.";
    "set_generator" => SetGenerator(SetGenerator),
        "Replace a generated clip's content: change a title's text (styling preserved) or recolor a solid/shape. Not valid for media clips.";
    "split_clip" => SplitClip(SplitClip),
        "Split a clip at a timeline position (seconds) into two abutting clips.";
    "trim_clip" => TrimClip(TrimClip),
        "Re-place / trim a clip to a new timeline start and duration in seconds. Trimming a media clip's head advances its source in-point.";
    "move_clip" => MoveClip(MoveClip),
        "Move a clip to a track at a new start time (seconds), keeping its duration.";
    "remove_clip" => RemoveClip(RemoveClip),
        "Remove a clip, leaving a gap where it sat.";
}

#[cfg(test)]
use super::cases::{Expect, R24, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "add_clip",
        args: |f| {
            serde_json::json!({
                "track": f.overlay,
                "media": f.media,
                "source_start": 1.0,
                "source_duration": 2.0,
                "start": 0.0,
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::AddClip {
            track: f.track_id(f.overlay),
            media: f.media_id(f.media),
            source: aria_models::TimeRange::at_rate(24, 48, R24),
            start: f.at(0.0),
        }),
        // The source range is checked against the media's real extent, so
        // the model learns the length instead of guessing again.
        invalid: |f| {
            serde_json::json!({
                "track": f.overlay,
                "media": f.media,
                "source_start": 59.0,
                "source_duration": 4.0,
                "start": 0.0,
            })
        },
        refusal: "exceeds media",
    },
    ToolCase {
        tool: "extract_audio",
        args: |f| serde_json::json!({ "clip": f.clip, "track": f.audio }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::ExtractAudio {
            clip: f.clip_id(f.clip),
            to_track: Some(f.track_id(f.audio)),
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "track": f.main }),
        refusal: "extract_audio needs an audio track",
    },
    ToolCase {
        tool: "duplicate_clip",
        args: |f| serde_json::json!({ "clip": f.clip, "to_track": f.overlay, "start": 0.0 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::DuplicateClip {
            clip: f.clip_id(f.clip),
            to_track: f.track_id(f.overlay),
            start: f.at(0.0),
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "to_track": f.text, "start": 0.0 }),
        refusal: "cannot be duplicated onto track",
    },
    ToolCase {
        tool: "add_generated",
        args: |f| {
            serde_json::json!({
                "track": f.text,
                "generator": { "type": "text", "content": "HELLO" },
                "start": 6.0,
                "duration": 2.0,
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::AddGenerated {
            track: f.track_id(f.text),
            generator: aria_models::Generator::text("HELLO"),
            timeline: aria_models::TimeRange::at_rate(144, 48, R24),
        }),
        // The historical failure: a bare string where the tagged object goes.
        invalid: |f| {
            serde_json::json!({
                "track": f.text,
                "generator": "HELLO",
                "start": 6.0,
                "duration": 2.0,
            })
        },
        refusal: "{\"type\": \"text\", \"content\": \"Hello\"}",
    },
    ToolCase {
        tool: "set_generator",
        args: |f| {
            serde_json::json!({
                "clip": f.title,
                "generator": { "type": "text", "content": "OUTRO" },
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetGenerator {
            clip: f.clip_id(f.title),
            generator: aria_models::Generator::text("OUTRO"),
        }),
        invalid: |f| {
            serde_json::json!({
                "clip": f.clip,
                "generator": { "type": "text", "content": "OUTRO" },
            })
        },
        refusal: "set_generator only works on generated",
    },
    ToolCase {
        tool: "split_clip",
        args: |f| serde_json::json!({ "clip": f.clip, "at": 5.0 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SplitClip {
            clip: f.clip_id(f.clip),
            at: f.at(5.0),
        }),
        // A split on the clip's own edge would produce an empty half.
        invalid: |f| serde_json::json!({ "clip": f.clip, "at": 0.0 }),
        refusal: "is not strictly inside clip",
    },
    ToolCase {
        tool: "trim_clip",
        args: |f| serde_json::json!({ "clip": f.clip, "start": 1.0, "duration": 3.0 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::TrimClip {
            clip: f.clip_id(f.clip),
            timeline: aria_models::TimeRange::at_rate(24, 72, R24),
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "start": 1.0, "duration": 0.0 }),
        refusal: "duration must be positive",
    },
    ToolCase {
        tool: "move_clip",
        args: |f| serde_json::json!({ "clip": f.clip, "to_track": f.overlay, "start": 20.0 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::MoveClip {
            clip: f.clip_id(f.clip),
            to_track: f.track_id(f.overlay),
            start: f.at(20.0),
        }),
        invalid: |f| serde_json::json!({ "clip": f.clip, "to_track": f.text, "start": 20.0 }),
        refusal: "cannot live on track",
    },
    ToolCase {
        tool: "remove_clip",
        args: |f| serde_json::json!({ "clip": f.clip }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::RemoveClip {
            clip: f.clip_id(f.clip),
        }),
        invalid: |_| serde_json::json!({ "clip": 9_090 }),
        refusal: "clip 9090 does not exist",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn clip_tools_lower_remap_and_refuse() {
        cases::check_group("clips", super::CASES);
    }
}
