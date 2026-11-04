//! The Effect menu: the effect controls, the effect applied last, the copied
//! effect, the selected clips' effects, grade, mask and key, then one submenu
//! per effect category listing every video and audio effect in the catalogs.

use std::borrow::Cow;

use crate::app::AppState;
use crate::app::effects_browser::EntryKind;
use crate::types::ActivePanel;

use super::menu_guards as guards;
use super::menu_items::{MenuItem, MenuItems, report};

/// Brings Effect Controls forward. Its key, Shift+4, is drawn on the Window
/// row of the panel, as every pane key is.
static EFFECT_HEAD: MenuItem = MenuItem::new("fx-controls", "Effect Controls", |_, s, _| {
    s.focus_pane(ActivePanel::Inspector)
});

static EFFECT_ITEMS: &[MenuItem] = &[
    MenuItem::new("fx-paste", "Paste Effect", |_, s, _| {
        let pasted = s.paste_effect_to_selection();
        report(s, pasted)
    })
    .when(guards::copied_effect),
    MenuItem::new("fx-remove-all", "Remove All Effects", |_, s, _| {
        let removed = s.remove_all_effects_selected();
        report(s, removed)
    })
    .when(guards::effects)
    .rule(),
    MenuItem::new("seq-effect-reset", "Reset Effect Parameters", |_, s, _| {
        crate::commands::reach::reset_effects(s)
    })
    .key("key-effect-reset")
    .when(guards::effects),
    MenuItem::new("seq-grade-reset", "Reset Grade", |_, s, _| {
        let reset = s.clear_selected_adjustments();
        report(s, reset)
    })
    .when(guards::selection),
    MenuItem::new("seq-mask-clear", "Remove Mask", |_, s, _| {
        let cleared = s.clear_selected_mask();
        report(s, cleared)
    })
    .when(guards::mask),
    MenuItem::new("seq-chroma-clear", "Remove Chroma Key", |_, s, _| {
        let cleared = s.clear_selected_chroma();
        report(s, cleared)
    })
    .when(guards::selection),
];

/// The row that applies the effect applied last, captioned with its name.
fn last_effect_row(state: &AppState) -> MenuItem {
    let label = state
        .last_effect
        .as_deref()
        .and_then(aria_models::effect_spec)
        .map_or("Apply Last Effect", |spec| {
            crate::interned::intern(&format!("Apply {}", spec.label))
        });
    MenuItem::new("fx-last", label, |_, s, _| {
        let applied = s.apply_last_effect();
        report(s, applied)
    })
    .when(guards::last_effect)
    .rule()
}

/// The id prefix of a row that applies one video effect. The rest of the id
/// is the effect's id.
pub(crate) const APPLY_PREFIX: &str = "fx-apply:";
/// The id prefix of a row that applies one audio effect. The rest of the id is
/// the effect's id.
pub(crate) const APPLY_AUDIO_PREFIX: &str = "fx-apply-audio:";

/// Apply the video effect the row names to the selected clip.
fn apply_video(id: &str, state: &mut AppState, _: &mut gpui::Context<AppState>) {
    if let Some(effect) = id.strip_prefix(APPLY_PREFIX) {
        let applied = state.add_effect_to_selected(effect);
        report(state, applied)
    }
}

/// Apply the audio effect the row names to every selected clip with sound.
fn apply_audio(id: &str, state: &mut AppState, _: &mut gpui::Context<AppState>) {
    if let Some(effect) = id.strip_prefix(APPLY_AUDIO_PREFIX) {
        let applied = state.add_audio_effect_to_selection(effect);
        report(state, applied)
    }
}

/// One row per video effect that changes its input, then one per audio effect,
/// each under its category's submenu, in the order the Effects panel lists
/// them. Generators place clips of their own and are offered where clips are
/// placed, not here. The video categories are a group of the drop, and the
/// audio categories that are not video categories too are the next.
fn push_category_rows(items: &mut Vec<MenuItem>) {
    let intern = crate::interned::intern;
    let entries = crate::catalog_text::effect_entries();
    let video_start = items.len();
    for entry in entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::VideoEffect)
    {
        let row = MenuItem::new(
            intern(&format!("{APPLY_PREFIX}{}", entry.id)),
            intern(&entry.label),
            apply_video,
        )
        .when(guards::selection)
        .under(intern(&entry.category));
        items.push(if items.len() == video_start {
            row.rule()
        } else {
            row
        });
    }
    let audio_start = items.len();
    let mut ruled = false;
    for entry in entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::AudioEffect)
    {
        let category = intern(&entry.category);
        let row = MenuItem::new(
            intern(&format!("{APPLY_AUDIO_PREFIX}{}", entry.id)),
            intern(&entry.label),
            apply_audio,
        )
        .when(guards::sound)
        .under(category);
        // A category the video effects use is drawn in their group already.
        let opens = !ruled
            && !items[video_start..audio_start]
                .iter()
                .any(|video| video.under == Some(category));
        ruled |= opens;
        items.push(if opens { row.rule() } else { row });
    }
}

pub(super) fn effect_items(state: &AppState) -> MenuItems {
    let mut items = Vec::with_capacity(EFFECT_ITEMS.len() + 2);
    items.push(EFFECT_HEAD);
    items.push(last_effect_row(state));
    items.extend_from_slice(EFFECT_ITEMS);
    push_category_rows(&mut items);
    Cow::Owned(items)
}

#[cfg(test)]
#[path = "menu_effect_tests.rs"]
mod tests;
