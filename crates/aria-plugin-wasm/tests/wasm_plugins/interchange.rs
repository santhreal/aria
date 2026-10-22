//! WASM project importers and exporters, video and audio encoders, and external video decoders.

use std::fs;
use std::path::Path;

use aria_core::{
    AudioEncoderConfig, ColorSpace, CpuImage, EncoderConfig, FrameData, PixelFormat, Plane,
    Rational, RationalTime, Rect, Rotation, VideoFrame,
};
use aria_models::MarkerColor;
use aria_plugin_wasm::{Limits, load};

use super::{FIXTURE_WASM, SERIAL, setup_host_context, write_package};

/// Importing a `.mrk` file applies all parsed `AddMarker` commands as a single undoable
/// transaction on `ProjectHost`, and exporting writes the exact `.mrk` bytes to disk.
#[test]
fn import_applies_commands_in_one_undo_step_and_export_writes_exact_bytes() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let input_path = temp.path().join("scene.mrk");
    fs::write(
        &input_path,
        "# comment line\n12 teal Opening Shot\nmalformed_line\n45 orange Cut To Wide\n",
    )
    .expect("write input .mrk");

    let (report, _receipt) = ctx
        .import_file(&host, &input_path)
        .expect("import .mrk file");
    assert_eq!(report.skipped, vec!["malformed_line".to_owned()]);

    let markers = host.snapshot().timeline().markers().to_vec();
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[0].tick, RationalTime::new(12, Rational::FPS_24));
    assert_eq!(markers[0].color, MarkerColor::Teal);
    assert_eq!(markers[0].name, "Opening Shot");
    assert_eq!(markers[1].tick, RationalTime::new(45, Rational::FPS_24));
    assert_eq!(markers[1].color, MarkerColor::Orange);
    assert_eq!(markers[1].name, "Cut To Wide");

    // Export the project to `.mrk` and assert exact bytes.
    let output_path = temp.path().join("exported.mrk");
    let export_report = ctx
        .export_file("example.markers", host.snapshot().as_ref(), &output_path)
        .expect("export .mrk file");
    assert!(export_report.skipped.is_empty());
    let exported_text = fs::read_to_string(&output_path).expect("read exported .mrk");
    assert_eq!(
        exported_text,
        "12 teal Opening Shot\n45 orange Cut To Wide\n"
    );

    // One undo reverts BOTH imported markers in a single step.
    assert!(host.undo().expect("undo import transaction"));
    assert!(
        host.snapshot().timeline().markers().is_empty(),
        "both imported markers must undo in one step"
    );

    ctx.unmount("com.example.wasm-tools").expect("unmount");
}

