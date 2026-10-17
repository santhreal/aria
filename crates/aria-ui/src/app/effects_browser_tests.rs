//! Contracts for the Effects panel's tree, search, favourites and apply.
//!
//! The class closed here is "an entry the editor can apply is missing from the
//! panel, listed in the wrong place, or applies to the wrong target". The tree
//! is checked row by row against a fixed list, so a row that moves, drops out,
//! or appears twice reads as a diff. The catalog share is checked against the
//! model catalogs at run time, so an effect registered later is listed without
//! this file naming it. Every [`EntryKind`] is enumerated, so a new kind fails
//! here until its group, drag and apply are decided.
//!
//! Not covered: how the rows are drawn. The panel is a direct read of
//! [`tree`]'s output; the drawn census covers the sidebar as a whole.

use std::collections::BTreeSet;
use std::path::PathBuf;

use aria_models::{ClipId, EffectInstance, Preset};
use aria_plugin::{Plugin, PluginContext, PluginError, PluginManifest};

use super::{
    EffectsBrowser, Entry, EntryKind, Failed, FolderKey, Group, Node, QUERY_MAX_LEN, favourite_key,
    sort_for_tree,
};
use crate::app::AppState;
use crate::app::premiere_tests::{with_clip, with_media};
use crate::types::{CatalogDrag, SidebarTab};

/// A small fixed catalog: two folders and the Other folder under Video
/// Effects, one audio effect, one picture and one sound transition, one
/// generator.
fn rows() -> Vec<Entry> {
    let mut rows = vec![
        Entry::new(
            EntryKind::VideoEffect,
            "gaussian_blur",
            "Gaussian Blur",
            "Blur & Sharpen",
        ),
        Entry::new(
            EntryKind::VideoEffect,
            "box_blur",
            "Box Blur",
            "Blur & Sharpen",
        ),
        Entry::new(EntryKind::VideoEffect, "vignette", "Vignette", "Stylize"),
        Entry::new(
            EntryKind::VideoEffect,
            "odd",
            "Odd One",
            aria_models::OTHER_VIDEO_CATEGORY,
        ),
        Entry::new(EntryKind::AudioEffect, "gain", "Gain", "Amplitude"),
        Entry::new(
            EntryKind::Transition,
            "cross_dissolve",
            "Cross Dissolve",
            "",
        ),
        Entry::new(
            EntryKind::AudioTransition,
            "constant_power",
            "Constant Power",
            "",
        ),
        Entry::new(EntryKind::Generator, "bars", "Égaliseur Bars", ""),
    ];
    sort_for_tree(&mut rows);
    rows
}

/// The tree as one line per row: indent by depth, `v`/`>` for an open or
/// closed folder with its count, `*` after a starred entry, `!` for a failure.
fn shape(nodes: &[Node<'_>]) -> Vec<String> {
    nodes
        .iter()
        .map(|node| match node {
            Node::Folder {
                label,
                depth,
                open,
                count,
                ..
            } => format!(
                "{}{} {label} ({count})",
                "  ".repeat(usize::from(*depth)),
                if *open { "v" } else { ">" }
            ),
            Node::Entry {
                entry,
                depth,
                favourite,
                ..
            } => format!(
                "{}{}{}",
                "  ".repeat(usize::from(*depth)),
                entry.label,
                if *favourite { " *" } else { "" }
            ),
            Node::Failed { failed } => format!("  ! {}: {}", failed.source, failed.reason),
        })
        .collect()
}

/// The tree with every group listed.
fn tree<'a>(
    entries: &[&'a [Entry]],
    failed: &'a [Failed],
    favourites: &[String],
    query: &str,
    open: &BTreeSet<FolderKey>,
) -> Vec<Node<'a>> {
    super::tree(entries, failed, favourites, query, open, None)
}

fn open(keys: &[FolderKey]) -> BTreeSet<FolderKey> {
    keys.iter().cloned().collect()
}

fn folder(group: Group, category: &str) -> FolderKey {
    FolderKey {
        group,
        category: category.to_owned().into(),
    }
}

#[test]
fn a_closed_tree_lists_every_group_with_its_count() {
    let rows = rows();
    let nodes = tree(&[&rows], &[], &[], "", &BTreeSet::new());
    assert_eq!(
        shape(&nodes),
        [
            "> Video Effects (4)",
            "> Audio Effects (1)",
            "> Video Transitions (1)",
            "> Audio Transitions (1)",
            "> Generators (1)",
            "> Animation Presets (0)",
        ]
    );
}

