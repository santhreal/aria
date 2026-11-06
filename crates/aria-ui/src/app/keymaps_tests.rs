//! Named key sets, the move question, and dropping a row on the keyboard image,
//! driven through the state the Settings rows call.
//!
//! The classes closed here:
//!
//! - a set that does not come back as it was saved, through Settings or through
//!   the next launch. Rebindings that trade two chords were lost at launch,
//!   because the saved rows were installed one at a time in id order and the
//!   first row met the other binding still on its shipped chord;
//! - a chord taken from its holder without an answer, or a question that
//!   outlives the row it was asked for;
//! - an import or an export that writes something other than the keys in
//!   force.
//!
//! Bindings are picked out of the shipped table at run time.
//!
//! Not covered: the native file dialog, which the picker seam stands in for,
//! and the Settings draw.

use super::KeyMove;
use crate::app::picker::Answer;
use crate::app::rebind::tests::{free_key, state_in};
use crate::app::{AppState, DialogKind};
use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::keys::keyboard_image::Modifier;
use crate::keys::keymap_sets::{self, KEYMAPS_DIR};
use crate::keys::{Binding, Chord, KeyRoute};
use crate::settings::{SAVE_DEBOUNCE_MS, UiSettings};

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
        .expect("the shipped table has two plain editor chords")
}

fn state(prefix: &'static str) -> (AppState, std::path::PathBuf) {
    let dir = crate::scratch_pool::scratch(prefix);
    (state_in(&dir), dir)
}

/// Arm `id` and press the bare key `key`.
fn press(state: &mut AppState, id: &'static str, key: &str) {
    state.begin_rebind(id);
    state.capture_rebind(key, false, false, false);
}

fn label(state: &AppState, id: &str) -> Option<String> {
    state.chord_label(id)
}

/// How the free key reads as a plain chord, which is how a binding moved onto
/// it reads and is saved.
fn free() -> String {
    Chord::plain(free_key()).label()
}

/// The regression: a trade made in Settings is saved as two rows, and each
/// row's chord is the other binding's shipped one.
#[test]
fn a_traded_pair_comes_back_after_a_launch() {
    let (a, b) = (plain(0), plain(1));
    let (mut state, dir) = state("keymaps-trade-launch");
    press(&mut state, a.id, free_key());
    press(&mut state, b.id, a.chord.key);
    press(&mut state, a.id, b.chord.key);
    assert_eq!(label(&state, a.id), Some(b.chord.label()));
    assert_eq!(label(&state, b.id), Some(a.chord.label()));
    state.ui_settings.save(&dir).expect("save");

    let next = state_in(&dir);
    assert_eq!(label(&next, a.id), Some(b.chord.label()));
    assert_eq!(label(&next, b.id), Some(a.chord.label()));
}

#[test]
fn a_saved_set_loads_back_over_other_rebindings() {
    let (a, b) = (plain(0), plain(1));
    let (mut state, _) = state("keymaps-save-load");
    press(&mut state, a.id, free_key());
    state.clear_binding(b.id);
    state.save_keymap_as("  Mine ").expect("save");
    assert_eq!(&*state.keymap_sets(), ["Mine"]);
    assert_eq!(state.active_keymap(), Some("Mine"));

    state.reset_bindings();
    assert_eq!(label(&state, a.id), Some(a.chord.label()));
    state.load_keymap("Mine").expect("load");
    assert_eq!(label(&state, a.id), Some(free()));
    assert_eq!(label(&state, b.id), None);
    assert_eq!(
        state.ui_settings.keys.get(a.id),
        Some(&Some(free())),
        "the loaded set was not mirrored for the next launch"
    );
    assert_eq!(state.ui_settings.keys.get(b.id), Some(&None));
    let said = state.status_message.clone().unwrap_or_default();
    assert!(said.starts_with("Loaded Mine: 2 keys"), "{said}");
}

#[test]
fn a_set_with_a_row_this_build_lacks_loads_the_rest_and_names_the_skip() {
    let a = plain(0);
    let (mut state, dir) = state("keymaps-skip");
    let keymaps = dir.join(KEYMAPS_DIR);
    std::fs::create_dir_all(&keymaps).expect("dir");
    std::fs::write(
        keymaps.join("Odd.toml"),
        format!(
            "version = 1\n[keys]\nkey-no-such-binding = \"q\"\n{} = \"{}\"\n",
            a.id,
            free_key()
        ),
    )
    .expect("write");
    state.load_keymap("Odd").expect("load");
    assert_eq!(label(&state, a.id), Some(free()));
    let said = state.status_message.clone().unwrap_or_default();
    assert!(
        said.contains("1 key,")
            && said.contains("1 skipped")
            && said.contains("key-no-such-binding"),
        "{said}"
    );
    assert!(!state.ui_settings.keys.contains_key("key-no-such-binding"));
}

