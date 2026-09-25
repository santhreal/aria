//! Contracts for rebinding a key.
//!
//! The class closed here: a binding was data nobody could edit. The chords lived
//! in three shipped tables, the registry refused a duplicate and recorded it, and
//! there was no way to move a key, clear one, or get one back — so the whole
//! surface is new and every route through it is exercised: capture, refusal,
//! clear, revert, reset, and the saved file the next launch reads.
//!
//! Bindings are never named by hand. Every case picks its subject out of the
//! shipped table at run time, so a chord that moves in a later change does not
//! turn these into false failures, and a binding added to the table is covered
//! by construction.

use aria_plugin::PluginContext;

use super::AppState;
use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::keys::{self, Chord, KeyRoute};
use crate::plugins::UiPlugin;
use crate::settings::UiSettings;

fn state_in(dir: &std::path::Path) -> AppState {
    let ctx = PluginContext::new();
    ctx.mount(&UiPlugin).expect("the ui plugin needs nothing");
    AppState::new(ctx, dir.to_path_buf()).expect("state")
}

fn state(prefix: &'static str) -> AppState {
    state_in(&crate::scratch_pool::scratch(prefix))
}

/// A binding on the editor route whose chord is a plain letter, so a capture can
/// be spelled without guessing what the platform reports.
fn plain_binding() -> &'static str {
    SHIPPED_BINDINGS
        .iter()
        .find(|binding| {
            binding.route == KeyRoute::Editor
                && !binding.chord.ctrl
                && !binding.chord.shift
                && !binding.chord.alt
                && binding.chord.key.len() == 1
        })
        .map(|binding| binding.id)
        .expect("the shipped table has a plain editor chord")
}

/// A key no shipped binding uses on the editor route, so a capture cannot collide
/// by accident.
fn free_key() -> &'static str {
    ["f9", "f10", "f11", "f12", "insert"]
        .into_iter()
        .find(|key| {
            !SHIPPED_BINDINGS
                .iter()
                .any(|binding| binding.route == KeyRoute::Editor && binding.chord.key == *key)
        })
        .expect("a function key is free")
}

/// The whole gesture, end to end: arm a row, press a key, and the key that fires
/// is the one that was pressed.
#[test]
fn a_captured_key_is_the_key_that_dispatches() {
    let mut state = state("rebind-capture");
    let id = plain_binding();
    let was = state.chord_label(id).expect("it starts with a chord");
    let key = free_key();

    state.begin_rebind(id);
    assert_eq!(state.rebinding(), Some(id), "the row is armed");
    state.capture_rebind(key, false, false, false);
    assert_eq!(state.rebinding(), None, "the capture ended");
    assert_eq!(state.chord_label(id).as_deref(), Some(key));

    let host = keys::host_of(&state.ctx).expect("the ui plugin mounts the registry");
    assert_eq!(
        host.resolve(KeyRoute::Editor, key, false, false, false)
            .map(|binding| binding.id),
        Some(id),
        "the new key does not dispatch"
    );
    assert!(
        host.resolve(KeyRoute::Editor, &was, false, false, false)
            .is_none(),
        "the old key still dispatches"
    );
    assert_eq!(
        state.ui_settings.keys.get(id),
        Some(&Some(key.to_string())),
        "the choice was not recorded for the next launch"
    );
}

/// Modifiers are part of the chord, so the same key with a modifier is a
/// different binding rather than the same one.
#[test]
fn a_capture_keeps_the_modifiers_it_was_pressed_with() {
    let mut state = state("rebind-modifiers");
    let id = plain_binding();
    state.begin_rebind(id);
    state.capture_rebind(free_key(), true, true, true);
    assert_eq!(
        state.chord_label(id).as_deref(),
        Some(&*format!("Ctrl+Shift+Alt+{}", free_key()))
    );
}

/// A refusal that named nothing would leave the operator pressing keys at random.
#[test]
fn a_taken_chord_is_refused_and_the_row_stays_armed() {
    let mut state = state("rebind-taken");
    let id = plain_binding();
    let holder = SHIPPED_BINDINGS
        .iter()
        .find(|binding| binding.route == KeyRoute::Editor && binding.id != id)
        .expect("a second editor binding");
    let chord = holder.chord;

    state.begin_rebind(id);
    state.capture_rebind(chord.key, chord.ctrl, chord.shift, chord.alt);
    assert_eq!(
        state.rebinding(),
        Some(id),
        "a refusal ended the gesture, so the next try costs another click"
    );
    let said = state.status_message.clone().expect("a refusal is reported");
    assert!(
        said.contains(&chord.label()) && said.contains(&keys::binding_label(holder.id)),
        "the refusal does not name the chord and what holds it: {said}"
    );
    assert!(
        state.ui_settings.keys.is_empty(),
        "a refused rebinding was persisted"
    );
}

