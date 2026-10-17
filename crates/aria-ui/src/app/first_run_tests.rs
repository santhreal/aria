use super::{AppState, DialogKind, describe_engine_error};
use crate::types::UiRoute;
use aria_models::{ClipId, ClipParam};
use aria_plugin::PluginContext;
use std::path::PathBuf;

fn scratch() -> PathBuf {
    crate::scratch_pool::scratch("first-run")
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// The selected clip's level in decibels, read the way the fader row reads it:
/// off the projected clip, through the conversion `surfaces::param_units`
/// prints. `None` at silence, which has no decibel value.
fn level_db(state: &AppState) -> Option<f32> {
    state
        .selected_ui_clip()
        .and_then(|clip| aria_analysis::db_from_linear(clip.volume))
}

#[test]
fn new_project_starts_on_home_then_empty_timeline() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir.clone()).unwrap();
    assert_eq!(state.route, UiRoute::Home);
    assert_eq!(state.dialog, DialogKind::None);
    state.new_project().unwrap();
    assert_eq!(state.route, UiRoute::Editor);
    assert_eq!(state.dialog, DialogKind::None);
    assert!(
        state
            .store
            .sequence
            .tracks
            .iter()
            .all(|track| track.clips.is_empty())
    );
    let err = state.export_to(dir.join("empty.mp4")).unwrap_err();
    let msg = describe_engine_error(&err);
    assert!(msg.contains("Nothing to export"));
    assert!(msg.contains("empty"));
}

#[test]
fn import_fixture_then_place_clip() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    let media = state.import_media(fixture()).expect("import fixture");
    assert_eq!(state.store.media.len(), 1);
    assert!(!state.store.media[0].missing);
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place clip");
    let clips: usize = state
        .store
        .sequence
        .tracks
        .iter()
        .map(|track| track.clips.len())
        .sum();
    assert_eq!(clips, 1);
    state.save_project().unwrap();
    assert!(state.project_path.as_ref().unwrap().exists());
}

#[test]
fn fitting_the_sequence_sets_zoom_from_its_duration() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    let media = state.import_media(fixture()).expect("import fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place clip");
    state.set_zoom(4.0, false);
    state.scroll_timeline_to(120.0, false);
    state.last_window_w = 1440.0;
    state.zoom_fit_sequence();
    assert!(state.zoom < 4.0);
    assert!((state.store.zoom - state.zoom).abs() < f32::EPSILON);
    assert_eq!(state.scroll_x, 0.0);
    assert!(state.status_message.is_none());
}

#[test]
fn relink_repairs_missing_media() {
    let dir = scratch();
    let local = dir.join("still.png");
    std::fs::copy(fixture(), &local).unwrap();
    let mut state = AppState::new(PluginContext::new(), dir.clone()).unwrap();
    state.new_project().unwrap();
    let media = state.import_media(local.clone()).expect("import still");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place clip");
    state.save_project().unwrap();
    let project = state.project_path.clone().unwrap();
    std::fs::remove_file(&local).unwrap();
    let mut reopened = AppState::new(PluginContext::new(), dir).unwrap();
    reopened
        .open_project(project)
        .expect("open with missing media");
    assert!(reopened.store.media[0].missing);
    assert!(
        reopened
            .last_error
            .as_ref()
            .is_some_and(|msg| msg.contains("Relink"))
    );
    std::fs::copy(fixture(), &local).unwrap();
    reopened
        .relink_media(media, local)
        .expect("relink after restore");
    assert!(!reopened.store.media[0].missing);
}

#[test]
fn corrupt_project_file_is_human() {
    let dir = scratch();
    let path = dir.join("bad.aria");
    std::fs::write(&path, "{not json").unwrap();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    let err = state.open_project(path).unwrap_err();
    let msg = describe_engine_error(&err);
    assert!(msg.contains("corrupted"));
    assert!(msg.contains(".aria"));
}