#[test]
fn a_name_that_cannot_be_a_file_writes_nothing() {
    let (mut state, dir) = state("keymaps-bad-name");
    for name in ["", "a/b", ".hidden", "a:b"] {
        assert!(state.save_keymap_as(name).is_err(), "{name:?} was saved");
    }
    assert!(
        !dir.join(KEYMAPS_DIR).exists(),
        "a refused name wrote a file"
    );
    assert_eq!(state.active_keymap(), None);
    assert!(state.load_keymap("../ui-settings").is_err());
}

#[test]
fn deleting_a_set_keeps_the_keys_in_force() {
    let a = plain(0);
    let (mut state, _) = state("keymaps-delete");
    press(&mut state, a.id, free_key());
    state.save_keymap_as("Mine").expect("save");
    state.delete_keymap("Mine").expect("delete");
    assert!(state.keymap_sets().is_empty());
    assert_eq!(state.active_keymap(), None);
    assert_eq!(label(&state, a.id), Some(free()));
    assert!(state.delete_keymap("Mine").is_err(), "a second delete");
}

#[test]
fn an_import_is_added_under_a_free_name_installed_and_saved_for_the_next_launch() {
    let a = plain(0);
    let (mut state, dir) = state("keymaps-import");
    state
        .save_keymap_as("Mine")
        .expect("an existing set of that name");
    let outside = dir.join("elsewhere").join("Mine.toml");
    let mut set = keymap_sets::KeySet::new();
    set.insert(a.id.to_owned(), Some(Chord::plain(free_key())));
    keymap_sets::write_keymap(&outside, &set).expect("write");

    state.supply_pick(Answer::file(outside));
    state.import_keymap();
    assert_eq!(state.picks_unused(), 0, "the import never asked");
    assert_eq!(state.last_error, None);
    assert_eq!(&*state.keymap_sets(), ["Mine", "Mine 2"]);
    assert_eq!(state.active_keymap(), Some("Mine 2"));
    assert_eq!(label(&state, a.id), Some(free()));
    assert_eq!(
        keymap_sets::read_keymap(&keymap_sets::keymap_path(&dir, "Mine 2").expect("path")),
        Ok(set),
    );
    let saved = UiSettings::load(&dir);
    assert_eq!(saved.keys.get(a.id), Some(&Some(free())));
    assert_eq!(saved.keymap.as_deref(), Some("Mine 2"));
}

#[test]
fn an_import_this_build_cannot_read_changes_nothing() {
    let a = plain(0);
    let (mut state, dir) = state("keymaps-import-bad");
    let outside = dir.join("Broken.toml");
    std::fs::write(&outside, format!("version = 9\n[keys]\n{} = \"q\"\n", a.id)).expect("write");
    state.supply_pick(Answer::file(outside));
    state.import_keymap();
    let err = state.last_error.clone().expect("the refusal is reported");
    assert!(err.contains("version 9"), "{err}");
    assert!(state.keymap_sets().is_empty());
    assert_eq!(label(&state, a.id), Some(a.chord.label()));
    assert!(state.ui_settings.keys.is_empty());
}

#[test]
fn an_export_writes_the_keys_in_force() {
    let (a, b) = (plain(0), plain(1));
    let (mut state, dir) = state("keymaps-export");
    press(&mut state, a.id, free_key());
    state.clear_binding(b.id);
    let out = dir.join("out").join("keys.toml");
    state.supply_pick(Answer::file(out.clone()));
    state.export_keymap();
    let host = crate::keys::host_of(&state.ctx).expect("mounted");
    assert_eq!(keymap_sets::read_keymap(&out), Ok(host.current_set()));
    assert_eq!(host.current_set().len(), 2);
    assert!(state.keymap_sets().is_empty(), "an export is not a set");
}