/// Folders sort by name with the catalog's Other folder last, entries sort by
/// label, and a group without folders lists its entries one level down.
#[test]
fn an_open_folder_lists_its_entries_by_label_with_other_last() {
    let rows = rows();
    let nodes = tree(
        &[&rows],
        &[],
        &[],
        "",
        &open(&[
            FolderKey::group(Group::VideoEffects),
            folder(Group::VideoEffects, "Blur & Sharpen"),
            folder(Group::VideoEffects, aria_models::OTHER_VIDEO_CATEGORY),
            FolderKey::group(Group::Transitions),
            FolderKey::group(Group::AudioTransitions),
        ]),
    );
    assert_eq!(
        shape(&nodes),
        [
            "v Video Effects (4)",
            "  v Blur & Sharpen (2)",
            "    Box Blur",
            "    Gaussian Blur",
            "  > Stylize (1)",
            "  v Other (1)",
            "    Odd One",
            "> Audio Effects (1)",
            "v Video Transitions (1)",
            "  Cross Dissolve",
            "v Audio Transitions (1)",
            "  Constant Power",
            "> Generators (1)",
            "> Animation Presets (0)",
        ]
    );
}

/// A search ignores case, matches the label, the id and the folder, opens
/// every folder holding a match whatever was open before, and hides every
/// group with none.
#[test]
fn a_search_opens_matching_folders_and_hides_the_rest() {
    let rows = rows();
    let closed = BTreeSet::new();
    let by_label = tree(&[&rows], &[], &[], "BLUR", &closed);
    assert_eq!(
        shape(&by_label),
        [
            "v Video Effects (2)",
            "  v Blur & Sharpen (2)",
            "    Box Blur",
            "    Gaussian Blur",
        ]
    );
    let by_folder = tree(&[&rows], &[], &[], "sharpen", &closed);
    assert_eq!(shape(&by_folder), shape(&by_label));
    let by_id = tree(&[&rows], &[], &[], "cross_diss", &closed);
    assert_eq!(
        shape(&by_id),
        ["v Video Transitions (1)", "  Cross Dissolve"]
    );
    let sound = tree(&[&rows], &[], &[], "power", &closed);
    assert_eq!(
        shape(&sound),
        ["v Audio Transitions (1)", "  Constant Power"]
    );
    // Case folding is not ASCII-only.
    let accented = tree(&[&rows], &[], &[], "ÉGAL", &closed);
    assert_eq!(shape(&accented), ["v Generators (1)", "  Égaliseur Bars"]);
    assert!(tree(&[&rows], &[], &[], "no such effect", &closed).is_empty());
    // Surrounding blanks are not part of the search, and a blank search is
    // no search.
    assert_eq!(
        shape(&tree(&[&rows], &[], &[], "  blur ", &closed)),
        shape(&by_label)
    );
    assert_eq!(
        shape(&tree(&[&rows], &[], &[], "   ", &closed)),
        shape(&tree(&[&rows], &[], &[], "", &closed))
    );
}

/// A starred entry is listed first under Favourites and again, marked, in its
/// own folder. A key saved for another kind with the same id stars nothing.
#[test]
fn favourites_list_first_and_stay_in_their_own_folder() {
    let rows = rows();
    let favourites = [
        favourite_key(EntryKind::VideoEffect, "vignette"),
        favourite_key(EntryKind::Transition, "cross_dissolve"),
        favourite_key(EntryKind::AudioEffect, "vignette"),
    ];
    let mut folders = EffectsBrowser::default().open;
    folders.insert(FolderKey::group(Group::VideoEffects));
    folders.insert(folder(Group::VideoEffects, "Stylize"));
    let nodes = tree(&[&rows], &[], &favourites, "", &folders);
    assert_eq!(
        shape(&nodes)[..6],
        [
            "v Favourites (2)",
            "  Cross Dissolve *",
            "  Vignette *",
            "v Video Effects (4)",
            "  > Blur & Sharpen (2)",
            "  v Stylize (1)",
        ]
    );
    assert_eq!(shape(&nodes)[6], "    Vignette *");
    // Under a search, Favourites lists only the starred matches.
    let searched = tree(&[&rows], &[], &favourites, "vign", &folders);
    assert_eq!(
        shape(&searched),
        [
            "v Favourites (1)",
            "  Vignette *",
            "v Video Effects (1)",
            "  v Stylize (1)",
            "    Vignette *",
        ]
    );
}

