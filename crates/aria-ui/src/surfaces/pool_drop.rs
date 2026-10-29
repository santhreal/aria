//! What the pool accepts when something is dropped on it.
//!
//! Three gestures file things in the pool: a source onto a folder, a folder onto
//! a folder, and a source onto a spot in the freeform view. Each used to be a
//! `on_drop` closure calling a different setter, spread over two panels, and a
//! drop is the one control a census cannot press: there is no button to find.
//!
//! So a drop is a payload and a place, and one function says what it means. The
//! panels build the payload and call [`apply`]; nothing else decides what a drop
//! does. A new payload cannot compile without an arm in both matches below, and
//! `app::drawn_census` reads the gesture list back out of [`Dropped::what`] and
//! requires a dressed drop for each one.

use aria_engine::EngineError;

use crate::app::AppState;

/// A payload, and where it landed.
///
/// `None` for a folder is the pool root, which is a real destination: it is how
/// something comes back out of a folder.
#[derive(Debug)]
pub enum Dropped {
    /// A source, onto a folder.
    MediaOnBin { media: u64, bin: Option<u64> },
    /// A folder, onto a folder.
    BinOnBin { bin: u64, parent: Option<u64> },
    /// A source, at a spot in the freeform pool.
    MediaAtSpot { media: u64, x: f32, y: f32 },
}

impl Dropped {
    /// The gesture, named for the drop.
    ///
    /// Matched exhaustively, so a new payload is a compile error here and the
    /// census then requires a dressed drop for it.
    pub fn what(&self) -> &'static str {
        match self {
            Dropped::MediaOnBin { .. } => "a source onto a folder",
            Dropped::BinOnBin { .. } => "a folder onto a folder",
            Dropped::MediaAtSpot { .. } => "a source onto a spot in the pool",
        }
    }
}

/// File a dropped payload.
///
/// The project owns the rules a drop can break — a folder cannot go inside its
/// own subtree — so a refusal comes back as an error for the panel to report
/// rather than being pre-checked here in a second place.
pub fn apply(state: &mut AppState, drop: Dropped) -> Result<(), EngineError> {
    match drop {
        Dropped::MediaOnBin { media, bin } => state.move_media_to_bin(media, bin),
        Dropped::BinOnBin { bin, parent } => state.move_bin(bin, parent),
        Dropped::MediaAtSpot { media, x, y } => state.set_pool_item_position(media, x, y),
    }
}
