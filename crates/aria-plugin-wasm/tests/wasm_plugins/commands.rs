//! WASM command execution, project queries, event subscriptions, background jobs, and AI tools.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use aria_commands::{Command, EditCommand};
use aria_core::{Rational, RationalTime, TimeRange};
use aria_models::{ClipId, Generator, MarkerColor, TrackKind};
use aria_plugin::package::Package;
use aria_plugin::{
    AiToolError, AiToolOutput, AiToolTier, CommandError, DropAction, JobManager, JobSnapshot,
    JobState, Origin, PlayheadMoved, ResolveDropped, SettingsService, TransactError, ViewFrame,
};
use aria_plugin_wasm::{Limits, load};

use super::{FIXTURE_WASM, FLUSH_TIMEOUT, SERIAL, setup_host_context, write_package};

/// Mounting a WASM package registers commands, importers, exporters, audio effects,
/// and event subscriptions; running commands edits the project, reads snapshots/views/settings,
/// undoes edits, and writes plugin storage.
#[test]
fn command_execution_settings_snapshot_undo_and_storage_work_end_to_end() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");
    assert!(ctx.is_mounted("com.example.wasm-tools"));
    let logs = ctx.plugin_logs("com.example.wasm-tools");
    assert!(
        logs.iter().any(|line| {
            line.level == aria_plugin::LogLevel::Info
                && &*line.text == "mounting com.example.wasm-tools"
        }),
        "expected Info log 'mounting com.example.wasm-tools', got: {logs:?}"
    );

    let example_pkg_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wasm-plugin");
    let example_pkg = Package::open(&example_pkg_dir).expect("open examples/wasm-plugin package");
    let declared_marker =
        example_pkg.command_spec("example.add-marker".to_owned(), "Add Marker".to_owned());
    assert!(declared_marker.icon.is_some());
    assert_eq!(
        declared_marker
            .enablement
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("!playing")
    );

    let add_marker_spec = ctx
        .commands()
        .into_iter()
        .find(|s| s.id == "example.add-marker")
        .expect("registered example.add-marker spec");
    assert!(add_marker_spec.icon.is_some());
    assert_eq!(
        add_marker_spec
            .enablement
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("!playing")
    );

    // Configure setting `example.marker_color` to `"pink"` and run `example.add-marker`.
    settings.set("example.marker_color", serde_json::json!("pink"));
    ctx.run_command("example.add-marker")
        .expect("run example.add-marker");

    let snap = host.snapshot();
    let markers = snap.timeline().markers();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "wasm-marker-v1");
    assert_eq!(markers[0].color, MarkerColor::Pink);
    assert_eq!(markers[0].tick, RationalTime::new(24, Rational::FPS_24));

    // Verify receipt was written to plugin storage (`<pkg>/plugin-data/com.example.wasm-tools/last-receipt.json`).
    let data_dir = pkg.data_dir(temp.path());
    let receipt_bytes = fs::read(data_dir.join("last-receipt.json")).expect("receipt stored");
    let receipt_json: serde_json::Value =
        serde_json::from_slice(&receipt_bytes).expect("valid receipt json");
    assert_eq!(receipt_json["revision"], 1);

    // Run `example.inspect-and-undo`, which calls `project-snapshot`, `editor-view`, and `project-undo`.
    ctx.run_command("example.inspect-and-undo")
        .expect("run inspect-and-undo");
    assert!(
        host.snapshot().timeline().markers().is_empty(),
        "project-undo reverted the marker addition"
    );
    let inspection_bytes =
        fs::read(data_dir.join("last-inspection.json")).expect("inspection stored");
    let inspection: serde_json::Value =
        serde_json::from_slice(&inspection_bytes).expect("valid inspection json");
    assert_eq!(inspection["undid"], true);
    assert_eq!(inspection["playhead"]["value"], 15);

    // Run `example.storage-roundtrip` to verify `storage-set`, `storage-get`, `storage-keys`, `storage-delete`.
    ctx.run_command("example.storage-roundtrip")
        .expect("run storage-roundtrip");
    assert!(!data_dir.join("alpha").exists(), "alpha was deleted");
    assert_eq!(
        fs::read(data_dir.join("beta")).expect("beta exists"),
        b"two"
    );
    let check_json: serde_json::Value = serde_json::from_slice(
        &fs::read(data_dir.join("storage-check.json")).expect("storage-check.json"),
    )
    .expect("valid json");
    let keys = check_json["keys_before_delete"]
        .as_array()
        .expect("keys array")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    assert!(keys.contains(&"alpha") && keys.contains(&"beta"));

    // Registration outside `mount` is refused without faulting the plugin.
    let late = ctx.run_command("example.late-register");
    assert_eq!(
        late,
        Err(CommandError::Failed(
            "com.example.wasm-tools: register-command is permitted only during mount".to_owned()
        ))
    );
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    ctx.unmount("com.example.wasm-tools").expect("unmount");
}

