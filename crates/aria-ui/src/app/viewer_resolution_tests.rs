//! The Viewer renders each frame at the resolution its settings choose: the
//! paused one while stopped, the playback one, or Auto's level, during a run.
//!
//! Defends against a reduced setting that never reaches the render, and a
//! setting that does not survive a restart. The proxy switch is held to the
//! same two contracts. Does not cover the Auto level
//! stepping on live vsyncs; `resolution_tests` covers the stepping itself.

use std::time::{Duration, Instant};

use gpui::AppContext;

use super::AppState;
use super::premiere_tests::with_clip;
use crate::chrome::menu_items::{items_of, row_caption};
use crate::resolution::{PausedResolution, PlaybackResolution, Resolution};
use crate::settings::UiSettings;
use crate::types::AppMenu;

#[test]
fn a_stopped_viewer_renders_at_the_paused_resolution_and_a_run_at_the_playback_one() {
    let (mut state, _) = with_clip();
    let canvas = state.canvas_size_tuple();
    assert_eq!(state.preview_fit(), None, "paused at Full by default");
    state.ui_settings.paused_resolution = PausedResolution::Half;
    state.ui_settings.playback_resolution = PlaybackResolution::Quarter;
    assert_eq!(state.preview_fit(), Resolution::Half.fit(canvas));
    state.arm_presenting();
    assert_eq!(state.preview_fit(), Resolution::Quarter.fit(canvas));
    state.ui_settings.playback_resolution = PlaybackResolution::Auto;
    assert_eq!(state.preview_fit(), None, "Auto starts at Full");
    state.stop_presenting();
    assert_eq!(state.preview_fit(), Resolution::Half.fit(canvas));
}

#[test]
fn auto_renders_at_its_level_and_a_fixed_choice_ignores_it() {
    let (mut state, _) = with_clip();
    let canvas = state.canvas_size_tuple();
    let now = Instant::now();
    state.auto_resolution.start(now);
    assert!(
        state
            .auto_resolution
            .dropped(3, now + Duration::from_millis(10))
    );
    assert_eq!(state.playback_fit(), Resolution::Half.fit(canvas));
    assert_eq!(state.playback_resolution_text(), "Auto 1/2");
    state.ui_settings.playback_resolution = PlaybackResolution::Full;
    assert_eq!(state.playback_fit(), None);
    assert_eq!(state.playback_resolution_text(), "Full");
    state.ui_settings.playback_resolution = PlaybackResolution::Eighth;
    assert_eq!(state.playback_fit(), Resolution::Eighth.fit(canvas));
    assert_eq!(state.playback_resolution_text(), "1/8");
}

/// Run one View menu row and return its caption and check mark afterwards.
/// Shared with the scope waveform contracts.
pub(super) fn run_view_row(
    cx: &gpui::TestAppContext,
    state: &gpui::Entity<AppState>,
    id: &str,
) -> (String, bool) {
    cx.update(|app| {
        state.update(app, |state: &mut AppState, cx| {
            let item = items_of(AppMenu::View, &*state)
                .into_iter()
                .find(|item| item.id == id)
                .unwrap_or_else(|| panic!("the View menu has no {id} row"));
            (item.run)(id, state, cx);
            (row_caption(&item, state), item.is_checked(state))
        })
    })
}