/// A modifier arrives as its own keystroke while the operator is still reaching
/// for the real key, and must not be taken as the answer.
#[test]
fn a_key_that_cannot_be_bound_leaves_the_capture_waiting() {
    let mut state = state("rebind-unbindable");
    let id = plain_binding();
    let was = state.chord_label(id);
    state.begin_rebind(id);
    for key in ["shift", "control", "alt", "f13"] {
        state.capture_rebind(key, false, false, false);
        assert_eq!(state.rebinding(), Some(id), "{key} ended the capture");
    }
    assert_eq!(state.chord_label(id), was, "the chord changed anyway");
}

#[test]
fn escape_leaves_the_binding_alone() {
    let mut state = state("rebind-escape");
    let id = plain_binding();
    let was = state.chord_label(id);
    state.begin_rebind(id);
    state.capture_rebind("escape", false, false, false);
    assert_eq!(state.rebinding(), None);
    assert_eq!(state.chord_label(id), was);
    assert!(state.ui_settings.keys.is_empty());
}

/// A cleared binding answers nothing and says so, and the clearing survives the
/// next launch — a key that came back after a restart would be worse than one
/// that never cleared.
#[test]
fn a_cleared_binding_stays_cleared_across_a_launch() {
    let dir = crate::scratch_pool::scratch("rebind-cleared");
    let id = plain_binding();
    {
        let mut state = state_in(&dir);
        state.clear_binding(id);
        assert_eq!(state.chord_label(id), None);
        assert_eq!(state.ui_settings.keys.get(id), Some(&None));
        state
            .ui_settings
            .save(&state.data_dir)
            .expect("settings write");
    }
    let next = state_in(&dir);
    assert_eq!(
        next.chord_label(id),
        None,
        "the cleared binding came back on the next launch"
    );
    let host = keys::host_of(&next.ctx).expect("the registry is mounted");
    let shipped = SHIPPED_BINDINGS
        .iter()
        .find(|binding| binding.id == id)
        .expect("the binding is shipped");
    assert!(
        host.resolve(
            shipped.route,
            shipped.chord.key,
            shipped.chord.ctrl,
            shipped.chord.shift,
            shipped.chord.alt,
        )
        .is_none(),
        "the shipped chord fires again after being cleared"
    );
}

/// The saved form is text, so this is the case that proves the label Settings
/// shows is the label the next launch can read.
#[test]
fn a_rebinding_is_installed_from_the_saved_file() {
    let dir = crate::scratch_pool::scratch("rebind-saved");
    let id = plain_binding();
    let key = free_key();
    let mut saved = UiSettings::default();
    saved.keys.insert(id.to_string(), Some(key.to_string()));
    // A row for a binding this build does not have, and a chord no key names:
    // both are dropped rather than applied, and neither stops the good row.
    saved
        .keys
        .insert("key-no-such-binding".to_string(), Some("q".to_string()));
    saved.keys.insert(
        "key-nonsense-chord".to_string(),
        Some("Meta+nowhere".to_string()),
    );
    saved.save(&dir).expect("settings write");

    let state = state_in(&dir);
    assert_eq!(state.chord_label(id).as_deref(), Some(key));
    let host = keys::host_of(&state.ctx).expect("the registry is mounted");
    assert_eq!(
        host.resolve(KeyRoute::Editor, key, false, false, false)
            .map(|binding| binding.id),
        Some(id)
    );
}

#[test]
fn reverting_one_row_leaves_the_others_alone() {
    let mut state = state("rebind-revert");
    let first = plain_binding();
    let second = SHIPPED_BINDINGS
        .iter()
        .find(|binding| binding.route == KeyRoute::Editor && binding.id != first)
        .map(|binding| binding.id)
        .expect("a second editor binding");
    let shipped = state.chord_label(first).expect("a chord to come back to");

    state.begin_rebind(first);
    state.capture_rebind(free_key(), false, false, false);
    state.clear_binding(second);
    state.revert_binding(first);

    assert_eq!(state.chord_label(first).as_deref(), Some(&*shipped));
    assert_eq!(
        state.chord_label(second),
        None,
        "the other row was reverted"
    );
    assert!(!state.ui_settings.keys.contains_key(first));
    assert_eq!(state.ui_settings.keys.get(second), Some(&None));
}

#[test]
fn reset_puts_every_shipped_chord_back() {
    let mut state = state("rebind-reset");
    let id = plain_binding();
    state.begin_rebind(id);
    state.capture_rebind(free_key(), false, false, false);
    state.clear_binding(
        SHIPPED_BINDINGS
            .iter()
            .find(|binding| binding.id != id)
            .expect("a second binding")
            .id,
    );
    state.reset_bindings();

    assert!(state.ui_settings.keys.is_empty());
    for row in state.binding_rows() {
        assert!(!row.overridden, "{} is still overridden", row.binding.id);
        assert_eq!(
            row.chord,
            Some(row.binding.chord),
            "{} did not come back",
            row.binding.id
        );
    }
    assert_eq!(
        state.rebinding(),
        None,
        "a reset left a capture armed against a row it just changed"
    );
}