/// Subscribed notification, waterfall, and serial events are delivered to the WASM guest.
#[test]
fn event_subscriptions_handle_notifications_waterfalls_and_serial_claims() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    // Post PlayheadMoved and flush the event bus.
    ctx.events().post(PlayheadMoved {
        at: RationalTime::new(60, Rational::FPS_24),
    });
    assert!(ctx.events().flush(FLUSH_TIMEOUT));

    let data_dir = pkg.data_dir(temp.path());
    let count_str = fs::read_to_string(data_dir.join("event-count")).expect("event-count written");
    let count: u64 = count_str.trim().parse().expect("u64 count");
    assert!(count >= 1);

    // Waterfall `CommandProposed`: a marker named `"veto-by-wasm"` is refused by the guest.
    let vetoed = host.transact(
        Origin::Editor,
        "Add marker",
        vec![Command::Edit(EditCommand::AddMarker {
            at: RationalTime::new(10, Rational::FPS_24),
            name: "veto-by-wasm".to_owned(),
            color: Some(MarkerColor::Red),
        })],
    );
    assert_eq!(
        vetoed,
        Err(TransactError::Refused {
            label: "Add marker",
            reason: "vetoed by wasm plugin".to_owned(),
        })
    );

    // Serial `ResolveDropped`: dropping a `.mrk` path is claimed as `ImportMedia`,
    // and dropping an `https://` URL is claimed as `Handled`.
    let claimed: Option<DropAction> = ctx
        .events()
        .serial(&ResolveDropped::path(temp.path().join("markers.mrk")));
    assert_eq!(claimed, Some(DropAction::ImportMedia));
    let claimed_url: Option<DropAction> = ctx
        .events()
        .serial(&ResolveDropped::url("https://example.invalid/clip"));
    assert_eq!(claimed_url, Some(DropAction::Handled));
    ctx.unmount("com.example.wasm-tools").expect("unmount");
}

/// The job labelled `label` on `manager` once `done` holds for it, within
/// five seconds.
fn wait_job(manager: &JobManager, label: &str, done: impl Fn(&JobSnapshot) -> bool) -> JobSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let found = manager
            .snapshot()
            .into_iter()
            .find(|job| job.label == label);
        if let Some(job) = &found
            && done(job)
        {
            return found.expect("found above");
        }
        assert!(
            Instant::now() < deadline,
            "job {label:?} never reached the state: {found:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A job a WASM command starts steps on the host's jobs board under the
/// plugin's name to its last step, and a job that steps until cancelled ends
/// cancelled with `job-end` told so.
#[test]
fn wasm_jobs_step_to_done_and_end_cancelled_when_cancelled() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (mut ctx, _host, _view, _settings) = setup_host_context();
    let manager = Arc::new(JobManager::new());
    let held = ctx.register_arc(Arc::clone(&manager));
    ctx._services.push(held);
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");
    let data_dir = pkg.data_dir(temp.path());
    let stored = |key: &str| fs::read_to_string(data_dir.join(key)).expect("stored by the guest");

    ctx.run_command("example.count-job")
        .expect("starts the job");
    let counted = wait_job(&manager, "Example WASM Tools: count", |job| {
        job.state.is_terminal()
    });
    assert_eq!(counted.state, JobState::Done);
    assert_eq!(counted.detail, "counted to 4");
    assert_eq!(counted.progress, Some(1.0));
    assert_eq!(stored("job-end"), format!("{}:false", stored("last-job")));

    ctx.run_command("example.endless-job")
        .expect("starts the job");
    let running = wait_job(&manager, "Example WASM Tools: endless", |job| {
        job.detail.starts_with("step ")
    });
    assert_eq!(
        running.progress, None,
        "a step without a fraction stays indeterminate"
    );
    assert!(manager.cancel(running.id));
    let ended = wait_job(&manager, "Example WASM Tools: endless", |job| {
        job.state.is_terminal()
    });
    assert_eq!(ended.state, JobState::Cancelled);
    assert_eq!(stored("job-end"), format!("{}:true", stored("last-job")));
    assert!(ctx.is_mounted("com.example.wasm-tools"));
}

