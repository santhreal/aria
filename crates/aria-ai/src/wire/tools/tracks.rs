//! Track tools: creating a lane, removing one, and the per-lane switches.


tool_group! {
    "add_track" => AddTrack(AddTrack),
        "Add a track to the timeline stack (video, audio, text, sticker overlay, or submix bus). Lanes keep CapCut zones: submix buses below everything, then audio, then the main video track, overlays above it, text on top — the index only orders a lane within its zone. A submix holds no clips: it sums the lanes routed into it (set_track_output, add_send) under one fader.";
    "remove_track" => RemoveTrack(RemoveTrack),
        "Remove a track and any clips still on it. The main track (marked in describe_project) is permanent and cannot be removed.";
    "set_track_enabled" => SetTrackEnabled(SetTrackEnabled),
        "Show or hide a visual track in the composite.";
    "set_track_muted" => SetTrackMuted(SetTrackMuted),
        "Mute or unmute an audio track.";
    "set_track_locked" => SetTrackLocked(SetTrackLocked),
        "Lock or unlock a track's clips against editing.";
    "set_track_targeted" => SetTrackTargeted(SetTrackTargeted),
        "Pick whether a track is the destination for source-monitor edits (insert, overwrite, fit to fill) and media drops of its kind. Clearing it sends those edits to another lane of the same kind.";
    "set_track_sync_lock" => SetTrackSyncLock(SetTrackSyncLock),
        "Pick whether a track rides along with an insert on another track, so picture and sound stay in step. Clearing it pins the track's clips in place.";
}

#[cfg(test)]
use super::cases::{Expect, ToolCase};

#[cfg(test)]
pub(super) const CASES: &[ToolCase] = &[
    ToolCase {
        tool: "add_track",
        args: |_| serde_json::json!({ "kind": "audio", "name": "Music", "index": 0 }),
        expect: Expect::Edit(|_| aria_commands::EditCommand::AddTrack {
            kind: aria_models::TrackKind::Audio,
            name: "Music".into(),
            index: Some(0),
            pinned: false,
        }),
        invalid: |_| serde_json::json!({ "kind": "hologram", "name": "Music" }),
        refusal: "unknown variant `hologram`",
    },
    ToolCase {
        tool: "remove_track",
        args: |f| serde_json::json!({ "track": f.overlay }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::RemoveTrack {
            track: f.track_id(f.overlay),
        }),
        // The main lane is permanent; the refusal says why rather than
        // leaving the model to retry the same call.
        invalid: |f| serde_json::json!({ "track": f.main }),
        refusal: "is the main track and cannot be removed",
    },
    ToolCase {
        tool: "set_track_enabled",
        args: |f| serde_json::json!({ "track": f.overlay, "enabled": false }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetTrackEnabled {
            track: f.track_id(f.overlay),
            enabled: false,
        }),
        invalid: |_| serde_json::json!({ "track": 4_040, "enabled": true }),
        refusal: "track 4040 does not exist",
    },
    ToolCase {
        tool: "set_track_muted",
        args: |f| serde_json::json!({ "track": f.audio, "muted": true }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetTrackMuted {
            track: f.track_id(f.audio),
            muted: true,
        }),
        invalid: |f| serde_json::json!({ "track": f.audio }),
        refusal: "missing field `muted`",
    },
    ToolCase {
        tool: "set_track_locked",
        args: |f| serde_json::json!({ "track": f.text, "locked": true }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetTrackLocked {
            track: f.track_id(f.text),
            locked: true,
        }),
        invalid: |f| serde_json::json!({ "track": f.text, "locked": "yes" }),
        refusal: "invalid arguments for set_track_locked",
    },
    ToolCase {
        tool: "set_track_targeted",
        args: |f| serde_json::json!({ "track": f.main, "targeted": true }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetTrackTargeted {
            track: f.track_id(f.main),
            targeted: true,
        }),
        invalid: |_| serde_json::json!({ "track": 4_041, "targeted": false }),
        refusal: "track 4041 does not exist",
    },
    ToolCase {
        tool: "set_track_sync_lock",
        args: |f| serde_json::json!({ "track": f.audio, "sync_lock": false }),
        expect: Expect::Edit(|f| aria_commands::EditCommand::SetTrackSyncLock {
            track: f.track_id(f.audio),
            sync_lock: false,
        }),
        invalid: |f| serde_json::json!({ "track": f.audio, "sync_locked": false }),
        refusal: "invalid arguments for set_track_sync_lock",
    },
];

#[cfg(test)]
mod tests {
    use super::super::cases;

    #[test]
    fn track_tools_lower_remap_and_refuse() {
        cases::check_group("tracks", super::CASES);
    }
}
