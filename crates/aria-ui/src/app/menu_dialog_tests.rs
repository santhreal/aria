//! A dialog opened while a popover is out takes the keyboard.
//!
//! The class closed here: a dialog drawn over a popover that still held the
//! keys. With the Window menu's Workspaces flyout out, Ctrl+Shift+P opened the
//! palette under a drop that stayed open, and the letters typed next lit menu
//! rows instead of filtering the palette, because an open drop answers letters
//! ahead of a focused box. The first case opens every menu of the bar, bare and
//! with each of its flyouts out, from [`AppMenu::ALL`] and the registry the bar
//! draws, and sends the chord and a query through the shell's key handler. The
//! second opens every popover the state holds, then each dialog kind through
//! [`AppState::open_dialog`], the call every dialog opens through.
//!
//! Not covered: a popover added later that [`AppState::close_popovers`] does
//! not close, and a popover out in another window.

use gpui::{TestAppContext, VisualTestContext};

use crate::app::{AppState, DialogKind};
use crate::chrome::menu_items::{DropEntry, drop_entries};
use crate::test_support::drawn_shell;
use crate::types::{AppMenu, ContextTarget, EditorTool};

fn press(visual: &mut VisualTestContext, keys: &str) {
    visual.simulate_keystrokes(keys);
    visual.run_until_parked();
}

#[gpui::test]
fn the_palette_chord_over_an_open_drop_closes_it_and_takes_the_query(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "menu-dialog-palette");
    visual.update(|_, app| app.set_reduce_motion(true));
    let mut flyouts_opened = 0;
    for menu in AppMenu::ALL {
        let subs: Vec<usize> = visual.read(|app| {
            drop_entries(&state.read(app).menu_rows(menu))
                .iter()
                .enumerate()
                .filter(|(_, entry)| matches!(entry, DropEntry::Sub { .. }))
                .map(|(at, _)| at)
                .collect()
        });
        flyouts_opened += subs.len();
        for flyout in std::iter::once(None).chain(subs.into_iter().map(Some)) {
            let case = format!("{} with flyout {flyout:?}", menu.label());
            visual.update(|_, app| {
                state.update(app, |state, cx| {
                    state.toggle_menu(menu);
                    if let Some(at) = flyout {
                        state.open_menu_flyout(at);
                    }
                    cx.notify();
                });
            });
            visual.run_until_parked();
            assert_eq!(
                visual.read(|app| state.read(app).open_menu),
                Some(menu),
                "{case}: the drop did not open"
            );
            press(visual, "ctrl-shift-p");
            press(visual, "b l u r");
            let (dialog, open, query) = visual.read(|app| {
                let state = state.read(app);
                (state.dialog, state.open_menu, state.palette_query.clone())
            });
            assert_eq!(dialog, DialogKind::Palette, "{case}: no palette");
            assert_eq!(open, None, "{case}: the drop stayed open under the palette");
            assert_eq!(query, "blur", "{case}: the typing missed the palette");
            press(visual, "escape");
            assert_eq!(
                visual.read(|app| state.read(app).dialog),
                DialogKind::None,
                "{case}: Escape did not close the palette"
            );
        }
    }
    assert!(flyouts_opened > 0, "no menu of the bar has a flyout");
}

/// Every dialog kind. The match has no wildcard, so a new kind fails to
/// compile until it is listed here as well.
fn every_dialog() -> Vec<DialogKind> {
    let all = [
        DialogKind::Export,
        DialogKind::Settings,
        DialogKind::Palette,
        DialogKind::Jobs,
        DialogKind::ProjectReport,
        DialogKind::MediaSupport,
        DialogKind::TimelineReport,
        DialogKind::UnsavedChanges,
        DialogKind::InstallPlugin,
        DialogKind::SpeedDuration,
        DialogKind::AudioGain,
        DialogKind::About,
        DialogKind::RemoveAttributes,
        DialogKind::KeyframeVelocity,
        DialogKind::MarkerEdit,
        DialogKind::Plugin("test.dialog"),
    ];
    for kind in all {
        match kind {
            DialogKind::None
            | DialogKind::Export
            | DialogKind::Settings
            | DialogKind::Palette
            | DialogKind::Jobs
            | DialogKind::ProjectReport
            | DialogKind::MediaSupport
            | DialogKind::TimelineReport
            | DialogKind::UnsavedChanges
            | DialogKind::InstallPlugin
            | DialogKind::SpeedDuration
            | DialogKind::AudioGain
            | DialogKind::About
            | DialogKind::RemoveAttributes
            | DialogKind::KeyframeVelocity
            | DialogKind::MarkerEdit
            | DialogKind::Plugin(_) => {}
        }
    }
    all.to_vec()
}

fn editor_state() -> AppState {
    let ctx = aria_plugin::PluginContext::new();
    ctx.mount(&crate::plugins::UiPlugin)
        .expect("the ui plugin needs nothing");
    let mut state = AppState::new(ctx, crate::scratch_pool::scratch("menu-dialog")).expect("state");
    state.enter_editor();
    state
}

#[test]
fn every_dialog_closes_every_popover() {
    for kind in every_dialog() {
        let mut state = editor_state();
        state.toggle_menu(AppMenu::Window);
        state.open_menu_flyout(0);
        state.open_context(ContextTarget::Preview, 10.0, 10.0);
        state.open_tool_variants(EditorTool::Pen);
        assert!(
            state.open_menu.is_some()
                && state.menu_flyout.is_some()
                && state.context_menu.is_some()
                && state.tool_variants_open().is_some(),
            "{kind:?}: a popover did not open before the dialog"
        );
        state.open_dialog(kind);
        assert_eq!(state.dialog, kind, "{kind:?} did not open");
        assert_eq!(
            (
                state.open_menu,
                state.menu_flyout,
                state.context_menu.clone(),
                state.tool_variants_open(),
            ),
            (None, None, None, None),
            "{kind:?} opened over a popover that stayed open"
        );
    }
}