/// A taken chord is a question, and nothing moves until it is answered.
#[test]
fn a_taken_chord_moves_only_on_enter() {
    let (a, holder) = (plain(0), plain(1));
    let (mut state, _) = state("keymaps-move");
    press(&mut state, a.id, holder.chord.key);
    assert_eq!(
        state.pending_key_move(),
        Some(KeyMove {
            id: a.id,
            chord: holder.chord,
            holder: holder.id,
        })
    );
    assert_eq!(label(&state, holder.id), Some(holder.chord.label()));
    assert_eq!(label(&state, a.id), Some(a.chord.label()));
    assert!(state.ui_settings.keys.is_empty(), "the question was saved");

    state.capture_rebind("enter", false, false, false);
    assert_eq!(state.rebinding(), None);
    assert_eq!(state.pending_key_move(), None);
    assert_eq!(label(&state, a.id), Some(holder.chord.label()));
    assert_eq!(label(&state, holder.id), None);
    assert_eq!(
        state.ui_settings.keys.get(a.id),
        Some(&Some(holder.chord.label()))
    );
    assert_eq!(state.ui_settings.keys.get(holder.id), Some(&None));
    let said = state.status_message.clone().unwrap_or_default();
    assert!(said.contains("has no key"), "{said}");
}

#[test]
fn keep_leaves_the_holder_and_the_row_armed_and_escape_ends_it() {
    let (a, holder) = (plain(0), plain(1));
    let (mut state, _) = state("keymaps-keep");
    press(&mut state, a.id, holder.chord.key);
    state.keep_key();
    assert_eq!(state.pending_key_move(), None);
    assert_eq!(state.rebinding(), Some(a.id), "Keep ended the capture");
    assert_eq!(label(&state, holder.id), Some(holder.chord.label()));

    press(&mut state, a.id, holder.chord.key);
    state.capture_rebind("escape", false, false, false);
    assert_eq!(state.rebinding(), None);
    assert_eq!(state.pending_key_move(), None);
    assert_eq!(label(&state, holder.id), Some(holder.chord.label()));
    assert_eq!(label(&state, a.id), Some(a.chord.label()));
}

/// A question belongs to the capture it was asked in: a row disarmed and armed
/// again starts with no question, so Enter is a chord again. Revert disarms
/// without answering the question, which is the path that left it standing.
#[test]
fn a_question_does_not_outlive_its_capture() {
    let (a, holder) = (plain(0), plain(1));
    let (mut state, _) = state("keymaps-stale");
    press(&mut state, a.id, holder.chord.key);
    state.revert_binding(holder.id);
    assert_eq!(state.pending_key_move(), None, "a disarmed row still asks");
    state.begin_rebind(a.id);
    assert_eq!(state.pending_key_move(), None);
    state.capture_rebind(free_key(), false, false, false);
    assert_eq!(label(&state, a.id), Some(free()));
    assert_eq!(label(&state, holder.id), Some(holder.chord.label()));
}

#[test]
fn a_row_dropped_on_a_key_takes_it_with_the_image_modifiers() {
    let (a, holder) = (plain(0), plain(1));
    let (mut state, _) = state("keymaps-drop");
    for modifier in Modifier::ALL {
        state.toggle_key_image_modifier(modifier);
    }
    state.drop_binding_on_key(a.id, free_key());
    assert_eq!(
        label(&state, a.id),
        Some(Chord::ctrl_shift_alt(free_key()).label())
    );

    for modifier in Modifier::ALL {
        state.toggle_key_image_modifier(modifier);
    }
    state.drop_binding_on_key(a.id, holder.chord.key);
    assert_eq!(
        state.pending_key_move().map(|pending| pending.holder),
        Some(holder.id),
        "a drop on a held key moved it without asking"
    );
    assert_eq!(label(&state, holder.id), Some(holder.chord.label()));
    state.move_key();
    assert_eq!(label(&state, a.id), Some(holder.chord.label()));
}

/// A filter that is not the section's title lists the bindings whose name or
/// chord contains it, and nothing else.
#[test]
fn the_settings_filter_lists_the_bindings_it_names() {
    let a = plain(0);
    let (mut state, _) = state("keymaps-filter");
    let total = state.binding_rows().len();
    let name = state.binding_name(a.id);
    state.settings_form.filter = name.to_uppercase();
    let listing = state
        .listed_binding_rows()
        .expect("the row is listed by name");
    assert!(!listing.whole, "a binding's name matched the section title");
    assert!(listing.rows.iter().any(|(row, _)| row.binding.id == a.id));
    assert!(
        listing.rows.len() < total,
        "a name filter listed every binding"
    );

    press(&mut state, a.id, free_key());
    state.settings_form.filter = format!("  {}  ", free_key().to_uppercase());
    let listing = state
        .listed_binding_rows()
        .expect("the row is listed by chord");
    assert!(listing.rows.iter().any(|(row, _)| row.binding.id == a.id));

    for title in ["", "keyboard shortcuts", " KEY ", "Shortcuts"] {
        state.settings_form.filter = title.to_owned();
        let listing = state
            .listed_binding_rows()
            .expect("the title lists the section");
        assert!(listing.whole, "{title:?}");
        assert_eq!(listing.rows.len(), total, "{title:?}");
    }

    state.settings_form.filter = "zz-no-binding-zz".to_owned();
    assert!(state.listed_binding_rows().is_none());
}

