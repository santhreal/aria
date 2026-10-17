//! Named key sets: the file format, the names, and installing a set over the
//! shipped table.
//!
//! The class closed here is a saved set that does not come back as it was
//! saved. Three ways were open: a file read with a different chord than it was
//! written with, a name that escapes the keymaps directory, and an install that
//! applies rows one at a time, so a set that trades two chords loses whichever
//! row is applied first. The last one was live in the launch path before sets
//! existed: `ui-settings.json` rows were installed one at a time in id order.
//!
//! Every binding is picked out of the shipped table at run time, so a chord
//! that moves later does not make these fail for the wrong reason.
//!
//! Not covered: the Settings rows that call these, which `app/keymaps_tests.rs`
//! drives through the state.

use super::*;
use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::keys::{Binding, KeyRoute};

/// A registry holding the shipped table.
fn host() -> KeyHost {
    let host = KeyHost::default();
    for binding in SHIPPED_BINDINGS {
        host.add(*binding);
    }
    host
}

/// The `n`th editor binding whose shipped chord is a plain single letter.
fn plain(n: usize) -> &'static Binding {
    SHIPPED_BINDINGS
        .iter()
        .filter(|binding| {
            binding.route == KeyRoute::Editor
                && !binding.chord.ctrl
                && !binding.chord.shift
                && !binding.chord.alt
                && binding.chord.key.len() == 1
        })
        .nth(n)
        .expect("the shipped table has three plain editor chords")
}

/// A chord no shipped editor binding holds.
fn free_chord() -> Chord {
    aria_plugin::keys::BINDABLE_KEYS
        .iter()
        .copied()
        .map(Chord::ctrl_shift_alt)
        .find(|chord| {
            !SHIPPED_BINDINGS
                .iter()
                .any(|binding| binding.route == KeyRoute::Editor && binding.chord == *chord)
        })
        .expect("a Ctrl+Shift+Alt chord is free")
}

/// Which binding a chord dispatches on the editor route.
fn on(host: &KeyHost, chord: Chord) -> Option<&'static str> {
    host.resolve(
        KeyRoute::Editor,
        chord.key,
        chord.ctrl,
        chord.shift,
        chord.alt,
    )
    .map(|binding| binding.id)
}

#[test]
fn a_set_reads_back_as_it_was_written() {
    let mut set = KeySet::new();
    set.insert(plain(0).id.to_owned(), Some(free_chord()));
    set.insert(plain(1).id.to_owned(), None);
    // Every key the vocabulary names, with every modifier, so a key whose label
    // the parser reads differently is caught.
    for (index, key) in aria_plugin::keys::BINDABLE_KEYS.iter().enumerate() {
        set.insert(
            format!("key-probe-{index}"),
            Some(Chord::ctrl_shift_alt(*key)),
        );
    }
    let text = render_keymap(&set).expect("render");
    assert_eq!(parse_keymap(&text), Ok(set.clone()), "{text}");

    let dir = crate::scratch_pool::scratch("keymap-roundtrip");
    let path = keymap_path(&dir, "My keys").expect("a plain name");
    write_keymap(&path, &set).expect("write");
    assert_eq!(read_keymap(&path), Ok(set));
    assert_eq!(keymap_names(&dir), Ok(vec!["My keys".to_owned()]));
}

#[test]
fn a_file_this_build_cannot_read_is_refused_whole_with_its_reason() {
    let good = format!("[keys]\n{} = \"{}\"\n", plain(0).id, free_chord().label());
    let cases: &[(String, &str)] = &[
        (format!("version = 2\n{good}"), "version 2"),
        (good.clone(), "no version"),
        (
            format!("version = 1\nextra = true\n{good}"),
            "not a keymap file",
        ),
        (
            format!("version = 1\n[keys]\n{} = \"Hyper+q\"\n", plain(0).id),
            "names no key",
        ),
        (
            format!("version = 1\n[keys]\n{} = \"Ctrl+\"\n", plain(0).id),
            "names no key",
        ),
        (
            format!("version = 1\n[keys]\n{} = 7\n", plain(0).id),
            "not a keymap file",
        ),
        ("version = \"1\"\n".to_owned(), "not a keymap file"),
    ];
    for (text, reason) in cases {
        let err = parse_keymap(text).expect_err(text);
        assert!(err.contains(reason), "{text:?} was refused as {err:?}");
    }
    assert!(parse_keymap(&format!("version = 1\n{good}")).is_ok());
}

#[test]
fn a_file_over_the_size_cap_is_refused_unread() {
    let dir = crate::scratch_pool::scratch("keymap-cap");
    let path = dir.join("big.toml");
    let filler = "#".repeat(MAX_KEYMAP_BYTES as usize + 1);
    std::fs::write(&path, format!("version = 1\n{filler}\n")).expect("write");
    let err = read_keymap(&path).expect_err("over the cap");
    assert!(err.contains("larger than"), "{err}");
}