/// A failure is listed last, open, with its reason, and a search reads the
/// reason as well as the name.
#[test]
fn failures_list_last_with_their_reason() {
    let rows = rows();
    let failed = [Failed {
        source: "Broken FX".into(),
        reason: "needs a GPU".into(),
    }];
    let folders = EffectsBrowser::default().open;
    let nodes = shape(&tree(&[&rows], &failed, &[], "", &folders));
    // Last: every group folder comes before it, whatever groups the catalog has.
    assert_eq!(
        nodes[nodes.len() - 2..],
        ["v Failed to Load (1)", "  ! Broken FX: needs a GPU"]
    );
    let searched = tree(&[&rows], &failed, &[], "gpu", &BTreeSet::new());
    assert_eq!(
        shape(&searched),
        ["v Failed to Load (1)", "  ! Broken FX: needs a GPU"]
    );
}

/// Starring twice unstars, and the favourites survive the settings file.
#[test]
fn a_favourite_toggles_and_round_trips_through_the_settings_file() {
    let (mut state, _) = with_clip();
    state.toggle_effect_favourite(EntryKind::VideoEffect, "vignette");
    state.toggle_effect_favourite(EntryKind::Preset, "Warm");
    assert_eq!(state.effect_favourites(), ["video:vignette", "preset:Warm"]);
    state.toggle_effect_favourite(EntryKind::VideoEffect, "vignette");
    assert_eq!(state.effect_favourites(), ["preset:Warm"]);

    let text = serde_json::to_string(&state.ui_settings).expect("serialize");
    assert!(
        text.contains("\"favouriteEffects\":[\"preset:Warm\"]"),
        "{text}"
    );
    let back: crate::settings::UiSettings = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(back.favourite_effects, ["preset:Warm"]);
    let older: crate::settings::UiSettings =
        serde_json::from_str(r#"{"sidebarWidth":300.0}"#).expect("older settings file");
    assert!(older.favourite_effects.is_empty());
}

#[test]
fn the_search_stops_at_its_cap() {
    let (mut state, _) = with_clip();
    for _ in 0..QUERY_MAX_LEN + 10 {
        state.effects_query_push('é');
    }
    assert_eq!(state.effects_browser.query.chars().count(), QUERY_MAX_LEN);
    state.effects_query_pop();
    assert_eq!(
        state.effects_browser.query.chars().count(),
        QUERY_MAX_LEN - 1
    );
    state.clear_effects_query();
    assert!(state.effects_browser.query.is_empty());
}

/// A closed folder opens and an open one closes; a single click picks a row
/// without applying it.
#[test]
fn folders_toggle_and_a_pick_applies_nothing() {
    let (mut state, clip) = with_clip();
    let video = FolderKey::group(Group::VideoEffects);
    state.toggle_effects_folder(video.clone());
    assert!(state.effects_browser.open.contains(&video));
    state.toggle_effects_folder(video.clone());
    assert!(!state.effects_browser.open.contains(&video));

    let before = state.engine.lock().project().clip(clip).cloned();
    state.pick_effects_entry(EntryKind::VideoEffect, "vignette".into());
    assert_eq!(
        state.effects_browser.picked,
        Some((EntryKind::VideoEffect, "vignette".into()))
    );
    assert_eq!(state.engine.lock().project().clip(clip).cloned(), before);
}

/// Shift+5 opens Effects & Presets when it is closed and brings it forward with
/// its box holding the keys. The box takes keys while either surface that
/// draws it, the panel or the Media panel's Effects tab, is the front tab of
/// its frame, and none once neither is; the bin search takes focus from it.
#[test]
fn shift_5_focuses_the_search_and_a_hidden_box_takes_no_keys() {
    use crate::dock::{PANEL_EFFECTS, PANEL_HISTORY, PANEL_JOBS, PANEL_PROJECT};
    let (mut state, _) = with_clip();
    assert!(!state.effects_search_focused());
    state.sidebar_tab = SidebarTab::Media;
    state.dock_tree.close(PANEL_EFFECTS);
    assert!(!state.dock_tree.is_open(PANEL_EFFECTS));

    let binding = crate::keys::bindings::SHIPPED_BINDINGS
        .iter()
        .find(|binding| binding.id == "key-pane-effects")
        .expect("Shift+5 is bound");
    let crate::keys::KeyBody::Plain(run) = binding.run else {
        panic!("key-pane-effects runs on the state alone");
    };
    run(&mut state);
    assert!(state.dock_tree.is_active(PANEL_EFFECTS));
    let field = |state: &AppState| crate::keys::fields::focused(state).map(|field| field.id);
    assert_eq!(field(&state), Some("aria.field.effects-search"));

    // Behind another tab of its frame the panel draws nothing.
    let frame = state.dock_tree.find(PANEL_EFFECTS).expect("open");
    assert!(state.dock_tree.add_tab(frame, PANEL_HISTORY));
    assert_eq!(field(&state), None);
    // The Media panel's Effects tab draws the same box while Media is in front.
    assert!(state.dock_tree.activate(PANEL_PROJECT));
    state.sidebar_tab = SidebarTab::Effects;
    assert_eq!(field(&state), Some("aria.field.effects-search"));
    let frame = state.dock_tree.find(PANEL_PROJECT).expect("open");
    assert!(state.dock_tree.add_tab(frame, PANEL_JOBS));
    assert_eq!(field(&state), None);
    assert!(state.dock_tree.activate(PANEL_EFFECTS));
    assert_eq!(field(&state), Some("aria.field.effects-search"));

    state.focus_media_search(true);
    assert!(!state.effects_browser.focused);
    state.focus_effects_search(true);
    assert!(!state.media_search_focus);
}

/// The shared rows hold every video effect, audio effect, transition and clip
/// type the catalogs list, each in its group and folder, in tree order.
#[test]
fn the_catalog_share_lists_every_catalog_entry_in_tree_order() {
    let shared = crate::catalog_text::effect_entries();
    let has = |kind: EntryKind, id: &str, category: &str| {
        shared
            .iter()
            .any(|entry| entry.kind == kind && entry.id == id && entry.category == category)
    };
    for spec in aria_models::effect_catalog().iter() {
        if spec.generator {
            assert!(has(EntryKind::Generator, &spec.id, ""), "{}", spec.id);
        } else {
            assert!(
                has(EntryKind::VideoEffect, &spec.id, spec.category()),
                "{}",
                spec.id
            );
        }
    }
    for spec in aria_models::audio_effect_catalog().iter() {
        assert!(
            has(EntryKind::AudioEffect, &spec.id, spec.category()),
            "{}",
            spec.id
        );
    }
    for spec in aria_models::transition_catalog().iter() {
        assert!(has(EntryKind::Transition, &spec.id, ""), "{}", spec.id);
    }
    for curve in aria_models::AudioTransition::ALL {
        assert!(
            has(EntryKind::AudioTransition, curve.id(), ""),
            "{}",
            curve.id()
        );
    }
    for spec in aria_models::clip_type_catalog().iter() {
        assert!(has(EntryKind::ClipType, &spec.id, ""), "{}", spec.id);
    }
    let mut sorted = shared.to_vec();
    sort_for_tree(&mut sorted);
    assert_eq!(&sorted[..], &shared[..]);
    // A second read at the same revisions is the same share, not a rebuild.
    assert!(std::sync::Arc::ptr_eq(
        &shared,
        &crate::catalog_text::effect_entries()
    ));
}

/// Every kind lists under one group, carries a drag the timeline lands on the
/// right target class, and saves favourites under a tag no other kind uses.
#[test]
fn every_kind_has_a_group_a_drop_target_and_its_own_tag() {
    let mut tags = BTreeSet::new();
    for kind in EntryKind::ALL {
        // `true` for a kind that places a clip of its own on empty lane space.
        let (group, places) = match kind {
            EntryKind::VideoEffect => (Group::VideoEffects, false),
            EntryKind::AudioEffect => (Group::AudioEffects, false),
            EntryKind::Transition => (Group::Transitions, false),
            EntryKind::AudioTransition => (Group::AudioTransitions, false),
            EntryKind::Generator | EntryKind::ClipType => (Group::Generators, true),
            EntryKind::Preset => (Group::Presets, false),
        };
        assert_eq!(kind.group(), group, "{kind:?}");
        let drag = CatalogDrag {
            kind: kind.drag(),
            id: "x".into(),
            label: "x".into(),
        };
        assert_eq!(drag.targets_a_clip(), !places, "{kind:?}");
        assert!(tags.insert(kind.tag()), "{kind:?} reuses a tag");
    }
}

/// A preset the panel lists applies as the Presets section applies it, a
/// saved name shadows a plugin preset of the same name, and the tooltip names
/// where each entry came from.
#[test]
fn presets_list_once_and_name_their_origin() {
    static MANIFEST: PluginManifest =
        PluginManifest::new("test.effects-pack", "Effects Pack", "0.1.0", &[], &[]);
    struct Pack;
    impl Plugin for Pack {
        fn manifest(&self) -> &PluginManifest {
            &MANIFEST
        }
        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            for name in ["Studio Vignette", "Pack Only"] {
                ctx.register_preset(Preset::new(
                    name,
                    vec![EffectInstance::new("vignette")],
                    Vec::new(),
                    Vec::new(),
                ))?;
            }
            Ok(())
        }
    }

    let (mut state, clip) = with_clip();
    state.ctx.mount(&Pack).expect("mount the pack");
    state.add_effect_to_selected("vignette").expect("vignette");
    state
        .save_selected_preset("studio vignette")
        .expect("save preset");
    state.remove_all_effects_selected().expect("strip effects");

    let listed: Vec<String> = state
        .effects_preset_entries()
        .iter()
        .map(|entry| entry.label.to_string())
        .collect();
    assert_eq!(listed, ["Pack Only", "studio vignette"]);
    assert_eq!(
        state.effects_entry_origin(EntryKind::Preset, "Studio Vignette"),
        "Saved preset"
    );
    assert_eq!(
        state.effects_entry_origin(EntryKind::Preset, "Pack Only"),
        "Plugin: Effects Pack"
    );
    assert_eq!(
        state.effects_entry_origin(EntryKind::VideoEffect, "vignette"),
        "Built in"
    );

    state
        .apply_effects_entry(EntryKind::Preset, "Pack Only")
        .expect("apply");
    let effects: Vec<String> = state
        .engine
        .lock()
        .project()
        .clip(clip)
        .expect("clip")
        .effects
        .iter()
        .map(|effect| effect.effect_id.clone())
        .collect();
    assert_eq!(effects, ["vignette"]);
}

