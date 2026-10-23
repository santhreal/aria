//! The Clip menu: naming, speed, linking and grouping, nesting, rendering,
//! audio gain and channels, framing, track mattes, and the clip analyzers.
//!
//! [`super::menu_items::items_for`] dispatches here, so the bar, the palette
//! and the tests reach these rows through the one registry.

use std::borrow::Cow;

use crate::app::AppState;
use crate::app::audio_channels::AudioChannels;

use super::menu_items::{MenuItem, MenuItems, report};

/// The id prefix of a Clip > Analyze row for one registered clip analyzer.
const CLIP_ANALYZE_PREFIX: &str = "clip-analyze:";

const AUDIO_CHANNELS: &str = "Audio Channels";
const ANALYZE: &str = "Analyze";

/// Clip > Audio Channels > the choice row `id` picks.
fn run_audio_channels(id: &str, state: &mut AppState, _: &mut gpui::Context<AppState>) {
    let Some(choice) = AudioChannels::of_row(id) else {
        return;
    };
    let set = state.set_audio_channels(choice);
    report(state, set)
}

/// Whether the selection plays the choice row `id` picks.
fn audio_channels_checked(id: &str, state: &AppState) -> bool {
    AudioChannels::of_row(id).is_some_and(|choice| state.selection_audio_channels() == Some(choice))
}

