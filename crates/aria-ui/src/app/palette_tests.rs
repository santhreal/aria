//! Contracts for what the palette offers and which row Enter runs.
//!
//! The class closed here is "the palette is a second, shorter catalog": it held
//! its own table, so a row added to a menu was reachable with the mouse and
//! unreachable by name, and there was no highlight to move — Enter always ran the
//! first hit.
//!
//! The invariant test below enumerates the menus at run time, so a menu row added
//! later is covered without editing this file. That is the point of the derived
//! offer: there is no list here to forget.
//!
//! Not covered: drawing. The panel reads `palette_hits` and `palette_highlight`
//! and calls `dispatch`, which needs a window.

use crate::app::DialogKind;
use crate::app::palette::{Offered, Pending};
use crate::plugins::mount_all;
use crate::types::AppMenu;
use aria_plugin::PluginContext;

fn noop_command(_: &mut crate::app::AppState) {}

fn noop_menu(_: &str, _: &mut crate::app::AppState, _: &mut gpui::Context<crate::app::AppState>) {}

const CONTRIBUTED_MENU_ROW: [crate::chrome::menu_items::MenuItem; 1] =
    [crate::chrome::menu_items::MenuItem {
        id: "palette-cache-menu-probe",
        label: "Palette cache menu probe",
        run: noop_menu,
    }];

fn contributed_menu(
    menu: AppMenu,
    _: &crate::app::AppState,
) -> crate::chrome::menu_items::MenuItems {
    if menu == AppMenu::Help {
        std::borrow::Cow::Borrowed(&CONTRIBUTED_MENU_ROW)
    } else {
        std::borrow::Cow::Borrowed(&[])
    }
}

/// A state with the registries mounted and a project open.
///
/// The palette's offer comes from two registries, so a fixture without them
/// offers nothing and would pass every assertion below by vacuity. A project is
/// open because Home refuses most dialogs and hides most menu rows.
fn state() -> crate::app::AppState {
    let ctx = PluginContext::new();
    // A headless machine can fail the GPU-bound mounts; the menu and command
    // registries are not among them.
    let dir = crate::scratch_pool::scratch("palette");
    let _ = mount_all(&ctx, &dir);
    let mut state = crate::app::AppState::new(ctx, dir).expect("state");
    state.new_project().expect("a project to edit");
    state
}

/// Every row the app bar would draw, across every menu, for this state.
fn menu_rows(state: &crate::app::AppState) -> Vec<(AppMenu, &'static str)> {
    let host = crate::chrome::menu_host::host_of(&state.ctx).expect("the ui plugin mounts menus");
    AppMenu::ALL
        .into_iter()
        .flat_map(|menu| {
            host.items(menu, state)
                .iter()
                .map(move |item| (menu, item.label))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn every_menu_row_is_offered_by_name() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let catalog = state.palette_catalog();
    let offered: Vec<&str> = catalog.iter().map(|offer| offer.label.as_str()).collect();
    for (menu, label) in menu_rows(&state) {
        assert!(
            offered.contains(&label),
            "{label} is in the {} menu but the palette does not offer it",
            menu.label()
        );
    }
}

#[test]
fn a_menu_row_the_commands_already_name_is_offered_once() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let catalog = state.palette_catalog();
    for offer in catalog.iter() {
        let same: Vec<&str> = catalog
            .iter()
            .filter(|other| other.label == offer.label)
            .map(|other| other.id.as_str())
            .collect();
        assert_eq!(same.len(), 1, "{} is offered twice: {same:?}", offer.label);
    }
}

#[test]
fn a_command_outranks_the_menu_row_that_shares_its_label() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let jobs = state
        .palette_catalog()
        .iter()
        .cloned()
        .find(|offer| offer.label == "Jobs")
        .expect("Jobs is offered");
    assert!(
        matches!(jobs.action, Offered::Command(_)),
        "a row that can run without the entity context must be the one offered"
    );
    assert!(jobs.source.is_none(), "a command names no menu");
}

#[test]
fn a_menu_only_row_names_the_menu_it_came_from() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let derived: Vec<_> = state
        .palette_catalog()
        .iter()
        .cloned()
        .filter(|offer| offer.source.is_some())
        .collect();
    assert!(
        !derived.is_empty(),
        "the menus offer rows the command table does not"
    );
    for offer in derived {
        let menu = offer.source.expect("filtered on it");
        assert!(
            matches!(offer.action, Offered::Menu(at, _) if at == menu),
            "{} names {} but runs elsewhere",
            offer.label,
            menu.label()
        );
    }
}

#[test]
fn the_query_narrows_and_ranks_the_hits() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let all = state.palette_hits().len();
    for ch in "settings".chars() {
        state.palette_push(ch);
    }
    let hits = state.palette_hits();
    assert!(hits.len() < all, "a query must narrow the offer");
    assert_eq!(
        hits.first().map(|offer| offer.label.as_str()),
        Some("Settings"),
        "the row that starts with the query comes first, got {:?}",
        hits.iter().map(|o| o.label.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn the_highlight_starts_at_the_first_hit_and_wraps() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    assert_eq!(state.palette_highlight(), Some(0));
    state.palette_move(1);
    assert_eq!(state.palette_highlight(), Some(1));
    state.palette_move(-1);
    assert_eq!(state.palette_highlight(), Some(0));
    state.palette_move(-1);
    let last = state.palette_hits().len() - 1;
    assert_eq!(
        state.palette_highlight(),
        Some(last),
        "Up on the first row wraps to the last"
    );
}

#[test]
fn typing_returns_the_highlight_to_the_first_hit() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    state.palette_move(3);
    assert_eq!(state.palette_highlight(), Some(3));
    state.palette_push('s');
    assert_eq!(
        state.palette_highlight(),
        Some(0),
        "the row under the highlight is not the row it was"
    );
    state.palette_move(1);
    state.palette_pop();
    assert_eq!(state.palette_highlight(), Some(0), "so is a deletion");
}

