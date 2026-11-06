//! The editor's own state files are replaced, never rewritten in place.
//!
//! The class closed here: a save that truncates the file it replaces. Window
//! layout and keys (`ui-settings.json`), named workspaces, exported
//! workspaces, key sets, recents, assistant conversations, installed skills
//! and baked looks were written with `fs::write`, which empties the file
//! before the new bytes land: a crash, a kill or a full disk during the write
//! left a cut-off file, and the next launch read it as corrupt and fell back
//! to defaults without a word. Each writer now goes through
//! `aria_settings::replace_file`, which writes a synced temporary beside the
//! file and renames it over.
//!
//! The test observes the difference without a crash: a hard link taken before
//! the save still holds the old bytes after a replacement (the rename swaps
//! the directory entry) but the new bytes after an in-place write (both names
//! share the truncated inode).
//!
//! Not covered: a writer added later that bypasses `replace_file`, preset
//! files (a preset is only written under a name no listed preset holds), the
//! baked-look cache (`render_reach::write_look`, reached only through a
//! render), and the project file, which `aria-models` replaces on its own
//! path.
//!
//! The second class: `ui-settings.json` going back to an older copy. A
//! debounced write (a panel drag, a toggle) waits with the settings as they
//! were; a write made at once (a key set picked, the session ending) did not
//! cancel it, so the waiting write landed after and put the older copy back.
//! The debounced write now syncs off the UI thread, so a ledger of the last
//! generation written keeps two writes from landing out of order. Not
//! covered: two background writes racing on real threads; the ledger is
//! driven directly instead.

use std::fs;
use std::path::Path;
use std::time::Duration;

use aria_plugin::PluginContext;

use super::AppState;
use crate::keys::keymap_sets::{self, KEYMAPS_DIR, KeySet};
use crate::layouts::{self, SavedWorkspace};
use crate::scratch_pool::scratch;
use crate::settings::{OUTLINE_MIN, SAVE_DEBOUNCE_MS, UiSettings};
use crate::types::AgentEntryKind;

const OLD: &[u8] = b"the bytes on disk before the save";

/// Put `OLD` at `path`, link it, run `save`, and require that the save
/// replaced the file: the link keeps `OLD`, `path` holds something else, and
/// no temporary is left beside it.
fn assert_replaced(writer: &str, path: &Path, save: impl FnOnce()) {
    let dir = path.parent().expect("a parent");
    fs::create_dir_all(dir).expect("mkdir");
    fs::write(path, OLD).expect("seed the old file");
    let observer = dir.join(format!("{writer}.observer"));
    let _ = fs::remove_file(&observer);
    fs::hard_link(path, &observer).expect("link the old file");

    save();

    assert_eq!(
        fs::read(&observer).expect("read the link"),
        OLD,
        "{writer} rewrote {} in place",
        path.display()
    );
    assert_ne!(
        fs::read(path).expect("read the saved file"),
        OLD,
        "{writer} did not save {}",
        path.display()
    );
    let leftovers: Vec<_> = fs::read_dir(dir)
        .expect("list")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".aria-tmp-") || name.contains(".aria-backup-"))
        .collect();
    assert!(leftovers.is_empty(), "{writer} left {leftovers:?}");
}

#[test]
fn every_state_file_writer_replaces_the_file_it_saves() {
    let dir = scratch("state-files");
    let mut state = AppState::new(PluginContext::new(), dir.clone()).expect("state");
    let workspace = SavedWorkspace::new("Mine", state.panes(), state.dock_tree.clone());

    assert_replaced("ui-settings", &UiSettings::path(&dir), || {
        UiSettings::default().save(&dir).expect("save ui-settings");
    });

    let named = layouts::workspace_file_path(&dir, "Mine").expect("a workspace path");
    assert_replaced("named workspace", &named, || {
        layouts::save_named_workspace(&dir, "Mine", state.panes(), &state.dock_tree)
            .expect("save the workspace");
    });

    let exported = dir.join("exports").join("mine.json");
    assert_replaced("exported workspace", &exported, || {
        layouts::export_workspace_json(&exported, &workspace).expect("export the workspace");
    });

    let keymap = dir.join(KEYMAPS_DIR).join("Mine.toml");
    assert_replaced("key set", &keymap, || {
        keymap_sets::write_keymap(&keymap, &KeySet::new()).expect("write the key set");
    });

    let project = dir.join("project.aria");
    fs::write(&project, b"{}").expect("a project file");
    assert_replaced("recents", &dir.join(super::recents::RECENTS_FILE), || {
        super::recents::remember(&dir, &project);
    });

    state.project_path = Some(project);
    state.push_agent_message(AgentEntryKind::ALL[0], "a line to keep");
    let conversation = state.conversation_path().expect("a conversation path");
    assert_replaced("conversation", &conversation, || {
        state.save_conversation().expect("save the conversation");
    });

    let dropped = dir.join("drop").join("cut-notes").join("SKILL.md");
    fs::create_dir_all(dropped.parent().expect("a parent")).expect("mkdir");
    fs::write(
        &dropped,
        "---\nname: Cut notes\ndescription: Notes\n---\nBody.\n",
    )
    .expect("a skill file");
    let installed = state
        .agent_dir()
        .join("skills")
        .join("cut-notes")
        .join("SKILL.md");
    assert_replaced("skill", &installed, || {
        state
            .install_skill_file(&dropped)
            .expect("install the skill");
    });
}

#[gpui::test]
fn a_write_made_now_is_not_undone_by_an_older_debounced_write(cx: &mut gpui::TestAppContext) {
    let (state, visual) = crate::test_support::drawn_shell(cx, "settings-order");
    let data_dir = visual.read(|app| state.read(app).data_dir.clone());
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.ui_settings.outline_width = OUTLINE_MIN + 10.0;
            state.persist_settings_later(cx);
            state.ui_settings.outline_width = OUTLINE_MIN + 20.0;
            state.write_settings_now().expect("write now");
        })
    });
    visual
        .executor()
        .advance_clock(Duration::from_millis(SAVE_DEBOUNCE_MS + 50));
    visual.run_until_parked();
    assert_eq!(
        UiSettings::load(&data_dir).outline_width,
        OUTLINE_MIN + 20.0,
        "the debounced write put back the copy from before the write made now"
    );
}

#[test]
fn a_settings_write_older_than_the_last_one_landed_is_skipped() {
    let dir = scratch("settings-ledger");
    let written = parking_lot::Mutex::new(0);
    let at = |width: f32| UiSettings {
        outline_width: width,
        ..UiSettings::default()
    };
    let write = |generation: u64, width: f32| {
        super::selection::write_settings(&written, generation, &at(width), &dir).expect("write");
        UiSettings::load(&dir).outline_width
    };

    assert_eq!(write(2, OUTLINE_MIN + 20.0), OUTLINE_MIN + 20.0);
    assert_eq!(
        write(1, OUTLINE_MIN + 10.0),
        OUTLINE_MIN + 20.0,
        "an older write landed"
    );
    assert_eq!(
        write(2, OUTLINE_MIN + 30.0),
        OUTLINE_MIN + 30.0,
        "a rewrite of the same generation was skipped"
    );
    assert_eq!(write(3, OUTLINE_MIN + 40.0), OUTLINE_MIN + 40.0);
    assert_eq!(*written.lock(), 3);
}
