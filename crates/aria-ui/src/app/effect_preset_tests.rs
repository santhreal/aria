//! Contracts for animation presets (`.ariapreset`).
//!
//! Closes the defect class: "a clip's effect chain, audio inserts, and keyframed
//! properties cannot be saved, named, filtered by part, listed alongside plugin
//! contributions, imported by file drop, or applied across clips anchored at the
//! playhead in a single undo step".

use std::fs;
use std::path::PathBuf;

use aria_commands::{Command, EditCommand};
use aria_models::{
    AudioInsertTarget, ClipId, ClipParam, Easing, EffectInstance, ParamValue, Preset, RationalTime,
};
use aria_plugin::{Plugin, PluginContext, PluginError, PluginManifest};

use super::AppState;
use crate::app::presets::{legacy_effect_presets_path, load_presets, presets_dir};
use crate::surfaces::effects_rack::{self, Gesture};

fn scratch() -> PathBuf {
    crate::scratch_pool::scratch("animation_preset")
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4")
}

/// Create a test state with two visual clips placed on the timeline.
fn with_two_clips() -> (AppState, ClipId, ClipId) {
    let mut state = AppState::new(PluginContext::new(), scratch()).expect("state");
    state.new_project().expect("new project");
    let media = state.import_media(fixture()).expect("import fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place first clip");
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
fn preset_saved_from_clip_applies_effects_audio_and_keyframes_to_another_clip() {
    let (mut state, clip_a, clip_b) = with_two_clips();
    let rate = state.playhead.rate;

    // Configure clip A with visual effects, an audio insert, and keyframed opacity.
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
    state
        .apply_command(Command::Edit(EditCommand::AddAudioEffect {
            target: AudioInsertTarget::Clip(clip_a),
            effect_id: "gain".to_string(),
        }))
        .expect("add audio insert");
    state
        .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_a,
            param: ClipParam::Opacity,
            at: RationalTime::new(0, rate),
            value: ParamValue::Scalar(0.25),
            easing: Easing::Linear,
            tangents: None,
        }))
        .expect("keyframe 0");
    state
        .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_a,
            param: ClipParam::Opacity,
            at: RationalTime::new(12, rate),
            value: ParamValue::Scalar(0.85),
            easing: Easing::EaseOut,
            tangents: None,
        }))
        .expect("keyframe 12");

    state.set_playhead(0);
    state
        .save_selected_preset("Dreamy Glow")
        .expect("save preset");

    assert_eq!(state.presets().len(), 1);
    let preset = state.preset("Dreamy Glow").expect("preset found");
    assert_eq!(preset.name, "Dreamy Glow");
    assert_eq!(preset.effects.len(), 2);
    assert_eq!(preset.effects[0].effect_id, "gaussian_blur");
    assert_eq!(preset.effects[0].sample_param("radius", 0.0), Some(16.0));
    assert_eq!(preset.effects[1].effect_id, "vignette");
    assert_eq!(preset.audio_effects.len(), 1);
    assert_eq!(preset.audio_effects[0].effect_id, "gain");
    assert_eq!(preset.properties.len(), 1);
    assert_eq!(preset.properties[0].id, "opacity");

    // Select clip B and move playhead outside clip B so anchor defaults to clip B's start.
    state.select_clip(clip_b);
    state.set_playhead(0);
    state.apply_preset("Dreamy Glow").expect("apply preset");

    let clip_b_model = state
        .engine
        .lock()
        .project()
        .clip(clip_b)
        .cloned()
        .expect("clip B exists");
    assert_eq!(clip_b_model.effects.len(), 2);
    assert_eq!(clip_b_model.effects[0].effect_id, "gaussian_blur");
    assert_eq!(
        clip_b_model.effects[0].sample_param("radius", 0.0),
        Some(16.0)
    );
    assert_eq!(clip_b_model.effects[1].effect_id, "vignette");
    assert_eq!(clip_b_model.audio_effects.len(), 1);
    assert_eq!(clip_b_model.audio_effects[0].effect_id, "gain");
    let kfs = clip_b_model.transform.opacity.keyframes();
    assert_eq!(kfs.len(), 2);
    assert_eq!(kfs[0].tick, 0);
    assert_eq!(kfs[1].tick, 12);
}