static CLIP_ITEMS: &[MenuItem] = &[
    MenuItem::new("clip-rename", "Rename…", |_, s, _| {
        if let Some(clip) = s.selected_clip() {
            s.begin_rename(crate::types::RenameTarget::Clip(clip.raw()));
        }
    })
    .when(super::menu_guards::selection),
    MenuItem::new("clip-subclip", "Make Subclip", |_, s, _| {
        let made = s.create_subclip_from_source(None).map(drop);
        report(s, made)
    })
    .when(super::menu_guards::source_range),
    MenuItem::new("clip-speed-duration", "Speed/Duration…", |_, s, _| {
        s.open_speed_duration()
    })
    .key("key-speed-duration")
    .when(super::menu_guards::selection),
    MenuItem::new("clip-time-remap", "Time Remapping", |_, s, _| {
        let remapped = s.toggle_time_remap();
        report(s, remapped)
    })
    .key("key-time-remap")
    .when(super::menu_guards::selection)
    .checked(|_, s| s.selection_is_remapped()),
    MenuItem::new("clip-enable", "Enable", |_, s, _| {
        let toggled = s.toggle_enabled_selected();
        report(s, toggled)
    })
    .key("key-clip-enable")
    .when(super::menu_guards::selection)
    .checked(|_, s| s.selection_is_enabled()),
    MenuItem::new("seq-link", "Link", |_, s, _| {
        let linked = s.link_selected();
        report(s, linked)
    })
    .when(super::menu_guards::several),
    MenuItem::new("seq-unlink", "Unlink", |_, s, _| {
        let unlinked = s.unlink_selected();
        report(s, unlinked)
    })
    .when(super::menu_guards::linked),
    MenuItem::new("clip-group", "Group", |_, s, _| {
        let grouped = s.group_selected();
        report(s, grouped)
    })
    .key("key-clip-group")
    .when(super::menu_guards::several),
    MenuItem::new("clip-ungroup", "Ungroup", |_, s, _| {
        let ungrouped = s.ungroup_selected();
        report(s, ungrouped)
    })
    .key("key-clip-ungroup")
    .when(super::menu_guards::grouped),
    MenuItem::new("seq-nest", "Nest…", |_, s, _| {
        crate::commands::reach::nest_selection(s)
    })
    .when(super::menu_guards::selection),
    MenuItem::new("clip-replace-source", "From Source Monitor", |_, s, _| {
        let replaced = s.replace_with_source();
        report(s, replaced)
    })
    .when(super::menu_guards::source)
    .under("Replace with Clip"),
    MenuItem::new("seq-freeze", "Freeze Frame", |_, s, _| {
        let frozen = s.freeze_selected_frame();
        report(s, frozen)
    })
    .when(super::menu_guards::selection),
    MenuItem::new("seq-bake-clip", "Render and Replace", |_, s, _| {
        if let Err(err) = s.render_and_replace() {
            s.status_message = Some(err);
        }
    })
    .when(super::menu_guards::selection),
    MenuItem::new("seq-bake-restore", "Restore Unrendered", |_, s, _| {
        if let Err(err) = s.restore_unrendered() {
            s.status_message = Some(err);
        }
    })
    .when(super::menu_guards::selection),
    MenuItem::new("clip-audio-gain", "Audio Gain…", |_, s, _| {
        s.open_audio_gain()
    })
    .key("key-audio-gain")
    .when(super::menu_guards::selection),
    MenuItem::new("clip-channels-stereo", "Stereo", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-swap", "Swap Channels", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-left", "Left to Both", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-right", "Right to Both", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-mono", "Mono", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("seq-extract-audio", "Extract Audio", |_, s, _| {
        let extracted = s.extract_selected_audio();
        report(s, extracted)
    })
    .when(super::menu_guards::selection),
    MenuItem::new("seq-flip-h", "Flip Horizontal", |_, s, _| {
        let flipped = s.toggle_selected_flip_h();
        report(s, flipped)
    })
    .when(super::menu_guards::selection)
    .under("Transform"),
    MenuItem::new("seq-flip-v", "Flip Vertical", |_, s, _| {
        let flipped = s.toggle_selected_flip_v();
        report(s, flipped)
    })
    .when(super::menu_guards::selection)
    .under("Transform"),
    MenuItem::new("seq-crop-reset", "Reset Framing", |_, s, _| {
        let reset = s.clear_selected_crop();
        report(s, reset)
    })
    .when(super::menu_guards::selection)
    .under("Transform"),
    MenuItem::new("seq-matte-luma", "Luma Matte from Above", |_, s, _| {
        let set = s.set_selected_matte_source(aria_models::MatteSource::Luma);
        report(s, set)
    })
    .when(super::menu_guards::selection)
    .under("Track Matte"),
    MenuItem::new("seq-matte-alpha", "Alpha Matte from Above", |_, s, _| {
        let set = s.set_selected_matte_source(aria_models::MatteSource::Alpha);
        report(s, set)
    })
    .when(super::menu_guards::selection)
    .under("Track Matte"),
    MenuItem::new("seq-matte-clear", "Remove Track Matte", |_, s, _| {
        let cleared = s.clear_selected_matte();
        report(s, cleared)
    })
    .when(super::menu_guards::selection)
    .under("Track Matte"),
];

fn run_clip_analyzer(id: &str, state: &mut AppState, cx: &mut gpui::Context<AppState>) {
    if let Some(analyzer_id) = id.strip_prefix(CLIP_ANALYZE_PREFIX)
        && let Err(err) = state.run_registered_analyzer(analyzer_id, cx)
    {
        state.report_error(err);
    }
}

pub(super) fn clip_items(state: &AppState) -> MenuItems {
    let analyzers = state
        .ctx
        .analyzers_for_scope(aria_plugin::AnalyzerScope::Clip);
    if analyzers.is_empty() {
        return Cow::Borrowed(CLIP_ITEMS);
    }
    let intern = crate::interned::intern;
    let mut items = Vec::with_capacity(CLIP_ITEMS.len() + analyzers.len());
    items.extend_from_slice(CLIP_ITEMS);
    for entry in analyzers {
        let spec = entry.spec();
        items.push(
            MenuItem::new(
                intern(&format!("{CLIP_ANALYZE_PREFIX}{}", spec.id)),
                intern(&spec.label),
                run_clip_analyzer,
            )
            .when(super::menu_guards::sound)
            .under(ANALYZE),
        );
    }
    Cow::Owned(items)
}