/// A plugin that did not mount is listed with its name and reason.
#[test]
fn a_plugin_that_did_not_mount_is_listed_with_its_reason() {
    static MANIFEST: PluginManifest =
        PluginManifest::new("test.broken-fx", "Broken FX", "0.1.0", &[], &[]);
    struct Broken;
    impl Plugin for Broken {
        fn manifest(&self) -> &PluginManifest {
            &MANIFEST
        }
        fn mount(&self, _: &PluginContext) -> Result<(), PluginError> {
            Err(PluginError::MountFailed(
                "test.broken-fx".into(),
                "needs a GPU".into(),
            ))
        }
    }

    let (state, _) = with_clip();
    assert!(state.ctx.mount(&Broken).is_err());
    let failures = state.effects_failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].source, "Broken FX");
    assert!(
        failures[0].reason.contains("needs a GPU"),
        "{}",
        failures[0].reason
    );
}

/// Applying an entry reaches the same edit its menu row sends, on the
/// selection: a video effect on the selected clip, an audio effect on a clip
/// with sound, and a generator with nothing behind its id refused with no
/// clip placed.
#[test]
fn applying_an_entry_edits_the_selection() {
    let (mut state, clip) = with_clip();
    state
        .apply_effects_entry(EntryKind::VideoEffect, "vignette")
        .expect("apply");
    let ui = state.selected_ui_clip().expect("clip");
    assert_eq!(ClipId::from_raw(ui.id), clip);
    assert!(ui.effects.iter().any(|effect| effect.id == "vignette"));

    let clips = |state: &AppState| {
        state
            .store
            .sequence
            .tracks
            .iter()
            .map(|track| track.clips.len())
            .sum::<usize>()
    };
    let before = clips(&state);
    assert!(
        state
            .apply_effects_entry(EntryKind::Generator, "test.unregistered_generator")
            .is_err()
    );
    assert_eq!(clips(&state), before);

    let (mut sound, _) = with_media(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4"),
    );
    let audible = sound
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.can_hold_inserts)
        .map(|clip| ClipId::from_raw(clip.id))
        .expect("the fixture places a clip with sound");
    sound.select_clip(audible);
    sound
        .apply_effects_entry(EntryKind::AudioEffect, "gain")
        .expect("apply");
    let inserts = sound
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| ClipId::from_raw(clip.id) == audible)
        .map(|clip| clip.audio_inserts.clone())
        .expect("clip");
    assert!(inserts.iter().any(|insert| insert.effect_id == "gain"));
}

