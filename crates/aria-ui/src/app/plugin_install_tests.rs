//! Installing a plugin package from the editor: the prompt, its answers, and
//! every way an `.ariaplugin` reaches it.
//!
//! The class this closes: a package that runs before its requested
//! capabilities were shown, and a prompt whose answer leaves the plugin folder in
//! some third state. Each test checks the mounted set and the plugin folder on
//! disk, not only what the prompt says. Not covered: the prompt as drawn,
//! which `tests_shell.rs` opens over a real project.

use std::fs;
use std::path::{Path, PathBuf};

use aria_plugin::PluginContext;
use aria_plugin::package;

use super::*;
use crate::app::picker::Answer;
use crate::types::UiRoute;

const LOOKS: &str = "com.example.looks";
const TOOLS: &str = "com.example.tools";

fn state(name: &str) -> (AppState, PathBuf) {
    let data = crate::scratch_pool::scratch(name);
    let state = AppState::new(PluginContext::new(), data.clone()).expect("state");
    (state, data)
}

/// An archive of a data package with `permissions` (TOML) and, when `native`,
/// a library for this platform, written outside the plugin folder.
fn archive(data: &Path, id: &str, version: &str, permissions: &str, native: bool) -> PathBuf {
    let name = id.rsplit('.').next().unwrap_or(id);
    let source = data.join("source").join(format!("{name}-{version}"));
    fs::create_dir_all(&source).expect("source dir");
    let manifest = format!(
        "[plugin]\nid = {id:?}\nname = \"{}\"\nversion = {version:?}\napi = 1\n\
         publisher = \"Example\"\n{permissions}",
        title(name)
    );
    fs::write(source.join(package::MANIFEST_FILE), manifest).expect("manifest");
    if native {
        let lib = source.join(package::LIBRARY_DIR);
        fs::create_dir_all(&lib).expect("lib dir");
        fs::write(lib.join(package::library_name()), b"not a library").expect("library");
    }
    let out = data.join(format!("{name}-{version}.{}", package::ARCHIVE_EXTENSION));
    package::pack(&source, &out).expect("packs");
    out
}