/// A key's tooltip reads the chord under the image's modifiers, so it names
/// what pressing that chord would run.
#[test]
fn a_key_tip_names_what_the_key_holds_under_the_image_modifiers() {
    let a = plain(0);
    let (mut state, _) = state("keymaps-tip");
    let held = format!("{} (Editor)", state.binding_name(a.id));
    let tip = state.key_cap_tip(a.chord.key);
    assert!(
        tip.starts_with(&a.chord.label()) && tip.contains(&held),
        "{tip}"
    );

    state.toggle_key_image_modifier(Modifier::Ctrl);
    state.toggle_key_image_modifier(Modifier::Alt);
    let tip = state.key_cap_tip(a.chord.key);
    assert!(
        tip.starts_with(&Chord::ctrl_alt(a.chord.key).label()),
        "{tip}"
    );
    assert!(!tip.contains(&held), "{tip}");
}

/// A set copied into the keymaps directory while the app runs is listed the
/// next time Settings opens.
#[test]
fn opening_settings_reads_the_sets_again() {
    let (mut state, dir) = state("keymaps-refresh");
    let keymaps = dir.join(KEYMAPS_DIR);
    std::fs::create_dir_all(&keymaps).expect("dir");
    std::fs::write(keymaps.join("Copied.toml"), "version = 1\n").expect("write");
    assert!(state.keymap_sets().is_empty());
    state.open_dialog(DialogKind::Settings);
    assert_eq!(&*state.keymap_sets(), ["Copied"]);
}

/// The Save as field saves the keys in force under the name typed into it and
/// shows the name of the set in force.
#[test]
fn the_save_as_field_saves_a_set_under_the_typed_name() {
    use crate::app::settings_form::SettingsField;
    let a = plain(0);
    let (mut state, dir) = state("keymaps-field");
    state.open_dialog(DialogKind::Settings);
    press(&mut state, a.id, free_key());
    state.focus_setting(Some(SettingsField::KeymapName));
    for ch in "Edit keys".chars() {
        state.type_key(ch);
    }
    state.commit_setting().expect("a plain name");
    assert_eq!(state.active_keymap(), Some("Edit keys"));
    assert_eq!(
        state.setting_draft(SettingsField::KeymapName),
        ("Edit keys".to_owned(), false)
    );
    let saved =
        keymap_sets::read_keymap(&keymap_sets::keymap_path(&dir, "Edit keys").expect("path"))
            .expect("the set was written");
    assert_eq!(
        saved.get(a.id),
        Some(&Some(crate::keys::Chord::plain(free_key())))
    );

    state.focus_setting(Some(SettingsField::KeymapName));
    assert_eq!(
        state.settings_form.draft, "Edit keys",
        "the field opens on the set in force"
    );
    state.settings_form.draft = "a/b".into();
    assert!(state.commit_setting().is_err());
    assert!(
        state.setting_draft(SettingsField::KeymapName).1,
        "a refused name dropped the draft"
    );
}

/// The shell's own key handler: a taken chord asks, Enter moves it, and the
/// move is in `ui-settings.json` before the window closes.
#[gpui::test]
fn a_key_moved_from_the_keyboard_is_saved_before_the_window_closes(cx: &mut gpui::TestAppContext) {
    let (a, holder) = (plain(0), plain(1));
    let (state, visual) = crate::test_support::drawn_shell(cx, "keymaps-shell-move");
    let data_dir = visual.read(|app| state.read(app).data_dir.clone());
    visual.update(|_, app| {
        state.update(app, |state, _| {
            // A row is armed from the Settings dialog, so it is up meanwhile.
            state.open_dialog(DialogKind::Settings);
            state.begin_rebind(a.id);
        })
    });
    visual.simulate_keystrokes(holder.chord.key);
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state
            .read(app)
            .pending_key_move()
            .map(|pending| pending.holder)),
        Some(holder.id),
        "the taken chord did not ask"
    );
    visual.simulate_keystrokes("enter");
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(SAVE_DEBOUNCE_MS + 50));
    visual.run_until_parked();
    let saved = UiSettings::load(&data_dir);
    assert_eq!(saved.keys.get(a.id), Some(&Some(holder.chord.label())));
    assert_eq!(saved.keys.get(holder.id), Some(&None));
}