#[test]
fn dropped_files_land_end_to_end() {
    let dir = scratch();
    let a = dir.join("a.png");
    let b = dir.join("b.png");
    std::fs::copy(fixture(), &a).unwrap();
    std::fs::copy(fixture(), &b).unwrap();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    state
        .import_dropped_paths(vec![a, b])
        .expect("drop two stills");
    let clips: Vec<(i64, i64)> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].0, 0);
    assert_eq!(clips[1].0, clips[0].1);
}

#[test]
fn playhead_follows_elapsed_wall_time() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    let media = state.import_media(fixture()).expect("import still");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place still");
    state.playing = true;
    state.playback_anchor_tick = 0;
    state.playback_started =
        Some(std::time::Instant::now() - std::time::Duration::from_millis(125));
    assert!(state.advance_playhead());
    assert!(state.playhead.value >= 2);
}

#[test]
fn title_places_on_a_text_lane() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    state
        .add_generated(aria_models::Generator::text("Title"))
        .expect("place title");
    let titles: usize = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.kind == crate::types::UiTrackKind::Text)
        .map(|track| track.clips.len())
        .sum();
    assert_eq!(titles, 1);
}

#[test]
fn inspector_audio_edits_selected_clip() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    let media = state.import_media(fixture()).expect("import fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place clip");
    let id = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("placed clip")
        .id;
    state.select_clip(ClipId::from_raw(id));
    state.set_selected_volume(0.5).expect("volume");
    let clip = state.selected_ui_clip().expect("selected");
    assert!((clip.volume - 0.5).abs() < 1e-6);
    state
        .set_param_scalar(ClipParam::Pan, None, -0.25)
        .expect("pan");
    let clip = state.selected_ui_clip().expect("selected");
    assert!((clip.pan + 0.25).abs() < 1e-6);
    state.set_selected_fade_in(12).expect("fade in");
    state.set_selected_fade_out(8).expect("fade out");
    let clip = state.selected_ui_clip().expect("selected");
    assert_eq!(clip.fade_in_ticks, 12);
    assert_eq!(clip.fade_out_ticks, 8);
    state.toggle_selected_reverse().expect("reverse");
    let clip = state.selected_ui_clip().expect("selected");
    assert!(clip.reversed);
    state.toggle_selected_reverse().expect("unreverse");
    let clip = state.selected_ui_clip().expect("selected");
    assert!(!clip.reversed);
}

/// The volume fader moves in decibels, so a step is the same gesture at unity
/// and at a whisper, and the bottom of the travel is silence rather than an
/// endless approach to zero. A linear step cannot express either: 0.05 is
/// inaudible at the top of the range and mutes anything under it.
#[test]
fn the_volume_fader_steps_in_decibels_and_bottoms_out_at_silence() {
    let floor = super::clip_audio::VOLUME_FLOOR_DB;
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    let media = state.import_media(fixture()).expect("import fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place clip");
    let id = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("placed clip")
        .id;
    state.select_clip(ClipId::from_raw(id));

    // A step down from unity is a decibel, not a fifth of the multiplier.
    state.set_selected_volume(1.0).expect("unity");
    state.nudge_selected_volume_db(-1.0).expect("step down");
    let quieter = state.selected_ui_clip().expect("selected").volume;
    assert!((quieter - aria_analysis::linear_from_db(-1.0)).abs() < 1e-5);
    assert!((level_db(&state).expect("a level") + 1.0).abs() < 1e-3);

    // The same step back up returns the level it came from.
    state.nudge_selected_volume_db(1.0).expect("step up");
    assert!((state.selected_ui_clip().expect("selected").volume - 1.0).abs() < 1e-5);

    // Under the floor there is nothing to hear, so the fader mutes.
    state
        .set_selected_volume(aria_analysis::linear_from_db(floor))
        .expect("floor");
    state.nudge_selected_volume_db(-1.0).expect("step under");
    assert_eq!(state.selected_ui_clip().expect("selected").volume, 0.0);
    assert_eq!(level_db(&state), None);

    // Coming off silence lands on the floor, not seventy steps below unity.
    state
        .nudge_selected_volume_db(1.0)
        .expect("step off silence");
    let recovered = level_db(&state).expect("a level again");
    assert!((recovered - floor).abs() < 1e-3);

    // With nothing selected the gesture is a no-op, not an error.
    state.selection.clear();
    state.nudge_selected_volume_db(-1.0).expect("no selection");
    assert_eq!(level_db(&state), None);
}

