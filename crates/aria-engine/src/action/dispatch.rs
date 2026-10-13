//! Every command's handler, as a row.
//!
//! `aria-commands` holds what a command *is* — its wire id, its history label,
//! its reach, the clips and lanes it addresses, whether it can break an
//! abutment, and what undoing it puts back. This module holds the other half:
//! the handler that applies it, and the outcome it reports. One row per
//! command, in [`rows`], and one dispatcher that walks a row.
//!
//! Splitting it that way is forced by the dependency direction — the engine
//! depends on the command crate, not the reverse — but it also means each half
//! is exhaustive over the axis on its own, and neither can drift without the
//! compiler saying so.
//!
//! ## Fail by default
//!
//! [`rows::apply_edit`] and [`rows::apply_project`] are the only exhaustive
//! matches over [`EditCommand`] and [`ProjectCommand`] in this crate, and both
//! are generated from the tables by [`edit_ops`] / [`project_ops`]. A variant
//! with no row makes the generated match non-exhaustive, which does not
//! compile: a command cannot reach the engine without a handler, and cannot
//! have a handler without a row in `aria-commands` either.
//!
//! ## What the row no longer says
//!
//! The transitions guard used to be a hand-written `transitions_guard(ctx)`
//! call at the top of twelve arms, and reviewing whether the thirteenth needed
//! one meant reading all seventy-seven. It is now the `junctions:` column of
//! the command's row, read once in [`dispatch_edit`].

use aria_commands::{Command, EditCommand, EditOutcome, Inverse, ProjectCommand};

use super::edit::add_track::RemoveTrackAction;
use super::edit::audio_effects::InsertEdit;
use super::edit::remove_media::RemoveMediaAction;
use super::edit::restore_transitions::RestoreTransitionsAction;
use super::edit::set_track_flags::FlagPatch;
use super::edit::set_track_routing::RouteEdit;
use super::edit::{self, remove_clip::RemoveClipAction};
use super::project;
use super::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;
use aria_models::{
    AudioInsertTarget, BinId, ClipId, MarkerId, MediaId, ModelError, TrackId, TrackOutput,
    Transition,
};

/// Capture transitions only on tracks the structural edit can touch.
///
/// Clip targets resolve through the timeline index; lane targets include move
/// destinations. Unrelated tracks retain their transition storage without a
/// clone on every structural edit.
pub(super) fn transitions_guard(
    ctx: &ApplyContext<'_>,
    edit: &EditCommand,
) -> Option<Vec<(TrackId, Vec<Transition>)>> {
    if !ctx.project.has_transitions() {
        return None;
    }
    let timeline = ctx.project.timeline();
    let targets = edit.targets();
    let mut tracks = targets.tracks;
    tracks.extend(
        targets
            .clips
            .into_iter()
            .filter_map(|clip| timeline.track_of(clip)),
    );
    tracks.sort_unstable();
    tracks.dedup();

    let snapshot: Vec<_> = tracks
        .into_iter()
        .filter_map(|track_id| {
            let transitions = timeline.track(track_id)?.transitions();
            (!transitions.is_empty()).then(|| (track_id, transitions.to_vec()))
        })
        .collect();
    (!snapshot.is_empty()).then_some(snapshot)
}

/// After a structural edit, prune junctions whose abutment broke. If anything
/// was pruned, fold a transitions-restore into the inverse so undo brings the
/// dropped junctions back; otherwise the primary inverse stands alone.
fn finalize_structural(
    ctx: &mut ApplyContext<'_>,
    guard: Option<Vec<(TrackId, Vec<Transition>)>>,
    primary: Box<dyn EditAction>,
) -> Box<dyn EditAction> {
    let Some(snapshot) = guard else {
        return primary;
    };
    ctx.project.prune_dead_transitions();
    let changed = snapshot.iter().any(|(track_id, before)| {
        ctx.project
            .timeline()
            .track(*track_id)
            .is_none_or(|track| track.transitions() != before.as_slice())
    });
    if changed {
        Box::new(CompoundAction {
            actions: vec![primary, Box::new(RestoreTransitionsAction { snapshot })],
        })
    } else {
        primary
    }
}