#[test]
fn preset_list_survives_reload_from_profile_root() {
    let (mut state, clip_a, _) = with_two_clips();
    let data_dir = state.data_dir.to_path_buf();

    state.select_clip(clip_a);
    state
        .add_effect_to_selected("vignette")
        .expect("add vignette");
    state
        .save_selected_preset("Vignette Preset")
        .expect("save 1");

    state
        .add_effect_to_selected("gaussian_blur")
        .expect("add blur");
    state
        .save_selected_preset("Blur Plus Vignette")
        .expect("save 2");

    assert_eq!(state.presets().len(), 2);

    drop(state);
    let reloaded = AppState::new(PluginContext::new(), data_dir).expect("reloaded state");
    assert_eq!(reloaded.presets().len(), 2);
    assert!(reloaded.preset("Vignette Preset").is_some());
    let p2 = reloaded
        .preset("Blur Plus Vignette")
        .expect("preset 2 found");
    assert_eq!(p2.effects.len(), 2);
}

#[test]
fn legacy_effects_json_migrated_to_ariapreset_files_and_removed() {
    let dir = scratch();
    let legacy_path = legacy_effect_presets_path(&dir);
    fs::create_dir_all(legacy_path.parent().unwrap()).expect("create presets dir");
    let legacy_json = serde_json::json!({
        "version": 1,
        "presets": [
            {
                "name": "Legacy Warm",
                "effects": [
                    { "effect_id": "vignette" }
                ]
            },
            {
                "name": "Legacy Blur",
                "effects": [
                    { "effect_id": "gaussian_blur" }
                ]
            }
        ]
    });
    fs::write(&legacy_path, legacy_json.to_string()).expect("write legacy effects.json");

    let state = AppState::new(PluginContext::new(), dir.clone()).expect("state");
    assert!(
        !legacy_path.exists(),
        "effects.json must be removed after migrating to .ariapreset files"
    );
    assert_eq!(state.presets().len(), 2);
    assert!(state.preset("Legacy Blur").is_some());
    assert!(state.preset("Legacy Warm").is_some());
    assert!(
        presets_dir(&dir).join("legacy-blur.ariapreset").is_file(),
        "migrated .ariapreset file must exist on disk"
    );
    assert!(
        presets_dir(&dir).join("legacy-warm.ariapreset").is_file(),
        "migrated .ariapreset file must exist on disk"
    );
}

#[test]
fn preset_validation_refuses_empty_duplicate_and_empty_part_selection() {
    let (mut state, clip_a, _) = with_two_clips();
    state.select_clip(clip_a);
    state
        .add_effect_to_selected("gaussian_blur")
        .expect("add blur");

    let err_empty = state.save_selected_preset("").unwrap_err();
    assert!(err_empty.contains("cannot be empty"));

    let err_spaces = state.save_selected_preset("   ").unwrap_err();
    assert!(err_spaces.contains("cannot be empty"));

    state
        .save_selected_preset("Custom Look")
        .expect("save initial");

    let err_exact = state.save_selected_preset("Custom Look").unwrap_err();
    assert!(err_exact.contains("duplicate"));

    let err_case = state.save_selected_preset("custom look").unwrap_err();
    assert!(err_case.contains("duplicate"));

    // Open draft, type a fresh name, toggle off the only effect, and confirm save is refused
    // while keeping the draft open.
    state.open_preset_draft();
    for ch in "Empty Parts".chars() {
        state.preset_draft_push(ch);
    }
    effects_rack::apply(&mut state, Gesture::TogglePresetEffect(0)).expect("toggle effect off");
    let refused = effects_rack::apply(&mut state, Gesture::SavePreset)
        .expect_err("saving with all parts unchecked must be refused");
    assert!(refused.to_string().contains("empty"));
    assert!(
        state.preset_draft_open(),
        "draft stays open when refused so a part can be re-enabled"
    );
    assert_eq!(state.preset_draft_text(), "Empty Parts");
}

