//! WASM component loading, WIT world checks, hot reloading, capability denials, and fuel, deadline, and memory limits.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aria_core::Rational;
use aria_models::{Project, audio_effect_spec};
use aria_plugin::package::Package;
use aria_plugin::{CommandError, PluginDeactivated};
use aria_plugin_wasm::{Limits, LoadError, ReloadOutcome, Reloader, load};
use parking_lot::Mutex;

use super::{FIXTURE_WASM, FLUSH_TIMEOUT, SERIAL, setup_host_context, write_package};

const LEGACY_FIXTURE_WASM: &[u8] = include_bytes!("../fixtures/legacy_v1_0_plugin.wasm");

/// Denied permissions return the exact error string, record a `CapabilityDenial` in
/// `PluginContext::denials()`, and leave the plugin mounted.
#[test]
fn permission_denials_return_exact_error_and_record_capability_denial() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");

    // Case 1: `project = "none"` denies both read and write project operations.
    let pkg_none = write_package(temp.path(), "none", &[], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg_none, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let err_write = ctx
        .run_command("example.add-marker")
        .expect_err("project write must fail when project = none");
    assert_eq!(
        err_write,
        CommandError::Failed(
            "permission denied: com.example.wasm-tools: project write requires permissions.project = \"read-write\""
                .to_owned()
        )
    );
    assert_eq!(ctx.denials().len(), 1);
    assert_eq!(ctx.denials()[0].plugin, "com.example.wasm-tools");
    assert!(ctx.denials()[0].service.contains("ProjectHost"));

    let err_fs = ctx
        .run_command("example.read-unpermitted-path")
        .expect_err("unpermitted fs read must fail");
    assert_eq!(
        err_fs,
        CommandError::Failed(
            "permission denied: com.example.wasm-tools: fs read for \"/etc/hosts\" is not granted by permissions.fs"
                .to_owned()
        )
    );
    assert_eq!(ctx.denials().len(), 2);
    assert!(ctx.denials()[1].service.contains("FsPermission"));
    assert!(ctx.is_mounted("com.example.wasm-tools"));
    ctx.unmount("com.example.wasm-tools").expect("unmount");

    // Case 2: `project = "read"` permits `project-snapshot` and `editor-view`, but denies `project-undo` / `project-transact`.
    let pkg_read = write_package(temp.path(), "read", &[], FIXTURE_WASM);
    let plugin_read = load(&pkg_read, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin_read.as_ref()).expect("mount wasm plugin");

    let err_readonly = ctx
        .run_command("example.inspect-and-undo")
        .expect_err("project-undo must fail when project = read");
    assert_eq!(
        err_readonly,
        CommandError::Failed(
            "permission denied: com.example.wasm-tools: project write requires permissions.project = \"read-write\" (have \"read\")"
                .to_owned()
        )
    );
    assert_eq!(ctx.denials().len(), 1);
    assert!(ctx.denials()[0].service.contains("ProjectWritePermission"));
    ctx.unmount("com.example.wasm-tools").expect("unmount");
}

