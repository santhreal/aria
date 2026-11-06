//! Project permission level enforcement and capability denial recording tests.

use aria_sdk::PluginRuntime;
use tempfile::TempDir;

use super::*;

#[derive(Default)]
struct PermissionGuest;

impl SdkPlugin for PermissionGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_command("perm.snapshot", "Snapshot")?;
        host.register_command("perm.view", "View")?;
        host.register_command("perm.clip", "Clip")?;
        host.register_command("perm.clips_in_range", "ClipsInRange")?;
        host.register_command("perm.clips_at", "ClipsAt")?;
        host.register_command("perm.selected_clips", "SelectedClips")?;
        host.register_command("perm.markers_in_range", "MarkersInRange")?;
        host.register_command("perm.in_out", "InOut")?;
        host.register_command("perm.transact", "Transact")?;
        host.register_command("perm.undo", "Undo")?;
        host.register_command("perm.render_frame", "RenderFrame")?;
        host.register_command("perm.seek", "Seek")?;
        host.register_command("perm.play", "Play")?;
        host.register_command("perm.stop", "Stop")?;
        host.register_command("perm.open", "Open")?;
        host.register_command("perm.save", "Save")?;
        host.register_command("perm.save_as", "SaveAs")?;
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        let t0 = SdkRationalTime::new(0, SdkRational::FPS_24);
        let range = SdkTimeRange::new(t0, SdkRationalTime::new(24, SdkRational::FPS_24));
        match id {
            "perm.snapshot" => host.project_snapshot().map(drop),
            "perm.view" => host.editor_view().map(drop),
            "perm.clip" => host.clip(1).map(drop),
            "perm.clips_in_range" => host.clips_in_range(None, range).map(drop),
            "perm.clips_at" => host.clips_at(None, t0).map(drop),
            "perm.selected_clips" => host.selected_clips().map(drop),
            "perm.markers_in_range" => host.markers_in_range(None, range).map(drop),
            "perm.in_out" => host.in_out().map(drop),
            "perm.transact" => host.project_transact_json("noop", "[]").map(drop),
            "perm.undo" => host.project_undo().map(drop),
            "perm.render_frame" => host
                .render_frame(
                    aria_sdk::FrameSource::Open,
                    t0,
                    aria_sdk::FrameSize::Exact {
                        width: 0,
                        height: 180,
                    },
                )
                .map(drop),
            "perm.seek" => host
                .editor_seek(SdkRationalTime::new(12, SdkRational::FPS_24))
                .map(drop),
            "perm.play" => host.editor_play(2),
            "perm.stop" => host.editor_stop(),
            "perm.open" => {
                let path = host
                    .storage_get("target_path")?
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_else(|| "/work/cut.aria".to_owned());
                host.project_open(&path)
            }
            "perm.save" => host.project_save().map(drop),
            "perm.save_as" => {
                let path = host
                    .storage_get("target_path")?
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_else(|| "/work/cut.aria".to_owned());
                host.project_save_as(&path).map(drop)
            }
            other => Err(format!("unknown: {other}")),
        }
    }
}