/// Every function in `app/keymaps.rs` whose name starts with `export_keymap`.
fn keymap_export_writers_in_source() -> std::collections::BTreeSet<String> {
    use crate::test_support::sources::{code_lines, production_sources};

    let mut writers = std::collections::BTreeSet::new();
    for (rel, code) in production_sources() {
        if rel != "app/keymaps.rs" {
            continue;
        }
        for line in code_lines(&code) {
            let trimmed = line.trim();
            for prefix in ["pub fn ", "pub(crate) fn ", "pub(super) fn ", "fn "] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let end = rest
                        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .unwrap_or(rest.len());
                    let name = &rest[..end];
                    if name.starts_with("export_keymap") {
                        writers.insert(name.to_owned());
                    }
                }
            }
        }
    }
    writers
}

/// WHY: `export_keymap` and `export_keymap_to` wrote the TOML keymap directly
/// to whatever path was chosen, so picking the open `.aria` project file or a
/// media clip in the project's pool replaced the project or its footage with
/// `version = 1\n[keys]`. Every `export_keymap*` writer in `app/keymaps.rs` is
/// derived from source at run time and tested against the project file and a
/// pool media file (direct, `..`, and `.toml` symlink spellings). Not caught:
/// overwriting an unrelated file outside the open project.
#[test]
fn every_keymap_export_writer_refuses_the_project_file_and_its_media() {
    use std::collections::BTreeSet;
    use std::path::Path;

    let derived = keymap_export_writers_in_source();
    type Runner = (
        &'static str,
        Box<dyn Fn(&mut AppState, &Path) -> Result<(), String>>,
    );
    let runners: Vec<Runner> = vec![
        (
            "export_keymap",
            Box::new(|state, path| {
                state.last_error = None;
                state.supply_pick(Answer::file(path));
                state.export_keymap();
                match state.last_error.clone() {
                    Some(err) => Err(err),
                    None => Ok(()),
                }
            }),
        ),
        (
            "export_keymap_to",
            Box::new(|state, path| state.export_keymap_to(path)),
        ),
    ];
    let covered: BTreeSet<String> = runners.iter().map(|(name, _)| (*name).to_owned()).collect();
    assert_eq!(
        covered, derived,
        "every export_keymap* writer in app/keymaps.rs must be exercised in this table"
    );

    let a = plain(0);
    let (mut state, dir) = state("keymaps-export-guard");
    press(&mut state, a.id, free_key());
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4")
        .canonicalize()
        .expect("fixture exists");
    let media = dir.join("clip.mp4");
    std::fs::copy(&fixture, &media).expect("copy media fixture");
    state.import_media(media.clone()).expect("import media");
    let project_file = dir.join("project.aria");
    state
        .save_project_to(project_file.clone())
        .expect("save project");

    let project_bytes = std::fs::read(&project_file).expect("read project");
    let media_bytes = std::fs::read(&media).expect("read media");
    let sub = dir.join("nested");
    std::fs::create_dir_all(&sub).expect("create nested dir");

    let mut targets = vec![
        (project_file.clone(), "the project file"),
        (media.clone(), "media this project uses"),
        (sub.join("..").join("project.aria"), "the project file"),
        (sub.join("..").join("clip.mp4"), "media this project uses"),
    ];
    #[cfg(unix)]
    {
        let project_link = dir.join("project-keys.toml");
        let media_link = dir.join("media-keys.toml");
        std::os::unix::fs::symlink(&project_file, &project_link).expect("symlink project");
        std::os::unix::fs::symlink(&media, &media_link).expect("symlink media");
        targets.push((project_link, "the project file"));
        targets.push((media_link, "media this project uses"));
    }

    for (writer, run) in &runners {
        for (target, what) in &targets {
            state.status_message = Some("Prior status".into());
            let err = run(&mut state, target).expect_err(&format!(
                "{writer} overwrote {what} at {}",
                target.display()
            ));
            assert_eq!(
                err,
                format!("{} is {what}; export to another file", target.display()),
                "{writer} over {what}"
            );
            assert_eq!(
                state.status_message.as_deref(),
                Some("Prior status"),
                "{writer} clobbered status_message on refused export"
            );
            assert_eq!(
                std::fs::read(&project_file).expect("read project"),
                project_bytes,
                "{writer} modified the project file via {}",
                target.display()
            );
            assert_eq!(
                std::fs::read(&media).expect("read media"),
                media_bytes,
                "{writer} modified the media file via {}",
                target.display()
            );
        }
    }
}
