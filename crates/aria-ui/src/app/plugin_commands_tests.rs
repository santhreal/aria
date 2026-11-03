//! Plugin commands as the palette and the context menus offer them.
//!
//! The class closed here is a plugin command reachable from one surface and not
//! another. A command registered on the context is offered by the palette under
//! its title; a command placed in a context menu location is drawn in the menu
//! that location names; every surface runs it through `run_plugin_command`,
//! which reports a failure under the command's title. The locations are read
//! from `MenuLocation::ALL`, so a location added without a menu to draw it in
//! fails here.
//!
//! Not covered: drawing the rows, which needs a window. The menu bar rows are
//! covered in `chrome/plugin_menus_tests.rs`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_plugin::{
    CommandSpec, ContextMenuRequested, MenuCommand, MenuLocation, MenuPath, PluginContext,
    Registration,
};

use crate::app::palette::Offered;
use crate::app::{AppState, ContextPluginRow};
use crate::types::ContextTarget;

fn state() -> AppState {
    let dir = crate::scratch_pool::scratch("plugin-commands");
    AppState::new(PluginContext::new(), dir).expect("state")
}

/// Register `id`, counting its runs.
fn counted(state: &AppState, id: &str, title: &str) -> (Registration, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&runs);
    let registration = state
        .ctx
        .register_command(CommandSpec::new(id, title), move |_: &PluginContext| {
            seen.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .expect("the command registers");
    (registration, runs)
}

fn place(state: &AppState, command: &str, path: &str) -> Registration {
    state
        .ctx
        .register_menu_entry(MenuCommand {
            command: command.into(),
            path: MenuPath::parse(path).expect("a menu path"),
        })
        .expect("the menu entry registers")
}

fn plugin_offers(state: &AppState) -> Vec<(String, String)> {
    state
        .palette_catalog()
        .iter()
        .filter_map(|offer| match &offer.action {
            Offered::Plugin(spec) => Some((spec.id.clone(), offer.label.to_string())),
            _ => None,
        })
        .collect()
}

/// Every menu a context target opens, one per target kind.
fn every_target() -> Vec<ContextTarget> {
    vec![
        ContextTarget::Clip { clip: 1, track: 2 },
        ContextTarget::Track { track: 2 },
        ContextTarget::Transition { clip: 1 },
        ContextTarget::Preview,
        ContextTarget::PoolItem { media: 3 },
        ContextTarget::Recent {
            path: PathBuf::from("cut.aria"),
        },
        ContextTarget::Effect { clip: 1, index: 0 },
        ContextTarget::ClipBadge { clip: 1, track: 2 },
    ]
}

fn command_rows(state: &AppState) -> Vec<(&'static str, &'static str)> {
    state
        .context_plugin_rows
        .iter()
        .filter_map(|row| match row {
            ContextPluginRow::Command { command, label, .. } => Some((*command, *label)),
            ContextPluginRow::Hook(_) => None,
        })
        .collect()
}

#[test]
fn the_palette_offers_a_plugin_command_by_title_while_it_is_registered() {
    let mut state = state();
    assert!(plugin_offers(&state).is_empty());

    let (registration, runs) = counted(&state, "grade.reset", "Reset Grade");
    assert_eq!(
        plugin_offers(&state),
        [("grade.reset".to_owned(), "Reset Grade".to_owned())],
        "a cached catalog hid a command registered after it was built"
    );

    for ch in "reset grade".chars() {
        state.palette_push(ch);
    }
    assert!(state.run_palette_highlight().is_none(), "the row ran here");
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    assert_eq!(state.last_error, None);

    drop(registration);
    assert!(
        plugin_offers(&state).is_empty(),
        "a cached catalog kept a command its plugin dropped"
    );
}

/// A command in a menu bar menu is offered once, as the command, and not a
/// second time as the menu row that names it.
#[test]
fn a_placed_command_is_offered_once() {
    let state = state();
    let (_command, _) = counted(&state, "grade.reset", "Reset Grade");
    let _row = place(&state, "grade.reset", "Sequence/Color Tools");
    let offered: Vec<_> = state
        .palette_catalog()
        .iter()
        .filter(|offer| offer.id == "grade.reset")
        .map(|offer| offer.label.to_string())
        .collect();
    assert_eq!(offered, ["Reset Grade"]);
}

#[test]
fn a_failing_command_is_reported_under_its_title() {
    let mut state = state();
    let _refuses = state
        .ctx
        .register_command(
            CommandSpec::new("grade.refuse", "Refuse Grade"),
            |_: &PluginContext| Err("no clip is selected".to_owned()),
        )
        .expect("registers");
    state.run_plugin_command("grade.refuse");
    assert_eq!(
        state.last_error.as_deref(),
        Some("Refuse Grade: no clip is selected")
    );

    let _panics = state
        .ctx
        .register_command(
            CommandSpec::new("grade.panic", "Panic Grade"),
            |_: &PluginContext| -> Result<(), String> { panic!("lut table empty") },
        )
        .expect("registers");
    state.run_plugin_command("grade.panic");
    assert_eq!(
        state.last_error.as_deref(),
        Some("Panic Grade: the command panicked: lut table empty")
    );

    state.run_plugin_command("grade.gone");
    assert_eq!(
        state.last_error.as_deref(),
        Some("No command is registered as grade.gone")
    );
}

/// Every context menu location is drawn by exactly one context menu, with the
/// group in the label, before the rows handlers add; a command that is not
/// registered draws nothing.
#[test]
fn every_context_location_draws_its_placed_commands_in_one_menu() {
    let mut state = state();
    let bus = Arc::clone(state.ctx.event_bus());
    let _hook = bus.waterfall::<ContextMenuRequested, _>(|request| {
        request.add("Hook Row", || {});
    });
    let (_command, runs) = counted(&state, "grade.reset", "Reset Grade");

    for location in MenuLocation::ALL.into_iter().filter(|at| !at.in_menu_bar()) {
        let path = format!("{}/Color Tools", location.label());
        let row = place(&state, "grade.reset", &path);
        let mut drawn_in = Vec::new();
        for target in every_target() {
            state.open_context(target.clone(), 10.0, 10.0);
            let rows = command_rows(&state);
            if rows.is_empty() {
                continue;
            }
            assert_eq!(rows, [("grade.reset", "Color Tools: Reset Grade")]);
            assert!(
                matches!(
                    state.context_plugin_rows.last(),
                    Some(ContextPluginRow::Hook(_))
                ),
                "{target:?}: the placed row must come before the handler's rows"
            );
            let placed = state.context_plugin_rows[0].clone();
            let before = runs.load(Ordering::Relaxed);
            state.run_context_plugin_row(&placed);
            assert_eq!(runs.load(Ordering::Relaxed), before + 1, "{target:?}");
            drawn_in.push(target);
        }
        assert_eq!(
            drawn_in.len(),
            1,
            "{location:?} must be drawn by one context menu, drawn by {drawn_in:?}"
        );
        drop(row);
        for target in every_target() {
            state.open_context(target, 10.0, 10.0);
            assert!(
                command_rows(&state).is_empty(),
                "{location:?} outlived its row"
            );
        }
    }

    let _orphan = place(&state, "grade.unregistered", "Clip");
    state.open_context(ContextTarget::Clip { clip: 1, track: 2 }, 10.0, 10.0);
    assert!(
        command_rows(&state).is_empty(),
        "a row whose command is not registered was drawn"
    );
}

#[test]
fn plugin_commands_drive_editor_transport_and_project_session_through_shared_services() {
    use aria_commands::{Command, EditCommand};
    use aria_core::{Rational, RationalTime};
    use aria_models::TrackKind;
    use aria_plugin::{EditorView, Origin, ProjectHost};

    let dir = crate::scratch_pool::scratch("plugin-transport-session");
    let mut state = AppState::new(PluginContext::new(), dir.clone()).expect("state");
    let first_path = dir.join("first.aria");
    let second_path = dir.join("second.aria");

    let first_move = first_path.clone();
    let _save_as_first = state
        .ctx
        .register_command(
            CommandSpec::new("session.save_as_first", "Save First"),
            move |ctx: &PluginContext| {
                let host = ctx.get::<ProjectHost>();
                host.save_as(&Origin::plugin("test.session"), &first_move)
                    .map(|_| ())
                    .map_err(|err| err.to_string())
            },
        )
        .expect("registers save_as_first");
    state.run_plugin_command("session.save_as_first");
    assert_eq!(state.last_error, None);
    assert_eq!(state.project_path.as_deref(), Some(first_path.as_path()));
    assert!(first_path.is_file());

    // Dirty the session with a track addition, then save to `second_path`.
    state
        .host
        .apply(
            &Origin::plugin("test.session"),
            Command::Edit(EditCommand::AddTrack {
                name: "V2".into(),
                kind: TrackKind::Video,
                index: None,
                pinned: false,
            }),
        )
        .expect("add track");
    state.sync_from_plugin_host();
    assert!(state.host.is_dirty());

    let second_move = second_path.clone();
    let _save_as_second = state
        .ctx
        .register_command(
            CommandSpec::new("session.save_as_second", "Save Second"),
            move |ctx: &PluginContext| {
                let host = ctx.get::<ProjectHost>();
                host.save_as(&Origin::plugin("test.session"), &second_move)
                    .map(|_| ())
                    .map_err(|err| err.to_string())
            },
        )
        .expect("registers save_as_second");
    state.run_plugin_command("session.save_as_second");
    assert_eq!(state.last_error, None);
    assert_eq!(state.project_path.as_deref(), Some(second_path.as_path()));
    assert!(!state.host.is_dirty());

    // Re-open `first_path` from a plugin command and verify `AppState` resets to `first_path`.
    let open_first = first_path.clone();
    let _open_cmd = state
        .ctx
        .register_command(
            CommandSpec::new("session.open_first", "Open First"),
            move |ctx: &PluginContext| {
                let host = ctx.get::<ProjectHost>();
                host.open(&Origin::plugin("test.session"), &open_first)
                    .map_err(|err| err.to_string())
            },
        )
        .expect("registers open_first");
    state.run_plugin_command("session.open_first");
    assert_eq!(state.last_error, None);
    assert_eq!(state.project_path.as_deref(), Some(first_path.as_path()));

    // Drive transport (`play`, `seek`, `stop`) from a plugin command and verify `AppState` and `EditorView`.
    let _transport_cmd = state
        .ctx
        .register_command(
            CommandSpec::new("transport.drive", "Drive Transport"),
            |ctx: &PluginContext| {
                let view = ctx.get::<EditorView>();
                view.play(2, ctx.events())?;
                view.seek(RationalTime::new(0, Rational::FPS_24), ctx.events())?;
                Ok(())
            },
        )
        .expect("registers transport.drive");
    state.run_plugin_command("transport.drive");
    assert_eq!(state.last_error, None);
    assert!(state.playing);
    assert_eq!(state.shuttle.as_plugin_speed(), 2);
    assert_eq!(
        state.editor_view.current().map(|s| (s.playing, s.speed)),
        Some((true, 2))
    );

    let _stop_cmd = state
        .ctx
        .register_command(
            CommandSpec::new("transport.stop", "Stop Transport"),
            |ctx: &PluginContext| {
                let view = ctx.get::<EditorView>();
                view.stop(ctx.events())
            },
        )
        .expect("registers transport.stop");
    state.run_plugin_command("transport.stop");
    assert_eq!(state.last_error, None);
    assert!(!state.playing);
    assert_eq!(
        state.editor_view.current().map(|s| (s.playing, s.speed)),
        Some((false, 1))
    );
}
