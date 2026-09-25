use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::MAX_MULTI_CLIP_REFS;

/// Split a clip at a timeline position into two abutting clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SplitClip {
    pub clip: u64,
    /// Timeline position of the cut, in seconds. Must fall strictly inside
    /// the clip.
    pub at: f64,
}

/// Re-place / trim a clip to a new timeline range. Trimming the head of a
/// media clip advances its source in-point to match (like dragging a trim
/// handle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrimClip {
    pub clip: u64,
    /// New timeline start of the clip, in seconds.
    pub start: f64,
    /// New clip length, in seconds.
    pub duration: f64,
}

/// Move a clip to a track at a new start time, keeping its duration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MoveClip {
    pub clip: u64,
    /// Destination track id (may be the clip's current track).
    pub to_track: u64,
    /// New timeline start, in seconds.
    pub start: f64,
}

/// Remove a clip, leaving a gap where it sat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RemoveClip {
    pub clip: u64,
}

/// Remove a track and any clips still on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RemoveTrack {
    pub track: u64,
}

/// Toggle whether a visual track contributes to the composite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetTrackEnabled {
    pub track: u64,
    pub enabled: bool,
}

/// Toggle whether an audio track is silenced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetTrackMuted {
    pub track: u64,
    pub muted: bool,
}

/// Toggle whether a track's clips are editable (selection / move / trim).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetTrackLocked {
    pub track: u64,
    pub locked: bool,
}

/// Toggle whether a track is the destination for a source-monitor edit
/// (insert, overwrite, fit to fill) or a media drop of its kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetTrackTargeted {
    pub track: u64,
    pub targeted: bool,
}

/// Toggle whether a track rides along with an insert on another track. A
/// sync-locked track shifts by the same amount so picture and sound stay in
/// step; clearing it pins the track in place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetTrackSyncLock {
    pub track: u64,
    pub sync_lock: bool,
}

/// Remove a clip and slide later clips on its track left to close the gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RippleDelete {
    pub clip: u64,
}

/// Shift every clip on a track that starts at or after `from` by `delta`
/// seconds (negative shifts left). Rejected if a left shift would collide
/// with an earlier clip or push a clip before time 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShiftClips {
    pub track: u64,
    /// Clips starting at or after this timeline position (seconds) shift.
    pub from: f64,
    /// Signed shift amount in seconds.
    pub delta: f64,
}

/// Insert a trimmed range of media at a timeline position, first shifting
/// every clip at or after that position right to make room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RippleInsert {
    /// Target track id (video or audio).
    pub track: u64,
    /// Media pool id of the source file.
    pub media: u64,
    /// In-point within the source media, in seconds.
    pub source_start: f64,
    /// Length of the source range to use, in seconds.
    pub source_duration: f64,
    /// Timeline position of the insert, in seconds.
    pub at: f64,
}

/// Place a trimmed range of media at a timeline position, covering whatever is
/// already there. Nothing downstream moves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OverwriteClip {
    /// Target track id (video or audio).
    pub track: u64,
    /// Media pool id of the source file.
    pub media: u64,
    /// In-point within the source media, in seconds.
    pub source_start: f64,
    /// Length of the source range to use, in seconds.
    pub source_duration: f64,
    /// Timeline position the clip lands at, in seconds.
    pub at: f64,
}

/// Place a trimmed range of media at a timeline position and retime it so it
/// fills exactly `duration` seconds — a four-point edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FitToFill {
    /// Target track id (video or audio).
    pub track: u64,
    /// Media pool id of the source file.
    pub media: u64,
    /// In-point within the source media, in seconds.
    pub source_start: f64,
    /// Length of the source range to use, in seconds.
    pub source_duration: f64,
    /// Timeline position the clip lands at, in seconds.
    pub at: f64,
    /// Timeline length the clip must fill, in seconds. The clip is retimed to
    /// match it.
    pub duration: f64,
    /// Push the rest of the lane right to open the span instead of covering
    /// what is already in it.
    pub ripple: bool,
}

/// Put two or more clips into one link group: linked clips select, move,
/// and trim together. Replaces any previous links on those clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LinkClips {
    /// Ids of the clips to link (at least two).
    #[schemars(length(min = 2, max = MAX_MULTI_CLIP_REFS))]
    pub clips: Vec<u64>,
}

/// Dissolve every link group touched by one or more clips. Naming any member
/// clears the complete group; distinct members of the same group are harmless
/// and coalesced by the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UnlinkClips {
    /// Ids of linked clips whose complete groups should be dissolved. List
    /// each clip id once; callers do not need to enumerate every group member.
    #[schemars(length(min = 1, max = MAX_MULTI_CLIP_REFS))]
    pub clips: Vec<u64>,
}

/// Marker flag colors (the editor's fixed palette).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireMarkerColor {
    Teal,
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
}

/// Drop a named, colored marker on the timeline ruler — an anchor for
/// navigation and for aligning edits ("cut at the marker", beat sync).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AddMarker {
    /// Timeline position of the marker, in seconds.
    pub at: f64,
    /// Short label shown beside the flag (e.g. "Drop", "Beat 1"). Omit for
    /// an unnamed marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Flag color. Omit to cycle the palette automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<WireMarkerColor>,
}

/// Remove a timeline marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RemoveMarker {
    /// The marker to remove.
    pub marker: u64,
}

/// Move, rename, recolor, or annotate an existing timeline marker. Omitted
/// fields keep their current value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetMarker {
    /// The marker to change.
    pub marker: u64,
    /// New timeline position in seconds. Omit to keep the position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
    /// New label ("" clears it). Omit to keep the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New flag color. Omit to keep the color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<WireMarkerColor>,
    /// New note ("" clears it). Omit to keep the comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Canvas aspect-ratio presets the agent may pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum WireCanvasAspect {
    /// Follow the footage: canvas shape and size derive from the largest
    /// video media on the timeline (the default).
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "16:9")]
    Wide16x9,
    #[serde(rename = "9:16")]
    Tall9x16,
    #[serde(rename = "1:1")]
    Square1x1,
    #[serde(rename = "4:5")]
    Portrait4x5,
    #[serde(rename = "21:9")]
    Cinema21x9,
}

impl WireCanvasAspect {
    /// The serialized name (`"auto"`, `"16:9"`, …), for transcripts.
    pub fn name(self) -> &'static str {
        match self {
            WireCanvasAspect::Auto => "auto",
            WireCanvasAspect::Wide16x9 => "16:9",
            WireCanvasAspect::Tall9x16 => "9:16",
            WireCanvasAspect::Square1x1 => "1:1",
            WireCanvasAspect::Portrait4x5 => "4:5",
            WireCanvasAspect::Cinema21x9 => "21:9",
        }
    }
}

/// Set the project canvas: the aspect-ratio preset the composite renders
/// at, the background color shown where no clip covers the canvas, and/or an
/// explicit output size. Omitted fields keep their current value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetCanvas {
    /// Canvas aspect preset: "auto" follows the footage; "16:9", "9:16",
    /// "1:1", "4:5", and "21:9" fix the shape (clips re-fit automatically).
    /// Omit to keep the current preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect: Option<WireCanvasAspect>,
    /// Canvas background color as `[red, green, blue]`, each 0-255. Omit
    /// to keep the current background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<[u8; 3]>,
    /// Explicit canvas size as `[width, height]` in pixels. Each edge must be
    /// even and between 16 and 8192. Omit to keep the current size; send
    /// `[0, 0]` to go back to deriving it from the aspect or the footage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<[u32; 2]>,
}
