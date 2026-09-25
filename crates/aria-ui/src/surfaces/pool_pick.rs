//! What a click on a pool card does.
//!
//! The card was the last drawn control whose behaviour lived entirely inside a
//! mouse listener: select this item, then place it at the playhead unless its
//! media is offline. Nothing could enumerate that, so the offline case read like
//! an ordinary early return, and the selection it wrote could only ever hold one
//! item. The sidebar's multicam button reads that selection, so the group it
//! built always had exactly one angle.
//!
//! Three rows, one per way of clicking a card. The set they write is
//! [`PoolSelection`](crate::app::pool_selection::PoolSelection), which owns the
//! order; whether the click also sends the clip to the timeline is
//! [`Reach::places`] rather than a condition here, so a census presses the rows
//! and asserts both halves.
//!
//! The right-hand button is not a row: it selects the card outright and opens a
//! context menu, which is an overlay the panel owns.

use aria_engine::EngineError;
use aria_models::MediaId;

use crate::app::AppState;
use crate::types::SidebarTab;

/// What a click on a pool card asks for.
///
/// The modifiers are read once, in [`Reach::of`], so the platform's own
/// multi-select key is the panel's business and what it means is this table's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// This item alone, and on the timeline.
    Replace,
    /// Add this item to the selection, or drop it if it is already in.
    Toggle,
    /// Every card between the last one picked outright and this one.
    Span,
}

impl Reach {
    /// What the operator did, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Replace => "a pool card clicked",
            Self::Toggle => "a pool card added to the selection",
            Self::Span => "a run of pool cards picked from the last one",
        }
    }

    /// Which row a click's modifiers ask for.
    ///
    /// Bools rather than a gpui event: which physical key is the platform's
    /// multi-select modifier is the panel's business.
    pub fn of(multi: bool, span: bool) -> Self {
        match (multi, span) {
            // A span wins a chord: extending is the more specific of the two,
            // and a range that toggled its own ends would be unpredictable.
            (_, true) => Self::Span,
            (true, false) => Self::Toggle,
            (false, false) => Self::Replace,
        }
    }

    /// Whether the click also sends the item to the timeline.
    ///
    /// Only a plain click does. Building a selection is not an edit, and a
    /// modified click that dropped a clip per card would leave one undo entry
    /// per angle behind a multicam group.
    pub fn places(&self) -> bool {
        matches!(self, Self::Replace)
    }
}

/// The pool in the order the list draws it, which is the order a span runs
/// along: a range is what the operator saw between two cards, not a range of
/// ids, and the audio tab draws a subset with video items sitting between its
/// rows in the pool itself.
fn visible(state: &AppState, tab: SidebarTab) -> Vec<u64> {
    state.pool_rows(tab).ids()
}

/// Whether the item's media has gone missing.
///
/// An offline item is still selectable, so its properties and its relink button
/// stay reachable, and placing it would put a clip on the timeline with nothing
/// to decode.
fn offline(state: &AppState, media: u64) -> bool {
    state
        .store
        .media
        .iter()
        .find(|item| item.id == media)
        .is_some_and(|item| item.missing)
}

/// Where a placed clip lands.
fn at(state: &AppState) -> i64 {
    state.playhead.value
}

/// Run one click on a card.
///
/// Answers whether a clip landed on the timeline, which is what the panel needs
/// in order to ask for a preview of it. A selection change is not worth a
/// render, and neither is a click on an offline item.
pub fn apply(
    state: &mut AppState,
    reach: Reach,
    media: u64,
    tab: SidebarTab,
) -> Result<bool, EngineError> {
    let list = visible(state, tab);
    match reach {
        Reach::Replace => state.pool_selection.replace(media),
        Reach::Toggle => state.pool_selection.toggle(media),
        Reach::Span => state.pool_selection.span(media, &list),
    }
    if !reach.places() || offline(state, media) {
        return Ok(false);
    }
    let tick = at(state);
    state.drop_media_on_timeline(MediaId::from_raw(media), None, tick)?;
    Ok(true)
}