/// Result of applying a wire [`Command`] through the engine.
///
/// Serializes adjacently tagged (`{"type": "Imported", "value": {"media": 3}}`,
/// `{"type": "Saved"}`) so shells and the AI agent read one stable shape;
/// `Edited` nests the [`EditOutcome`] wire object under `value`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ApplyOutcome {
    Imported {
        media: aria_models::MediaId,
    },
    RemovedMedia {
        media: aria_models::MediaId,
    },
    Saved,
    Opened,
    Loaded,
    Relinked {
        media: aria_models::MediaId,
    },
    /// A media-pool entry's label colour changed.
    LabeledMedia {
        media: aria_models::MediaId,
    },
    /// The timeline was exported to a file; `frames` is the number written.
    Exported {
        frames: u64,
    },
    /// A `.ariat` template file was written; the session is unchanged.
    SavedTemplate,
    /// The session was replaced with a filled template — a new, unsaved
    /// project (history cleared, project path reset).
    AppliedTemplate,
    Edited(EditOutcome),
}

/// What applying a command reports: the outcome, plus the undo step when the
/// command recorded one.
type Dispatched = (ApplyOutcome, Option<Box<dyn EditAction>>);

pub fn dispatch(
    command: Command,
    ctx: &mut ApplyContext<'_>,
) -> Result<(ApplyOutcome, Option<Box<dyn EditAction>>), EngineError> {
    match command {
        Command::Project(project) => dispatch_project(project, ctx),
        Command::Edit(edit) => dispatch_edit(edit, ctx),
    }
}

fn dispatch_project(
    command: ProjectCommand,
    ctx: &mut ApplyContext<'_>,
) -> Result<Dispatched, EngineError> {
    // `spec` is `&'static`, so reading it does not borrow the command that the
    // table is about to consume.
    let spec = command.spec();
    let (outcome, inverse) = rows::apply_project(ctx, command)?;
    debug_assert!(
        matches!(spec.inverse, Inverse::Conditional(_))
            || inverse.is_some() == spec.inverse.is_undoable(),
        "{} recorded {} undo step, but its row says its undo {}",
        spec.id,
        if inverse.is_some() { "an" } else { "no" },
        spec.inverse.reason(),
    );
    Ok((outcome, inverse))
}

fn dispatch_edit(edit: EditCommand, ctx: &mut ApplyContext<'_>) -> Result<Dispatched, EngineError> {
    // A structural edit can leave a transition sitting on an abutment that no
    // longer exists. Which edits those are is the row's `junctions:` column.
    let guard = if edit.spec().structural {
        transitions_guard(ctx, &edit)
    } else {
        None
    };
    let Applied { outcome, inverse } = rows::apply_edit(ctx, edit)?;
    let inverse = finalize_structural(ctx, guard, inverse);
    Ok((ApplyOutcome::Edited(outcome), Some(inverse)))
}

/// One applied edit: what changed, and the step that puts it back.
///
/// Every edit records an undo step — that is the `undo:` column of every edit
/// row — so this is not an `Option`, and `dispatch_edit` needs no branch for a
/// case the table forbids.
struct Applied {
    outcome: EditOutcome,
    inverse: Box<dyn EditAction>,
}

// --- outcome columns --------------------------------------------------------
//
// One per `EditOutcome` shape, so a row states what it changed as a word
// instead of nesting two constructors and a `Some` seventy-seven times. The
// `created*` forms take the handler's `(id, inverse)` tuple whole; the rest
// take the id the command already named.

fn created((clip, inverse): (ClipId, Box<dyn EditAction>)) -> Applied {
    Applied {
        outcome: EditOutcome::Created(clip),
        inverse,
    }
}

fn created_track((track, inverse): (TrackId, Box<dyn EditAction>)) -> Applied {
    Applied {
        outcome: EditOutcome::CreatedTrack(track),
        inverse,
    }
}

fn created_marker((marker, inverse): (MarkerId, Box<dyn EditAction>)) -> Applied {
    Applied {
        outcome: EditOutcome::CreatedMarker(marker),
        inverse,
    }
}

fn created_bin((bin, inverse): (BinId, Box<dyn EditAction>)) -> Applied {
    Applied {
        outcome: EditOutcome::CreatedBin(bin),
        inverse,
    }
}

fn updated(clip: ClipId, inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::Updated(clip),
        inverse,
    }
}