#[test]
fn permission_levels_enforce_access_and_record_capability_denials() {
    static PERM_RUNTIME: LazyLock<PluginRuntime<PermissionGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(PermissionGuest);
        // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
        assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
        rt
    });
    let table = plugin_table!(PERM_RUNTIME);

    let temp = TempDir::new().expect("temp dir");
    let data_root = temp.path().join("data");

    // Case 1: `project = "none"` denies snapshot, view, transact, and undo and records CapabilityDenial.
    {
        let manifest_toml = basic_manifest("test.permnone", "PermNone", "none");
        let pkg_dir = write_package(temp.path(), "test.permnone", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let _host = make_project_host(&ctx);
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.permnone");

        for cmd in [
            "perm.snapshot",
            "perm.view",
            "perm.clip",
            "perm.clips_in_range",
            "perm.clips_at",
            "perm.selected_clips",
            "perm.markers_in_range",
            "perm.in_out",
            "perm.transact",
            "perm.undo",
            "perm.render_frame",
            "perm.seek",
            "perm.play",
            "perm.stop",
            "perm.open",
            "perm.save",
            "perm.save_as",
        ] {
            let err = ctx.run_command(cmd).expect_err("must be denied");
            assert!(
                err.to_string().contains("permission denied"),
                "{cmd}: {err}"
            );
        }
        let denials = ctx.denials();
        assert!(!denials.is_empty(), "expected CapabilityDenial entries");
        assert!(denials.iter().all(|d| d.plugin == "test.permnone"));
    }

    // Case 2: `project = "read"` allows snapshot and view, denies transact and undo with CapabilityDenial.
    {
        let manifest_toml = basic_manifest("test.permread", "PermRead", "read");
        let pkg_dir = write_package(temp.path(), "test.permread", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let _host = make_project_host(&ctx);
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.permread");

        for cmd in [
            "perm.snapshot",
            "perm.view",
            "perm.clip",
            "perm.clips_in_range",
            "perm.clips_at",
            "perm.selected_clips",
            "perm.markers_in_range",
            "perm.in_out",
        ] {
            ctx.run_command(cmd)
                .unwrap_or_else(|err| panic!("{cmd} allowed under read: {err}"));
        }
        assert!(
            ctx.denials().is_empty(),
            "read operations must not record denials when project = read"
        );

        // Under `project = "read"`, `render_frame` passes the permission gate and fails on invalid 0 width.
        let rf_err = ctx
            .run_command("perm.render_frame")
            .expect_err("0 width must fail bounds check");
        assert!(
            !rf_err.to_string().contains("permission denied"),
            "render_frame must be permitted under project = read: {rf_err}"
        );
        assert!(ctx.denials().is_empty());

        for write_cmd in [
            "perm.transact",
            "perm.undo",
            "perm.seek",
            "perm.play",
            "perm.stop",
            "perm.open",
            "perm.save",
            "perm.save_as",
        ] {
            let err = ctx
                .run_command(write_cmd)
                .expect_err("write operation denied under read");
            assert!(
                err.to_string().contains("permission denied"),
                "{write_cmd}: {err}"
            );
        }

        let denials = ctx.denials();
        assert_eq!(denials.len(), 1);
        assert_eq!(denials[0].plugin, "test.permread");
        assert!(
            denials[0].service.contains("ProjectWrite"),
            "{:?}",
            denials[0]
        );
    }

    // Case 3: `project = "read-write"` with `fs = ["project"]` allows transport and session
    // save/open inside the project directory, and denies filesystem paths outside granted scopes.
    {
        let mut manifest_toml = basic_manifest("test.permrw", "PermRW", "read-write");
        manifest_toml.push_str("fs = [\"project\"]\n");
        let pkg_dir = write_package(temp.path(), "test.permrw", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let host = make_project_host(&ctx);
        let proj_dir = temp.path().join("proj");
        std::fs::create_dir_all(&proj_dir).expect("proj_dir");
        host.save_as(&crate::Origin::Editor, proj_dir.join("seed.aria"))
            .expect("seed project path");
        let view = Arc::new(crate::EditorView::new());
        let _view_reg = ctx.register_arc(Arc::clone(&view));
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.permrw");

        ctx.run_command("perm.seek").expect("seek allowed");
        ctx.run_command("perm.play").expect("play allowed");
        ctx.run_command("perm.stop").expect("stop allowed");
        assert_eq!(view.take_transport_requests().len(), 3);

        let allowed_file = proj_dir.join("saved.aria");
        host.storage_set(
            "test.permrw",
            "target_path",
            allowed_file.to_str().expect("utf8").as_bytes().to_vec(),
        );
        ctx.run_command("perm.save_as")
            .expect("save_as in project dir allowed");
        ctx.run_command("perm.save")
            .expect("save to existing allowed path allowed");
        ctx.run_command("perm.open")
            .expect("open from project dir allowed");
        assert!(ctx.denials().is_empty());

        let outside_file = temp.path().join("outside.aria");
        host.storage_set(
            "test.permrw",
            "target_path",
            outside_file.to_str().expect("utf8").as_bytes().to_vec(),
        );
        let fs_err = ctx
            .run_command("perm.save_as")
            .expect_err("save_as outside granted fs scopes must fail");
        assert!(fs_err.to_string().contains("permission denied"), "{fs_err}");
        assert!(!ctx.denials().is_empty());
    }
}