/// The component's AI tools are listed as `plugin_<name>` with their tier and
/// schema, a Workspace tool edits the project through `project-transact`, a
/// ReadOnly tool returns its PNG, a refusal reaches the caller as written, and
/// the tools leave the list when the plugin unmounts.
#[test]
fn wasm_ai_tools_list_edit_the_project_return_images_and_leave_on_unmount() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let listed: Vec<_> = ctx
        .ai_tools()
        .into_iter()
        .map(|tool| (tool.call_name, tool.plugin_name.to_string(), tool.spec.tier))
        .collect();
    assert_eq!(
        listed,
        [
            (
                "plugin_example_add_marker".to_owned(),
                "Example WASM Tools".to_owned(),
                AiToolTier::Workspace
            ),
            (
                "plugin_example_swatch".to_owned(),
                "Example WASM Tools".to_owned(),
                AiToolTier::ReadOnly
            ),
        ]
    );
    let add = ctx
        .ai_tool("plugin_example_add_marker")
        .expect("add marker is listed");
    assert_eq!(
        add.spec.parameters["required"],
        serde_json::json!(["frame"])
    );

    let cancel = AtomicBool::new(false);
    let output = ctx
        .call_ai_tool(
            "plugin_example_add_marker",
            &serde_json::json!({ "frame": 48, "name": "from the assistant" }),
            AiToolTier::Workspace,
            &cancel,
        )
        .expect("add marker runs");
    assert_eq!(
        output,
        AiToolOutput::text("added marker from the assistant at frame 48")
    );
    let snap = host.snapshot();
    let markers = snap.timeline().markers();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "from the assistant");
    assert_eq!(markers[0].tick, RationalTime::new(48, Rational::FPS_24));

    let swatch = ctx
        .call_ai_tool(
            "plugin_example_swatch",
            &serde_json::json!({}),
            AiToolTier::ReadOnly,
            &cancel,
        )
        .expect("swatch runs");
    assert_eq!(swatch.text, "the example swatch");
    assert_eq!(swatch.images.len(), 1);
    assert_eq!(swatch.images[0].label, "swatch");
    assert_eq!(
        swatch.images[0].png[12..24],
        *b"IHDR\0\0\0\x01\0\0\0\x01",
        "a 1x1 PNG"
    );

    assert_eq!(
        ctx.call_ai_tool(
            "plugin_example_add_marker",
            &serde_json::json!({ "name": "no frame" }),
            AiToolTier::Workspace,
            &cancel,
        ),
        Err(AiToolError::Failed(
            "frame must be a whole number".to_owned()
        ))
    );
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    ctx.unmount("com.example.wasm-tools").expect("unmount");
    assert!(ctx.ai_tools().is_empty());
}

