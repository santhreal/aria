//! The Clip menu: naming, speed, linking and grouping, nesting, rendering,
//! audio gain and channels, framing, track mattes, and the clip analyzers.
//!
//! [`super::menu_items::items_for`] dispatches here, so the bar, the palette
//! and the tests reach these rows through the one registry.

use std::borrow::Cow;

use crate::app::AppState;
use crate::app::audio_channels::AudioChannels;
use crate::app::decline::Decline;

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

/// Disables an Audio Channels row with no selected clip with sound, or whose
/// choice every such clip already plays, through the plan its body runs.
fn audio_channels_ready(id: &str, state: &AppState) -> Option<Decline> {
    let choice = AudioChannels::of_row(id)?;
    super::menu_guards::sound(id, state).or_else(|| {
        state
            .channel_targets(choice)
            .is_empty()
            .then_some(Decline::SameChannels)
    })
}

/// Clip > Rename…, which the clip context menu mirrors.
pub(crate) const RENAME: MenuItem = MenuItem::new("clip-rename", "Rename…", |_, s, _| {
    if let Some(clip) = s.selected_clip() {
        s.begin_rename(crate::types::RenameTarget::Clip(clip.raw()));
    }
})
.when(super::menu_guards::selection);

/// Clip > Speed/Duration…, which the clip context menu mirrors.
pub(crate) const SPEED_DURATION: MenuItem =
    MenuItem::new("clip-speed-duration", "Speed/Duration…", |_, s, _| {
        s.open_speed_duration()
    })
    .key("key-speed-duration")
    .when(super::menu_guards::selection)
    .rule();

/// Clip > Time Remapping, which the clip context menu mirrors.
pub(crate) const TIME_REMAP: MenuItem =
    MenuItem::new("clip-time-remap", "Time Remapping", |_, s, _| {
        let remapped = s.toggle_time_remap();
        report(s, remapped)
    })
    .key("key-time-remap")
    .when(super::menu_guards::remappable)
    .checked(|_, s| s.selection_is_remapped());

/// Clip > Enable, which the clip context menu mirrors.
pub(crate) const ENABLE: MenuItem = MenuItem::new("clip-enable", "Enable", |_, s, _| {
    let toggled = s.toggle_enabled_selected();
    report(s, toggled)
})
.key("key-clip-enable")
.when(super::menu_guards::selection)
.checked(|_, s| s.selection_is_enabled())
.rule();

/// Clip > Link, which the clip context menu mirrors.
pub(crate) const LINK: MenuItem = MenuItem::new("seq-link", "Link", |_, s, _| {
    let linked = s.link_selected();
    report(s, linked)
})
.when(super::menu_guards::linkable);

/// Clip > Unlink, which the clip context menu mirrors.
pub(crate) const UNLINK: MenuItem = MenuItem::new("seq-unlink", "Unlink", |_, s, _| {
    let unlinked = s.unlink_selected();
    report(s, unlinked)
})
.when(super::menu_guards::linked);

/// Clip > Group, which the clip context menu mirrors.
pub(crate) const GROUP: MenuItem = MenuItem::new("clip-group", "Group", |_, s, _| {
    let grouped = s.group_selected();
    report(s, grouped)
})
.key("key-clip-group")
.when(super::menu_guards::groupable);

/// Clip > Ungroup, which the clip context menu mirrors.
pub(crate) const UNGROUP: MenuItem = MenuItem::new("clip-ungroup", "Ungroup", |_, s, _| {
    if let Some(why) = super::menu_guards::grouped("clip-ungroup", s) {
        s.decline(why);
        return;
    }
    let ungrouped = s.ungroup_selected();
    report(s, ungrouped)
})
.key("key-clip-ungroup")
.when(super::menu_guards::grouped);

/// Clip > Freeze Frame, which the clip context menu mirrors.
pub(crate) const FREEZE: MenuItem = MenuItem::new("seq-freeze", "Freeze Frame", |_, s, _| {
    let frozen = s.freeze_selected_frame();
    report(s, frozen)
})
.when(freeze_ready)
.rule();

/// Clip > Audio Gain…, which the clip context menu mirrors.
pub(crate) const AUDIO_GAIN: MenuItem =
    MenuItem::new("clip-audio-gain", "Audio Gain…", |_, s, _| {
        s.open_audio_gain()
    })
    .key("key-audio-gain")
    .when(super::menu_guards::gain)
    .rule();

/// Clip > Extract Audio, which the clip context menu mirrors.
pub(crate) const EXTRACT_AUDIO: MenuItem =
    MenuItem::new("seq-extract-audio", "Extract Audio", |_, s, _| {
        let extracted = s.extract_selected_audio();
        report(s, extracted)
    })
    .when(super::menu_guards::extractable);

