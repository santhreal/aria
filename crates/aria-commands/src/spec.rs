//! Every command, as a row.
//!
//! A command used to be an enum variant plus a hand-written arm in each place
//! that cared: the history label, the operator-reach declaration, the ids a
//! lock policy has to check, whether a structural edit can break a transition
//! junction, and — in `aria-engine` — the handler and the inverse. Nothing tied
//! those together, so a command could be dispatchable and unlabelled, or
//! labelled and unreachable, and no test went red. Adding one variant meant
//! finding every match again.
//!
//! A row here carries what the command layer knows about one command: its wire
//! id, its history label, how an operator reaches it, which clips and lanes it
//! addresses, whether a lock may refuse it, whether it can break an abutment,
//! and what undoing it puts back. `aria-engine` holds the other half — the
//! handler that applies it — keyed by the same id, and its own table is the
//! only other place a command is described.
//!
//! ## Fail by default
//!
//! [`EditCommand::spec`] and [`ProjectCommand::spec`] are the only exhaustive
//! matches over either axis in this crate, and both are generated from the
//! tables below. A variant with no row is a non-exhaustive match, which is a
//! compile error, so a command cannot ship without a label, a reach, a target
//! shape, and a statement of what its undo restores.
//!
//! ## Undo
//!
//! [`Inverse`] is the column `aria-engine/tests/inverse_edits.rs` is the
//! contract for. Every row names what undoing it puts back; a row that records
//! no undo step says so and says why, so "no inverse" is a declaration rather
//! than a silent no-op nobody noticed.

use crate::command::{EditCommand, EditTargets, ProjectCommand};
use crate::reach::Reach;

mod clip;
mod insert;
mod project;
mod track;

#[cfg(test)]
mod tests;

use clip::clip_rows;
use insert::insert_rows;
use project::{project_command_rows, project_edit_rows};
use track::track_rows;
/// What stepping back over an applied command puts back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inverse {
    /// Applying the command records an undo step; the text names what that
    /// step restores.
    Restores(&'static str),
    /// Applying the command records no undo step, for the stated reason.
    None(&'static str),
    /// An undo step is recorded only in the stated case.
    Conditional(&'static str),
}

impl Inverse {
    /// Whether applying the command always records an undo step.
    pub fn is_undoable(self) -> bool {
        matches!(self, Inverse::Restores(_))
    }

    /// The row's own words about its undo, whichever kind it declared.
    pub fn reason(self) -> &'static str {
        match self {
            Inverse::Restores(text) | Inverse::None(text) | Inverse::Conditional(text) => text,
        }
    }
}

/// One timeline edit.
pub struct EditSpec {
    /// The variant name, which is also its wire tag.
    pub id: &'static str,
    /// What the command is called in the history list.
    pub label: &'static str,
    /// Whether a shipped operator surface sends it.
    pub reach: Reach,
    /// Whether the edit can change a clip's extent or placement, and so break
    /// an abutment a transition sits on. The engine snapshots the timeline's
    /// junctions before a `true` row and prunes the dead ones after.
    pub structural: bool,
    /// Whether the edit must be allowed even on a locked lane. Unlocking is the
    /// way out of a lock, so the lock cannot guard it.
    pub ignores_track_lock: bool,
    /// What undoing the command puts back.
    pub inverse: Inverse,
    /// Reads the clips and lanes this command names.
    ///
    /// Private, and correct only for its own row's variants: it is one of the
    /// shape readers below, chosen by the group the row sits in. Reaching it
    /// through [`EditCommand::spec`] is the only correct call, which is what
    /// [`EditCommand::targets`] does.
    targets: fn(&EditCommand) -> EditTargets,
}

/// One project-level action.
pub struct ProjectSpec {
    /// The variant name, which is also its wire tag.
    pub id: &'static str,
    /// What the command is called in the history list.
    pub label: &'static str,
    /// Whether a shipped operator surface sends it.
    pub reach: Reach,
    /// What undoing the command puts back.
    pub inverse: Inverse,
}

/// Every timeline edit, in table order.
pub fn edit_specs() -> &'static [EditSpec] {
    SHIPPED_EDITS
}

/// Every project-level action, in table order.
pub fn project_specs() -> &'static [ProjectSpec] {
    SHIPPED_PROJECT
}

