//! The Timeline Display Settings, driven through the View menu rows and the
//! transport follow.
//!
//! # Why
//!
//! A display choice that the menu offers but the draw or the follow never
//! reads is a row that does nothing; one that is not saved resets on every
//! launch. The rows are read from the View menu at run time and every enum
//! variant is mapped to its row by an exhaustive match, so a new choice
//! without a row fails to compile and a new row without a choice fails here.
//! The follow cases pin the smooth centring and the hold a hand scroll puts
//! on the follow.
//!
//! What they do not catch: whether the strip and the waveform draw the chosen
//! cells and heights. `timeline_display` pins those mappings, and the media
//! module reads them from the same setting.

use std::collections::BTreeMap;
use std::time::Duration;

use gpui::AppContext;

use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::menu_keys::MenuNav;
use crate::app::premiere_tests::with_clip;
use crate::chrome::menu_items::items_of;
use crate::chrome::menu_view_rows::TIMELINE_DISPLAY;
use crate::plugins::UiPlugin;
use crate::settings::UiSettings;
use crate::timeline_display::{Follow, Thumbnails, TimelineDisplay, WaveformScale};
use crate::types::AppMenu;

const DISPLAY: &str = "Timeline Display Settings";

/// The row that selects each choice. Exhaustive, so a new variant needs a row.
fn thumbnails_row(choice: Thumbnails) -> &'static str {
    match choice {
        Thumbnails::Head => "view-thumbs-head",
        Thumbnails::HeadAndTail => "view-thumbs-head-tail",
        Thumbnails::Continuous => "view-thumbs-continuous",
    }
}

fn waveform_row(choice: WaveformScale) -> &'static str {
    match choice {
        WaveformScale::Logarithmic => "view-waveform-log",
        WaveformScale::Linear => "view-waveform-linear",
        WaveformScale::Hidden => "view-waveform-hidden",
    }
}

fn follow_row(choice: Follow) -> &'static str {
    match choice {
        Follow::Page => "view-follow-page",
        Follow::Smooth => "view-follow-smooth",
    }
}

/// The row ids the current setting checks, one per group.
fn rows_of(display: TimelineDisplay) -> [&'static str; 3] {
    [
        thumbnails_row(display.thumbnails),
        waveform_row(display.waveform),
        follow_row(display.follow),
    ]
}

/// The group a row belongs to: its second id segment.
fn group(id: &str) -> &str {
    id.split('-').nth(1).expect("a view-<group>-<choice> id")
}

#[test]
fn every_display_row_checks_its_choice_alone_and_a_restart_reads_it_back() {
    let cx = gpui::TestAppContext::single();
    let (state, _) = with_clip();
    let data_dir = state.data_dir.clone();
    let state = cx.update(|app| app.new(|_| state));
    let ids: Vec<&'static str> = cx.update(|app| {
        items_of(AppMenu::View, state.read(app))
            .iter()
            .filter(|item| item.under == Some(DISPLAY))
            .map(|item| item.id)
            .collect()
    });
    let groups: BTreeMap<&str, usize> = ids.iter().fold(BTreeMap::new(), |mut groups, id| {
        *groups.entry(group(id)).or_default() += 1;
        groups
    });
    assert_eq!(
        groups,
        BTreeMap::from([("follow", 2), ("thumbs", 3), ("waveform", 3)]),
        "the submenu rows are {ids:?}"
    );

    for id in &ids {
        let (display, checked) = cx.update(|app| {
            state.update(app, |state: &mut AppState, cx| {
                let items = items_of(AppMenu::View, &*state);
                let item = items.iter().find(|item| item.id == *id).expect("the row");
                (item.run)(id, state, cx);
                let checked: Vec<&'static str> = items
                    .iter()
                    .filter(|item| item.under == Some(DISPLAY) && item.is_checked(state))
                    .map(|item| item.id)
                    .collect();
                (state.ui_settings.timeline_display, checked)
            })
        });
        assert!(
            rows_of(display).contains(id),
            "{id} did not select its choice: {display:?}"
        );
        assert_eq!(
            checked,
            ids.iter()
                .copied()
                .filter(|row| rows_of(display).contains(row))
                .collect::<Vec<_>>(),
            "after {id} the checked rows are not one per group"
        );
    }

    let last = cx.update(|app| state.read(app).ui_settings.timeline_display);
    assert_ne!(
        last,
        TimelineDisplay::default(),
        "the walk ends on non-defaults"
    );
    cx.executor().advance_clock(Duration::from_millis(
        crate::settings::SAVE_DEBOUNCE_MS + 50,
    ));
    cx.run_until_parked();
    assert_eq!(UiSettings::load(&data_dir).timeline_display, last);
}