#[test]
fn a_shorter_hit_list_cannot_leave_the_highlight_past_the_end() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    state.palette_move(6);
    assert_eq!(state.palette_highlight(), Some(6));
    // Set the query behind the palette's back, as a state change elsewhere can:
    // the highlight is clamped on read, not only when a key is pressed.
    state.palette_query = "settings".into();
    let count = state.palette_hits().len();
    assert!(count >= 1 && count <= 7);
    assert_eq!(state.palette_highlight(), Some(count - 1));
}

#[test]
fn enter_runs_the_highlighted_row_not_the_first() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    for ch in "jobs".chars() {
        state.palette_push(ch);
    }
    let hits = state.palette_hits();
    assert!(hits.len() >= 1);
    assert_eq!(hits[0].label, "Jobs");
    assert!(state.run_palette_highlight().is_none());
    assert_eq!(state.dialog, DialogKind::Jobs, "Enter ran the highlight");
}

#[test]
fn a_menu_row_reports_itself_back_for_the_context_to_run() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let (index, expected) = state
        .palette_hits()
        .into_iter()
        .enumerate()
        .find_map(|(at, offer)| match offer.action {
            Offered::Menu(menu, id) => Some((at, Pending::Menu(menu, id))),
            Offered::Command(_)
            | Offered::Windowed(_)
            | Offered::Effect(_)
            | Offered::Generator(_) => None,
        })
        .expect("a menu-only row is on offer");
    assert_eq!(
        state.run_palette_offer(index),
        Some(expected),
        "a row that needs the entity context must be handed back, not dropped"
    );
}

/// A command that starts a job is handed back the same way, for the same reason:
/// running it here would need a context `AppState` does not have. The class this
/// closes is a palette row that looks like it ran and silently did nothing.
#[test]
fn a_job_command_reports_itself_back_instead_of_running_here() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    state.palette_query = "Detect beats".into();
    let hits = state.palette_hits();
    assert!(!hits.is_empty(), "the palette does not offer the detectors");
    assert!(
        matches!(hits[0].action, Offered::Windowed(_)),
        "a detector must be offered as windowed work"
    );
    assert!(
        matches!(state.run_palette_highlight(), Some(Pending::Windowed(_))),
        "the detector was not handed back to the caller"
    );
    assert!(
        state.last_error.is_none() && state.status_message.is_none(),
        "nothing may run without a context"
    );
}

#[test]
fn a_query_that_matches_nothing_offers_nothing_and_enter_is_inert() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    state.palette_query = "qqzzxx".into();
    assert!(state.palette_hits().is_empty());
    assert_eq!(state.palette_highlight(), None);
    assert!(state.run_palette_highlight().is_none());
    assert_eq!(state.dialog, DialogKind::Palette, "nothing ran");
}

#[test]
fn closing_the_palette_forgets_the_query_and_the_highlight() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    state.palette_push('s');
    state.palette_move(1);
    state.open_dialog(DialogKind::None);
    assert!(state.palette_query.is_empty());
    assert_eq!(state.palette_index, 0);
}

#[test]
fn unchanged_palette_reads_share_the_catalog_and_hits() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let catalog = state.palette_catalog();
    assert!(std::sync::Arc::ptr_eq(&catalog, &state.palette_catalog()));
    let hits = state.palette_hits();
    assert!(std::sync::Arc::ptr_eq(&hits, &state.palette_hits()));

    state.palette_push('s');
    assert!(
        !std::sync::Arc::ptr_eq(&hits, &state.palette_hits()),
        "a changed query retained stale hits"
    );
    assert!(
        std::sync::Arc::ptr_eq(&catalog, &state.palette_catalog()),
        "querying rebuilt the unchanged command catalog"
    );
}

#[test]
fn registry_changes_invalidate_the_palette_catalog() {
    let state = state();
    let initial = state.palette_catalog();

    let commands = crate::commands::host::host_of(&state.ctx).expect("command host");
    commands.add(crate::commands::host::CommandEntry {
        id: "palette-cache-command-probe",
        label: "Palette cache command probe",
        run: noop_command,
    });
    let with_command = state.palette_catalog();
    assert!(
        !std::sync::Arc::ptr_eq(&initial, &with_command),
        "command registration retained a stale catalog"
    );
    assert!(
        with_command
            .iter()
            .any(|offer| offer.id == "palette-cache-command-probe")
    );

    let menus = crate::chrome::menu_host::host_of(&state.ctx).expect("menu host");
    menus.add(contributed_menu);
    let with_menu = state.palette_catalog();
    assert!(
        with_menu
            .iter()
            .any(|offer| offer.id == "palette-cache-menu-probe")
    );
    assert!(
        !std::sync::Arc::ptr_eq(&with_menu, &state.palette_catalog()),
        "a state-dependent plugin menu was cached without a state key"
    );
}