#[test]
fn the_view_menu_rows_step_through_every_choice_and_save_it() {
    let cx = gpui::TestAppContext::single();
    let (state, _) = with_clip();
    let data_dir = state.data_dir.clone();
    let state = cx.update(|app| app.new(|_| state));
    let mut captions = Vec::new();
    for _ in PlaybackResolution::ALL {
        captions.push(run_view_row(&cx, &state, "view-playback-resolution").0);
    }
    assert_eq!(
        captions,
        [
            "Playback Resolution: Full",
            "Playback Resolution: 1/2",
            "Playback Resolution: 1/4",
            "Playback Resolution: 1/8",
            "Playback Resolution: Auto",
        ]
    );
    run_view_row(&cx, &state, "view-playback-resolution");
    assert_eq!(
        run_view_row(&cx, &state, "view-paused-resolution").0,
        "Paused Resolution: 1/2"
    );
    cx.executor().advance_clock(Duration::from_millis(
        crate::settings::SAVE_DEBOUNCE_MS + 50,
    ));
    cx.run_until_parked();
    let saved = UiSettings::load(&data_dir);
    assert_eq!(saved.playback_resolution, PlaybackResolution::Full);
    assert_eq!(saved.paused_resolution, PausedResolution::Half);
    assert_eq!(
        run_view_row(&cx, &state, "view-paused-resolution").0,
        "Paused Resolution: Full"
    );
}

#[test]
fn the_proxy_switch_reaches_the_engine_and_a_restart_reads_it_back() {
    let cx = gpui::TestAppContext::single();
    let (state, _) = with_clip();
    let data_dir = state.data_dir.clone();
    let engine = std::sync::Arc::clone(state.engine());
    assert!(engine.lock().preview_proxies(), "proxies are on by default");
    let revision = engine.lock().proxy_revision();
    let state = cx.update(|app| app.new(|_| state));
    assert_eq!(
        run_view_row(&cx, &state, "view-proxies"),
        ("Proxies".to_string(), false)
    );
    assert!(!engine.lock().preview_proxies());
    assert_eq!(
        engine.lock().proxy_revision(),
        revision + 1,
        "frames rendered from the proxies are not invalidated"
    );
    cx.executor().advance_clock(Duration::from_millis(
        crate::settings::SAVE_DEBOUNCE_MS + 50,
    ));
    cx.run_until_parked();
    assert!(!UiSettings::load(&data_dir).use_proxies);
    let reopened = AppState::new(aria_plugin::PluginContext::new(), data_dir).expect("reopen");
    assert!(
        !reopened.engine().lock().preview_proxies(),
        "the saved switch never reached a new session's engine"
    );
    assert_eq!(
        run_view_row(&cx, &state, "view-proxies"),
        ("Proxies".to_string(), true)
    );
    assert!(engine.lock().preview_proxies());
}

/// The View menu's Transparency Grid row switches the engine's preview
/// transparency, invalidates frames rendered over the other background, saves
/// the choice, and a new session reads it back.
#[test]
fn the_transparency_grid_reaches_the_engine_and_a_restart_reads_it_back() {
    let cx = gpui::TestAppContext::single();
    let (state, _) = with_clip();
    let data_dir = state.data_dir.clone();
    let engine = std::sync::Arc::clone(state.engine());
    assert!(
        !engine.lock().preview_transparency(),
        "the grid is off by default"
    );
    let revision = engine.lock().proxy_revision();
    let state = cx.update(|app| app.new(|_| state));
    assert_eq!(
        run_view_row(&cx, &state, "view-transparency-grid"),
        ("Transparency Grid".to_string(), true)
    );
    assert!(engine.lock().preview_transparency());
    assert_eq!(
        engine.lock().proxy_revision(),
        revision + 1,
        "frames rendered over the background are not invalidated"
    );
    cx.executor().advance_clock(Duration::from_millis(
        crate::settings::SAVE_DEBOUNCE_MS + 50,
    ));
    cx.run_until_parked();
    assert!(UiSettings::load(&data_dir).transparency_grid);
    let reopened = AppState::new(aria_plugin::PluginContext::new(), data_dir).expect("reopen");
    assert!(
        reopened.engine().lock().preview_transparency(),
        "the saved switch never reached a new session's engine"
    );
    assert_eq!(
        run_view_row(&cx, &state, "view-transparency-grid"),
        ("Transparency Grid".to_string(), false)
    );
    assert!(!engine.lock().preview_transparency());
}