#[test]
fn close_project_returns_home() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    assert_eq!(state.route, UiRoute::Editor);
    state.close_project();
    assert_eq!(state.route, UiRoute::Home);
    assert_eq!(state.dialog, DialogKind::None);
}

#[test]
fn drop_aria_on_home_opens_project() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir.clone()).unwrap();
    state.new_project().unwrap();
    let path = dir.join("from-home.aria");
    state.project_path = Some(path.clone());
    state.save_project().unwrap();
    state.go_home();
    assert_eq!(state.route, UiRoute::Home);
    state.drop_on_home(vec![path]).unwrap();
    assert_eq!(state.route, UiRoute::Editor);
}

#[test]
fn drop_media_on_home_starts_project() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    assert_eq!(state.route, UiRoute::Home);
    state.drop_on_home(vec![fixture()]).unwrap();
    assert_eq!(state.route, UiRoute::Editor);
    assert_eq!(state.store.media.len(), 1);
}

#[test]
fn home_rail_persists_in_settings() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir.clone()).unwrap();
    state.set_home_rail(crate::types::HomeRail::Samples);
    state.ui_settings.save(&dir).unwrap();
    let loaded = crate::settings::UiSettings::load(&dir);
    assert_eq!(loaded.home_rail, crate::types::HomeRail::Samples);
}

/// How the pool draws itself is a habit, not a fact about the project, so it has
/// to come back on the next launch. Asserted through the verb and the loading
/// constructor rather than the field, because that is the path the chip takes.
#[test]
fn the_pool_view_survives_a_relaunch() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir.clone()).unwrap();
    assert_eq!(state.pool_view(), crate::types::PoolView::Tiles);
    state.set_pool_view(crate::types::PoolView::Icons);
    assert_eq!(state.pool_view(), crate::types::PoolView::Icons);
    state.ui_settings.save(&dir).unwrap();
    let again = AppState::new(PluginContext::new(), dir.clone()).unwrap();
    assert_eq!(again.pool_view(), crate::types::PoolView::Icons);

    // Freeform view selection also persists across relaunches.
    let mut state = again;
    state.set_pool_view(crate::types::PoolView::Freeform);
    assert_eq!(state.pool_view(), crate::types::PoolView::Freeform);
    state.ui_settings.save(&dir).unwrap();
    let freeform_again = AppState::new(PluginContext::new(), dir).unwrap();
    assert_eq!(freeform_again.pool_view(), crate::types::PoolView::Freeform);
}

#[test]
fn editor_defaults_to_pointer_ripple_snap() {
    let dir = scratch();
    let state = AppState::new(PluginContext::new(), dir).unwrap();
    assert_eq!(state.tool, crate::types::EditorTool::Pointer);
    assert_eq!(state.edit_mode, crate::types::EditMode::Ripple);
    assert!(state.snap_enabled);
    assert!(state.open_menu.is_none());
}

#[test]
fn toggle_menu_opens_and_closes() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.toggle_menu(crate::types::AppMenu::File);
    assert_eq!(state.open_menu, Some(crate::types::AppMenu::File));
    state.toggle_menu(crate::types::AppMenu::File);
    assert!(state.open_menu.is_none());
    state.toggle_menu(crate::types::AppMenu::Edit);
    state.go_home();
    assert!(state.open_menu.is_none());
}

