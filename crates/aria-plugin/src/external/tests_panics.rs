//! Guest panic isolation across mount, command, event, import, export, and
//! encoder callbacks.

use aria_core::{
    ColorSpace, CpuImage, EncoderConfig, FrameData, PixelFormat, Plane, Rational, RationalTime,
    Rect, Rotation, VideoFrame,
};
use aria_sdk::PluginRuntime;
use tempfile::TempDir;

use super::*;
use crate::{CommandError, DropAction, InterchangeError, ResolveDropped, SelectionChanged};

#[test]
fn panics_in_mount_command_event_import_and_export_isolate_and_fail_plugin() {
    let temp = TempDir::new().expect("temp dir");
    let data_root = temp.path().join("data");

    // 1. Panic in `mount`: mount fails, registrations are dropped, failure recorded.
    {
        static MOUNT_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest {
                panic_on_mount: true,
                ..ContractGuest::default()
            });
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(MOUNT_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicmount", "PanicMount", "none");
        let pkg_dir = write_package(temp.path(), "test.panicmount", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        let err = ctx
            .mount(plugin.as_ref())
            .expect_err("panicking mount must fail");
        assert!(err.to_string().contains("mount explosion"), "{err}");
        assert!(!ctx.is_mounted("test.panicmount"));
        assert_eq!(ctx.failures().len(), 1);
        assert!(ctx.failures()[0].reason.contains("mount explosion"));
    }

    // 2. Panic in `command_run`: returns `CommandError::Panicked`, fails plugin, drops registrations.
    {
        static CMD_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(CMD_PANIC_RT);
        let manifest_toml = basic_manifest("test.paniccmd", "PanicCmd", "none");
        let pkg_dir = write_package(temp.path(), "test.paniccmd", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.paniccmd");

        let err = ctx
            .run_command("contract.panic_cmd")
            .expect_err("panicking command must fail");
        assert_eq!(err, CommandError::Panicked("command boom".to_owned()));
        assert!(!ctx.is_mounted("test.paniccmd"));
        assert!(ctx.commands().is_empty());
        assert!(ctx.failures()[0].reason.contains("command boom"));
    }

    // 3. Panic in `event`: fails plugin and drops registrations.
    {
        static EV_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(EV_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicev", "PanicEv", "none");
        let pkg_dir = write_package(temp.path(), "test.panicev", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.panicev");

        let res: Option<DropAction> = ctx
            .events()
            .serial(&ResolveDropped::path(PathBuf::from("panic.cimp")));
        assert_eq!(res, None);
        assert!(!ctx.is_mounted("test.panicev"));
        assert!(ctx.failures()[0].reason.contains("event boom"));
    }

    // 4. Panic in `import`: rolls back transaction, fails plugin, drops registrations.
    {
        static IMP_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(IMP_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicimp", "PanicImp", "read-write");
        let pkg_dir = write_package(temp.path(), "test.panicimp", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let host = make_project_host(&ctx);
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.panicimp");

        let panic_file = temp.path().join("panic.cimp");
        std::fs::write(&panic_file, b"x").expect("write panic import file");
        let err = ctx
            .import_file(&host, &panic_file)
            .expect_err("panicking import must fail");
        assert!(err.to_string().contains("import boom"), "{err}");
        assert!(!ctx.is_mounted("test.panicimp"));
        assert!(ctx.importers().is_empty());
        assert!(ctx.failures()[0].reason.contains("import boom"));
    }

    // 5. Panic in `export`: leaves destination unchanged, fails plugin, drops registrations.
    {
        static EXP_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(EXP_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicexp", "PanicExp", "read");
        let pkg_dir = write_package(temp.path(), "test.panicexp", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let host = make_project_host(&ctx);
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.panicexp");

        let panic_out = temp.path().join("panic.cexp");
        std::fs::write(&panic_out, b"untouched").expect("seed panic export dest");
        let err = ctx
            .export_file("contract.exp-panic", &host.snapshot(), &panic_out)
            .expect_err("panicking export must fail");
        assert!(
            matches!(err, InterchangeError::ExportPanicked { .. }),
            "{err:?}"
        );
        assert_eq!(std::fs::read(&panic_out).expect("read dest"), b"untouched");
        assert!(!ctx.is_mounted("test.panicexp"));
        assert!(ctx.exporters().is_empty());
        assert!(ctx.failures()[0].reason.contains("export boom"));

        // Post an event after failure to verify no dead handler runs.
        ctx.events().post(SelectionChanged { clips: vec![] });
    }

    // 6. Panic in `encoder_push_frame`: fails plugin and drops encoder registration.
    {
        static ENC_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(ENC_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicenc", "PanicEnc", "none");
        let pkg_dir = write_package(temp.path(), "test.panicenc", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.panicenc");

        let panic_out = temp.path().join("panic_push.cenc");
        let enc_cfg = EncoderConfig::new((4, 2), Rational::FPS_24, ColorSpace::BT709)
            .with_bitrate_kbps(1_000);
        let mut session = ctx
            .open_encoder("contract.enc", &panic_out, &enc_cfg)
            .expect("open session");
        let plane = Plane::new(vec![0u8; 4 * 2 * 4], 4 * 4, 2);
        let frame = VideoFrame::new(
            RationalTime::zero(Rational::FPS_24),
            PixelFormat::Rgba8,
            ColorSpace::BT709,
            (4, 2),
            Rect::from_size(4, 2),
            Rotation::None,
            FrameData::Cpu(CpuImage::new(vec![plane])),
        );
        let err = session.push(&frame).expect_err("panicking push must fail");
        assert!(err.to_string().contains("encoder push boom"), "{err}");
        assert!(!ctx.is_mounted("test.panicenc"));
        assert!(ctx.encoders().is_empty());
        assert!(ctx.failures()[0].reason.contains("encoder push boom"));
    }
}
