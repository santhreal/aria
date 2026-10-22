//! Host contract operations, late-registration guard, event dispatch,
//! importer/exporter transactions, and encoder streaming tests.

use aria_core::{
    AudioEncoderConfig, ColorSpace, CpuImage, EncoderConfig, FrameData, PixelFormat, Plane,
    Rational, RationalTime, Rect, Rotation, TimeRange, VideoFrame,
};
use aria_models::{Generator, MarkerColor};
use aria_sdk::PluginRuntime;
use tempfile::TempDir;

use super::*;
use crate::{CommandError, DropAction, EditorView, Origin, ResolveDropped, ViewFrame};

#[test]
fn full_contract_operations_and_registration_guard_and_interchange_work() {
    static RUNTIME: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(ContractGuest::default());
        // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
        assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
        rt
    });
    let table = plugin_table!(RUNTIME);

    let temp = TempDir::new().expect("temp dir");
    let data_root = temp.path().join("data");
    let manifest_toml = basic_manifest("test.contract", "Contract", "read-write");
    let pkg_dir = write_package(temp.path(), "test.contract", &manifest_toml, None);
    let pkg = Package::open(&pkg_dir).expect("open package");

    let ctx = PluginContext::new();
    let _settings_reg = ctx.register_arc::<dyn SettingsService>(Arc::new(TestSettings));
    let host = make_project_host(&ctx);
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
            timeline: TimeRange::at_rate(0, 48, aria_core::Rational::FPS_24),
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
            at: RationalTime::new(10, aria_core::Rational::FPS_24),
            name: "SeedMarker".to_owned(),
            color: Some(MarkerColor::Blue),
        }),
    )
    .expect("seed marker");

    let view = Arc::new(EditorView::new());
    view.publish(
        ViewFrame {
            selection: &[seeded_clip_id],
            playhead: RationalTime::new(12, aria_core::Rational::FPS_24),
            mark_in: Some(RationalTime::new(4, aria_core::Rational::FPS_24)),
            mark_out: Some(RationalTime::new(36, aria_core::Rational::FPS_24)),
            playing: false,
            speed: 1,
        },
        ctx.events(),
    );
    let _view_reg = ctx.register_arc(view);
    let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
    ctx.mount(plugin.as_ref()).expect("mount contract plugin");

    // 1. Run `contract.exercise` covering storage, settings, project_snapshot, editor_view,
    //    project_transact, and project_undo.
    ctx.run_command("contract.exercise")
        .expect("contract.exercise should succeed");
    assert!(
        ctx.denials().is_empty(),
        "expected zero denials for read-write plugin"
    );

    // 2. Registration outside `mount` is refused.
    let late_err = ctx
        .run_command("contract.late_register")
        .expect_err("registration outside mount must fail");
    assert_eq!(
        late_err,
        CommandError::Failed("registration calls are permitted only during mount".to_owned())
    );

    // 3. `ResolveDropped` returns `DropAction::ImportTimeline` for paths and `DropAction::Handled` for URLs.
    let claim = ctx
        .events()
        .serial(&ResolveDropped::path(PathBuf::from("timeline.cimp")));
    assert_eq!(claim, Some(DropAction::ImportTimeline));
    let url_claim = ctx
        .events()
        .serial(&ResolveDropped::url("https://example.invalid/asset.mp4"));
    assert_eq!(url_claim, Some(DropAction::Handled));

    // 4. `CommandProposed` veto stops disallowed edits.
    let veto_err = host
        .apply(
            &Origin::Editor,
            Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Video,
                name: "VETO_ME".to_owned(),
                index: None,
                pinned: true,
            }),
        )
        .expect_err("VETO_ME track must be refused");
    assert!(
        veto_err.to_string().contains("blocked by contract guest"),
        "{veto_err}"
    );

    // 5. `import_file` applies multiple commands as ONE transaction and rolls back on error.
    let before_tracks = host.snapshot().timeline().tracks_ordered().count();
    let import_path = temp.path().join("scene.cimp");
    std::fs::write(&import_path, b"dummy").expect("write import file");
    let (report, _receipt) = ctx
        .import_file(&host, &import_path)
        .expect("import_file should succeed");
    assert_eq!(report.skipped, vec!["unsupported transition".to_owned()]);
    assert_eq!(
        host.snapshot().timeline().tracks_ordered().count(),
        before_tracks + 2
    );
    // One undo reverts both imported tracks at once.
    assert!(host.undo().expect("undo import"));
    assert_eq!(
        host.snapshot().timeline().tracks_ordered().count(),
        before_tracks
    );

    let fail_import_path = temp.path().join("fail.cimp");
    std::fs::write(&fail_import_path, b"dummy").expect("write fail import");
    let imp_err = ctx
        .import_file(&host, &fail_import_path)
        .expect_err("failing import must return error");
    assert!(
        imp_err.to_string().contains("corrupt cimp file"),
        "{imp_err}"
    );
    assert_eq!(
        host.snapshot().timeline().tracks_ordered().count(),
        before_tracks
    );

    // 6. `export_file` writes atomically and leaves destination untouched on error.
    let export_path = temp.path().join("out.cexp");
    let export_report = ctx
        .export_file("contract.exp", &host.snapshot(), &export_path)
        .expect("export_file should succeed");
    assert_eq!(export_report.skipped, vec!["skipped generator".to_owned()]);
    let exported_text = std::fs::read_to_string(&export_path).expect("read exported file");
    assert!(exported_text.starts_with("EXPORTED:"), "{exported_text}");

    let fail_export_path = temp.path().join("fail.cexp");
    std::fs::write(&fail_export_path, b"original").expect("seed destination");
    let exp_err = ctx
        .export_file("contract.exp-fail", &host.snapshot(), &fail_export_path)
        .expect_err("failing export must return error");
    assert!(
        exp_err.to_string().contains("disk quota exceeded"),
        "{exp_err}"
    );
    assert_eq!(
        std::fs::read(&fail_export_path).expect("read unchanged destination"),
        b"original"
    );

    // 6b. `open_encoder` writes video frames and audio blocks through the native encoder.
    // A time at 30000/1001 reaches the guest as whole-rate ticks: frame 3 starts
    // 3 * 1001 / 30000 s in, and an audio block at frame 5 starts 5005 / 30000 s in.
    let enc_out = temp.path().join("out.cenc");
    let enc_cfg = EncoderConfig::new((8, 4), Rational::FPS_24, ColorSpace::BT709)
        .with_audio(AudioEncoderConfig::new(48_000, 2))
        .with_bitrate_kbps(2_000);
    let mut enc_session = ctx
        .open_encoder("contract.enc", &enc_out, &enc_cfg)
        .expect("open contract.enc");
    let plane = Plane::new(vec![0u8; 8 * 4 * 4], 8 * 4, 4);
    let frame = VideoFrame::new(
        RationalTime::new(3, Rational::FPS_29_97),
        PixelFormat::Rgba8,
        ColorSpace::BT709,
        (8, 4),
        Rect::from_size(8, 4),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![plane])),
    );
    enc_session.push(&frame).expect("push frame");
    enc_session
        .push_audio(
            &[0.5, -0.5, 0.25, -0.25],
            RationalTime::new(5, Rational::FPS_29_97),
        )
        .expect("push audio");
    enc_session.finish().expect("finish encoder");
    assert_eq!(
        std::fs::read_to_string(&enc_out).expect("read encoded output"),
        "8x4@24:1:4:f3003/30000,a5005/30000"
    );

    // 7. Unmount calls the guest's `unmount` callback and drops all registrations.
    ctx.unmount("test.contract").expect("unmount test.contract");
    assert!(!ctx.is_mounted("test.contract"));
    assert!(ctx.commands().is_empty());
    assert!(ctx.importers().is_empty());
    assert!(ctx.exporters().is_empty());
    assert!(ctx.encoders().is_empty());
}
