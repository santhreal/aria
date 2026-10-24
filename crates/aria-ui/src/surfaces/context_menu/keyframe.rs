//! The rows a keyframe's menu draws, on a property row, in the graph, and on a
//! mask's Path row.
//!
//! Opening the menu selects the keyframe under the pointer unless it is already
//! in the selection, so the rows that act on the selected keyframes act on it
//! and on whatever else was selected with it. Linear and Rove Across Time act
//! on that keyframe alone, by the clip, property and tick the facts hold.
//!
//! A Path keyframe is the mask's whole outline at one time, so its menu shapes
//! the segment that leaves it and removes it, and has no velocity to ease.

use aria_engine::EngineError;
use aria_models::{ClipId, ClipParam, Easing};

use crate::app::AppState;

use super::{MenuItem, MenuRow, always};

/// The keyframe facts a row reads, gathered once when the menu opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyframeMenu {
    pub clip: ClipId,
    pub param: ClipParam,
    /// The keyframe's timeline tick.
    pub tick: i64,
    /// A keyframe lies on each side of it, so it can rove.
    pub interior: bool,
    pub hold: bool,
    pub roving: bool,
    /// Keyframes are copied, so a paste has something to add.
    pub copied: bool,
}

impl KeyframeMenu {
    /// Read the facts a menu would gather when it opens over the keyframe at
    /// timeline `tick` on `param` of `clip`, or `None` when no property row
    /// draws one there.
    pub fn of(state: &AppState, clip: u64, param: ClipParam, tick: i64) -> Option<Self> {
        let clip = ClipId::from_raw(clip);
        let mark = state.keyframe_mark(clip, param, tick)?;
        Some(Self {
            clip,
            param,
            tick,
            interior: mark.interior,
            hold: mark.hold,
            roving: mark.roving,
            copied: state.has_copied_keyframes(),
        })
    }
}

/// A keyframe's menu, in the order it draws.
pub const KEYFRAME_ITEMS: &[MenuItem<KeyframeMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-key-linear",
        label: |_| "Linear",
        when: always(),
        checked: None,
        run: |state, key| state.set_keyframe_linear_at(key.clip, key.param, key.tick),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-key-easy-ease",
        label: |_| "Easy Ease",
        when: always(),
        checked: None,
        run: |state, _| state.easy_ease_selected_or_playhead(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-key-easy-ease-in",
        label: |_| "Easy Ease In",
        when: always(),
        checked: None,
        run: |state, _| state.easy_ease_in_selected_or_playhead(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-key-easy-ease-out",
        label: |_| "Easy Ease Out",
        when: always(),
        checked: None,
        run: |state, _| state.easy_ease_out_selected_or_playhead(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-key-hold",
        label: |_| "Hold",
        when: always(),
        checked: Some(|key| key.hold),
        run: |state, _| state.toggle_hold_selected_or_playhead(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-key-rove",
        label: |_| "Rove Across Time",
        when: |key| key.interior,
        checked: Some(|key| key.roving),
        run: |state, key| state.toggle_roving_keyframe(key.clip, key.param, key.tick),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-key-copy",
        label: |_| "Copy Keyframes",
        when: always(),
        checked: None,
        run: |state, _| {
            state.copy_selected_keyframes();
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-key-paste",
        label: |_| "Paste Keyframes at Playhead",
        when: |key| key.copied,
        checked: None,
        run: |state, _| state.paste_keyframes_at_playhead().map(drop),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-key-delete",
        label: |_| "Delete Keyframes",
        when: always(),
        checked: None,
        run: |state, _| state.delete_selected_keyframes().map(drop),
    }),
];

/// The Path keyframe facts a row reads, gathered once when the menu opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskPathKeyMenu {
    pub clip: ClipId,
    pub mask: u32,
    /// The keyframe's timeline tick.
    pub tick: i64,
    pub hold: bool,
}

impl MaskPathKeyMenu {
    /// Read the facts a menu would gather when it opens over the Path keyframe
    /// at timeline `tick` of the mask at `mask` on `clip`, or `None` when that
    /// mask's Path row draws none there.
    pub fn of(state: &AppState, clip: u64, mask: u32, tick: i64) -> Option<Self> {
        let clip = ClipId::from_raw(clip);
        let mark = state.mask_path_key_mark(clip, mask, tick)?;
        Some(Self {
            clip,
            mask,
            tick,
            hold: mark.hold,
        })
    }

    /// Shape the segment leaving the keyframe with `easing`.
    fn ease(&self, state: &mut AppState, easing: Easing) -> Result<(), EngineError> {
        state.ease_mask_path_key(self.clip, self.mask, self.tick, easing)
    }
}

/// A Path keyframe's menu, in the order it draws.
pub const MASK_PATH_KEY_ITEMS: &[MenuItem<MaskPathKeyMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-path-key-linear",
        label: |_| "Linear",
        when: always(),
        checked: None,
        run: |state, key| key.ease(state, Easing::Linear),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-path-key-ease",
        label: |_| "Easy Ease",
        when: always(),
        checked: None,
        run: |state, key| key.ease(state, Easing::EaseInOut),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-path-key-ease-in",
        label: |_| "Easy Ease In",
        when: always(),
        checked: None,
        run: |state, key| key.ease(state, Easing::EaseIn),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-path-key-ease-out",
        label: |_| "Easy Ease Out",
        when: always(),
        checked: None,
        run: |state, key| key.ease(state, Easing::EaseOut),
    }),
    // A held segment goes back to the straight one a new keyframe starts with.
    MenuItem::Row(MenuRow {
        id: "ctx-path-key-hold",
        label: |_| "Hold",
        when: always(),
        checked: Some(|key| key.hold),
        run: |state, key| {
            let easing = if key.hold {
                Easing::Linear
            } else {
                Easing::Hold
            };
            key.ease(state, easing)
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-path-key-delete",
        label: |_| "Delete Keyframe",
        when: always(),
        checked: None,
        run: |state, key| state.remove_mask_path_key(key.clip, key.mask, key.tick),
    }),
];
