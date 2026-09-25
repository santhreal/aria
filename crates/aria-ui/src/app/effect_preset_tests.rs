//! Contracts for effect presets (BACKLOG 58).
//!
//! Closes the defect class: "a clip's effect chain cannot be saved, named,
//! listed, or reused across clips without manual attribute pasting".
//!
//! Contracts proven:
//! 1. A preset saved from a clip's effect stack captures the whole chain and applies
//!    the exact stack and parameters to another clip.
//! 2. Presets survive an app restart (reload from the same profile data root).
//! 3. Empty and duplicate preset names are refused with clear error messages.
//! 4. Corrupt or unparseable preset files load safely as an empty list without panicking.
//! 5. Applying a preset is a single atomic undo entry that undoes cleanly.

use std::fs;
use std::path::PathBuf;

use aria_models::ClipId;
use aria_plugin::PluginContext;

use super::AppState;
use crate::app::effect_presets::effect_presets_path;

fn scratch() -> PathBuf {
    crate::scratch_pool::scratch("effect_preset")
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// Create a test state with two visual clips placed on the timeline.
fn with_two_clips() -> (AppState, ClipId, ClipId) {
    let mut state = AppState::new(PluginContext::new(), scratch()).expect("state");
    state.new_project().expect("new project");
    let media = state.import_media(fixture()).expect("import fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place first clip");
    // Past the end of the first clip, read from the store rather than assumed:
    // a drop that lands inside a clip cuts it, and this fixture is about
    // presets rather than about what an insert does to what it lands on.
    let after_first = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| clip.timeline_end_ticks)
        .max()
        .expect("the first clip is placed");
    state
        .drop_media_on_timeline(media, None, after_first)
        .expect("place second clip");

    let clips: Vec<ClipId> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|c| ClipId::from_raw(c.id))
        .collect();

    assert!(clips.len() >= 2, "must have at least two clips");
    let first = clips[0];
    let second = clips[1];
    state.select_clip(first);
    (state, first, second)
}

#[test]
fn effect_preset_saved_from_clip_applies_to_another_clip() {
    let (mut state, clip_a, clip_b) = with_two_clips();

    // Configure clip A with an effect stack.
    state.select_clip(clip_a);
    state
        .add_effect_to_selected("gaussian_blur")
        .expect("add blur");
    state
        .set_selected_effect_param(0, 0, 16.0)
        .expect("set blur radius");
    state
        .add_effect_to_selected("vignette")
        .expect("add vignette");

    // Save as named preset.
    state
        .save_selected_effect_preset("Dreamy Glow")
        .expect("save preset");

    // Verify preset list in memory and on disk.
    assert_eq!(state.effect_presets().len(), 1);
    let preset = state.effect_preset("Dreamy Glow").expect("preset found");
    assert_eq!(preset.name, "Dreamy Glow");
    assert_eq!(preset.effects.len(), 2);
    assert_eq!(preset.effects[0].effect_id, "gaussian_blur");
    assert_eq!(preset.effects[0].sample_param("radius", 0.0), Some(16.0));
    assert_eq!(preset.effects[1].effect_id, "vignette");

    // Select clip B (initially empty effects).
    state.select_clip(clip_b);
    let initial_b_effects = state
        .engine
        .lock()
        .project()
        .clip(clip_b)
        .map(|c| c.effects.to_vec())
        .unwrap_or_default();
    assert!(initial_b_effects.is_empty());

    // Apply the preset to clip B.
    state
        .apply_effect_preset("Dreamy Glow")
        .expect("apply preset");

    // Clip B now has the identical effect stack.
    let applied_effects = state
        .engine
        .lock()
        .project()
        .clip(clip_b)
        .map(|c| c.effects.to_vec())
        .expect("clip B exists");
    assert_eq!(applied_effects.len(), 2);
    assert_eq!(applied_effects[0].effect_id, "gaussian_blur");
    assert_eq!(applied_effects[0].sample_param("radius", 0.0), Some(16.0));
    assert_eq!(applied_effects[1].effect_id, "vignette");
}

#[test]
fn effect_preset_list_survives_reload_from_profile_root() {
    let (mut state, clip_a, _) = with_two_clips();
    let data_dir = state.data_dir.to_path_buf();

    // Save two presets.
    state.select_clip(clip_a);
    state
        .add_effect_to_selected("vignette")
        .expect("add vignette");
    state
        .save_selected_effect_preset("Vignette Preset")
        .expect("save 1");

    state
        .add_effect_to_selected("gaussian_blur")
        .expect("add blur");
    state
        .save_selected_effect_preset("Blur Plus Vignette")
        .expect("save 2");

    assert_eq!(state.effect_presets().len(), 2);

    // Drop and re-open state from the same data_dir.
    drop(state);
    let reloaded = AppState::new(PluginContext::new(), data_dir).expect("reloaded state");
    assert_eq!(reloaded.effect_presets().len(), 2);
    assert!(reloaded.effect_preset("Vignette Preset").is_some());
    let p2 = reloaded
        .effect_preset("Blur Plus Vignette")
        .expect("preset 2 found");
    assert_eq!(p2.effects.len(), 2);
}

