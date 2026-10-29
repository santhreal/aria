//! The rows an effect header's menu draws.
//!
//! The menu opens on one effect in the Effects tab, from a right-click on its
//! header or from the header's menu button. Each row acts on that effect by its
//! clip and its place in the chain, which the facts hold, rather than on
//! whatever is selected by the time the row is pressed. On the drawn clip of a
//! selection, a row that edits the effect also edits it on every other
//! selected clip holding the same effect there, as one undo step.

use aria_models::ClipId;

use crate::app::AppState;
use crate::app::effect_header::{DUPLICATE_EFFECT_LABEL, PASTE_EFFECT_LABEL};
use crate::types::RenameTarget;

use super::{MenuItem, MenuRow, always};

/// The effect facts a row reads, gathered once when the menu opens.
#[derive(Debug, Clone, Copy)]
pub struct EffectMenu {
    /// The clip whose chain holds the effect.
    pub clip: u64,
    /// Where the effect sits in the chain.
    pub index: usize,
    /// A copied effect is held, so a paste has something to add.
    pub copied: bool,
}

impl EffectMenu {
    /// Read the effect facts a menu would gather when it opens over the
    /// effect at `index` in `clip`'s chain.
    pub fn of(state: &AppState, clip: u64, index: usize) -> Self {
        Self {
            clip,
            index,
            copied: state.copied_effect.is_some(),
        }
    }

    fn clip(&self) -> ClipId {
        ClipId::from_raw(self.clip)
    }
}

/// An effect header's menu, in the order it draws.
pub const EFFECT_ITEMS: &[MenuItem<EffectMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-fx-reset",
        label: |_| "Reset",
        when: always(),
        checked: None,
        run: |state, fx| {
            state.effect_verb(fx.clip(), fx.index, None, |state, clip| {
                state.reset_effect(clip, fx.index)
            })
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-fx-rename",
        label: |_| "Rename Effect",
        when: always(),
        checked: None,
        run: |state, fx| {
            state.begin_rename(RenameTarget::Effect {
                clip: fx.clip,
                index: fx.index,
            });
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-fx-duplicate",
        label: |_| "Duplicate",
        when: always(),
        checked: None,
        run: |state, fx| {
            state.effect_verb(
                fx.clip(),
                fx.index,
                Some(DUPLICATE_EFFECT_LABEL),
                |state, clip| state.duplicate_effect(clip, fx.index),
            )
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-fx-copy",
        label: |_| "Copy",
        when: always(),
        checked: None,
        run: |state, fx| {
            state.copy_effect(fx.clip(), fx.index);
            Ok(())
        },
    }),
    // Below rather than at the end: the menu was opened on a place in the
    // chain, and the render order is the chain's order.
    MenuItem::Row(MenuRow {
        id: "ctx-fx-paste",
        label: |_| "Paste Below",
        when: |fx| fx.copied,
        checked: None,
        run: |state, fx| {
            state.effect_verb(
                fx.clip(),
                fx.index,
                Some(PASTE_EFFECT_LABEL),
                |state, clip| state.paste_effect(clip, fx.index + 1),
            )
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-fx-save-preset",
        label: |_| "Save as Preset…",
        when: always(),
        checked: None,
        run: |state, fx| {
            state.save_effect_as_preset(fx.clip(), fx.index);
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-fx-remove",
        label: |_| "Remove Effect",
        when: always(),
        checked: None,
        run: |state, fx| {
            state.effect_verb(fx.clip(), fx.index, None, |state, clip| {
                state.remove_effect(clip, fx.index)
            })
        },
    }),
];
