//! The rows a context menu draws, one table per target.
//!
//! The menu used to be forty `item(id, label, theme, cx, |state| …)` calls
//! inside a `match` on the target. Each closure was the only place its command
//! was named, so nothing could count them, and a row wired to the wrong call
//! read the same as a row wired to the right one.
//!
//! A table per target keeps the shape the menu already had (the target decides
//! what is offered) while making every row drivable: `app::drawn_census` runs
//! each row's `run` against a fixture and reports what it asked for. Rows whose
//! text is data, not code, stay where their data is: the transition blends come
//! from `app::clip_transitions`, and the label swatches are a [`MenuItem::Labels`]
//! block, because a colour swatch is not a text row and the panel owns drawing
//! it. The position of that block is declared here so one place still holds the
//! menu's order.
//!
//! The tables themselves live one file per target under `context_menu/`, because
//! together they were most of a 562-line file. What stays here is what every
//! target shares: the row shape, the item kinds, and the preview menu, which
//! reads no facts.

use aria_engine::EngineError;

use crate::app::AppState;

pub use badge::{BADGE_ITEMS, BadgeMenu, BadgeProperty, draw_property};
pub use clip::{CLIP_ITEMS, ClipMenu, TRANSITION_ITEMS, apply_blend};
pub use effect::{EFFECT_ITEMS, EffectMenu};
pub use pool::{POOL_ITEMS, PoolMenu};
pub use recent::RECENT_ITEMS;
pub use track::{TRACK_ITEMS, TrackMenu};

mod badge;
mod clip;
mod effect;
mod pool;
mod recent;
mod track;

/// One row in a context menu.
///
/// `label` is a function because several rows name the state they would move to
/// (`Mute` against `Unmute`), and `when` because several are offered only when
/// the edit is possible. Both read the facts gathered when the menu opened, so a
/// row never re-reads the session while the menu is on screen.
pub struct MenuRow<F> {
    /// Element id, which the row builder also uses as its hover key.
    pub id: &'static str,
    /// The text the row draws.
    pub label: fn(&F) -> &'static str,
    /// Whether the row is offered at all.
    pub when: fn(&F) -> bool,
    /// What clicking it asks for. An engine refusal is reported by the builder,
    /// once, rather than by each row.
    pub run: fn(&mut AppState, &F) -> Result<(), EngineError>,
}

/// One entry in a menu: a command row, or a block the panel owns.
pub enum MenuItem<F> {
    /// A row that acts on the project or the session. The census presses these.
    Row(MenuRow<F>),
    /// A row whose press leaves the editor: a file manager window, a program the
    /// desktop owns. Drawn exactly like [`MenuItem::Row`] and never pressed by a
    /// test, because a suite that opens a file manager opens one on whoever ran
    /// it. Declared at the row rather than listed in the census, so the default
    /// for a new row is to be measured.
    Outside(MenuRow<F>),
    /// The label palette: one swatch per colour, plus the groups already in use.
    /// The panel draws it, because a swatch row is not a text row.
    Labels,
    /// Every blend the transition catalog offers, with the applied one marked.
    /// The rows are data, so they are generated where their data is rather than
    /// listed here; what they ask for is [`apply_blend`].
    Blends,
    /// Every property a clip bar can draw as its line, with the drawn one
    /// marked. Generated from the clip like [`MenuItem::Blends`]; what each row
    /// asks for is [`draw_property`].
    Properties,
}

impl<F> MenuItem<F> {
    /// The row this entry draws, if it draws one.
    pub fn row(&self) -> Option<&MenuRow<F>> {
        match self {
            Self::Row(row) | Self::Outside(row) => Some(row),
            Self::Labels | Self::Blends | Self::Properties => None,
        }
    }
}

/// Shorthand for a row that is always offered.
const fn always<F>() -> fn(&F) -> bool {
    |_| true
}

/// The preview menu, which acts on the frame rather than on anything under the
/// pointer, so it reads no facts.
pub const PREVIEW_ITEMS: &[MenuItem<()>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-fit",
        label: |_| "Fit Timeline",
        when: always(),
        run: |state, _| {
            state.zoom_fit_sequence();
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-reset-transform",
        label: |_| "Reset Transform",
        when: always(),
        run: |state, _| state.reset_selected_transform(),
    }),
];