#[test]
fn effect_preset_validation_refuses_empty_and_duplicate_names() {
    let (mut state, clip_a, _) = with_two_clips();
    state.select_clip(clip_a);
    state
        .add_effect_to_selected("gaussian_blur")
        .expect("add blur");

    // Empty and whitespace names refused.
    let err_empty = state.save_selected_effect_preset("").unwrap_err();
    assert!(err_empty.contains("cannot be empty"));

    let err_spaces = state.save_selected_effect_preset("   ").unwrap_err();
    assert!(err_spaces.contains("cannot be empty"));

    // Save initial valid preset.
    state
        .save_selected_effect_preset("Custom Look")
        .expect("save initial");

    // Duplicate name (exact or case-insensitive) refused.
    let err_exact = state
        .save_selected_effect_preset("Custom Look")
        .unwrap_err();
    assert!(err_exact.contains("duplicate"));

    let err_case = state
        .save_selected_effect_preset("custom look")
        .unwrap_err();
    assert!(err_case.contains("duplicate"));
}

/// A corrupt file is read the way the app reads one: on the way in, by the
/// loader the constructor calls. There is no reload button, so a test that
/// re-read through one would be asserting a path nothing ships.
#[test]
fn effect_preset_corrupt_file_yields_empty_list_and_no_panic() {
    let dir = scratch();
    let file = effect_presets_path(&dir);
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // Write completely invalid JSON.
    fs::write(&file, b"{{{ corrupt non-json garbage").expect("write corrupt file");
    let state = AppState::new(PluginContext::new(), dir.clone()).expect("state");
    assert!(state.effect_presets().is_empty());
    drop(state);

    // Write valid JSON with invalid effect id.
    let bad_preset = serde_json::json!({
        "version": 1,
        "presets": [
            {
                "name": "Bad",
                "effects": [
                    { "effect_id": "nonexistent_shader_effect", "params": {} }
                ]
            }
        ]
    });
    fs::write(&file, bad_preset.to_string()).expect("write bad schema");

    let state = AppState::new(PluginContext::new(), dir).expect("state");
    assert!(state.effect_presets().is_empty());
}

#[test]
fn effect_preset_applying_is_one_undo_entry() {
    let (mut state, clip_a, clip_b) = with_two_clips();

    // Prepare preset on clip A.
    state.select_clip(clip_a);
    state
        .add_effect_to_selected("gaussian_blur")
        .expect("add blur");
    state
        .set_selected_effect_param(0, 0, 8.0)
        .expect("set blur");
    state
        .save_selected_effect_preset("Single Step")
        .expect("save");

    // Clip B initially has 1 effect (vignette).
    state.select_clip(clip_b);
    state
        .add_effect_to_selected("vignette")
        .expect("add vignette to B");
    assert_eq!(
        state
            .engine
            .lock()
            .project()
            .clip(clip_b)
            .unwrap()
            .effects
            .len(),
        1
    );
    assert_eq!(
        state.engine.lock().project().clip(clip_b).unwrap().effects[0].effect_id,
        "vignette"
    );

    // Apply preset to clip B.
    state.apply_effect_preset("Single Step").expect("apply");
    assert_eq!(
        state
            .engine
            .lock()
            .project()
            .clip(clip_b)
            .unwrap()
            .effects
            .len(),
        1
    );
    assert_eq!(
        state.engine.lock().project().clip(clip_b).unwrap().effects[0].effect_id,
        "gaussian_blur"
    );

    // Exactly ONE undo restores clip B to vignette.
    assert!(state.undo(), "undo once");
    let restored = state
        .engine
        .lock()
        .project()
        .clip(clip_b)
        .unwrap()
        .effects
        .to_vec();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].effect_id, "vignette");

    // Redo brings back the preset.
    assert!(state.redo(), "redo");
    let redone = state
        .engine
        .lock()
        .project()
        .clip(clip_b)
        .unwrap()
        .effects
        .to_vec();
    assert_eq!(redone.len(), 1);
    assert_eq!(redone[0].effect_id, "gaussian_blur");
}

#[test]
fn effect_preset_delete_removes_and_persists() {
    let (mut state, clip_a, _) = with_two_clips();
    state.select_clip(clip_a);
    state
        .add_effect_to_selected("vignette")
        .expect("add vignette");
    state
        .save_selected_effect_preset("To Delete")
        .expect("save");
    assert_eq!(state.effect_presets().len(), 1);

    state.delete_effect_preset("To Delete").expect("delete");
    assert_eq!(state.effect_presets().len(), 0);

    // And the file the next launch reads holds none either.
    assert!(
        crate::app::effect_presets::load_effect_presets(&state.data_dir).is_empty(),
        "the delete must reach the file, not only the list"
    );
}
