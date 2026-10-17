//! The keyboard image: every bindable key has a cap, and a cap shows what
//! dispatches on it.
//!
//! The class closed here is an image that disagrees with the registry: a key
//! the vocabulary can bind with no cap to drop a command on, a cap for a key no
//! chord can name, or a cap coloured by the shipped chord after a rebinding
//! moved it. The vocabulary is read from `BINDABLE_KEYS` at run time, so a key
//! added to it is red here until the image places it.
//!
//! Not covered: how a cap is painted, which the Settings draw owns.

use super::*;
use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::keys::{KeyHost, KeyRoute};
use aria_plugin::keys::BINDABLE_KEYS;

fn named_caps() -> Vec<&'static str> {
    rows()
        .flat_map(|row| row.iter())
        .filter_map(|cap| match cap.kind {
            CapKind::Key(name) => Some(name),
            _ => None,
        })
        .collect()
}

#[test]
fn every_bindable_key_has_exactly_one_cap() {
    let caps = named_caps();
    for key in BINDABLE_KEYS {
        let count = caps.iter().filter(|cap| *cap == key).count();
        assert_eq!(count, 1, "`{key}` has {count} caps on the image");
    }
    for cap in &caps {
        assert!(
            BINDABLE_KEYS.contains(cap),
            "the image has a cap for `{cap}`, which no chord can name"
        );
    }
}

#[test]
fn every_modifier_has_a_cap_and_the_main_rows_are_one_width() {
    for modifier in Modifier::ALL {
        assert!(
            rows()
                .flat_map(|row| row.iter())
                .any(|cap| cap.kind == CapKind::Modifier(modifier)),
            "{} has no cap",
            modifier.label()
        );
    }
    for (index, row) in MAIN.iter().enumerate() {
        let units: f32 = row.iter().map(|cap| cap.width).sum();
        assert!(
            (units - MAIN_UNITS).abs() < 1e-4,
            "main row {index} is {units} units, not {MAIN_UNITS}"
        );
    }
    for (index, row) in NAV.iter().enumerate() {
        let units: f32 = row.iter().map(|cap| cap.width).sum();
        assert!(units <= MAIN_UNITS, "nav row {index} is {units} units");
    }
}

#[test]
fn toggling_a_modifier_twice_leaves_the_image_as_it_was() {
    for modifier in Modifier::ALL {
        let once = Mods::default().toggled(modifier);
        assert!(once.held(modifier));
        for other in Modifier::ALL.into_iter().filter(|other| *other != modifier) {
            assert!(!once.held(other), "{} also held", other.label());
        }
        assert_eq!(once.toggled(modifier), Mods::default());
    }
}

/// A key holds what dispatches on it now: the shipped chord before a
/// rebinding, the new key after it, and nothing once the binding is cleared.
#[test]
fn a_key_holds_the_binding_that_dispatches_on_it() {
    let host = KeyHost::default();
    for binding in SHIPPED_BINDINGS {
        host.add(*binding);
    }
    let binding = SHIPPED_BINDINGS
        .iter()
        .find(|binding| {
            binding.route == KeyRoute::Editor
                && binding.chord.ctrl
                && !binding.chord.shift
                && !binding.chord.alt
        })
        .expect("a Ctrl chord on the editor route");
    let ctrl = Mods::default().toggled(Modifier::Ctrl);
    let holds = |mods: Mods, key: &str| {
        held_by_key(&host.rows(), mods)
            .get(key)
            .is_some_and(|held| held.iter().any(|held| held.id == binding.id))
    };
    assert!(holds(ctrl, binding.chord.key));
    assert!(
        !holds(Mods::default(), binding.chord.key),
        "a Ctrl chord shows with no modifier held"
    );

    let free = BINDABLE_KEYS
        .iter()
        .copied()
        .find(|key| {
            host.resolve(KeyRoute::Editor, key, true, false, false)
                .is_none()
        })
        .expect("a free Ctrl key");
    host.set_override(binding.id, Some(ctrl.chord(free)))
        .expect("free");
    assert!(holds(ctrl, free));
    assert!(
        !holds(ctrl, binding.chord.key),
        "the old key still holds it"
    );

    host.set_override(binding.id, None).expect("clear");
    assert!(
        !held_by_key(&host.rows(), ctrl)
            .values()
            .flatten()
            .any(|held| held.id == binding.id),
        "a cleared binding is on a key"
    );
}

/// Every shipped binding shows under its own modifiers and under no set one
/// modifier away, for each of Ctrl, Shift and Alt. A comparison dropped from
/// the match puts a chord on a key under the wrong modifiers: Shift+J drawn on
/// J with nothing held.
#[test]
fn a_binding_shows_only_under_its_own_modifiers() {
    let host = KeyHost::default();
    for binding in SHIPPED_BINDINGS {
        host.add(*binding);
    }
    let rows = host.rows();
    let holds = |mods: Mods, key: &str, id: &str| {
        held_by_key(&rows, mods)
            .get(key)
            .is_some_and(|held| held.iter().any(|held| held.id == id))
    };
    let mut exercised = [false; Modifier::ALL.len()];
    for row in &rows {
        let Some(chord) = row.chord else {
            continue;
        };
        let own = Mods {
            ctrl: chord.ctrl,
            shift: chord.shift,
            alt: chord.alt,
        };
        assert!(holds(own, chord.key, row.binding.id), "{}", row.binding.id);
        for (index, modifier) in Modifier::ALL.into_iter().enumerate() {
            exercised[index] |= own.held(modifier);
            assert!(
                !holds(own.toggled(modifier), chord.key, row.binding.id),
                "{} shows with {} toggled",
                row.binding.id,
                modifier.label()
            );
        }
    }
    assert_eq!(
        exercised,
        [true; Modifier::ALL.len()],
        "a modifier no shipped chord holds is not checked"
    );
}

/// The kind is the route, except that a plugin's command is Plugin on any
/// route: the legend names what a key will run, and a plugin key is the one
/// kind that can come and go with an install.
#[test]
fn the_kind_is_the_route_or_plugin_for_a_plugin_command() {
    let shipped = SHIPPED_BINDINGS[0];
    for route in KeyRoute::ALL {
        let binding = Binding { route, ..shipped };
        assert_eq!(CommandKind::of(&binding).label(), route.label());
        let plugin = Binding {
            route,
            run: KeyBody::Command("A plugin command"),
            ..shipped
        };
        assert_eq!(CommandKind::of(&plugin), CommandKind::Plugin);
    }
    let held = [
        Held {
            id: "a",
            kind: CommandKind::Plugin,
        },
        Held {
            id: "b",
            kind: CommandKind::Editor,
        },
        Held {
            id: "c",
            kind: CommandKind::Plugin,
        },
    ];
    assert_eq!(
        kinds(&held).collect::<Vec<_>>(),
        vec![CommandKind::Editor, CommandKind::Plugin],
        "each kind once, in legend order"
    );
}
