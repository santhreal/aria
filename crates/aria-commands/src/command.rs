//! Structured editor commands.
//!
//! UI gestures and the AI agent both emit these values; the engine applies them
//! against project/timeline state with undo/redo.
//!
//! ## Wire format
//!
//! Every command (de)serializes as a single-level JSON object discriminated by
//! a `"type"` field holding the variant name — `{"type": "SplitClip", "clip":
//! 5, "at": …}`. [`Command`] itself is untagged: project and edit variant
//! names never collide, so the same flat object shape serves both (the shells'
//! FFI protocol and the future AI-agent stream speak this format). The
//! `tests/wire_format.rs` goldens lock it.
//!
//! ## Where a command is described
//!
//! This file and `edit_command.rs` hold the shapes and nothing else. A
//! command's label, reach, target shape, junction behaviour and undo are one
//! row in `spec_rows.rs`, and its handler is one row in `aria-engine`'s
//! dispatch table. Both tables generate the only exhaustive matches over these
//! enums, so a variant added to either file does not compile until both rows
//! exist.

use std::path::PathBuf;

use aria_core::ExportFormat;
use aria_models::{
    BinId, ClipId, LabelColor, MarkerId, MediaId, RationalTime, TemplateMeta, TrackId,
};
use serde::{Deserialize, Serialize};

use crate::EditCommand;

/// One media choice for [`ProjectCommand::ApplyTemplate`], in slot order: a
/// file to probe (like `Import`) and an optional in-point into it. `None`
/// starts from the beginning, matching CapCut ("takes from the start of the
/// clip"); the slot's locked timeline duration decides how much source is
/// drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplatePick {
    pub path: PathBuf,
    #[serde(default)]
    pub source_in: Option<RationalTime>,
}

/// A project-level action (media pool, not timeline placement).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProjectCommand {
    /// Register a file in the media pool.
    Import { path: PathBuf },
    /// Drop a source from the media pool (delete from the Library bin).
    /// Rejected while any clip still references it — callers that want to
    /// delete used media remove those clips first (one history group). The
    /// inverse re-inserts the source with its original id, so undo restores
    /// it and any clips removed alongside it reattach.
    RemoveMedia { media: MediaId },
    /// Write the current project to a `.aria` file.
    Save { path: PathBuf },
    /// Replace the session from a project file; every media path must exist.
    Open { path: PathBuf },
    /// Replace the session from a project file; missing media paths are kept but not relinked.
    Load { path: PathBuf },
    /// Re-point a media-pool entry at a new file (missing-media relink):
    /// re-probe `path` and refresh the entry's metadata in place, keeping
    /// its id so clips stay attached. Not undoable by design — it repairs
    /// project state to match the disk, and undoing back to a dead path is
    /// never a useful result.
    RelinkMedia { media: MediaId, path: PathBuf },
    /// Set (or clear) a media-pool entry's label colour. Undoable: the inverse
    /// restores the previous colour.
    SetMediaLabel {
        media: MediaId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<LabelColor>,
    },
    /// Render the timeline to a video file.
    ///
    /// `format` is what the caller asked for, one field per choice: absent
    /// fields mean the project's own output, so `{"path": …}` alone still
    /// exports exactly the sequence. It is resolved and validated against the
    /// installed encoder by `aria_render::ExportSettings::resolved`, which
    /// refuses a codec or a bitrate this machine cannot honour instead of
    /// writing a file nobody asked for.
    Export {
        path: PathBuf,
        #[serde(default)]
        format: ExportFormat,
    },
    /// Write the current project as a `.ariat` template file (CapCut
    /// "export template"): the finished timeline plus its `Replaceable` /
    /// text-editable markers, wrapped in `meta` for the gallery. The session
    /// itself is untouched — no dirty-flag change, no project-path change.
    SaveTemplate { path: PathBuf, meta: TemplateMeta },
    /// Replace the session with a `.ariat` template filled by `picks`
    /// (CapCut "use template"). Each pick is probed like `Import` and fills
    /// the next slot in order; fewer picks than slots keeps sample media in
    /// the rest (template preview), more is refused. Like `Open`/`Load` the
    /// history is cleared, but the result is a *new unsaved* project: the
    /// project path resets and the session reads dirty until saved. Missing
    /// sample-media paths are tolerated (matches `Load`).
    ApplyTemplate {
        path: PathBuf,
        picks: Vec<TemplatePick>,
    },
}

/// Top-level command surface: media registration or a timeline edit.
///
/// Untagged on the wire: the flat `{"type": …}` object of the inner enums is
/// the whole format (variant names are disjoint across the two sets), so
/// callers never spell the project/edit split in JSON.
///
/// `Edit` outweighs `Project` because some edits (layer styles, masks)
/// carry whole param blocks inline. Commands are transient — built once per
/// user action, applied, dropped — so boxing buys nothing and would churn
/// every construction site.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Command {
    Project(ProjectCommand),
    Edit(EditCommand),
}

impl Command {
    /// The display name of this command, for the history list and for
    /// any surface that reports what was just applied.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Project(command) => command.label(),
            Self::Edit(command) => command.label(),
        }
    }
}

/// What an applied edit produced, for callers to act on (e.g. select the new clip).
///
/// Adjacently tagged on the wire (`{"type": "Created", "id": 7}`): every
/// payload is a single entity id, and the unit variant carries none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id")]
pub enum EditOutcome {
    Created(ClipId),
    CreatedTrack(TrackId),
    Updated(ClipId),
    Removed(ClipId),
    RemovedTrack(TrackId),
    /// Clips on the track were ripple-shifted (no single clip to point at).
    ShiftedTrack(TrackId),
    /// A track flag (enabled / muted / locked) was changed.
    UpdatedTrack(TrackId),
    /// A ruler marker was added / changed / removed (M1 markers).
    CreatedMarker(MarkerId),
    UpdatedMarker(MarkerId),
    RemovedMarker(MarkerId),
    /// The project canvas settings changed (M1 canvas settings).
    UpdatedCanvas,
    /// The sequence frame rate was changed (and timeline resampled).
    UpdatedSequenceRate,
    /// The sequence audio layout was changed.
    UpdatedAudioLayout,
    /// The sequence time display was changed.
    UpdatedTimeDisplay,
    /// The master insert chain changed.
    UpdatedMaster,
    /// A project-level property (its display name) changed.
    UpdatedProject,
    /// A pool folder was created; the id is what a surface selects or renames.
    CreatedBin(BinId),
    /// The pool's folder structure or a source's folder changed. Which one is
    /// not stated: a pool surface redraws from the project either way.
    UpdatedPool,
    /// A secondary sequence was opened for editing (or returned to root).
    OpenedSequence,
    /// A source channel of the open sequence was patched.
    UpdatedSourcePatch,
}
/// The clips and lanes a command addresses.
///
/// A policy that has to answer "does this edit touch a locked lane" needs the
/// ids a command names, and every caller that worked them out by hand would go
/// stale on the next variant. [`EditCommand::targets`] reads them off the
/// command's registry row, so a new variant declares its target shape in the
/// table or does not compile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditTargets {
    /// Clips named directly. The lane each sits on is resolved by the caller,
    /// which is the only place the project is known.
    pub clips: Vec<ClipId>,
    /// Lanes named directly, including a move's destination.
    pub tracks: Vec<TrackId>,
}
