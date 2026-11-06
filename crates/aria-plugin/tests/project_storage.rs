//! Regression suite for per-project plugin key-value storage.
//!
//! WHY: closes the defect class where `PluginContext::storage_*`, native ABI
//! `AriaHostV1::storage_*`, and WASM `storage-*` host imports read and write a
//! global `<data_dir>/plugin-data/<plugin_id>/<key>` directory instead of the
//! open `.aria` project's per-plugin store (`Project::plugin_data`), causing
//! plugin data written in one project to leak into every other project and to
//! be lost when a `.aria` project file is moved or reopened with a fresh app
//! data directory.
//!
//! Does not catch: schema v3-to-v4 JSON migration edge cases in `aria-models`
//! (covered by `crates/aria-models/tests/schema_stability.rs` and
//! `crates/aria-models/src/persist/tests.rs`) or WASM guest WIT binding layout
//! (covered by `crates/aria-plugin-wasm/tests/wasm_plugins/commands.rs`).

use std::sync::Arc;

use aria_core::Rational;
use aria_engine::{Engine, EngineConfig};
use aria_models::Project;
use aria_plugin::{Origin, PluginContext, ProjectHost};
use parking_lot::Mutex;

fn make_host_session(
    storage_root: &std::path::Path,
) -> (PluginContext, Arc<ProjectHost>, aria_plugin::Registration) {
    let ctx = PluginContext::new().with_storage_root(storage_root.to_path_buf());
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let host = Arc::new(ProjectHost::new(Arc::new(Mutex::new(engine)), &ctx));
    let reg = ctx.register_arc(Arc::clone(&host));
    (ctx, host, reg)
}

#[test]
fn two_projects_same_plugin_writes_in_project_a_are_not_visible_in_project_b() {
    let dir = tempfile::tempdir().expect("tempdir");
    let storage_root = dir.path().join("plugin-data");
    let path_a = dir.path().join("project_a.aria");
    let path_b = dir.path().join("project_b.aria");

    Project::new("Project A", Rational::FPS_24)
        .save_to_file(&path_a)
        .expect("save project A");
    Project::new("Project B", Rational::FPS_24)
        .save_to_file(&path_b)
        .expect("save project B");

    let (ctx, host, _reg) = make_host_session(&storage_root);
    host.open(&Origin::Editor, &path_a).expect("open project A");

    ctx.storage_set("example.notes", "note", b"project-a-only")
        .expect("write plugin storage in project A");
    assert_eq!(
        ctx.storage_get("example.notes", "note")
            .expect("read in project A")
            .as_deref(),
        Some(&b"project-a-only"[..])
    );
    host.save(&Origin::Editor).expect("save project A");

    host.open(&Origin::Editor, &path_b).expect("open project B");
    assert_eq!(
        ctx.storage_get("example.notes", "note")
            .expect("read in project B"),
        None,
        "plugin write from project A leaked into project B"
    );
    assert!(
        ctx.storage_keys("example.notes")
            .expect("keys in project B")
            .is_empty(),
        "plugin keys from project A leaked into project B"
    );

    ctx.storage_set("example.notes", "note", b"project-b-only")
        .expect("write plugin storage in project B");
    host.save(&Origin::Editor).expect("save project B");

    host.open(&Origin::Editor, &path_a)
        .expect("reopen project A");
    assert_eq!(
        ctx.storage_get("example.notes", "note")
            .expect("read in reopened project A")
            .as_deref(),
        Some(&b"project-a-only"[..]),
        "project B overwrite corrupted project A's plugin store"
    );
}