// --- row constructors -------------------------------------------------------
//
// Short enough to keep a row on one line, and named so a row reads as a
// sentence rather than a path.

const fn operator(surface: &'static str) -> Reach {
    Reach::Operator(surface)
}

// There is no `planned` constructor: every row below names either an operator
// surface or an internal reason. `Reach::Planned` is still part of the
// vocabulary, and a row that needs it writes it out, which is loud enough to
// notice in review.

const fn internal(reason: &'static str) -> Reach {
    Reach::Internal(reason)
}

const fn restores(what: &'static str) -> Inverse {
    Inverse::Restores(what)
}

const fn no_undo(why: &'static str) -> Inverse {
    Inverse::None(why)
}

const fn undo_if(when: &'static str) -> Inverse {
    Inverse::Conditional(when)
}

// --- the table --------------------------------------------------------------

/// Declare the edit table: rows grouped by the clips and lanes they address.
///
/// The shape has to be the grouping key rather than a per-row word, because
/// reading a clip id out of a command needs a pattern that names the field, and
/// one reader per shape is what the engine's lock policy calls. Every other
/// column is per row.
macro_rules! edit_registry {
    (
        $(
            addresses $shape:tt {
                $(
                    $variant:ident {
                        label: $label:literal,
                        reach: $reach:expr,
                        junctions: $junctions:tt,
                        lock: $lock:tt,
                        undo: $undo:expr,
                    },
                )+
            }
        )+
    ) => {
        $( edit_registry!(@reader $shape, $($variant),+); )+

        pub(super) static SHIPPED_EDITS: &[EditSpec] = &[$($(
            EditSpec {
                id: stringify!($variant),
                label: $label,
                reach: $reach,
                structural: edit_registry!(@junctions $junctions),
                ignores_track_lock: edit_registry!(@lock $lock),
                inverse: $undo,
                targets: edit_registry!(@reader_name $shape),
            },
        )+)+];

        /// Which row of [`SHIPPED_EDITS`] a variant is, so the lookup is an
        /// index rather than a scan of the table.
        #[derive(Clone, Copy)]
        #[repr(usize)]
        enum EditRow {
            $($($variant,)+)+
        }

        impl EditCommand {
            /// This command's row.
            ///
            /// The match below is the one exhaustive match over [`EditCommand`]
            /// in this crate: a variant with no row in the table does not
            /// compile.
            pub fn spec(&self) -> &'static EditSpec {
                let row = match self {
                    $($(EditCommand::$variant { .. } => EditRow::$variant,)+)+
                };
                &SHIPPED_EDITS[row as usize]
            }
        }
    };

    (@junctions keeps) => { false };
    (@junctions breaks) => { true };

    (@lock guarded) => { false };
    (@lock ignored) => { true };

    (@reader_name nothing) => { read_nothing };
    (@reader_name clip) => { read_clip };
    (@reader_name clip_and_lane) => { read_clip_and_lane };
    (@reader_name clip_and_optional_lane) => { read_clip_and_optional_lane };
    (@reader_name lane) => { read_lane };
    (@reader_name clips) => { read_clips };
    (@reader_name insert) => { read_insert };
    (@reader_name patched_lanes) => { read_patched_lanes };

    // A shape reader answers for the variants of its own group. The fallback
    // arm keeps it total; it is unreachable, because a row hands out only the
    // reader of the group it sits in and `spec` routes a command only to its
    // own row.
    (@reader nothing, $($variant:ident),+) => {
        /// Commands that address the project as a whole: no clip and no lane
        /// for a policy to check.
        fn read_nothing(_command: &EditCommand) -> EditTargets {
            EditTargets::default()
        }
    };
    (@reader clip, $($variant:ident),+) => {
        /// One clip.
        fn read_clip(command: &EditCommand) -> EditTargets {
            match command {
                $(EditCommand::$variant { clip, .. })|+ => EditTargets {
                    clips: vec![*clip],
                    tracks: Vec::new(),
                },
                _ => EditTargets::default(),
            }
        }
    };
    (@reader clip_and_lane, $($variant:ident),+) => {
        /// One clip and a destination lane.
        fn read_clip_and_lane(command: &EditCommand) -> EditTargets {
            match command {
                $(EditCommand::$variant { clip, to_track, .. })|+ => EditTargets {
                    clips: vec![*clip],
                    tracks: vec![*to_track],
                },
                _ => EditTargets::default(),
            }
        }
    };
    (@reader clip_and_optional_lane, $($variant:ident),+) => {
        /// One clip and a destination lane the command may leave to the engine.
        fn read_clip_and_optional_lane(command: &EditCommand) -> EditTargets {
            match command {
                $(EditCommand::$variant { clip, to_track, .. })|+ => EditTargets {
                    clips: vec![*clip],
                    tracks: to_track.iter().copied().collect(),
                },
                _ => EditTargets::default(),
            }
        }
    };
    (@reader lane, $($variant:ident),+) => {
        /// One lane.
        fn read_lane(command: &EditCommand) -> EditTargets {
            match command {
                $(EditCommand::$variant { track, .. })|+ => EditTargets {
                    clips: Vec::new(),
                    tracks: vec![*track],
                },
                _ => EditTargets::default(),
            }
        }
    };
    (@reader clips, $($variant:ident),+) => {
        /// Several clips at once.
        fn read_clips(command: &EditCommand) -> EditTargets {
            match command {
                $(EditCommand::$variant { clips, .. })|+ => EditTargets {
                    clips: clips.to_vec(),
                    tracks: Vec::new(),
                },
                _ => EditTargets::default(),
            }
        }
    };
    (@reader patched_lanes, $($variant:ident),+) => {
        /// The picture lane and the sound lane of a patched edit, either of
        /// which the command may leave out.
        fn read_patched_lanes(command: &EditCommand) -> EditTargets {
            match command {
                $(EditCommand::$variant { track, sound, .. })|+ => EditTargets {
                    clips: Vec::new(),
                    tracks: track.iter().copied().chain(sound.lane()).collect(),
                },
                _ => EditTargets::default(),
            }
        }
    };
    (@reader insert, $($variant:ident),+) => {
        /// One insert chain: a clip's, a lane's, or the master's, which names
        /// neither.
        fn read_insert(command: &EditCommand) -> EditTargets {
            match command {
                $(EditCommand::$variant { target, .. })|+ => EditTargets {
                    clips: target.clip().into_iter().collect(),
                    tracks: target.track().into_iter().collect(),
                },
                _ => EditTargets::default(),
            }
        }
    };
}