/// An infinite-loop guest export is terminated by fuel exhaustion or wall-clock epoch
/// deadline within a bounded time, marking the plugin failed and dropping all registrations.
#[test]
fn infinite_loop_guest_terminates_by_fuel_and_by_epoch_deadline_and_faults_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let deactivated: Arc<Mutex<Vec<String>>> = Arc::default();
    let deactivated_sink = Arc::clone(&deactivated);
    let _sub = ctx.on_event(move |ev: &PluginDeactivated| {
        deactivated_sink.lock().push(ev.id.to_string());
    });

    // 1. Fuel exhaustion during `export_file` on an infinite-loop guest export.
    let fuel_limits = Limits {
        fuel_per_call: 500_000,
        memory_bytes: 16 * 1024 * 1024,
        call_deadline: Duration::from_secs(5),
    };
    let plugin = load(&pkg, &fuel_limits).expect("load with fuel limit");
    ctx.mount(plugin.as_ref()).expect("mount");
    assert!(!ctx.commands().is_empty());
    assert!(!ctx.exporters().is_empty());

    let loop_project = Project::new("INFINITE_LOOP", Rational::FPS_30);
    let out_path = temp.path().join("loop.mrk");
    let started = Instant::now();
    let export_err = ctx
        .export_file("example.markers", &loop_project, &out_path)
        .expect_err("infinite loop in export must fail by fuel");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "fuel termination took too long: {:?}",
        started.elapsed()
    );
    assert!(
        export_err.to_string().contains("fuel exhausted"),
        "expected fuel exhausted error, got: {export_err}"
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.commands().is_empty(), "commands dropped on fault");
    assert!(ctx.importers().is_empty(), "importers dropped on fault");
    assert!(ctx.exporters().is_empty(), "exporters dropped on fault");
    assert!(
        audio_effect_spec("example.gain").is_none(),
        "audio effect dropped on fault"
    );
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, "com.example.wasm-tools");
    assert!(failures[0].reason.contains("fuel exhausted"));

    // 2. Wall-clock epoch deadline exhaustion (with unlimited fuel) during `command-run`.
    let deadline_limits = Limits {
        fuel_per_call: u64::MAX,
        memory_bytes: 16 * 1024 * 1024,
        call_deadline: Duration::from_millis(25),
    };
    let plugin_deadline = load(&pkg, &deadline_limits).expect("load with deadline limit");
    ctx.mount(plugin_deadline.as_ref()).expect("remount");
    assert!(ctx.failures().is_empty(), "remount clears previous failure");

    let started = Instant::now();
    let cmd_err = ctx
        .run_command("example.infinite-loop")
        .expect_err("infinite loop command must fail by epoch deadline");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "epoch deadline took too long: {elapsed:?}"
    );
    let CommandError::Failed(reason) = cmd_err else {
        panic!("expected CommandError::Failed, got {cmd_err:?}");
    };
    assert!(
        reason.contains("call deadline exceeded"),
        "expected deadline exceeded, got: {reason}"
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.commands().is_empty());
    assert!(ctx.failures()[0].reason.contains("call deadline exceeded"));

    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    assert_eq!(
        *deactivated.lock(),
        vec![
            "com.example.wasm-tools".to_owned(),
            "com.example.wasm-tools".to_owned()
        ]
    );
}

/// A memory-hungry guest exceeding `Limits::memory_bytes` is stopped at the limit,
/// marked failed for the session, and has all registrations removed.
#[test]
fn memory_hungry_guest_is_stopped_at_limit_and_faults_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let tight_memory_limits = Limits {
        fuel_per_call: 500_000_000,
        memory_bytes: 4 * 1024 * 1024,
        call_deadline: Duration::from_secs(2),
    };
    let plugin = load(&pkg, &tight_memory_limits).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount");

    let err = ctx
        .run_command("example.eat-memory")
        .expect_err("64 MiB allocation must be stopped by 4 MiB limit");
    let CommandError::Failed(reason) = err else {
        panic!("expected CommandError::Failed, got {err:?}");
    };
    assert!(
        reason.contains("wasm trap"),
        "expected wasm trap on OOM, got: {reason}"
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.commands().is_empty());
    assert!(audio_effect_spec("example.gain").is_none());
    assert_eq!(ctx.failures().len(), 1);
    assert_eq!(ctx.failures()[0].id, "com.example.wasm-tools");
}

/// `Reloader::poll` mounts discovered packages, hot-reloads a package when `plugin.wasm`
/// changes on disk (swapping its runtime behaviour), and unmounts when removed.
#[test]
fn reloader_swaps_behaviour_when_wasm_fixture_changes_on_disk() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg_dir = temp.path().join("wasm-tools");
    let pkg = write_package(&pkg_dir, "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load v1");
    ctx.mount(plugin.as_ref()).expect("mount v1");

    let mut reloader = Reloader::new(temp.path(), Limits::default());
    // No files changed since `Reloader::new`: poll is a no-op.
    assert!(reloader.poll(&ctx).is_empty());

    ctx.run_command("example.add-marker")
        .expect("run v1 command");
    assert_eq!(
        host.snapshot().timeline().markers()[0].name,
        "wasm-marker-v1"
    );

    // Patch `"wasm-marker-v1"` -> `"wasm-marker-v2"` in `plugin.wasm` on disk.
    let needle = b"wasm-marker-v1";
    let replacement = b"wasm-marker-v2";
    let mut patched = FIXTURE_WASM.to_vec();
    let pos = patched
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("DEFAULT_MARKER_NAME present in wasm data section");
    patched[pos..pos + replacement.len()].copy_from_slice(replacement);
    fs::write(pkg_dir.join("plugin.wasm"), &patched).expect("write patched plugin.wasm");

    let outcomes = reloader.poll(&ctx);
    assert_eq!(
        outcomes,
        vec![ReloadOutcome::Reloaded {
            id: "com.example.wasm-tools".to_owned(),
            dir: pkg_dir.clone(),
        }]
    );
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    // Undo the v1 marker and run `"example.add-marker"` again: the reloaded plugin creates `"wasm-marker-v2"`.
    host.undo().expect("undo v1 marker");
    ctx.run_command("example.add-marker")
        .expect("run v2 command");
    let markers = host.snapshot().timeline().markers().to_vec();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "wasm-marker-v2");

    // The plugin's event handler writes its storage under the package
    // directory on the delivery thread. Every event posted so far runs before
    // the directory goes, or a write lands mid-removal and the removal fails.
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    // Removing the package directory and polling unmounts the plugin.
    fs::remove_dir_all(&pkg_dir).expect("remove package dir");
    let removed_outcomes = reloader.poll(&ctx);
    assert_eq!(
        removed_outcomes,
        vec![ReloadOutcome::Unmounted {
            id: "com.example.wasm-tools".to_owned(),
            dir: pkg_dir,
        }]
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.commands().is_empty());
}

