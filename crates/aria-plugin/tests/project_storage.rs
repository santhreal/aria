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

#![allow(unsafe_code)]

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

use std::sync::Arc;

use aria_core::Rational;
use aria_engine::{Engine, EngineConfig};
use aria_models::Project;
use aria_plugin::{Origin, PluginContext, ProjectHost};
use parking_lot::Mutex;

fn make_host_session() -> (PluginContext, Arc<ProjectHost>, aria_plugin::Registration) {
    let ctx = PluginContext::new();
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let host = Arc::new(ProjectHost::new(Arc::new(Mutex::new(engine)), &ctx));
    let reg = ctx.register_arc(Arc::clone(&host));
    (ctx, host, reg)
}

#[test]
fn two_projects_same_plugin_writes_in_project_a_are_not_visible_in_project_b() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path_a = dir.path().join("project_a.aria");
    let path_b = dir.path().join("project_b.aria");

    Project::new("Project A", Rational::FPS_24)
        .save_to_file(&path_a)
        .expect("save project A");
    Project::new("Project B", Rational::FPS_24)
        .save_to_file(&path_b)
        .expect("save project B");

    let (ctx, host, _reg) = make_host_session();
    host.open(&Origin::Editor, &path_a).expect("open project A");
    assert!(!host.is_dirty(), "freshly opened project A must be clean");

    ctx.storage_set("example.notes", "note", b"project-a-only")
        .expect("write plugin storage in project A");
    assert!(
        host.is_dirty(),
        "plugin storage_set must dirty the open project"
    );
    assert_eq!(
        ctx.storage_get("example.notes", "note")
            .expect("read in project A")
            .as_deref(),
        Some(&b"project-a-only"[..])
    );
    host.save(&Origin::Editor).expect("save project A");
    assert!(!host.is_dirty(), "saved project A must be clean");

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
    let project_path = dir.path().join("persisted.aria");

    let (ctx_1, host_1, _reg_1) = make_host_session();

    ctx_1
        .storage_set("example.notes", "note", b"saved-in-project-file")
        .expect("set text note");
    ctx_1
        .storage_set("example.notes", "binary", &[0x00, 0xff, 0x80, 0x7f])
        .expect("set binary blob");

    host_1
        .save_as(&Origin::Editor, &project_path)
        .expect("save_as project");

    let (ctx_2, host_2, _reg_2) = make_host_session();
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
    let project_path = dir.path().join("multi_plugin.aria");

    let (ctx_1, host_1, _reg_1) = make_host_session();
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

    let (ctx_2, host_2, _reg_2) = make_host_session();
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

    assert!(!host_2.is_dirty(), "freshly opened project must be clean");
    assert!(
        ctx_2
            .storage_delete("alpha.plugin", "shared")
            .expect("delete alpha shared")
    );
    assert!(
        host_2.is_dirty(),
        "plugin storage_delete must dirty the open project"
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
    let v3_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../aria-models/tests/fixtures/schema/schema-v3.aria");
    let working_path = dir.path().join("migrated.aria");
    std::fs::copy(&v3_fixture, &working_path).expect("copy schema-v3 fixture");
    let (ctx, host, _reg) = make_host_session();
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
fn every_storage_entry_point_validates_keys_and_fails_closed_when_no_project_host_is_registered() {
    // Derive the storage entry points from the C ABI table, C header, and WIT file at runtime
    // so adding a new `storage_*` / `storage-*` member without covering it turns this test RED.
    let tables_rs = include_str!("../../aria-plugin-abi/src/tables.rs");
    let c_abi_ops: Vec<&str> = tables_rs
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim().strip_prefix("pub ")?;
            let (name, _) = trimmed.split_once(':')?;
            name.starts_with("storage_").then_some(name.trim())
        })
        .collect();
    assert_eq!(
        c_abi_ops,
        vec![
            "storage_get",
            "storage_set",
            "storage_delete",
            "storage_keys"
        ],
        "new storage_* entry point in AriaHostV1 must be added to fail-closed coverage"
    );

    let header_h = include_str!("../../aria-plugin-abi/include/aria_plugin.h");
    let header_ops: Vec<&str> = header_h
        .lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once("(*storage_")?;
            let (suffix, _) = rest.split_once(")(")?;
            Some(suffix)
        })
        .collect();
    assert_eq!(
        header_ops,
        vec!["get", "set", "delete", "keys"],
        "aria_plugin.h storage_* entry points drifted from AriaHostV1"
    );

    let wit_src = include_str!("../../aria-plugin-wasm/wit/plugin.wit");
    let wit_ops: Vec<&str> = wit_src
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("import ")?;
            let (name, _) = rest.split_once(':')?;
            name.starts_with("storage-").then_some(name.trim())
        })
        .collect();
    assert_eq!(
        wit_ops,
        vec![
            "storage-get",
            "storage-set",
            "storage-delete",
            "storage-keys"
        ],
        "new storage-* import in plugin.wit must be added to fail-closed coverage"
    );

    let ctx = PluginContext::new();

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

    // 1. PluginContext::storage_* fails closed with an error naming why (no open project).
    for op in &c_abi_ops {
        let err = match *op {
            "storage_get" => ctx.storage_get("example.notes", "k").unwrap_err(),
            "storage_set" => ctx.storage_set("example.notes", "k", b"v").unwrap_err(),
            "storage_delete" => ctx.storage_delete("example.notes", "k").unwrap_err(),
            "storage_keys" => ctx.storage_keys("example.notes").unwrap_err(),
            other => panic!("unhandled storage op {other}"),
        };
        assert!(
            err.contains("no open project") && err.contains(op),
            "expected `{op}` error to state no open project, got: {err}"
        );
    }

    // 2. Native C ABI (`AriaHostV1::storage_*`) and Rust SDK (`aria_sdk::Host::storage_*`)
    // fail closed for every discovered entry point when no ProjectHost is registered,
    // and never create `<data_root>/plugin-data`.
    #[derive(Default)]
    struct NoProjectGuest;

    impl aria_sdk::Plugin for NoProjectGuest {
        fn mount(&self, host: &aria_sdk::Host) -> Result<(), String> {
            for cmd in [
                "noproj.storage_get",
                "noproj.storage_set",
                "noproj.storage_delete",
                "noproj.storage_keys",
            ] {
                host.register_command(cmd, cmd)?;
            }
            Ok(())
        }

        fn command_run(
            &self,
            host: &aria_sdk::Host,
            id: &str,
            _args_json: &str,
        ) -> Result<(), String> {
            match id {
                "noproj.storage_get" => host.storage_get("k").map(drop),
                "noproj.storage_set" => host.storage_set("k", b"v"),
                "noproj.storage_delete" => host.storage_delete("k"),
                "noproj.storage_keys" => host.storage_keys().map(drop),
                other => Err(format!("unknown command {other}")),
            }
        }
    }

    static NO_PROJ_RT: std::sync::LazyLock<aria_sdk::PluginRuntime<NoProjectGuest>> =
        std::sync::LazyLock::new(|| {
            let rt = aria_sdk::PluginRuntime::new(NoProjectGuest);
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe {
                rt.bind_host(
                    aria_plugin::external::host_table() as *const _,
                    aria_plugin_abi::ARIA_API_MAJOR,
                )
            });
            rt
        });
    let table = plugin_table!(NO_PROJ_RT);

    let temp = tempfile::tempdir().expect("tempdir");
    let data_root = temp.path().join("data");
    let pkg_dir = temp.path().join("test.noproj");
    std::fs::create_dir_all(&pkg_dir).expect("pkg_dir");
    std::fs::write(
        pkg_dir.join("plugin.toml"),
        r#"[plugin]
id = "test.noproj"
name = "NoProj"
version = "0.1.0"
api = "1"
publisher = "Test"
description = "Test"
license = "MIT"
"#,
    )
    .expect("write plugin.toml");
    let pkg = aria_plugin::package::Package::open(&pkg_dir).expect("open pkg");
    let plugin =
        aria_plugin::external::from_table(&pkg, table, Some(&data_root)).expect("from_table");
    ctx.mount(plugin.as_ref()).expect("mount test.noproj");

    for op in &c_abi_ops {
        let cmd = format!("noproj.{op}");
        let err = ctx
            .run_command(&cmd)
            .expect_err("storage call without ProjectHost must fail closed");
        let msg = err.to_string();
        assert!(
            msg.contains("no open project") && msg.contains(op),
            "expected `{cmd}` error to state no open project, got: {msg}"
        );
    }
    assert!(
        !data_root.join("plugin-data").exists(),
        "failed storage calls must not create <data_root>/plugin-data on disk"
    );
}
#[test]
fn project_saved_handler_storage_mutations_land_in_saved_file_and_keep_session_clean() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (ctx, host, _reg) = make_host_session();

    let ctx_for_saved = ctx.clone();
    let _saved_sub = ctx.on_event::<aria_plugin::ProjectSaved, _>(move |saved| {
        ctx_for_saved
            .storage_set(
                "example.notes",
                "saved_path",
                saved.path.to_string_lossy().as_bytes(),
            )
            .expect("write saved_path in ProjectSaved handler");
    });

    ctx.storage_set("example.notes", "body", b"hello")
        .expect("set body");
    assert!(
        host.is_dirty(),
        "storage_set before save must dirty session"
    );

    let saved_path = dir.path().join("saved_clean.aria");
    host.save_as(&Origin::Editor, &saved_path)
        .expect("save_as with ProjectSaved handler");
    assert!(
        !host.is_dirty(),
        "session must stay clean after ProjectSaved handler writes to plugin storage"
    );

    let on_disk = Project::load_from_file(&saved_path).expect("load saved_clean.aria");
    assert_eq!(
        on_disk.plugin_storage_get("example.notes", "body"),
        Some(&b"hello"[..])
    );
    assert_eq!(
        on_disk.plugin_storage_get("example.notes", "saved_path"),
        Some(saved_path.to_string_lossy().as_bytes()),
        "ProjectSaved handler storage write must land in the saved .aria file on disk"
    );
}

#[test]
fn storage_mutations_inside_command_proposed_handler_commit_without_deadlock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (ctx, host, _reg) = make_host_session();
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