fn updated_track(track: TrackId, inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::UpdatedTrack(track),
        inverse,
    }
}

fn updated_marker(marker: MarkerId, inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::UpdatedMarker(marker),
        inverse,
    }
}

fn updated_canvas(inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::UpdatedCanvas,
        inverse,
    }
}

fn updated_sequence_rate(inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::UpdatedSequenceRate,
        inverse,
    }
}

fn updated_audio_layout(inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::UpdatedAudioLayout,
        inverse,
    }
}

/// What an insert chain edit reports: the clip or lane it sits on, or the
/// master.
fn updated_inserts(target: AudioInsertTarget, inverse: Box<dyn EditAction>) -> Applied {
    let outcome = match target {
        AudioInsertTarget::Clip(clip) => EditOutcome::Updated(clip),
        AudioInsertTarget::Track(track) => EditOutcome::UpdatedTrack(track),
        AudioInsertTarget::Master => EditOutcome::UpdatedMaster,
    };
    Applied { outcome, inverse }
}

fn updated_project(inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::UpdatedProject,
        inverse,
    }
}

fn opened_sequence(inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::OpenedSequence,
        inverse,
    }
}

fn updated_pool(inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::UpdatedPool,
        inverse,
    }
}

fn removed(clip: ClipId, inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::Removed(clip),
        inverse,
    }
}

fn removed_track(track: TrackId, inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::RemovedTrack(track),
        inverse,
    }
}

fn removed_marker(marker: MarkerId, inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::RemovedMarker(marker),
        inverse,
    }
}

fn shifted(track: TrackId, inverse: Box<dyn EditAction>) -> Applied {
    Applied {
        outcome: EditOutcome::ShiftedTrack(track),
        inverse,
    }
}

/// The clip a multi-clip edit reports as its outcome.
///
/// An empty selection names no clip, and an edit that changes nothing is not an
/// edit, so it is refused rather than reported against a fabricated id.
fn first_of(clips: &[ClipId]) -> Result<ClipId, EngineError> {
    clips
        .first()
        .copied()
        .ok_or_else(|| EngineError::from(ModelError::InvalidRange))
}

// --- project outcome columns ------------------------------------------------

fn imported((media, inverse): (MediaId, Option<Box<dyn EditAction>>)) -> Dispatched {
    (ApplyOutcome::Imported { media }, inverse)
}

/// A project action that can be stepped back over.
fn undoable(outcome: ApplyOutcome, inverse: Box<dyn EditAction>) -> Dispatched {
    (outcome, Some(inverse))
}

/// A project action that records no undo step, for the reason its row states.
fn settled(outcome: ApplyOutcome) -> Dispatched {
    (outcome, None)
}

/// Generate the edit dispatcher from the table.
///
/// `$fields` captures the variant's whole brace group and is spliced in as the
/// pattern, so a row names its payload exactly as the variant declares it —
/// including `{ .. }` for a payload the handler does not read.
macro_rules! edit_ops {
    ($ctx:ident; $($variant:ident $fields:tt => $body:expr,)+) => {
        /// Apply one edit. Exhaustive over [`EditCommand`] by construction: a
        /// variant with no row above does not compile.
        pub(super) fn apply_edit(
            $ctx: &mut ApplyContext<'_>,
            command: EditCommand,
        ) -> Result<Applied, EngineError> {
            Ok(match command {
                $(EditCommand::$variant $fields => $body,)+
            })
        }
    };
}

/// Generate the project dispatcher from the table. See [`edit_ops`].
macro_rules! project_ops {
    ($ctx:ident; $($variant:ident $fields:tt => $body:expr,)+) => {
        /// Apply one project action. Exhaustive over [`ProjectCommand`] by
        /// construction: a variant with no row above does not compile.
        pub(super) fn apply_project(
            $ctx: &mut ApplyContext<'_>,
            command: ProjectCommand,
        ) -> Result<Dispatched, EngineError> {
            Ok(match command {
                $(ProjectCommand::$variant $fields => $body,)+
            })
        }
    };
}

// The tables. A sibling file rather than an inline module so this one stays
// readable: the rows are data and the machinery above is not.
#[path = "dispatch_rows.rs"]
mod rows;
