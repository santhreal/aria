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

        let tx_err = ctx
            .run_command("perm.transact")
            .expect_err("transact denied under read");
        assert!(tx_err.to_string().contains("permission denied"), "{tx_err}");
        let undo_err = ctx
            .run_command("perm.undo")
            .expect_err("undo denied under read");
        assert!(
            undo_err.to_string().contains("permission denied"),
            "{undo_err}"
        );

        let denials = ctx.denials();
        assert_eq!(denials.len(), 1);
        assert_eq!(denials[0].plugin, "test.permread");
        assert!(
            denials[0].service.contains("ProjectWrite"),
            "{:?}",
            denials[0]
        );
    }
}