#[test]
fn wasm_project_queries_round_trip_and_refuse_without_read_permission() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let (ctx, host, view, _settings) = setup_host_context();

    host.apply(
        &Origin::Editor,
        Command::Edit(EditCommand::AddTrack {
            kind: TrackKind::Sticker,
            name: "S1".to_owned(),
            index: None,
            pinned: false,
        }),
    )
    .expect("add sticker track");
    let track_id = host
        .snapshot()
        .timeline()
        .tracks_ordered()
        .find(|t| t.kind == TrackKind::Sticker)
        .expect("sticker track")
        .id;
    host.apply(
        &Origin::Editor,
        Command::Edit(EditCommand::AddGenerated {
            track: track_id,
            generator: Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            timeline: TimeRange::at_rate(0, 48, Rational::FPS_24),
        }),
    )
    .expect("seed clip");
    let seeded_clip_id = host
        .snapshot()
        .timeline()
        .track(track_id)
        .expect("track")
        .clips_ordered()
        .next()
        .expect("seeded clip")
        .id;
    host.apply(
        &Origin::Editor,
        Command::Edit(EditCommand::AddMarker {
            at: RationalTime::new(12, Rational::FPS_24),
            name: "WasmSeedMarker".to_owned(),
            color: Some(MarkerColor::Teal),
        }),
    )
    .expect("seed marker");
    view.publish(
        ViewFrame {
            selection: &[seeded_clip_id, ClipId::from_raw(999_999)],
            playhead: RationalTime::new(12, Rational::FPS_24),
            mark_in: Some(RationalTime::new(4, Rational::FPS_24)),
            mark_out: Some(RationalTime::new(36, Rational::FPS_24)),
            playing: false,
            speed: 1,
        },
        ctx.events(),
    );

    // 1. With `project = "none"`, `example.query-project` is refused and records a CapabilityDenial.
    let pkg_none = write_package(&temp.path().join("none"), "none", &[], FIXTURE_WASM);
    let plugin_none = load(&pkg_none, &Limits::default()).expect("load wasm plugin (none)");
    ctx.mount(plugin_none.as_ref()).expect("mount wasm plugin");
    let err = ctx
        .run_command("example.query-project")
        .expect_err("query-project must fail when project = none");
    assert!(
        err.to_string().contains("permission denied"),
        "unexpected error: {err}"
    );
    assert_eq!(ctx.denials().len(), 1);
    assert!(ctx.denials()[0].service.contains("ProjectHost"));
    ctx.unmount("com.example.wasm-tools").expect("unmount");

    // 2. With `project = "read"`, `example.query-project` succeeds and writes exact query results.
    let pkg_read = write_package(&temp.path().join("read"), "read", &[], FIXTURE_WASM);
    let plugin_read = load(&pkg_read, &Limits::default()).expect("load wasm plugin (read)");
    ctx.mount(plugin_read.as_ref()).expect("mount wasm plugin");
    ctx.run_command("example.query-project")
        .expect("query-project allowed under read");
    assert!(ctx.denials().is_empty());

    let data_dir = pkg_read.data_dir(&temp.path().join("read"));
    let summary_bytes = fs::read(data_dir.join("last-queries.json")).expect("last-queries.json");
    let summary: serde_json::Value =
        serde_json::from_slice(&summary_bytes).expect("valid summary json");
    let cid = seeded_clip_id.raw();
    assert_eq!(summary["in_range_ids"], serde_json::json!([cid]));
    assert_eq!(summary["first_by_id"]["id"], cid);
    assert_eq!(summary["first_by_id"]["track"], track_id.raw());
    assert_eq!(summary["first_by_id"]["start"], 0);
    assert_eq!(summary["first_by_id"]["end"], 48);
    assert_eq!(summary["first_by_id"]["kind"], "solid");
    assert_eq!(summary["first_by_id"]["name"], "Color");
    assert_eq!(summary["missing_is_none"], true);
    assert_eq!(summary["at_12_ids"], serde_json::json!([cid]));
    assert_eq!(summary["at_48_ids"], serde_json::json!([]));
    assert_eq!(summary["selected_ids"], serde_json::json!([cid]));
    assert_eq!(
        summary["marker_names"],
        serde_json::json!(["WasmSeedMarker"])
    );
    assert_eq!(summary["mark_in"], 4);
    assert_eq!(summary["mark_out"], 36);
    ctx.unmount("com.example.wasm-tools").expect("unmount");
}