#[test]
fn corrupt_ariapreset_file_is_surfaced_in_broken_presets() {
    let dir = scratch();
    let p_dir = presets_dir(&dir);
    fs::create_dir_all(&p_dir).expect("create presets dir");

    // Write one valid .ariapreset and two broken .ariapreset files.
    let good = Preset::new(
        "Good Vignette",
        vec![EffectInstance::new("vignette")],
        Vec::new(),
        Vec::new(),
    );
    fs::write(
        p_dir.join("good-vignette.ariapreset"),
        good.to_json().unwrap(),
    )
    .expect("write good preset");
    fs::write(
        p_dir.join("corrupt.ariapreset"),
        b"{{{ corrupt non-json garbage",
    )
    .expect("write corrupt file");
    let bad_effect = serde_json::json!({
        "version": 1,
        "name": "Bad",
        "effects": [
            { "effect_id": "nonexistent_shader_effect" }
        ]
    });
    fs::write(p_dir.join("bad-effect.ariapreset"), bad_effect.to_string())
        .expect("write bad schema");

    let state = AppState::new(PluginContext::new(), dir).expect("state");
    assert_eq!(state.presets().len(), 1);
    assert_eq!(state.presets()[0].name, "Good Vignette");
    assert_eq!(
        state.broken_presets().len(),
        2,
        "broken .ariapreset files must be surfaced in broken_presets, never dropped in silence"
    );
    assert!(
        state
            .broken_presets()
            .iter()
            .any(|b| b.path.ends_with("bad-effect.ariapreset")
                && b.reason.contains("nonexistent_shader_effect"))
    );
    assert!(
        state
            .broken_presets()
            .iter()
            .any(|b| b.path.ends_with("corrupt.ariapreset"))
    );
}

#[test]
fn preset_draft_part_toggles_exclude_unchecked_parts() {
    let (mut state, clip_a, _) = with_two_clips();
    let rate = state.playhead.rate;
    state.select_clip(clip_a);
    state
        .add_effect_to_selected("gaussian_blur")
        .expect("add blur");
    state
        .add_effect_to_selected("vignette")
        .expect("add vignette");
    state
        .apply_command(Command::Edit(EditCommand::AddAudioEffect {
            target: AudioInsertTarget::Clip(clip_a),
            effect_id: "gain".to_string(),
        }))
        .expect("add gain");
    state
        .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_a,
            param: ClipParam::Opacity,
            at: RationalTime::new(0, rate),
            value: ParamValue::Scalar(0.5),
            easing: Easing::Linear,
            tangents: None,
        }))
        .expect("kf opacity");
    state
        .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_a,
            param: ClipParam::Rotation,
            at: RationalTime::new(6, rate),
            value: ParamValue::Scalar(15.0),
            easing: Easing::Linear,
            tangents: None,
        }))
        .expect("kf rotation");

    state.open_preset_draft();
    for ch in "Selective".chars() {
        state.preset_draft_push(ch);
    }
    // Toggle off first visual effect (gaussian_blur), audio inserts, and opacity.
    effects_rack::apply(&mut state, Gesture::TogglePresetEffect(0)).expect("toggle fx 0");
    effects_rack::apply(&mut state, Gesture::TogglePresetAudioEffects).expect("toggle audio");
    effects_rack::apply(
        &mut state,
        Gesture::TogglePresetProperty("opacity".to_string()),
    )
    .expect("toggle opacity");
    effects_rack::apply(&mut state, Gesture::SavePreset).expect("save selective preset");

    let saved = state.preset("Selective").expect("Selective preset exists");
    assert_eq!(saved.effects.len(), 1);
    assert_eq!(saved.effects[0].effect_id, "vignette");
    assert!(saved.audio_effects.is_empty());
    assert_eq!(saved.properties.len(), 1);
    assert_eq!(saved.properties[0].id, "rotation");
}