/// Invalid component binaries and non-WASM packages fail closed with descriptive `LoadError` variants.
#[test]
fn invalid_component_or_non_wasm_tier_is_refused_with_exact_load_error() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");

    // 1. Data-only package (no `plugin.wasm`) is refused with `LoadError::NotWasmTier`.
    let data_dir = temp.path().join("data-only");
    fs::create_dir_all(&data_dir).expect("mkdir");
    fs::write(
        data_dir.join("plugin.toml"),
        r#"[plugin]
id = "com.example.data-only"
name = "Data Only"
version = "0.1.0"
api = 1
"#,
    )
    .expect("write plugin.toml");
    let data_pkg = Package::open(&data_dir).expect("open data pkg");
    let err = load(&data_pkg, &Limits::default())
        .err()
        .expect("data tier must be refused");
    assert_eq!(err, LoadError::NotWasmTier { path: data_dir });

    // 2. Corrupted `plugin.wasm` is refused with `LoadError::Compile`.
    let bad_dir = temp.path().join("bad-wasm");
    let bad_pkg = write_package(&bad_dir, "none", &[], b"not a valid wasm component");
    let bad_err = load(&bad_pkg, &Limits::default())
        .err()
        .expect("corrupt wasm must be refused");
    match &bad_err {
        LoadError::Compile { path, .. } => assert_eq!(path, &bad_dir.join("plugin.wasm")),
        other => panic!("unexpected error: {other:?}"),
    }
}

/// A valid component that does not implement the host's world is refused by
/// `load`, before any guest code runs, with the world and the mismatch in
/// the reason: one with none of the world's exports, and one importing a host
/// function this host does not provide, as a component built for a later
/// world would. Closes "a component for
/// another WIT version loads and fails only when mounted"; `aria plugin
/// check` relies on it. It does not cover a guest that traps in `mount`.
#[test]
fn a_component_for_another_world_is_refused_at_load() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    const PREAMBLE: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
    let foreign = "next-host-call";
    let mut imports = PREAMBLE.to_vec();
    // Type section: one function type, no parameters, no result.
    imports.extend([0x07, 0x05, 0x01, 0x40, 0x00, 0x01, 0x00]);
    // Import section: a function of that type under the foreign name.
    let name_len = u8::try_from(foreign.len()).expect("short name");
    imports.extend([0x0a, name_len + 5, 0x01, 0x00, name_len]);
    imports.extend(foreign.as_bytes());
    imports.extend([0x01, 0x00]);

    for (case, bytes, names) in [
        ("empty", PREAMBLE.to_vec(), "no export `"),
        ("foreign-import", imports, foreign),
    ] {
        let dir = temp.path().join(case);
        let package = write_package(&dir, "none", &[], &bytes);
        let Err(err) = load(&package, &Limits::default()) else {
            panic!("{case}: a component for another world loaded");
        };
        let LoadError::Compile { path, reason } = &err else {
            panic!("{case}: unexpected error: {err:?}");
        };
        assert_eq!(path, &dir.join("plugin.wasm"), "{case}");
        assert!(
            reason.starts_with(&format!("does not match world {}", aria_plugin_wasm::WORLD)),
            "{case}: {reason}"
        );
        assert!(reason.contains(names), "{case}: {reason}");
    }
}

#[test]
fn legacy_v1_0_wasm_component_without_parity_exports_still_loads_and_runs() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], LEGACY_FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg, &Limits::default()).expect("load legacy @1.0.0 wasm plugin");
    ctx.mount(plugin.as_ref())
        .expect("mount legacy @1.0.0 wasm plugin");
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    ctx.run_command("example.add-marker")
        .expect("run legacy command");
    assert_eq!(host.snapshot().timeline().markers().len(), 1);

    ctx.unmount("com.example.wasm-tools")
        .expect("unmount legacy plugin");
}