#[test]
fn save_and_reload_from_disk_preserves_plugin_store_in_project_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let storage_root_1 = dir.path().join("session1-plugin-data");
    let storage_root_2 = dir.path().join("session2-plugin-data");
    let project_path = dir.path().join("persisted.aria");

    let (ctx_1, host_1, _reg_1) = make_host_session(&storage_root_1);

    ctx_1
        .storage_set("example.notes", "note", b"saved-in-project-file")
        .expect("set text note");
    ctx_1
        .storage_set("example.notes", "binary", &[0x00, 0xff, 0x80, 0x7f])
        .expect("set binary blob");

    host_1
        .save_as(&Origin::Editor, &project_path)
        .expect("save_as project");

    // Open the saved `.aria` file in a brand-new session whose `storage_root` is empty.
    let (ctx_2, host_2, _reg_2) = make_host_session(&storage_root_2);
    host_2
        .open(&Origin::Editor, &project_path)
        .expect("open saved project in session 2");

    assert_eq!(
        ctx_2
            .storage_get("example.notes", "note")
            .expect("read note after reload")
            .as_deref(),
        Some(&b"saved-in-project-file"[..]),
        "plugin storage value did not survive saving and reloading the .aria project file"
    );
    assert_eq!(
        ctx_2
            .storage_get("example.notes", "binary")
            .expect("read binary blob after reload")
            .as_deref(),
        Some(&[0x00, 0xff, 0x80, 0x7f][..]),
        "binary plugin storage value did not survive saving and reloading the .aria project file"
    );
    assert_eq!(
        ctx_2
            .storage_keys("example.notes")
            .expect("keys after reload"),
        vec!["binary".to_owned(), "note".to_owned()]
    );
}

