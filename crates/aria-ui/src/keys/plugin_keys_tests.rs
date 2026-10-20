//! Plugin key defaults as bindings in the one key table.
//!
//! The class closed here is a plugin key that the shell dispatches from one
//! table while Settings, rebinding and the conflict list read another. Every
//! assertion goes through `KeyHost`: `resolve` is what a keystroke runs, `rows`
//! is what Settings lists, `conflicts` is what it explains, and `set_override`
//! is the rebinding. The routes are read from `KeyRoute::ALL`, so a route added
//! later is placed and resolved here without an edit.
//!
//! Not covered: the shell's keystroke handler, which needs a window; it calls
//! `resolve` and runs `KeyBody::Command` through `run_plugin_command`, which
//! `app/plugin_commands_tests.rs` covers.

use aria_plugin::{CommandSpec, KeyDefault, PluginContext, Registration};

use crate::keys::{Binding, Chord, KeyBody, KeyConflict, KeyHost, KeyOverrideError, KeyRoute};

fn command(ctx: &PluginContext, id: &str, title: &str) -> Registration {
    ctx.register_command(CommandSpec::new(id, title), |_: &PluginContext| Ok(()))
        .expect("the command registers")
}

fn key(ctx: &PluginContext, command: &str, chord: Chord, route: KeyRoute) -> Registration {
    ctx.register_key_default(KeyDefault {
        command: command.into(),
        chord,
        route,
    })
    .expect("the key default registers")
}

fn resolved(host: &KeyHost, route: KeyRoute, chord: Chord) -> Option<&'static str> {
    host.resolve(route, chord.key, chord.ctrl, chord.shift, chord.alt)
        .map(|binding| binding.id)
}

fn listed(host: &KeyHost) -> Vec<(&'static str, Option<Chord>)> {
    host.rows()
        .into_iter()
        .map(|row| (row.binding.id, row.chord))
        .collect()
}

const GRADE: Chord = Chord::ctrl_alt("g");

#[test]
fn a_plugin_key_is_placed_on_its_route_and_runs_its_command() {
    for route in KeyRoute::ALL {
        let ctx = PluginContext::new();
        let host = KeyHost::default();
        let _command = command(&ctx, "grade.reset", "Reset Grade");
        let _key = key(&ctx, "grade.reset", GRADE, route);
        host.sync_plugin_keys(&ctx);

        let binding = host
            .resolve(route, GRADE.key, GRADE.ctrl, GRADE.shift, GRADE.alt)
            .expect("the plugin key resolves");
        assert_eq!(binding.id, "grade.reset");
        assert!(matches!(binding.run, KeyBody::Command("Reset Grade")));
        for other in KeyRoute::ALL.into_iter().filter(|other| *other != route) {
            assert_eq!(
                resolved(&host, other, GRADE),
                None,
                "{route:?} leaked to {other:?}"
            );
        }
        assert_eq!(listed(&host), [("grade.reset", Some(GRADE))]);
        assert_eq!(host.label_of("grade.reset"), "Reset Grade");
        assert!(host.conflicts().is_empty());
    }
}

/// A key names a command by id; the binding exists while that command does,
/// and leaves with the registrations that made it.
#[test]
fn a_plugin_key_follows_its_command_and_its_registration() {
    let ctx = PluginContext::new();
    let host = KeyHost::default();
    let key = key(&ctx, "grade.reset", GRADE, KeyRoute::Editor);
    host.sync_plugin_keys(&ctx);
    assert_eq!(resolved(&host, KeyRoute::Editor, GRADE), None);
    assert!(
        listed(&host).is_empty(),
        "a key whose command is missing was listed"
    );

    let command = command(&ctx, "grade.reset", "Reset Grade");
    host.sync_plugin_keys(&ctx);
    assert_eq!(
        resolved(&host, KeyRoute::Editor, GRADE),
        Some("grade.reset")
    );

    drop(key);
    host.sync_plugin_keys(&ctx);
    assert_eq!(resolved(&host, KeyRoute::Editor, GRADE), None);
    assert!(listed(&host).is_empty());
    drop(command);
    host.sync_plugin_keys(&ctx);
    assert!(listed(&host).is_empty());
}

/// A shipped chord stays shipped, and the refusal is listed once however often
/// the registry syncs.
#[test]
fn a_taken_chord_is_refused_listed_and_left_with_its_holder() {
    let ctx = PluginContext::new();
    let host = KeyHost::default();
    assert!(host.add(Binding::inert(
        "key-save",
        Chord::ctrl("s"),
        KeyRoute::Editor
    )));
    let _command = command(&ctx, "grade.save", "Save Grade");
    let _key = key(&ctx, "grade.save", Chord::ctrl("s"), KeyRoute::Editor);
    let refusal = KeyConflict {
        held_by: "key-save",
        refused: "grade.save",
        chord: Chord::ctrl("s"),
    };
    for round in 0..3 {
        // Each round moves the command revision, which forces a full sync.
        let _moved = command(&ctx, &format!("grade.other{round}"), "Other");
        host.sync_plugin_keys(&ctx);
        assert_eq!(
            resolved(&host, KeyRoute::Editor, Chord::ctrl("s")),
            Some("key-save")
        );
        assert_eq!(
            host.conflicts(),
            std::slice::from_ref(&refusal),
            "round {round}"
        );
        assert_eq!(
            listed(&host),
            [("key-save", Some(Chord::ctrl("s"))), ("grade.save", None)],
            "a refused key is listed with no chord so it can be given one"
        );
        assert_eq!(host.chord_of("grade.save"), None);
    }
}