#[test]
fn apply_preset_anchors_at_playhead_inside_clip_and_clip_start_outside() {
    let (mut state, clip_a, clip_b) = with_two_clips();
    let rate = state.playhead.rate;

    // On clip A (starts at 0), place keyframes at tick 4 and tick 14, then save with playhead at tick 4.
    state.select_clip(clip_a);
    state
        .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_a,
            param: ClipParam::Opacity,
            at: RationalTime::new(4, rate),
            value: ParamValue::Scalar(0.2),
            easing: Easing::Linear,
            tangents: None,
        }))
        .expect("kf 4");
    state
        .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_a,
            param: ClipParam::Opacity,
            at: RationalTime::new(14, rate),
            value: ParamValue::Scalar(1.0),
            easing: Easing::EaseInOut,
            tangents: None,
        }))
        .expect("kf 14");
    state.set_playhead(4);
    state
        .save_selected_preset("Anchored Fade")
        .expect("save anchored fade");

    let clip_b_start = state
        .engine
        .lock()
        .project()
        .clip(clip_b)
        .unwrap()
        .timeline
        .start
        .value;

    // 1. Playhead inside clip B at clip_b_start + 5 -> keyframes land at local ticks 5 and 15.
    state.select_clip(clip_b);
    state.set_playhead(clip_b_start + 5);
    state
        .apply_preset("Anchored Fade")
        .expect("apply at playhead inside clip B");
    {
        let clip = state.engine.lock().project().clip(clip_b).unwrap().clone();
        let kfs = clip.transform.opacity.keyframes();
        assert_eq!(kfs.len(), 2);
        assert_eq!(kfs[0].tick, 5);
        assert_eq!(kfs[1].tick, 15);
    }

    // Undo and move playhead outside clip B (tick 0) -> keyframes land at local ticks 0 and 10.
    assert!(state.undo());
    state.set_playhead(0);
    state
        .apply_preset("Anchored Fade")
        .expect("apply with playhead outside clip B");
    {
        let clip = state.engine.lock().project().clip(clip_b).unwrap().clone();
        let kfs = clip.transform.opacity.keyframes();
        assert_eq!(kfs.len(), 2);
        assert_eq!(kfs[0].tick, 0);
        assert_eq!(kfs[1].tick, 10);
    }
}

#[test]
fn apply_preset_across_multiple_selected_clips_is_one_undo_entry() {
    let (mut state, clip_a, clip_b) = with_two_clips();

    state.select_clip(clip_a);
    state
        .add_effect_to_selected("gaussian_blur")
        .expect("add blur");
    state
        .set_selected_effect_param(0, 0, 8.0)
        .expect("set blur");
    state.save_selected_preset("Single Step").expect("save");

    // Reset clip A to vignette, and set clip B to vignette.
    assert!(state.undo());
    assert!(state.undo());
    state
        .add_effect_to_selected("vignette")
        .expect("add vignette to A");
    state.select_clip(clip_b);
    state
        .add_effect_to_selected("vignette")
        .expect("add vignette to B");

    // Select both clips and apply the preset.
    state.select_clip(clip_a);
    state.add_clip_to_selection(clip_b);
    state
        .apply_preset("Single Step")
        .expect("apply across selection");

    for clip_id in [clip_a, clip_b] {
        let fx = state
            .engine
            .lock()
            .project()
            .clip(clip_id)
            .unwrap()
            .effects
            .to_vec();
        assert_eq!(fx.len(), 2);
        assert_eq!(fx[0].effect_id, "vignette");
        assert_eq!(fx[1].effect_id, "gaussian_blur");
    }

    // One undo restores both clips to vignette.
    assert!(state.undo(), "undo once");
    for clip_id in [clip_a, clip_b] {
        let fx = state
            .engine
            .lock()
            .project()
            .clip(clip_id)
            .unwrap()
            .effects
            .to_vec();
        assert_eq!(fx.len(), 1);
        assert_eq!(fx[0].effect_id, "vignette");
    }

    // Redo brings back the preset on both clips.
    assert!(state.redo(), "redo");
    for clip_id in [clip_a, clip_b] {
        let fx = state
            .engine
            .lock()
            .project()
            .clip(clip_id)
            .unwrap()
            .effects
            .to_vec();
        assert_eq!(fx.len(), 2);
        assert_eq!(fx[0].effect_id, "vignette");
        assert_eq!(fx[1].effect_id, "gaussian_blur");
    }
}

