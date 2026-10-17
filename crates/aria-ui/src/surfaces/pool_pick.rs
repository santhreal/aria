//! What a click on a pool card does.
//!
//! Four rows, one per way of clicking a card. Three of them write the set in
//! [`PoolSelection`](crate::app::pool_selection::PoolSelection), which owns the
//! order. The fourth, a double-click, opens the card in a Source tab of the
//! Viewer. No click edits the project: a card reaches the sequence by a drag,
//! by Insert or Overwrite from its Source tab, or by Insert or Overwrite with
//! the pool selection and no Source tab in front.
//!
//! The right-hand button is not a row: it selects the card outright and opens a
//! context menu, which is an overlay the panel owns.

use crate::app::AppState;
use crate::types::SidebarTab;

/// What a click on a pool card asks for.
///
/// The modifiers and the click count are read once, in [`Reach::of`], so the
/// platform's own multi-select key is the panel's business and what it means
/// is this table's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// This item alone.
    Replace,
    /// Add this item to the selection, or drop it if it is already in.
    Toggle,
    /// Every card between the last one picked outright and this one.
    Span,
    /// This item alone, opened in a Source tab, or its open tab focused.
    Open,
}

impl Reach {
    /// What the operator did, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Replace => "a pool card clicked",
            Self::Toggle => "a pool card added to the selection",
            Self::Span => "a run of pool cards picked from the last one",
            Self::Open => "a pool card double-clicked",
        }
    }

    /// Which row a click's modifiers and click count ask for.
    ///
    /// Bools rather than a gpui event: which physical key is the platform's
    /// multi-select modifier is the panel's business. A modified click never
    /// opens, so a fast run of Ctrl-clicks builds a selection instead of
    /// opening tabs.
    pub fn of(multi: bool, span: bool, clicks: usize) -> Self {
        match (multi, span) {
            // A span wins a chord: extending is the more specific of the two,
            // and a range that toggled its own ends would be unpredictable.
            (_, true) => Self::Span,
            (true, false) => Self::Toggle,
            (false, false) if clicks >= 2 => Self::Open,
            (false, false) => Self::Replace,
        }
    }
}

/// The pool in the order the list draws it, which is the order a span runs
/// along: a range is what the operator saw between two cards, not a range of
/// ids, and the audio tab draws a subset with video items sitting between its
/// rows in the pool itself.
fn visible(state: &AppState, tab: SidebarTab) -> Vec<u64> {
    state.pool_rows(tab).ids()
}

/// Run one click on a card. An item that cannot be shown in the Viewer, such
/// as a missing file or an audio-only source, stays selected and the status
/// line reports why it did not open.
pub fn apply(state: &mut AppState, reach: Reach, media: u64, tab: SidebarTab) {
    match reach {
        Reach::Replace => state.pool_selection.replace(media),
        Reach::Toggle => state.pool_selection.toggle(media),
        Reach::Span => {
            let list = visible(state, tab);
            state.pool_selection.span(media, &list);
        }
        Reach::Open => {
            state.pool_selection.replace(media);
            state.open_source(media);
        }
    }
}