fn title(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The names in the plugin folder, sorted, hidden ones included.
fn plugin_folder(state: &AppState) -> Vec<String> {
    let Ok(entries) = fs::read_dir(state.plugin_installs.installed.dir()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn mounted_version(state: &AppState, id: &str) -> Option<String> {
    state
        .ctx
        .mounted()
        .into_iter()
        .find(|info| info.manifest.id == id)
        .map(|info| info.manifest.version.to_string())
}

#[test]
fn an_archive_waits_for_the_prompt_and_installs_on_confirm() {
    let (mut state, data) = state("install-confirm");
    let looks = archive(&data, LOOKS, "1.0.0", "[permissions]\ngpu = true\n", false);

    let rest = state.take_plugin_archives(vec![looks, PathBuf::from("clip.mp4")]);

    assert_eq!(rest, vec![PathBuf::from("clip.mp4")]);
    assert_eq!(state.dialog, DialogKind::InstallPlugin);
    assert_eq!(
        state.install_prompt(),
        Some(InstallPrompt {
            name: "Looks".into(),
            id: LOOKS.into(),
            publisher: "Example".into(),
            version: "1.0.0".into(),
            replaces: None,
            tier: "data",
            permissions: vec!["Run work on the GPU".into()],
            native: false,
            waiting: 0,
        })
    );
    assert!(!state.ctx.is_mounted(LOOKS), "it ran before the answer");
    assert!(state.plugin_installs.installed.package(LOOKS).is_none());

    state.confirm_plugin_install();

    assert_eq!(state.dialog, DialogKind::None);
    assert_eq!(state.install_prompt(), None);
    assert_eq!(mounted_version(&state, LOOKS).as_deref(), Some("1.0.0"));
    assert_eq!(
        state.status_message.as_deref(),
        Some("Installed Looks 1.0.0")
    );
    assert_eq!(plugin_folder(&state), vec![LOOKS.to_owned()]);
    let row = state
        .plugin_rows()
        .into_iter()
        .find(|row| row.id == LOOKS)
        .expect("the package has a row");
    assert_eq!(row.tier, "data");
    assert!(row.installed && row.can_toggle && row.enabled);
    assert_eq!(row.permissions, vec!["Run work on the GPU"]);
    assert!(state.plugin_can_toggle(LOOKS));
}

#[test]
fn cancel_deletes_the_unpacked_files_and_shows_the_next_archive() {
    let (mut state, data) = state("install-cancel");
    let looks = archive(&data, LOOKS, "1.0.0", "", false);
    let tools = archive(&data, TOOLS, "1.0.0", "", false);

    state.install_plugin_archives(vec![looks, tools]);
    let first = state.install_prompt().expect("a prompt");
    assert_eq!((first.id.as_str(), first.waiting), (LOOKS, 1));

    state.cancel_plugin_install();

    assert_eq!(state.dialog, DialogKind::InstallPlugin);
    let second = state.install_prompt().expect("the next prompt");
    assert_eq!((second.id.as_str(), second.waiting), (TOOLS, 0));

    state.confirm_plugin_install();

    assert!(!state.ctx.is_mounted(LOOKS));
    assert!(state.ctx.is_mounted(TOOLS));
    assert_eq!(
        plugin_folder(&state),
        vec![TOOLS.to_owned()],
        "the cancelled package left files behind"
    );
}

#[test]
fn leaving_the_prompt_unanswered_installs_nothing_and_leaves_nothing() {
    let (mut state, data) = state("install-abandon");
    let looks = archive(&data, LOOKS, "1.0.0", "", false);
    let tools = archive(&data, TOOLS, "1.0.0", "", false);
    state.install_plugin_archives(vec![looks, tools]);
    assert_eq!(state.dialog, DialogKind::InstallPlugin);

    // Escape closes a dialog by opening none.
    state.open_dialog(DialogKind::None);

    assert_eq!(state.install_prompt(), None);
    assert!(!state.ctx.is_mounted(LOOKS) && !state.ctx.is_mounted(TOOLS));
    assert!(
        plugin_folder(&state).is_empty(),
        "{:?}",
        plugin_folder(&state)
    );
    // Nothing queued survives to surprise a later prompt.
    state.confirm_plugin_install();
    assert!(!state.ctx.is_mounted(TOOLS));
}

#[test]
fn an_update_asks_only_for_what_it_adds_and_installs_silently_when_nothing() {
    let (mut state, data) = state("install-update");
    let gpu = "[permissions]\ngpu = true\n";
    state.install_plugin_archives(vec![archive(&data, LOOKS, "1.0.0", gpu, false)]);
    state.confirm_plugin_install();

    // The same grant: no prompt, and the new version is mounted.
    state.install_plugin_archives(vec![archive(&data, LOOKS, "2.0.0", gpu, false)]);
    assert_eq!(state.dialog, DialogKind::None);
    assert_eq!(state.install_prompt(), None);
    assert_eq!(mounted_version(&state, LOOKS).as_deref(), Some("2.0.0"));
    assert_eq!(
        state.status_message.as_deref(),
        Some("Updated Looks to 2.0.0")
    );

    // A wider grant: the prompt names the old version and only the addition.
    let wider = "[permissions]\ngpu = true\nclipboard = true\n";
    state.install_plugin_archives(vec![archive(&data, LOOKS, "3.0.0", wider, false)]);
    let prompt = state.install_prompt().expect("the update asks");
    assert_eq!(prompt.replaces.as_deref(), Some("2.0.0"));
    assert_eq!(prompt.permissions, vec!["Read the clipboard".to_owned()]);
    assert_eq!(mounted_version(&state, LOOKS).as_deref(), Some("2.0.0"));
    state.confirm_plugin_install();
    assert_eq!(mounted_version(&state, LOOKS).as_deref(), Some("3.0.0"));
    assert_eq!(plugin_folder(&state), vec![LOOKS.to_owned()]);
}

#[test]
fn a_native_package_is_named_as_native_in_the_prompt() {
    let (mut state, data) = state("install-native");
    state.install_plugin_archives(vec![archive(&data, TOOLS, "1.0.0", "", true)]);

    let prompt = state.install_prompt().expect("a prompt");
    assert!(prompt.native);
    assert_eq!(prompt.tier, "native");
    state.cancel_plugin_install();
    assert!(plugin_folder(&state).is_empty());
}

#[test]
fn an_archive_that_does_not_unpack_is_reported_and_the_next_still_prompts() {
    let (mut state, data) = state("install-bad");
    let bad = data.join(format!("bad.{}", package::ARCHIVE_EXTENSION));
    fs::write(&bad, b"not a zip").expect("bad archive");
    let looks = archive(&data, LOOKS, "1.0.0", "", false);

    state.install_plugin_archives(vec![bad, looks]);

    let error = state
        .last_error
        .clone()
        .expect("the bad archive is reported");
    assert!(error.starts_with("Could not install a plugin"), "{error}");
    assert_eq!(
        state.install_prompt().map(|prompt| prompt.id),
        Some(LOOKS.to_owned())
    );
}

#[test]
fn the_prompt_closes_back_to_the_dialog_it_opened_over() {
    let (mut state, data) = state("install-return");
    state.open_dialog(DialogKind::Settings);
    state.install_plugin_archives(vec![archive(&data, LOOKS, "1.0.0", "", false)]);
    assert_eq!(state.dialog, DialogKind::InstallPlugin);

    state.confirm_plugin_install();

    assert_eq!(state.dialog, DialogKind::Settings);
}

#[test]
fn a_drop_on_home_installs_without_starting_a_project_or_asking_to_save() {
    let (mut state, data) = state("install-home");
    assert_eq!(state.route, UiRoute::Home);

    state.request_drop_on_home(vec![archive(&data, LOOKS, "1.0.0", "", false)]);

    assert_eq!(state.route, UiRoute::Home, "a project was started");
    assert_eq!(state.dialog, DialogKind::InstallPlugin);
    assert!(state.pending_intent().is_none());
    state.confirm_plugin_install();
    assert!(state.ctx.is_mounted(LOOKS));
    assert_eq!(state.route, UiRoute::Home);
}

#[test]
fn a_drop_in_the_editor_sends_the_archive_to_the_prompt_not_the_pool() {
    let (mut state, data) = state("install-editor-drop");
    state.new_project().expect("a project");
    let pool = state.store.media.len();

    state
        .handle_dropped_paths(vec![archive(&data, LOOKS, "1.0.0", "", false)])
        .expect("the drop is handled");

    assert_eq!(
        state.store.media.len(),
        pool,
        "the archive went to the pool"
    );
    assert_eq!(state.dialog, DialogKind::InstallPlugin);
    assert_eq!(state.last_error, None);
}

#[test]
fn file_install_plugin_asks_for_an_archive_and_prompts_for_it() {
    let (mut state, data) = state("install-pick");
    let looks = archive(&data, LOOKS, "1.0.0", "", false);
    state.supply_pick(Answer::file(&looks));

    state.pick_plugin_archive();

    assert_eq!(state.picks_asked(), 1);
    assert_eq!(
        state.install_prompt().map(|prompt| prompt.id),
        Some(LOOKS.to_owned())
    );
}

#[test]
fn uninstall_asks_first_and_removes_only_on_yes() {
    let (mut state, data) = state("install-uninstall");
    state.install_plugin_archives(vec![archive(&data, LOOKS, "1.0.0", "", false)]);
    state.confirm_plugin_install();

    state.supply_pick(Answer::Confirmed(false));
    state.uninstall_plugin(LOOKS);
    assert!(state.ctx.is_mounted(LOOKS), "no removed it");
    assert_eq!(plugin_folder(&state), vec![LOOKS.to_owned()]);

    state.supply_pick(Answer::Confirmed(true));
    state.uninstall_plugin(LOOKS);
    assert!(!state.ctx.is_mounted(LOOKS));
    assert!(plugin_folder(&state).is_empty());
    assert_eq!(state.status_message.as_deref(), Some("Uninstalled Looks"));
    assert!(state.plugin_rows().iter().all(|row| row.id != LOOKS));

    state.last_error = None;
    state.uninstall_plugin(LOOKS);
    assert!(
        state.last_error.is_some(),
        "a missing package was not reported"
    );
}

#[test]
fn disable_reload_and_enable_an_installed_package_from_its_row() {
    let (mut state, data) = state("install-toggle");
    state.install_plugin_archives(vec![archive(&data, LOOKS, "1.0.0", "", false)]);
    state.confirm_plugin_install();

    assert!(state.set_plugin_enabled(LOOKS, false));
    assert!(!state.ctx.is_mounted(LOOKS));
    state.last_error = None;
    state.reload_plugin(LOOKS);
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("disabled")),
        "{:?}",
        state.last_error
    );

    assert!(state.set_plugin_enabled(LOOKS, true));
    assert!(state.ctx.is_mounted(LOOKS));
    state.reload_plugin(LOOKS);
    assert!(state.ctx.is_mounted(LOOKS));
    assert_eq!(
        state.status_message.as_deref(),
        Some(format!("Reloaded {LOOKS}").as_str())
    );
}

/// Every WASM limit row steps past both ends of its range: the value stops at
/// the edge, reaches the config, and is what the next start reads. A config
/// that does not read is refused and the limits in force stay.
#[test]
fn each_wasm_limit_clamps_persists_and_is_read_at_the_next_start() {
    let (mut state, data) = state("install-wasm-limits");
    for (limit, _, label) in WasmLimit::ALL {
        let range = limit.range();
        for (asked, kept) in [
            (0, *range.start()),
            (u64::MAX, *range.end()),
            (*range.start() + 1, *range.start() + 1),
        ] {
            state.set_wasm_limit(limit, asked).expect("sets");
            assert_eq!(limit.value(state.wasm_limits()), kept, "{label}");
            let saved = aria_settings::load(&state.config_path()).expect("config reads");
            assert_eq!(limit.value(saved.plugin_limits), kept, "{label}");
            let reopened = InstalledPlugins::open_developing(&data, None);
            assert_eq!(limit.value(reopened.limits()), kept, "{label}");
            assert_eq!(
                state.status_message.as_deref(),
                Some(format!("{label} set to {}", limit.describe(kept)).as_str())
            );
        }
    }

    let before = state.wasm_limits();
    fs::write(state.config_path(), "[plugin_limits\n").expect("break config");
    let err = state
        .set_wasm_limit(WasmLimit::Memory, 128)
        .expect_err("an unreadable config is refused");
    assert!(err.contains("fix the file"), "{err}");
    assert_eq!(state.wasm_limits(), before);

    assert_eq!(
        WasmLimit::Fuel.describe(500_000_000),
        "500,000,000 instructions"
    );
    assert_eq!(WasmLimit::Fuel.describe(1_000), "1,000 instructions");
    assert_eq!(WasmLimit::Memory.describe(64), "64 MiB");
    assert_eq!(WasmLimit::Memory.describe(4_096), "4,096 MiB");
    assert_eq!(WasmLimit::Memory.describe(0), "0 MiB");
}