/// Declare the project table.
macro_rules! project_registry {
    (
        $(
            $variant:ident {
                label: $label:literal,
                reach: $reach:expr,
                undo: $undo:expr,
            },
        )+
    ) => {
        pub(super) static SHIPPED_PROJECT: &[ProjectSpec] = &[$(
            ProjectSpec {
                id: stringify!($variant),
                label: $label,
                reach: $reach,
                inverse: $undo,
            },
        )+];

        /// Which row of [`SHIPPED_PROJECT`] a variant is.
        #[derive(Clone, Copy)]
        #[repr(usize)]
        enum ProjectRow {
            $($variant,)+
        }

        impl ProjectCommand {
            /// This command's row.
            ///
            /// The one exhaustive match over [`ProjectCommand`] in this crate:
            /// a variant with no row does not compile.
            pub fn spec(&self) -> &'static ProjectSpec {
                let row = match self {
                    $(ProjectCommand::$variant { .. } => ProjectRow::$variant,)+
                };
                &SHIPPED_PROJECT[row as usize]
            }
        }
    };
}

#[rustfmt::skip]
clip_rows!(track_rows, insert_rows, project_edit_rows, edit_registry,);

#[rustfmt::skip]
project_command_rows!(project_registry);

impl EditCommand {
    /// What this command is called in the history list.
    pub fn label(&self) -> &'static str {
        self.spec().label
    }

    /// How an operator reaches this edit.
    pub fn reach(&self) -> Reach {
        self.spec().reach
    }

    /// The clips and lanes this command addresses. Empty for commands that
    /// address the project as a whole (markers, canvas, project name, a new
    /// track, pool folders).
    pub fn targets(&self) -> EditTargets {
        (self.spec().targets)(self)
    }

    /// Whether this command must be allowed even on a locked lane.
    pub fn ignores_track_lock(&self) -> bool {
        self.spec().ignores_track_lock
    }
}

impl ProjectCommand {
    /// What this command is called in the history list.
    pub fn label(&self) -> &'static str {
        self.spec().label
    }

    /// How an operator reaches this project command.
    pub fn reach(&self) -> Reach {
        self.spec().reach
    }
}