/// A WASM encoder registered during `mount` opens a session, encodes video frames
/// and interleaved audio blocks, writes its output to the active export target path
/// via `fs-write` without needing `permissions.fs`, denies writes to any other path,
/// and faults the plugin on a guest trap.
#[test]
fn wasm_encoder_writes_output_enforces_fs_scope_and_faults_on_trap() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "none", &[], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let encoders = ctx.encoders();
    assert_eq!(encoders.len(), 1);
    assert_eq!(encoders[0].id, "example.raw-rgba");
    assert_eq!(encoders[0].extension, "rgba");
    assert!(encoders[0].audio);

    let cfg = EncoderConfig::new((4, 2), Rational::FPS_24, ColorSpace::BT709)
        .with_audio(AudioEncoderConfig::new(48_000, 2))
        .with_bitrate_kbps(2_000);
    let out_path = temp.path().join("rendered.rgba");
    let mut session = ctx
        .open_encoder("example.raw-rgba", &out_path, &cfg)
        .expect("open wasm encoder");
    let make_frame = || {
        let plane = Plane::new(vec![0u8; 4 * 2 * 4], 4 * 4, 2);
        VideoFrame::new(
            RationalTime::zero(Rational::FPS_24),
            PixelFormat::Rgba8,
            ColorSpace::BT709,
            (4, 2),
            Rect::from_size(4, 2),
            Rotation::None,
            FrameData::Cpu(CpuImage::new(vec![plane])),
        )
    };
    let frame = make_frame();
    session.push(&frame).expect("push frame 1");
    session.push(&frame).expect("push frame 2");
    session
        .push_audio(
            &[0.1, -0.1, 0.2, -0.2],
            RationalTime::zero(Rational::new(48_000, 1)),
        )
        .expect("push audio");
    session.finish().expect("finish wasm encoder");
    assert_eq!(
        fs::read_to_string(&out_path).expect("read encoded output"),
        "RGBA1 4x2@24/1:frames=2:bytes=64:samples=4\n"
    );

    // Attempting `fs-write` to a path other than the active encode path is denied and records a CapabilityDenial.
    let bad_fs_path = temp.path().join("unpermitted_fs.rgba");
    let mut bad_session = ctx
        .open_encoder("example.raw-rgba", &bad_fs_path, &cfg)
        .expect("open bad_fs session");
    let fs_err = bad_session
        .push(&frame)
        .expect_err("unpermitted fs_write inside encoder must fail");
    assert!(
        fs_err.to_string().contains("permission denied"),
        "unexpected error: {fs_err}"
    );
    drop(bad_session);
    assert!(
        !ctx.denials().is_empty(),
        "expected CapabilityDenial for unpermitted fs_write"
    );
    assert!(
        ctx.is_mounted("com.example.wasm-tools"),
        "normal guest error does not fault plugin"
    );

    // A guest trap during `encoder-push-frame` faults the plugin, removes its encoder, and records a failure.
    let trap_path = temp.path().join("trap_push.rgba");
    let mut trap_session = ctx
        .open_encoder("example.raw-rgba", &trap_path, &cfg)
        .expect("open trap session");
    let trap_err = trap_session
        .push(&frame)
        .expect_err("trapping push must fail");
    assert!(
        trap_err.to_string().contains("wasm trap"),
        "expected wasm trap in error: {trap_err}"
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.encoders().is_empty());
    assert_eq!(ctx.failures().len(), 1);
    assert!(
        ctx.failures()[0]
            .reason
            .contains("encoder example.raw-rgba panicked in push"),
        "unexpected failure reason: {}",
        ctx.failures()[0].reason
    );
}

/// Luma of pixel `(x, y)` in frame `index` of [`write_y4m`]'s files: distinct
/// per pixel and per frame, so a swapped plane or frame shows.
fn y4m_luma(index: u8, x: u32, y: u32) -> u8 {
    index * 40 + (y * 8 + x) as u8
}

/// Write an 8x4 YUV4MPEG2 file of `frames` 4:2:0 frames at 24 fps whose
/// frame `i` has luma [`y4m_luma`], U `100 + i`, and V `200 - i`.
fn write_y4m(path: &Path, frames: u8, extra: &str) {
    let mut bytes = format!("YUV4MPEG2 W8 H4 F24:1 Ip A1:1 C420jpeg{extra}\n").into_bytes();
    for index in 0..frames {
        bytes.extend_from_slice(b"FRAME\n");
        for y in 0..4 {
            for x in 0..8 {
                bytes.push(y4m_luma(index, x, y));
            }
        }
        bytes.extend_from_slice(&[100 + index; 8]);
        bytes.extend_from_slice(&[200 - index; 8]);
    }
    fs::write(path, bytes).expect("write y4m");
}

/// The planes of a CPU frame as `(bytes, stride, rows)`.
fn cpu_planes(frame: &VideoFrame) -> Vec<(Vec<u8>, usize, usize)> {
    let FrameData::Cpu(cpu) = &frame.data else {
        panic!("a plugin decoder returns CPU frames");
    };
    cpu.planes
        .iter()
        .map(|plane| (plane.data.clone(), plane.stride, plane.rows))
        .collect()
}

