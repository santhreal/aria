//! The buttons the pool's folder strip draws.
//!
//! Three buttons, two of which only mean something inside a folder: renaming the
//! pool root or deleting it are not edits the project has. That condition is a
//! row's own `when` rather than a `when_some` in the panel, so a button that
//! would do nothing is not drawn and a census can see why.

use aria_engine::EngineError;

use crate::app::AppState;

/// One button in the folder strip.
pub struct BinAction {
    /// Element id. Stable, because the strip draws at most one of each.
    pub id: &'static str,
    pub label: &'static str,
    /// Whether the strip offers it, given what is open.
    pub when: fn(&BinStrip) -> bool,
    /// What pressing it asks the session for.
    pub run: fn(&mut AppState, &BinStrip) -> Result<(), EngineError>,
}

/// What the strip knows while it draws: which folder is open, and how many sit
/// inside it.
///
/// Read once through [`BinStrip::of`] by both the panel and the census, so the
/// two cannot disagree about which folder a button was drawn for. `Copy`: a press
/// carries it into a closure and must not cost a copy of the tree.
#[derive(Clone, Copy)]
pub struct BinStrip {
    /// The open folder, or `None` at the pool root.
    pub open: Option<u64>,
    /// How many folders sit directly inside the open one.
    pub children: usize,
}

impl BinStrip {
    pub fn of(state: &AppState) -> Self {
        Self {
            open: state.current_bin,
            // The projected list, not a walk of the pool: the strip and this
            // count are the same answer, so they cannot disagree about how many
            // folders are inside the open one.
            children: state.bin_children.len(),
        }
    }
}

/// Every button the folder strip offers, in the order it draws them.
pub const BIN_ACTIONS: &[BinAction] = &[
    // Always offered: a folder can be made at the root or inside another one.
    BinAction {
        id: "bin-new",
        label: "New bin",
        when: |_| true,
        run: |state, _| state.create_bin(),
    },
    // Opens the name field rather than dispatching: the rename is what the field
    // commits, and the strip's job is to put the caret in it.
    BinAction {
        id: "bin-rename",
        label: "Rename",
        when: |strip| strip.open.is_some(),
        run: |state, strip| {
            if let Some(bin) = strip.open {
                state.begin_bin_rename(bin);
            }
            Ok(())
        },
    },
    BinAction {
        id: "bin-delete",
        label: "Delete bin",
        when: |strip| strip.open.is_some(),
        run: |state, _| state.remove_current_bin(),
    },
];