#[test]
fn the_first_plugin_to_ask_for_a_chord_keeps_it() {
    let ctx = PluginContext::new();
    let host = KeyHost::default();
    let _first = command(&ctx, "grade.first", "First");
    let _second = command(&ctx, "grade.second", "Second");
    let _first_key = key(&ctx, "grade.first", GRADE, KeyRoute::Editor);
    let _second_key = key(&ctx, "grade.second", GRADE, KeyRoute::Editor);
    host.sync_plugin_keys(&ctx);
    assert_eq!(
        resolved(&host, KeyRoute::Editor, GRADE),
        Some("grade.first")
    );
    assert_eq!(
        host.conflicts(),
        [KeyConflict {
            held_by: "grade.first",
            refused: "grade.second",
            chord: GRADE,
        }]
    );
    assert_eq!(host.label_of("grade.second"), "Second");
}

/// A refused key can be given a free chord; the chord another binding holds is
/// refused with the holder named, and a cleared one stays listed.
#[test]
fn a_refused_plugin_key_is_rebound_like_any_other() {
    let ctx = PluginContext::new();
    let host = KeyHost::default();
    assert!(host.add(Binding::inert(
        "key-save",
        Chord::ctrl("s"),
        KeyRoute::Editor
    )));
    assert!(host.add(Binding::inert(
        "key-open",
        Chord::ctrl("o"),
        KeyRoute::Editor
    )));
    let _command = command(&ctx, "grade.save", "Save Grade");
    let _key = key(&ctx, "grade.save", Chord::ctrl("s"), KeyRoute::Editor);
    host.sync_plugin_keys(&ctx);

    assert_eq!(
        host.set_override("grade.save", Some(Chord::ctrl("o"))),
        Err(KeyOverrideError::Taken(KeyConflict {
            held_by: "key-open",
            refused: "grade.save",
            chord: Chord::ctrl("o"),
        }))
    );
    host.set_override("grade.save", Some(GRADE))
        .expect("a free chord");
    host.sync_plugin_keys(&ctx);
    assert_eq!(resolved(&host, KeyRoute::Editor, GRADE), Some("grade.save"));
    assert_eq!(
        resolved(&host, KeyRoute::Editor, Chord::ctrl("s")),
        Some("key-save")
    );
    assert!(host.conflicts().is_empty());

    host.set_override("grade.save", None).expect("cleared");
    host.sync_plugin_keys(&ctx);
    assert_eq!(resolved(&host, KeyRoute::Editor, GRADE), None);
    assert!(listed(&host).contains(&("grade.save", None)));
    assert!(
        host.conflicts().is_empty(),
        "a cleared key asks for no chord"
    );

    host.remove_override("grade.save");
    host.sync_plugin_keys(&ctx);
    assert_eq!(
        host.conflicts().len(),
        1,
        "reverting asks for the default again"
    );
}

/// A freed chord goes to the plugin key that wanted it, and taking
/// it back for a shipped binding refuses the plugin key again.
#[test]
fn freeing_a_chord_places_the_plugin_key_that_wanted_it() {
    let ctx = PluginContext::new();
    let host = KeyHost::default();
    assert!(host.add(Binding::inert(
        "key-save",
        Chord::ctrl("s"),
        KeyRoute::Editor
    )));
    let _command = command(&ctx, "grade.save", "Save Grade");
    let _key = key(&ctx, "grade.save", Chord::ctrl("s"), KeyRoute::Editor);
    host.sync_plugin_keys(&ctx);
    assert_eq!(host.conflicts().len(), 1);

    host.set_override("key-save", Some(Chord::ctrl_shift("s")))
        .expect("a free chord");
    host.sync_plugin_keys(&ctx);
    assert_eq!(
        resolved(&host, KeyRoute::Editor, Chord::ctrl("s")),
        Some("grade.save")
    );
    assert!(host.conflicts().is_empty());

    assert!(
        matches!(
            host.set_override("key-save", Some(Chord::ctrl("s"))),
            Err(KeyOverrideError::Taken(KeyConflict {
                held_by: "grade.save",
                ..
            }))
        ),
        "a rebinding took a chord a plugin key holds"
    );
    host.reset_overrides();
    host.sync_plugin_keys(&ctx);
    assert_eq!(
        resolved(&host, KeyRoute::Editor, Chord::ctrl("s")),
        Some("key-save"),
        "a reset gives the shipped binding its chord back"
    );
    assert_eq!(host.conflicts().len(), 1);
}

/// A rebound chord for a plugin key outlives a sync, so a plugin that
/// registers another command does not reset the rebinding.
#[test]
fn an_override_moves_a_plugin_key_and_survives_a_sync() {
    let ctx = PluginContext::new();
    let host = KeyHost::default();
    let _command = command(&ctx, "grade.reset", "Reset Grade");
    let _key = key(&ctx, "grade.reset", GRADE, KeyRoute::Editor);
    host.sync_plugin_keys(&ctx);
    host.set_override("grade.reset", Some(Chord::ctrl_alt("j")))
        .expect("a free chord");
    let _later = command(&ctx, "grade.later", "Later");
    host.sync_plugin_keys(&ctx);
    assert_eq!(resolved(&host, KeyRoute::Editor, GRADE), None);
    assert_eq!(
        resolved(&host, KeyRoute::Editor, Chord::ctrl_alt("j")),
        Some("grade.reset")
    );
    let row = host
        .rows()
        .into_iter()
        .find(|row| row.binding.id == "grade.reset")
        .expect("listed");
    assert!(row.overridden);
}