#[test]
fn every_app_menu_has_named_items() {
    let dir = scratch();
    let state = AppState::new(PluginContext::new(), dir).unwrap();
    for menu in crate::types::AppMenu::ALL {
        let labels = crate::chrome::labels_for(menu, &state);
        assert!(!labels.is_empty(), "{menu:?} shipped with no items");
        for label in &labels {
            assert!(!label.is_empty(), "{menu:?} has a blank item");
        }
    }
    let file = crate::chrome::labels_for(crate::types::AppMenu::File, &state);
    assert!(file.contains(&"Save As…"));
    assert!(file.contains(&"Relink All…"));
    let sequence = crate::chrome::labels_for(crate::types::AppMenu::Sequence, &state);
    assert!(sequence.contains(&"Match Frame"));
    assert!(sequence.contains(&"Insert"));
    assert!(sequence.contains(&"Overwrite"));
    let markers = crate::chrome::labels_for(crate::types::AppMenu::Markers, &state);
    assert!(markers.contains(&"Add Marker"));
    let edit = crate::chrome::labels_for(crate::types::AppMenu::Edit, &state);
    assert!(edit.contains(&"Cut"));
    assert!(edit.contains(&"Select All"));
}

#[test]
fn reset_split_restores_every_pane() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.ui_settings.sidebar_width = 300.0;
    state.ui_settings.inspector_width = 400.0;
    state.ui_settings.timeline_height = 200.0;
    for pane in ["sidebar", "inspector", "timeline"] {
        state.reset_split(pane);
    }
    assert_eq!(
        state.ui_settings.sidebar_width,
        crate::settings::SIDEBAR_DEFAULT
    );
    assert_eq!(
        state.ui_settings.inspector_width,
        crate::settings::INSPECTOR_DEFAULT
    );
    assert_eq!(
        state.ui_settings.timeline_height,
        crate::settings::TIMELINE_DEFAULT_HEIGHT
    );
}

#[test]
fn shuttle_jkl_steps_and_stops() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    state.set_shuttle(1);
    assert!(state.playing);
    assert_eq!(state.shuttle, 1);
    state.set_shuttle(1);
    assert_eq!(state.shuttle, 2);
    state.set_shuttle(-1);
    assert_eq!(state.shuttle, -1);
    state.set_shuttle(0);
    assert!(!state.playing);
    assert_eq!(state.shuttle, 1);
}

#[test]
fn shuttle_from_stop_keeps_requested_speed() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    assert!(!state.playing);
    state.set_shuttle(-1);
    assert!(state.playing);
    assert_eq!(state.shuttle, -1);
}

#[test]
fn palette_query_filters_relink_and_clears_on_close() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();
    state.open_dialog(DialogKind::Palette);
    state.palette_query = "relink".into();
    let labels: Vec<&str> = crate::commands::BUILTIN_COMMANDS
        .iter()
        .map(|cmd| cmd.label)
        .collect();
    let hits = crate::popover::filter_indices(&state.palette_query, &labels);
    assert!(
        hits.iter().any(|i| labels[*i] == "Relink All"),
        "typed Relink must hit Relink All, got {hits:?}"
    );
    state.open_dialog(DialogKind::None);
    assert!(state.palette_query.is_empty());
}

#[test]
fn context_menu_stores_click_position() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.open_context(crate::types::ContextTarget::Preview, 412.0, 188.0);
    assert_eq!(state.context_at, (412.0, 188.0));
    assert_eq!(
        state.context_menu,
        Some(crate::types::ContextTarget::Preview)
    );
}

#[test]
fn context_menu_clamps_to_window() {
    let dir = scratch();
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.last_window_w = 400.0;
    state.last_window_h = 300.0;
    state.open_context(crate::types::ContextTarget::Preview, 390.0, 290.0);
    assert_eq!(state.context_at, (220.0, 60.0));
}