#[test]
fn two_plugins_in_one_project_are_isolated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let storage_root_1 = dir.path().join("session1-plugin-data");
    let storage_root_2 = dir.path().join("session2-plugin-data");
    let project_path = dir.path().join("multi_plugin.aria");

    let (ctx_1, host_1, _reg_1) = make_host_session(&storage_root_1);
    ctx_1
        .storage_set("alpha.plugin", "shared", b"alpha-value")
        .expect("alpha set shared");
    ctx_1
        .storage_set("alpha.plugin", "alpha-only", b"a")
        .expect("alpha set alpha-only");
    ctx_1
        .storage_set("beta.plugin", "shared", b"beta-value")
        .expect("beta set shared");
    ctx_1
        .storage_set("beta.plugin", "beta-only", b"b")
        .expect("beta set beta-only");
    host_1
        .save_as(&Origin::Editor, &project_path)
        .expect("save multi_plugin.aria");

    let (ctx_2, host_2, _reg_2) = make_host_session(&storage_root_2);
    host_2
        .open(&Origin::Editor, &project_path)
        .expect("open multi_plugin.aria");

    assert_eq!(
        ctx_2.storage_keys("alpha.plugin").expect("alpha keys"),
        vec!["alpha-only".to_owned(), "shared".to_owned()]
    );
    assert_eq!(
        ctx_2.storage_keys("beta.plugin").expect("beta keys"),
        vec!["beta-only".to_owned(), "shared".to_owned()]
    );
    assert_eq!(
        ctx_2
            .storage_get("alpha.plugin", "shared")
            .expect("alpha shared")
            .as_deref(),
        Some(&b"alpha-value"[..])
    );
    assert_eq!(
        ctx_2
            .storage_get("beta.plugin", "shared")
            .expect("beta shared")
            .as_deref(),
        Some(&b"beta-value"[..])
    );

    assert!(
        ctx_2
            .storage_delete("alpha.plugin", "shared")
            .expect("delete alpha shared")
    );
    assert_eq!(
        ctx_2
            .storage_get("alpha.plugin", "shared")
            .expect("alpha shared after delete"),
        None
    );
    assert_eq!(
        ctx_2
            .storage_get("beta.plugin", "shared")
            .expect("beta shared after alpha delete")
            .as_deref(),
        Some(&b"beta-value"[..])
    );
}
#[test]
fn previous_schema_version_project_file_loads_with_empty_plugin_store_and_saves_as_v4() {
    let dir = tempfile::tempdir().expect("tempdir");
    let storage_root = dir.path().join("plugin-data");
    let v3_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../aria-models/tests/fixtures/schema/schema-v3.aria");
    let working_path = dir.path().join("migrated.aria");
    std::fs::copy(&v3_fixture, &working_path).expect("copy schema-v3 fixture");
    let (ctx, host, _reg) = make_host_session(&storage_root);
    host.open(&Origin::Editor, &working_path)
        .expect("open schema v3 project file");
    assert!(
        ctx.storage_keys("example.notes")
            .expect("keys on migrated v3 project")
            .is_empty()
    );

    ctx.storage_set("example.notes", "migrated", b"v3-to-v4")
        .expect("write plugin storage after v3 migration");
    host.save(&Origin::Editor).expect("save migrated project");

    let reloaded = Project::load_from_file(&working_path).expect("reload saved v4 project");
    assert_eq!(reloaded.schema.version, aria_models::PROJECT_SCHEMA_VERSION);
    assert_eq!(
        reloaded.plugin_storage_get("example.notes", "migrated"),
        Some(&b"v3-to-v4"[..])
    );
}
#[test]
fn storage_validates_keys_and_falls_back_to_storage_root_when_no_project_host_is_registered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let storage_root = dir.path().join("fallback-plugin-data");
    let ctx = PluginContext::new().with_storage_root(storage_root.clone());

    for bad_key in ["", ".", "..", "a/b", "a\\b", "bad\0key"] {
        assert!(
            ctx.storage_get("example.notes", bad_key).is_err(),
            "storage_get must reject invalid key {bad_key:?}"
        );
        assert!(
            ctx.storage_set("example.notes", bad_key, b"x").is_err(),
            "storage_set must reject invalid key {bad_key:?}"
        );
        assert!(
            ctx.storage_delete("example.notes", bad_key).is_err(),
            "storage_delete must reject invalid key {bad_key:?}"
        );
    }

    ctx.storage_set("example.notes", "fallback", b"disk-bytes")
        .expect("fallback set");
    assert_eq!(
        std::fs::read(storage_root.join("example.notes").join("fallback"))
            .expect("fallback file on disk"),
        b"disk-bytes"
    );
    assert_eq!(
        ctx.storage_get("example.notes", "fallback")
            .expect("fallback get")
            .as_deref(),
        Some(&b"disk-bytes"[..])
    );
    assert_eq!(
        ctx.storage_keys("example.notes").expect("fallback keys"),
        vec!["fallback".to_owned()]
    );
    assert!(
        ctx.storage_delete("example.notes", "fallback")
            .expect("fallback delete")
    );
    assert!(!storage_root.join("example.notes").join("fallback").exists());
}
#[test]
fn storage_mutations_inside_command_proposed_handler_commit_without_deadlock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let storage_root = dir.path().join("plugin-data");
    let (ctx, host, _reg) = make_host_session(&storage_root);
    host.storage_set("example.notes", "stale", b"remove-me".to_vec());

    let ctx_for_handler = ctx.clone();
    let _hook = ctx.on_waterfall::<aria_plugin::CommandProposed, _>(move |_event| {
        ctx_for_handler
            .storage_set("example.notes", "seen_in_proposing", b"1")
            .expect("set in CommandProposed");
        assert_eq!(
            ctx_for_handler
                .storage_get("example.notes", "seen_in_proposing")
                .expect("get in CommandProposed")
                .as_deref(),
            Some(&b"1"[..])
        );
        assert!(
            ctx_for_handler
                .storage_delete("example.notes", "stale")
                .expect("delete in CommandProposed")
        );
    });

    let saved_path = dir.path().join("proposed.aria");
    host.save_as(&Origin::Editor, &saved_path)
        .expect("save_as while CommandProposed handler mutates plugin storage");
    assert_eq!(
        ctx.storage_get("example.notes", "seen_in_proposing")
            .expect("get after dispatch")
            .as_deref(),
        Some(&b"1"[..])
    );
    assert_eq!(
        ctx.storage_get("example.notes", "stale")
            .expect("get deleted key after dispatch"),
        None
    );
}