fn y4m_planes(index: u8) -> Vec<(Vec<u8>, usize, usize)> {
    let luma = (0..4)
        .flat_map(|y| (0..8).map(move |x| y4m_luma(index, x, y)))
        .collect();
    vec![
        (luma, 8, 4),
        (vec![100 + index; 8], 4, 2),
        (vec![200 - index; 8], 4, 2),
    ]
}

/// The example's YUV4MPEG2 decoder reads a `.y4m` file outside its
/// `permissions.fs` scopes through the host's decoder list: the probe, the
/// stream description, exact planes at a seek target, forward decode to end
/// of stream, and a backward seek. A trap while decoding fails the plugin
/// and unclaims the extension.
#[test]
fn wasm_decoder_reads_y4m_frames_exactly_and_a_trap_fails_the_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(
        &temp.path().join("pkg"),
        "read-write",
        &["pick"],
        FIXTURE_WASM,
    );
    let (ctx, _host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let listed = ctx.decoders();
    assert_eq!(listed.len(), 1);
    assert_eq!(&*listed[0].plugin_id, "com.example.wasm-tools");
    assert_eq!(listed[0].spec.id, "example.y4m");
    assert_eq!(listed[0].spec.extensions, ["y4m"]);

    let clip = temp.path().join("ramp.y4m");
    write_y4m(&clip, 5, "");
    let probed = aria_decoder::probe(&clip).expect("probes through the plugin");
    assert_eq!((probed.width, probed.height), (8, 4));
    assert_eq!(
        (probed.frame_rate, probed.frame_count),
        (Rational::new(24, 1), 5)
    );
    assert_eq!(probed.video_codec.as_deref(), Some("yuv4mpeg2"));
    assert!(!probed.has_audio);

    let mut decoder = aria_decoder::open_video_decoder(&clip, aria_decoder::OutputMode::Cpu)
        .expect("opens through the plugin");
    let info = decoder.info().clone();
    assert_eq!(info.coded_size, (8, 4));
    assert_eq!(info.pixel_format, PixelFormat::Yuv420p);
    assert_eq!(info.color, ColorSpace::BT601);
    assert_eq!(
        info.duration,
        Some(RationalTime::new(5, Rational::new(24, 1)))
    );

    let third = decoder
        .frame_at(RationalTime::new(3, Rational::new(24, 1)))
        .expect("decodes")
        .expect("frame 3 exists");
    assert_eq!(third.pts, RationalTime::new(3, Rational::new(24, 1)));
    assert_eq!(cpu_planes(&third), y4m_planes(3));
    let fourth = decoder.next_frame().expect("decodes").expect("frame 4");
    assert_eq!(cpu_planes(&fourth), y4m_planes(4));
    assert!(
        decoder.next_frame().expect("end of stream").is_none(),
        "a frame past the last one"
    );
    let first = decoder
        .frame_at(RationalTime::new(1, Rational::new(24, 1)))
        .expect("seeks back")
        .expect("frame 1 exists");
    assert_eq!(cpu_planes(&first), y4m_planes(1));
    drop(decoder);
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    let trapping = temp.path().join("trap.y4m");
    write_y4m(&trapping, 2, " XARIA=TRAP");
    let mut decoder = aria_decoder::open_video_decoder(&trapping, aria_decoder::OutputMode::Cpu)
        .expect("opens through the plugin");
    let err = decoder
        .frame_at(RationalTime::new(0, Rational::new(24, 1)))
        .expect_err("a trap is an error");
    assert!(err.to_string().contains("trap"), "{err}");
    drop(decoder);
    let failures = ctx.failures();
    assert!(
        failures
            .iter()
            .any(|failure| failure.id == "com.example.wasm-tools"),
        "the trap did not fail the plugin: {failures:?}"
    );
    assert!(
        ctx.decoders().is_empty(),
        "a failed plugin's decoder is listed"
    );
    assert!(
        !aria_decoder::external_decoder_extensions().contains(&"y4m".to_owned()),
        "a failed plugin's decoder still claims .y4m"
    );
}
