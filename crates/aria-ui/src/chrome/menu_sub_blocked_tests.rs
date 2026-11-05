//! A submenu whose every row is disabled is drawn and handled as a disabled
//! row.
//!
//! Closes the class "a submenu opens a door to rows that all refuse": with no
//! clip selected, every command row of the Clip drop drew faint while Replace
//! with Clip, Audio Channels, Transform, Track Matte and Analyze drew live and
//! opened flyouts of faint rows. Every submenu of every menu in
//! [`AppMenu::ALL`] is read from the registry, so a submenu added later is
//! held to the same rule: drawn blocked, with its first row's reason, exactly
//! when every row in it is blocked; and then opened by no key, click or
//! pointer, with Right, Enter and a click putting the reason on the status
//! line. A selected clip makes Clip > Transform live and openable, so the
//! rule does not shut a submenu that has a row to run, and synthetic rows in
//! every order show that one live row anywhere keeps a submenu open. Not
//! covered: the drawn fade, which the test frame does not report.

use std::path::PathBuf;

use aria_plugin::PluginContext;
use gpui::AppContext;

use super::drawn_entries;
use crate::app::AppState;
use crate::app::menu_keys::MenuNav;
use crate::chrome::menu_items::{DropEntry, MenuItem, drop_entries};
use crate::types::AppMenu;

fn state() -> AppState {
    let ctx = PluginContext::new();
    ctx.mount(&crate::plugins::UiPlugin)
        .expect("the ui plugin needs nothing");
    let mut state =
        AppState::new(ctx, crate::scratch_pool::scratch("menu-sub-blocked")).expect("state");
    state.enter_editor();
    state
}

/// Every submenu of `menu`: its entry index, caption, and the reason each row
/// in it gives, `None` for a row that can run.
fn submenus(
    state: &AppState,
    menu: AppMenu,
) -> Vec<(usize, &'static str, Vec<Option<&'static str>>)> {
    let rows = state.menu_rows(menu);
    drop_entries(&rows)
        .into_iter()
        .enumerate()
        .filter_map(|(at, entry)| match entry {
            DropEntry::Sub {
                caption,
                rows: held,
            } => Some((
                at,
                caption,
                held.iter()
                    .map(|row| rows[*row].blocked_by(state))
                    .collect(),
            )),
            DropEntry::Row(_) => None,
        })
        .collect()
}

/// Check every submenu of every menu against `state`; returns how many are
/// blocked.
fn check_every_submenu(state: &mut AppState, cx: &mut gpui::Context<AppState>) -> usize {
    let mut blocked = 0;
    for menu in AppMenu::ALL {
        for (at, caption, reasons) in submenus(state, menu) {
            let case = format!("{} > {caption}", menu.label());
            let shut = reasons
                .iter()
                .all(Option::is_some)
                .then(|| reasons[0])
                .flatten();
            state.toggle_menu(menu);
            let drawn = drawn_entries(menu, state);
            assert!(drawn[at].sub, "{case}: entry {at} is not the submenu");
            assert_eq!(drawn[at].blocked, shut, "{case}: the drawn entry's reason");

            state.point_menu_row(at);
            assert_eq!(
                state.menu_flyout.is_some(),
                shut.is_none(),
                "{case}: the pointer over it"
            );
            state.menu_flyout = None;
            state.status_message = None;
            state.open_menu_flyout(at);
            assert_eq!(
                state.menu_flyout.is_some(),
                shut.is_none(),
                "{case}: a click on it"
            );
            assert_eq!(
                state.status_message.as_deref(),
                shut,
                "{case}: a click states"
            );
            for key in [MenuNav::Right, MenuNav::Run] {
                state.menu_flyout = None;
                state.status_message = None;
                state.menu_row = Some(at);
                assert!(state.menu_key(key, cx), "{case}: the drop takes {key:?}");
                assert_eq!(
                    state.menu_flyout,
                    shut.is_none().then_some((at, Some(0))),
                    "{case}: {key:?} on it"
                );
                assert_eq!(
                    state.status_message.as_deref(),
                    shut,
                    "{case}: {key:?} states"
                );
                assert_eq!(
                    state.open_menu,
                    Some(menu),
                    "{case}: {key:?} keeps the drop"
                );
            }
            state.close_menu();
            blocked += usize::from(shut.is_some());
        }
    }
    blocked
}

#[test]
fn a_submenu_with_every_row_disabled_opens_nothing_and_states_why() {
    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state()));
    let blocked = cx.update(|app| entity.update(app, check_every_submenu));
    assert!(blocked > 0, "with no clip selected no submenu is blocked");

    let (blocked_with_clip, transform) = cx.update(|app| {
        entity.update(app, |state, cx| {
            let still =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
            crate::app::premiere_tests::place_media(state, still);
            let transform = submenus(state, AppMenu::Clip)
                .into_iter()
                .find(|(_, caption, _)| *caption == "Transform")
                .expect("Clip has a Transform submenu");
            (check_every_submenu(state, cx), transform)
        })
    });
    assert!(
        transform.2.iter().any(Option::is_none),
        "with a clip selected Clip > Transform has a row to run: {transform:?}"
    );
    assert!(
        blocked_with_clip < blocked,
        "a selected clip opens no submenu"
    );
}

/// One live row anywhere in a submenu keeps it open; with every row blocked it
/// states its first row's reason.
#[test]
fn a_submenu_is_blocked_only_when_every_row_is() {
    fn shut(_: &str, _: &AppState) -> Option<&'static str> {
        Some("Shut")
    }
    fn also_shut(_: &str, _: &AppState) -> Option<&'static str> {
        Some("Also shut")
    }
    let live = MenuItem::new("live", "Live", |_, _, _| {}).under("Sub");
    let (blocked, other) = (live.when(shut), live.when(also_shut));
    let state = state();
    for (rows, expected, case) in [
        (vec![live, live], None, "two live rows"),
        (vec![blocked, live], None, "blocked, then live"),
        (vec![live, blocked], None, "live, then blocked"),
        (vec![blocked, live, other], None, "live between two blocked"),
        (vec![blocked, other], Some("Shut"), "two blocked"),
        (vec![other, blocked, other], Some("Also shut"), "all three"),
    ] {
        let entries = drop_entries(&rows);
        assert_eq!(entries.len(), 1, "{case}: the rows share one submenu");
        assert_eq!(entries[0].blocked_by(&rows, &state), expected, "{case}");
    }
}