/// Every generator the editor ships is listed under Generators, and applying
/// it places a clip of its own: named after the generator, holding it first in
/// its chain. The shipped catalog has to list one, so the group cannot ship
/// empty.
#[test]
fn every_shipped_generator_is_listed_and_places_a_clip_of_its_own() {
    let shipped: Vec<_> = aria_models::shipped_effect_catalog()
        .iter()
        .filter(|spec| spec.generator)
        .collect();
    assert!(
        !shipped.is_empty(),
        "the shipped catalog lists no generator"
    );
    let shared = crate::catalog_text::effect_entries();
    let (mut state, _) = with_clip();
    let ids = |state: &AppState| {
        state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(|clip| clip.id)
            .collect::<BTreeSet<_>>()
    };
    for spec in shipped {
        assert!(
            shared
                .iter()
                .any(|entry| entry.kind == EntryKind::Generator && entry.id == spec.id.as_str()),
            "{} is listed under Generators",
            spec.id
        );
        let before = ids(&state);
        state
            .apply_effects_entry(EntryKind::Generator, &spec.id)
            .unwrap_or_else(|err| panic!("{} places: {err:?}", spec.id));
        let placed: Vec<_> = ids(&state).difference(&before).copied().collect();
        assert_eq!(placed.len(), 1, "{} places one clip", spec.id);
        let clip = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|clip| clip.id == placed[0])
            .expect("the placed clip");
        assert_eq!(clip.name.as_str(), spec.label.as_str(), "{}", spec.id);
        assert_eq!(
            clip.effects.first().map(|effect| effect.id.as_str()),
            Some(spec.id.as_str()),
            "{} is first in its clip's chain",
            spec.id
        );
    }
}

