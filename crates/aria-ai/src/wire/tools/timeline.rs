//! Timeline tools: the edits that move other clips — ripple, three-point
//! and four-point placement, and link groups.


tool_group! {
    "ripple_delete" => RippleDelete(RippleDelete),
        "Remove a clip and slide later clips on its track left to close the gap.";
    "shift_clips" => ShiftClips(ShiftClips),
        "Shift every clip on a track starting at/after a position by a signed number of seconds.";
    "ripple_insert" => RippleInsert(RippleInsert),
        "Insert a trimmed range of media at a timeline position, shifting later clips right to make room. Times are in seconds.";
    "overwrite_clip" => OverwriteClip(OverwriteClip),
        "Place a trimmed range of media at a timeline position, covering whatever is already in the span it occupies. Nothing downstream moves. Times are in seconds.";
    "fit_to_fill" => FitToFill(FitToFill),
        "Place a trimmed range of media at a timeline position and retime it to fill an exact timeline duration — a four-point edit. `ripple` pushes later clips right to open the span; otherwise the span is covered. Times are in seconds.";
    "link_clips" => LinkClips(LinkClips),
        "Link two or more clips so they select, move, and trim together (replaces their previous links).";
    "unlink_clips" => UnlinkClips(UnlinkClips),
        "Dissolve every link group touched by one or more clip ids. Naming any member clears the complete group; distinct members of the same group are coalesced.";
}

#[cfg(test)]
use super::cases::{Expect, R24, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "ripple_delete",
        args: |f| serde_json::json!({ "clip": f.clip }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::RippleDelete {
            clip: f.clip_id(f.clip),
        }),
        invalid: |_| serde_json::json!({ "clip": 9_091 }),
        refusal: "clip 9091 does not exist",
    },
    ToolCase {
        tool: "shift_clips",
        args: |f| serde_json::json!({ "track": f.main, "from": 0.0, "delta": 1.0 }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::ShiftClips {
            track: f.track_id(f.main),
            from: f.at(0.0),
            delta: f.at(1.0),
        }),
        // A delta that snaps to no frames would look applied and move
        // nothing, so it is refused with the rounding named.
        invalid: |f| serde_json::json!({ "track": f.main, "from": 0.0, "delta": 0.001 }),
        refusal: "rounds to zero frames",
    },
    ToolCase {
        tool: "ripple_insert",
        args: |f| {
            serde_json::json!({
                "track": f.main,
                "media": f.media,
                "source_start": 0.0,
                "source_duration": 2.0,
                "at": 1.0,
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::RippleInsert {
            track: f.track_id(f.main),
            media: f.media_id(f.media),
            source: aria_models::TimeRange::at_rate(0, 48, R24),
            at: f.at(1.0),
        }),
        invalid: |f| {
            serde_json::json!({
                "track": f.text,
                "media": f.media,
                "source_start": 0.0,
                "source_duration": 2.0,
                "at": 1.0,
            })
        },
        refusal: "media clips need a video or audio track",
    },
    ToolCase {
        tool: "overwrite_clip",
        args: |f| {
            serde_json::json!({
                "track": f.main,
                "media": f.media,
                "source_start": 3.0,
                "source_duration": 2.0,
                "at": 4.0,
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::OverwriteClip {
            track: f.track_id(f.main),
            media: f.media_id(f.media),
            source: aria_models::TimeRange::at_rate(72, 48, R24),
            at: f.at(4.0),
        }),
        invalid: |f| {
            serde_json::json!({
                "track": f.main,
                "media": f.media,
                "source_start": 3.0,
                "source_duration": 0.0,
                "at": 4.0,
            })
        },
        refusal: "source_duration must be positive",
    },
    ToolCase {
        tool: "fit_to_fill",
        args: |f| {
            serde_json::json!({
                "track": f.overlay,
                "media": f.media,
                "source_start": 0.0,
                "source_duration": 2.0,
                "at": 0.0,
                "duration": 4.0,
                "ripple": false,
            })
        },
        expect: Expect::Edit(|f| aria_commands::EditCommand::FitToFill {
            track: f.track_id(f.overlay),
            media: f.media_id(f.media),
            source: aria_models::TimeRange::at_rate(0, 48, R24),
            at: f.at(0.0),
            duration: f.at(4.0),
            ripple: false,
        }),
        invalid: |f| {
            serde_json::json!({
                "track": f.overlay,
                "media": f.media,
                "source_start": 0.0,
                "source_duration": 2.0,
                "at": 0.0,
                "duration": 0.0,
                "ripple": false,
            })
        },
        refusal: "duration must be positive",
    },
    ToolCase {
        tool: "link_clips",
        args: |f| serde_json::json!({ "clips": [f.clip, f.next] }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::LinkClips {
            clips: vec![f.clip_id(f.clip), f.clip_id(f.next)],
        }),
        invalid: |f| serde_json::json!({ "clips": [f.clip] }),
        refusal: "link_clips needs at least two clip ids",
    },
    ToolCase {
        tool: "unlink_clips",
        args: |f| serde_json::json!({ "clips": [f.title] }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::UnlinkClips {
            clips: vec![f.clip_id(f.title)],
        }),
        // Nothing to dissolve is a refusal, not a silent no-op edit.
        invalid: |f| serde_json::json!({ "clips": [f.clip] }),
        refusal: "found no linked clips",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn timeline_tools_lower_remap_and_refuse() {
        cases::check_group("timeline", super::CASES);
    }
}