#[test]
fn plugin_contributed_presets_are_listed_and_applicable() {
    static MANIFEST: PluginManifest =
        PluginManifest::new("test.preset-pack", "Preset Pack", "0.1.0", &[], &[]);
    struct PresetPlugin;
    impl Plugin for PresetPlugin {
        fn manifest(&self) -> &PluginManifest {
            &MANIFEST
        }
        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            ctx.register_preset(Preset::new(
                "Studio Vignette",
                vec![EffectInstance::new("vignette")],
                Vec::new(),
                Vec::new(),
            ))?;
            Ok(())
        }
    }

    let ctx = PluginContext::new();
    ctx.mount(&PresetPlugin).expect("mount plugin");
    let mut state = AppState::new(ctx, scratch()).expect("state");
    state.new_project().expect("new project");
    let media = state.import_media(fixture()).expect("import fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place clip");
    let clip_id = ClipId::from_raw(state.store.sequence.tracks[0].clips[0].id);
    state.select_clip(clip_id);

    let plugin_presets = state.plugin_presets();
    assert_eq!(plugin_presets.len(), 1);
    assert_eq!(&*plugin_presets[0].plugin, "Preset Pack");
    assert_eq!(plugin_presets[0].preset.name, "Studio Vignette");

    state
        .apply_preset("Studio Vignette")
        .expect("apply plugin preset");
    let fx = state
        .engine
        .lock()
        .project()
        .clip(clip_id)
        .unwrap()
        .effects
        .to_vec();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx[0].effect_id, "vignette");
}

#[test]
fn import_and_drop_ariapreset_installs_and_applies_to_selection() {
    let (mut state, clip_a, _) = with_two_clips();
    state.select_clip(clip_a);

    let external_dir = scratch();
    let dropped_path = external_dir.join("dropped-look.ariapreset");
    let preset = Preset::new(
        "Dropped Look",
        vec![EffectInstance::new("vignette")],
        Vec::new(),
        Vec::new(),
    );
    fs::write(&dropped_path, preset.to_json().unwrap()).expect("write external .ariapreset");

    state
        .handle_dropped_paths(vec![dropped_path.clone()])
        .expect("drop .ariapreset");

    assert!(state.preset("Dropped Look").is_some());
    let fx = state
        .engine
        .lock()
        .project()
        .clip(clip_a)
        .unwrap()
        .effects
        .to_vec();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx[0].effect_id, "vignette");

    // Dropping the same preset a second time is refused because the name is already taken.
    let err = state.import_preset_file(&dropped_path).unwrap_err();
    assert!(err.contains("already exists"));
}

#[test]
fn preset_delete_removes_ariapreset_file_and_persists() {
    let (mut state, clip_a, _) = with_two_clips();
    state.select_clip(clip_a);
    state
        .add_effect_to_selected("vignette")
        .expect("add vignette");
    state.save_selected_preset("To Delete").expect("save");
    assert_eq!(state.presets().len(), 1);

    state.delete_preset("To Delete").expect("delete");
    assert_eq!(state.presets().len(), 0);
    assert!(
        load_presets(&state.data_dir).0.is_empty(),
        "the delete must reach the file, not only the list"
    );
}