/// A filtered tree lists the one group, its starred entries under Favourites,
/// and every failure; a search inside it matches that group alone. Clearing
/// the filter lists every group again.
#[test]
fn a_filter_lists_one_group_with_its_favourites_and_failures() {
    let rows = rows();
    let failed = [Failed {
        source: "Broken FX".into(),
        reason: "needs a GPU".into(),
    }];
    let favourites = [
        favourite_key(EntryKind::AudioEffect, "gain"),
        favourite_key(EntryKind::VideoEffect, "vignette"),
    ];
    let folders = open(&[
        FolderKey::group(Group::Favourites),
        FolderKey::group(Group::AudioEffects),
        folder(Group::AudioEffects, "Amplitude"),
        FolderKey::group(Group::Failed),
    ]);
    let audio = Some(Group::AudioEffects);
    assert_eq!(
        shape(&super::tree(
            &[&rows],
            &failed,
            &favourites,
            "",
            &folders,
            audio
        )),
        [
            "v Favourites (1)",
            "  Gain *",
            "v Audio Effects (1)",
            "  v Amplitude (1)",
            "    Gain *",
            "v Failed to Load (1)",
            "  ! Broken FX: needs a GPU",
        ]
    );
    // "a" matches video and audio rows; only the audio one is listed.
    assert_eq!(
        shape(&super::tree(
            &[&rows],
            &[],
            &[],
            "a",
            &BTreeSet::new(),
            audio
        )),
        ["v Audio Effects (1)", "  v Amplitude (1)", "    Gain"]
    );

    let (mut state, _) = with_clip();
    assert_eq!(state.effects_browser.only, None);
    state.set_effects_filter(audio);
    assert_eq!(state.effects_browser.only, audio);
    state.set_effects_filter(None);
    assert_eq!(
        shape(&super::tree(
            &[&rows],
            &[],
            &[],
            "",
            &BTreeSet::new(),
            state.effects_browser.only
        )),
        shape(&tree(&[&rows], &[], &[], "", &BTreeSet::new()))
    );
}