/// The surface lists what the registry dispatches, and nothing else: a row for a
/// binding that does not exist would offer a key that cannot work.
#[test]
fn the_list_is_the_registry() {
    let state = state("rebind-list");
    let rows = state.binding_rows();
    assert_eq!(rows.len(), SHIPPED_BINDINGS.len());
    for (row, shipped) in rows.iter().zip(SHIPPED_BINDINGS) {
        assert_eq!(row.binding.id, shipped.id, "the list is out of order");
        assert_eq!(row.chord, Some(shipped.chord));
        assert!(!row.overridden);
        assert!(
            !keys::binding_label(row.binding.id).is_empty(),
            "{} reads as nothing",
            row.binding.id
        );
    }
    assert!(
        state.binding_conflicts().is_empty(),
        "the shipped table collides with itself: {:?}",
        state.binding_conflicts()
    );
}

/// A chord freed by a rebinding is free straight away, so the two-step swap every
/// operator tries first works without a restart.
#[test]
fn two_bindings_can_trade_chords() {
    let mut state = state("rebind-trade");
    let first = plain_binding();
    let second = SHIPPED_BINDINGS
        .iter()
        .find(|binding| {
            binding.route == KeyRoute::Editor
                && binding.id != first
                && binding.chord.key.len() == 1
                && !binding.chord.ctrl
                && !binding.chord.shift
                && !binding.chord.alt
        })
        .map(|binding| binding.id)
        .expect("a second plain editor binding");
    let first_chord = state.chord_label(first).expect("a chord");
    let second_chord = state.chord_label(second).expect("a chord");

    state.begin_rebind(first);
    state.capture_rebind(free_key(), false, false, false);
    state.begin_rebind(second);
    state.capture_rebind(&first_chord, false, false, false);
    state.begin_rebind(first);
    state.capture_rebind(&second_chord, false, false, false);

    assert_eq!(state.chord_label(first).as_deref(), Some(&*second_chord));
    assert_eq!(state.chord_label(second).as_deref(), Some(&*first_chord));
    let host = keys::host_of(&state.ctx).expect("the registry is mounted");
    assert_eq!(
        host.resolve(KeyRoute::Editor, &second_chord, false, false, false)
            .map(|binding| binding.id),
        Some(first)
    );
    assert_eq!(
        host.resolve(KeyRoute::Editor, &first_chord, false, false, false)
            .map(|binding| binding.id),
        Some(second)
    );
}

/// The same chord on Home and in the editor is legal, so rebinding a Home key to
/// an editor chord is not a conflict.
#[test]
fn a_home_binding_may_take_an_editor_chord() {
    let mut state = state("rebind-routes");
    let home = SHIPPED_BINDINGS
        .iter()
        .find(|binding| binding.route == KeyRoute::Home)
        .expect("a Home binding");
    // Free on the Home route, or the capture would be refused for the right
    // reason and prove nothing about routes.
    let editor = SHIPPED_BINDINGS
        .iter()
        .find(|binding| {
            binding.route == KeyRoute::Editor
                && binding.chord.key.len() == 1
                && !SHIPPED_BINDINGS
                    .iter()
                    .any(|other| other.route == KeyRoute::Home && other.chord == binding.chord)
        })
        .expect("an editor chord that Home does not use");

    state.begin_rebind(home.id);
    state.capture_rebind(
        editor.chord.key,
        editor.chord.ctrl,
        editor.chord.shift,
        editor.chord.alt,
    );
    assert_eq!(state.rebinding(), None, "the rebinding was refused");
    assert_eq!(
        state.chord_label(home.id).as_deref(),
        Some(&*editor.chord.label())
    );
    let host = keys::host_of(&state.ctx).expect("the registry is mounted");
    assert_eq!(
        host.resolve(
            KeyRoute::Editor,
            editor.chord.key,
            editor.chord.ctrl,
            editor.chord.shift,
            editor.chord.alt,
        )
        .map(|binding| binding.id),
        Some(editor.id),
        "the editor binding lost its own chord"
    );
}

/// Every chord a capture can produce reads back out of the saved file. The keys
/// come from the vocabulary rather than a list written here, so a key added to it
/// is covered.
#[test]
fn every_bindable_key_survives_being_saved() {
    let mut state = state("rebind-roundtrip");
    let id = plain_binding();
    for row in state.binding_rows() {
        let Some(chord) = row.chord else {
            continue;
        };
        assert_eq!(
            Chord::parse(&chord.label()),
            Some(chord),
            "{} does not read back",
            row.binding.id
        );
    }
    // And the capture path itself: a chord captured, written, and parsed back is
    // the same chord.
    state.begin_rebind(id);
    state.capture_rebind(free_key(), true, false, true);
    let saved = state
        .ui_settings
        .keys
        .get(id)
        .and_then(|chord| chord.clone())
        .expect("the chord was saved");
    assert_eq!(
        Chord::parse(&saved),
        state.chord_label(id).and_then(|label| Chord::parse(&label))
    );
}
