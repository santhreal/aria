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
    [crate::chrome::menu_items::MenuItem::new(
        "palette-cache-menu-probe",
        "Palette cache menu probe",
        noop_menu,
    )];

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
pub(crate) fn state() -> crate::app::AppState {
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
/// Each as its menu, its label, and the label the palette offers it under: a
/// row in a submenu is offered after its caption, unless a command already
/// offers its bare label.
fn menu_rows(state: &crate::app::AppState) -> Vec<(AppMenu, &'static str, gpui::SharedString)> {
    let host = crate::chrome::menu_host::host_of(&state.ctx).expect("the ui plugin mounts menus");
    AppMenu::ALL
        .into_iter()
        .flat_map(|menu| {
            host.items(menu, state)
                .iter()
                .map(move |item| (menu, item.label, crate::app::palette::offered_label(item)))
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
    // The catalog folds rows that differ only in case into one, as the
    // operator reads them as one; "New project" offers "New Project".
    let offers = |name: &str| offered.iter().any(|each| each.eq_ignore_ascii_case(name));
    for (menu, label, captioned) in menu_rows(&state) {
        assert!(
            offers(label) || offers(&captioned),
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

/// With nothing typed the palette reads like the menu bar: its rows open with
/// the menu rows in menu order, and every command follows the last of them.
/// A catalog that lists the commands first opens on whichever command
/// registered first, with the File menu's rows hundreds of rows down.
#[test]
fn an_empty_query_lists_the_menu_rows_first_in_menu_order() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let hits = state.palette_hits();
    let is_menu = |action: &Offered| match action {
        Offered::Menu(..) => true,
        Offered::Command(_)
        | Offered::Windowed(_)
        | Offered::Generator(_)
        | Offered::ClipType { .. }
        | Offered::Plugin(_) => false,
    };
    let leading: Vec<(AppMenu, &str)> = hits
        .iter()
        .map_while(|offer| match offer.action {
            Offered::Menu(menu, id) => Some((menu, id)),
            Offered::Command(_)
            | Offered::Windowed(_)
            | Offered::Generator(_)
            | Offered::ClipType { .. }
            | Offered::Plugin(_) => None,
        })
        .collect();
    assert!(
        !leading.is_empty(),
        "the first row, {}, is not a menu row",
        hits[0].label
    );
    if let Some(late) = hits[leading.len()..]
        .iter()
        .find(|offer| is_menu(&offer.action))
    {
        panic!("the menu row {} is listed after a command", late.label);
    }
    let host = crate::chrome::menu_host::host_of(&state.ctx).expect("the ui plugin mounts menus");
    let drawn: Vec<(AppMenu, &str)> = AppMenu::ALL
        .into_iter()
        .flat_map(|menu| {
            host.items(menu, &state)
                .iter()
                .map(move |item| (menu, item.id))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut rest = drawn.iter();
    for (menu, id) in &leading {
        assert!(
            rest.any(|row| row == &(*menu, *id)),
            "{} {id} is out of menu order",
            menu.label()
        );
    }
}

/// The job commands that share a label with an Analyze menu row and run a
/// different pipeline. The command starts the detector that fills the Analysis
/// tab; the menu row runs the registered analyzer of the same name. The palette
/// offers the command and folds the menu row under it.
const DETECTOR_LABEL_SHARED: &[&str] = &["pal-detect-beats", "pal-detect-silence", "pal-loudness"];

/// A command never repeats a menu row by name. The palette already offers
/// every menu row, so a command under a menu row's label is a second row for
/// one action, and the catalog folds the menu's row under it.
///
/// Walks what mounting registered, the command host and the plugin commands,
/// so a command a shipped plugin adds is covered with the compiled tables.
/// Matched on the label, case aside, bare and captioned. A command that repeats
/// a menu row's action under another label is not caught here.
#[test]
fn no_shipped_command_repeats_a_menu_row_by_name() {
    let state = state();
    let rows = menu_rows(&state);
    let shares = |label: &str| {
        rows.iter()
            .find(|(_, bare, captioned)| {
                label.eq_ignore_ascii_case(bare) || label.eq_ignore_ascii_case(captioned)
            })
            .map(|(menu, bare, _)| format!("{} > {bare}", menu.label()))
    };
    let host = crate::commands::host::host_of(&state.ctx).expect("the ui plugin mounts commands");
    let commands: Vec<(String, String)> = host
        .all()
        .into_iter()
        .map(|command| (command.id.to_owned(), command.label.to_owned()))
        .chain(
            state
                .ctx
                .commands()
                .into_iter()
                .map(|spec| (spec.id.clone(), spec.title.clone())),
        )
        .collect();
    let shipped =
        crate::commands::BUILTIN_COMMANDS.len() + crate::commands::BUILTIN_JOB_COMMANDS.len();
    assert!(
        commands.len() >= shipped,
        "mounting registered {} commands, fewer than the {shipped} shipped",
        commands.len()
    );
    let mut wrong = Vec::new();
    let mut shared = Vec::new();
    for (id, label) in &commands {
        match (shares(label), DETECTOR_LABEL_SHARED.contains(&id.as_str())) {
            (Some(row), false) => wrong.push(format!("{id} ({label}) repeats {row}")),
            (Some(_), true) => shared.push(id.as_str()),
            (None, _) => {}
        }
    }
    for id in DETECTOR_LABEL_SHARED {
        if !shared.contains(id) {
            wrong.push(format!(
                "{id} is listed as sharing a menu label and does not"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_command_outranks_the_menu_row_that_shares_its_label() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let commands = crate::commands::host::host_of(&state.ctx).expect("command host");
    commands.add(crate::commands::host::CommandEntry {
        id: "palette-outrank-probe",
        label: "Jobs",
        run: noop_command,
    });
    let catalog = state.palette_catalog();
    let jobs: Vec<_> = catalog
        .iter()
        .filter(|&offer| offer.label == "Jobs")
        .collect();
    assert_eq!(jobs.len(), 1, "Jobs is offered once");
    assert_eq!(jobs[0].id, "palette-outrank-probe");
    assert!(
        matches!(jobs[0].action, Offered::Command(_)),
        "a row that can run without the entity context must be the one offered"
    );
    assert!(jobs[0].source.is_none(), "a command names no menu");
}

#[test]
fn a_menu_only_row_names_the_menu_it_came_from() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let derived: Vec<_> = state
        .palette_catalog()
        .iter()
        .filter(|&offer| offer.source.is_some())
        .cloned()
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
    for ch in "mark".chars() {
        state.palette_push(ch);
    }
    let hits = state.palette_hits();
    assert!(hits.len() < all, "a query must narrow the offer");
    let labels: Vec<&str> = hits.iter().map(|offer| offer.label.as_str()).collect();
    let leads = |label: &&str| label.to_lowercase().starts_with("mark");
    let last_lead = labels.iter().rposition(leads);
    let first_other = labels.iter().position(|label| !leads(label));
    assert!(
        matches!((last_lead, first_other), (Some(lead), Some(other)) if lead < other),
        "every row that starts with the query comes before the rows that only contain it: \
         {labels:?}"
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
    // the highlight is clamped on read, not only when a key is pressed. The
    // longest label matches itself and next to nothing else, so the list
    // shrinks below the highlight however many rows the menus add.
    let longest = state
        .palette_catalog()
        .iter()
        .map(|offer| offer.label.to_string())
        .max_by_key(|label| label.len())
        .expect("the palette lists rows");
    state.palette_query = longest;
    let count = state.palette_hits().len();
    assert!((1..7).contains(&count), "{count} hits");
    assert_eq!(state.palette_highlight(), Some(count - 1));
}

fn probe_first(state: &mut crate::app::AppState) {
    state.status_message = Some("first".into());
}

fn probe_second(state: &mut crate::app::AppState) {
    state.status_message = Some("second".into());
}

#[test]
fn enter_runs_the_highlighted_row_not_the_first() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let commands = crate::commands::host::host_of(&state.ctx).expect("command host");
    let rows: [(
        &'static str,
        &'static str,
        crate::commands::host::CommandRun,
    ); 2] = [
        (
            "palette-enter-first",
            "Palette enter probe one",
            probe_first,
        ),
        (
            "palette-enter-second",
            "Palette enter probe two",
            probe_second,
        ),
    ];
    for (id, label, run) in rows {
        commands.add(crate::commands::host::CommandEntry { id, label, run });
    }
    for ch in "palette enter probe".chars() {
        state.palette_push(ch);
    }
    let hits = state.palette_hits();
    let labels: Vec<&str> = hits.iter().map(|offer| offer.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Palette enter probe one", "Palette enter probe two"]
    );
    state.palette_move(1);
    assert!(state.run_palette_highlight().is_none());
    assert_eq!(
        state.status_message.as_deref(),
        Some("second"),
        "Enter ran the first row, not the highlight"
    );
}

#[test]
fn a_menu_row_reports_itself_back_for_the_context_to_run() {
    let mut state = state();
    state.open_dialog(DialogKind::Palette);
    let (index, expected) = state
        .palette_hits()
        .iter()
        .enumerate()
        .find_map(|(at, offer)| match offer.action {
            Offered::Menu(menu, id) => Some((at, Pending::Menu(menu, id))),
            Offered::Command(_)
            | Offered::Windowed(_)
            | Offered::Generator(_)
            | Offered::ClipType { .. }
            | Offered::Plugin(_) => None,
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