/// Ten minutes of sequence at zoom 1 in a window with a viewport.
fn ten_minutes(follow: Follow) -> AppState {
    let (mut state, _clip) = with_clip();
    state.last_window_w = 1440.0;
    state.set_zoom(1.0, false);
    let fps = state.store.sequence.frame_rate.nominal_fps();
    state.store_mut().sequence.duration_ticks = fps * 600;
    state.scroll_timeline_to(0.0, false);
    state.ui_settings.timeline_display.follow = follow;
    state
}

/// Where the playhead sits in pixels, the way the follow places it.
fn playhead_px(state: &AppState) -> f32 {
    let fps = state.store.sequence.ticks_per_second();
    (state.playhead.value.max(0) as f32 / fps)
        * crate::transport::TIMELINE_BASE_PPS
        * crate::transport::clamp_zoom(state.zoom)
}

#[test]
fn a_smooth_follow_holds_the_playhead_centred_without_a_glide() {
    let mut state = ten_minutes(Follow::Smooth);
    let fps = state.store.sequence.frame_rate.nominal_fps();
    let width = state.timeline_viewport_px();
    for second in [30, 31, 200] {
        state.set_playhead(fps * second);
        assert!(state.follow_playhead(), "{second}s is off center");
        let offset = playhead_px(&state) - state.scroll_x;
        assert!(
            (offset - width / 2.0).abs() < 1.0,
            "at {second}s the playhead sits {offset}px into a {width}px view"
        );
        assert!(
            !state.scroll_eased,
            "a smooth follow moves with the playhead"
        );
    }
}

#[test]
fn a_hand_scroll_holds_the_follow_until_the_playhead_leaves_the_view() {
    for follow in [Follow::Page, Follow::Smooth] {
        let mut state = ten_minutes(follow);
        let fps = state.store.sequence.frame_rate.nominal_fps();
        let width = state.timeline_viewport_px();
        state.set_playhead(fps * 300);
        assert!(
            state.follow_playhead(),
            "{follow:?}: five minutes off screen"
        );

        // The view is moved by hand so the playhead sits near its right edge,
        // where either follow would otherwise move it.
        let held = playhead_px(&state) - width * 0.97;
        state.scroll_timeline_to(held, false);
        state.set_playhead(state.playhead.value + 1);
        assert!(
            !state.follow_playhead(),
            "{follow:?}: the hand scroll was undone"
        );
        assert_eq!(state.scroll_x, held);

        // Once the playhead runs out of the view, following resumes.
        state.set_playhead(state.playhead.value + fps * 5);
        assert!(
            state.follow_playhead(),
            "{follow:?}: the follow never resumed"
        );
        let at = playhead_px(&state);
        assert!(at > state.scroll_x && at < state.scroll_x + width);
    }
}

#[test]
fn the_display_button_opens_the_submenu_and_the_keys_pick_a_choice() {
    let ctx = PluginContext::new();
    ctx.mount(&UiPlugin).expect("the ui plugin needs nothing");
    let mut state =
        AppState::new(ctx, crate::scratch_pool::scratch("timeline-display")).expect("state");
    state.enter_editor();
    assert_eq!(state.display_menu(), None);
    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    let picked = cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            state.toggle_display_menu("timeline-display-settings", TIMELINE_DISPLAY);
            assert_eq!(state.open_menu, Some(AppMenu::View));
            assert_eq!(
                state.display_menu(),
                Some(TIMELINE_DISPLAY),
                "the submenu is out"
            );
            assert_eq!(state.menu_owner, Some("timeline-display-settings"));
            // The first choice is lit, so Down then Enter picks the second.
            assert!(state.menu_key(MenuNav::Down, cx));
            assert!(state.menu_key(MenuNav::Run, cx));
            state.ui_settings.timeline_display.thumbnails
        })
    });
    assert_eq!(picked, Thumbnails::HeadAndTail);
}