#[test]
fn a_name_that_cannot_be_a_file_in_the_keymaps_directory_is_refused() {
    let dir = std::path::Path::new("data");
    for name in [
        "", "   ", ".hidden", "a/b", "a\\b", "a:b", "a\nb", "a\0b", "../up",
    ] {
        assert!(keymap_path(dir, name).is_err(), "{name:?} was accepted");
    }
    let long = "k".repeat(MAX_NAME_BYTES + 1);
    assert!(
        keymap_path(dir, &long).is_err(),
        "a 65-byte name was accepted"
    );
    let edge = "k".repeat(MAX_NAME_BYTES);
    assert_eq!(
        keymap_path(dir, &edge),
        Ok(dir.join(KEYMAPS_DIR).join(format!("{edge}.toml")))
    );
    assert_eq!(
        keymap_path(dir, "  Editing  "),
        Ok(dir.join(KEYMAPS_DIR).join("Editing.toml")),
        "the name is trimmed"
    );
}

#[test]
fn the_names_are_the_toml_files_sorted_without_regard_to_case() {
    let dir = crate::scratch_pool::scratch("keymap-names");
    assert_eq!(keymap_names(&dir), Ok(Vec::new()), "a missing directory");
    let keymaps = dir.join(KEYMAPS_DIR);
    std::fs::create_dir_all(&keymaps).expect("dir");
    for file in ["beta.toml", "Alpha.toml", "gamma.txt", ".hidden.toml"] {
        std::fs::write(keymaps.join(file), "version = 1\n").expect("write");
    }
    assert_eq!(
        keymap_names(&dir),
        Ok(vec!["Alpha".to_owned(), "beta".to_owned()])
    );
    delete_keymap(&dir, "beta").expect("delete");
    assert_eq!(keymap_names(&dir), Ok(vec!["Alpha".to_owned()]));
    assert!(delete_keymap(&dir, "beta").is_err(), "a second delete");
}

/// The regression: the set trades two chords, so applying either row first
/// meets the other binding still on its shipped chord.
#[test]
fn a_set_that_trades_two_chords_installs_both() {
    let (a, b) = (plain(0), plain(1));
    for order in [[a, b], [b, a]] {
        let host = host();
        let mut set = KeySet::new();
        set.insert(order[0].id.to_owned(), Some(order[1].chord));
        set.insert(order[1].id.to_owned(), Some(order[0].chord));
        assert_eq!(host.install_set(&set), Vec::new());
        assert_eq!(on(&host, order[1].chord), Some(order[0].id));
        assert_eq!(on(&host, order[0].chord), Some(order[1].id));
        assert_eq!(host.current_set(), set);
    }
}

#[test]
fn a_row_for_a_binding_this_build_lacks_is_skipped_and_the_rest_install() {
    let host = host();
    let mut set = KeySet::new();
    set.insert("key-no-such-binding".to_owned(), Some(free_chord()));
    set.insert(plain(0).id.to_owned(), Some(free_chord()));
    assert_eq!(
        host.install_set(&set),
        vec![KeySetSkip {
            id: "key-no-such-binding".to_owned(),
            reason: SkipReason::Unknown,
        }]
    );
    assert_eq!(on(&host, free_chord()), Some(plain(0).id));
}

/// A chord held by a binding the set does not move is a real conflict: the row
/// is refused, the holder keeps it, and the refused binding keeps its own key.
#[test]
fn a_row_on_a_chord_an_unmoved_binding_holds_is_refused_and_keeps_its_key() {
    let (a, holder) = (plain(0), plain(2));
    let host = host();
    let mut set = KeySet::new();
    set.insert(a.id.to_owned(), Some(holder.chord));
    assert_eq!(
        host.install_set(&set),
        vec![KeySetSkip {
            id: a.id.to_owned(),
            reason: SkipReason::Taken {
                chord: holder.chord,
                held_by: holder.id,
            },
        }]
    );
    assert_eq!(on(&host, holder.chord), Some(holder.id));
    assert_eq!(host.chord_of(a.id), Some(a.chord));
    assert!(host.overrides().is_empty(), "the refusal left a rebinding");
}

/// The refused binding cannot go back to its shipped chord when the set gave
/// that chord to another binding: two bindings on one chord is the state the
/// registry exists to prevent.
#[test]
fn a_refused_row_whose_own_chord_the_set_gave_away_is_left_with_no_key() {
    let (a, b, holder) = (plain(0), plain(1), plain(2));
    let host = host();
    let mut set = KeySet::new();
    set.insert(a.id.to_owned(), Some(holder.chord));
    set.insert(b.id.to_owned(), Some(a.chord));
    let skipped = host.install_set(&set);
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!(on(&host, a.chord), Some(b.id));
    assert_eq!(host.chord_of(a.id), None);
    assert_eq!(on(&host, holder.chord), Some(holder.id));
}

#[test]
fn installing_a_set_replaces_every_rebinding_it_does_not_name() {
    let host = host();
    host.set_override(plain(0).id, None).expect("clear");
    let mut set = KeySet::new();
    set.insert(plain(1).id.to_owned(), None);
    assert_eq!(host.install_set(&set), Vec::new());
    assert_eq!(host.chord_of(plain(0).id), Some(plain(0).chord));
    assert_eq!(host.chord_of(plain(1).id), None);
    assert_eq!(host.current_set(), set);
}