static CLIP_ITEMS: &[MenuItem] = &[
    RENAME,
    MenuItem::new("clip-subclip", "Make Subclip", |_, s, _| {
        let made = s.create_subclip_from_source(None).map(drop);
        report(s, made)
    })
    .when(super::menu_guards::source_range),
    SPEED_DURATION,
    TIME_REMAP,
    ENABLE,
    LINK,
    UNLINK,
    GROUP,
    UNGROUP,
    MenuItem::new("seq-nest", "Nest…", |_, s, _| {
        crate::commands::reach::nest_selection(s)
    })
    .when(super::menu_guards::selection)
    .rule(),
    MenuItem::new("clip-replace-source", "From Source Monitor", |_, s, _| {
        let replaced = s.replace_with_source();
        report(s, replaced)
    })
    .when(super::menu_guards::source_replacement)
    .under("Replace with Clip"),
    FREEZE,
    MenuItem::new("seq-bake-clip", "Render and Replace", |_, s, _| {
        if let Err(err) = s.render_and_replace() {
            s.status_message = Some(err);
        }
    })
    .when(bake_ready),
    MenuItem::new("seq-bake-restore", "Restore Unrendered", |_, s, _| {
        if let Some(why) = baked_clip("seq-bake-restore", s) {
            s.decline(why);
            return;
        }
        if let Err(err) = s.restore_unrendered() {
            s.status_message = Some(err);
        }
    })
    .when(baked_clip),
    AUDIO_GAIN,
    MenuItem::new("clip-channels-stereo", "Stereo", run_audio_channels)
        .checked(audio_channels_checked)
        .when(audio_channels_ready)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-swap", "Swap Channels", run_audio_channels)
        .checked(audio_channels_checked)
        .when(audio_channels_ready)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-left", "Left to Both", run_audio_channels)
        .checked(audio_channels_checked)
        .when(audio_channels_ready)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-right", "Right to Both", run_audio_channels)
        .checked(audio_channels_checked)
        .when(audio_channels_ready)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-mono", "Mono", run_audio_channels)
        .checked(audio_channels_checked)
        .when(audio_channels_ready)
        .under(AUDIO_CHANNELS),
    EXTRACT_AUDIO,
    MenuItem::new("seq-flip-h", "Flip Horizontal", |_, s, _| {
        let flipped = s.toggle_selected_flip_h();
        report(s, flipped)
    })
    .when(super::menu_guards::selection)
    .under("Transform")
    .rule(),
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
    .when(super::menu_guards::framing)
    .under("Transform")
    .flyout_rule(),
    MenuItem::new("seq-matte-luma", "Luma Matte from Above", |_, s, _| {
        let set = s.set_selected_matte_source(aria_models::MatteSource::Luma);
        report(s, set)
    })
    .when(|_, s| matte_source_ready(s, aria_models::MatteSource::Luma))
    .under("Track Matte"),
    MenuItem::new("seq-matte-alpha", "Alpha Matte from Above", |_, s, _| {
        let set = s.set_selected_matte_source(aria_models::MatteSource::Alpha);
        report(s, set)
    })
    .when(|_, s| matte_source_ready(s, aria_models::MatteSource::Alpha))
    .under("Track Matte"),
    MenuItem::new("seq-matte-clear", "Remove Track Matte", |_, s, _| {
        let cleared = s.clear_selected_matte();
        report(s, cleared)
    })
    .when(super::menu_guards::matte)
    .under("Track Matte")
    .flyout_rule(),
];

/// Disables a Matte from Above row with no clip selected, or whose source the
/// selected clip's matte already reads, through the plan its body runs.
fn matte_source_ready(state: &AppState, source: aria_models::MatteSource) -> Option<Decline> {
    state.matte_source_to_set(source).err()
}

/// Disables Freeze Frame with no clip selected, a clip with no video frame to
/// hold, or one whose [`aria_models::FreezeRule`], which the engine's freeze
/// edit applies, fails at the playhead, in the words the run's error states.
/// Reads the rule the selection projected, so a drawn menu takes no session
/// lock.
pub(crate) fn freeze_ready(id: &str, state: &AppState) -> Option<Decline> {
    super::menu_guards::freezable(id, state).or_else(|| {
        state.selected_clip()?;
        let Some(rule) = state
            .first_selected_detail()
            .and_then(|detail| detail.freeze.as_ref())
        else {
            return Some(Decline::NoFrameToHold);
        };
        match rule.check(state.playhead.value) {
            Ok(()) => None,
            Err(aria_models::ModelError::Freeze(why)) => Some(Decline::Freeze(why)),
            Err(aria_models::ModelError::Split(why)) => Some(Decline::Split(why)),
            Err(_) => Some(Decline::NoFrameToHold),
        }
    })
}

/// Render and Replace's row: disabled for the reason the run would post, read
/// from the verdict the selection projected, because a guard runs on every
/// draw of the menu.
fn bake_ready(id: &str, state: &AppState) -> Option<Decline> {
    super::menu_guards::selection(id, state).or_else(|| {
        if state.project_path.is_none() {
            return Some(Decline::UnsavedRender);
        }
        state.selected_clip()?;
        state
            .first_selected_detail()
            .map_or(Some(Decline::NotBakeable), |detail| detail.bake)
    })
}

fn baked_clip(id: &str, state: &AppState) -> Option<Decline> {
    super::menu_guards::selection(id, state).or_else(|| {
        state.selected_clip()?;
        (!state
            .first_selected_detail()
            .is_some_and(|detail| detail.model.is_baked()))
        .then_some(Decline::NotRendered)
    })
}

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
    // The analyzers are a group of the drop.
    for (index, entry) in analyzers.into_iter().enumerate() {
        let spec = entry.spec();
        let row = MenuItem::new(
            intern(&format!("{CLIP_ANALYZE_PREFIX}{}", spec.id)),
            intern(&spec.label),
            run_clip_analyzer,
        )
        .when(super::menu_guards::sound)
        .under(ANALYZE);
        items.push(if index == 0 { row.rule() } else { row });
    }
    Cow::Owned(items)
}
