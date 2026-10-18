//! DMA-BUF surface checks, the tiled-fallback report, and the cause named
//! when a DMABuf pipeline cannot open.

use super::*;

#[test]
fn packed_nv12_bytes_split() {
    let w = 4u32;
    let h = 4u32;
    let mut bytes = vec![16u8; (w * h) as usize];
    bytes.extend(std::iter::repeat(128u8).take((w * h / 2) as usize));
    let planes = nv12_from_bytes(&bytes, w, h).expect("split");
    assert_eq!(planes.len(), 2);
    assert_eq!(planes[0].rows, 4);
    assert_eq!(planes[1].rows, 2);
}

#[test]
fn linear_nv12_surface_is_importable() {
    let s = DmaBufSurface {
        fourcc: DmaBufSurface::NV12,
        modifier: DmaBufSurface::LINEAR,
        plane_count: 1,
        fds: [5, 0, 0, 0],
        strides: [64, 64, 0, 0],
        offsets: [0, 256, 0, 0],
    };
    assert!(s.is_linear_nv12());
}

/// A tiled modifier must be named, not called unknown, and a vendor byte
/// Aria has no name for must still print its number: a new GPU family is
/// the case where this report matters most.
#[test]
fn every_drm_vendor_byte_gets_a_name() {
    // Vendor byte is the top byte; the low bits are the layout.
    assert_eq!(modifier_vendor(0), "none");
    assert_eq!(modifier_vendor(0x01 << 56 | 0x9), "Intel");
    assert_eq!(modifier_vendor(0x02 << 56 | 0x1a), "AMD");
    assert_eq!(modifier_vendor(0x03 << 56 | 0x10), "NVIDIA");
    assert_eq!(modifier_vendor(0x08 << 56 | 0x1), "Arm");
    assert_eq!(modifier_vendor(0xfe << 56), "DRM vendor 254");
    // Every byte resolves to something non-empty, so no modifier can be
    // reported as a blank vendor.
    for byte in 0u8..=255 {
        let name = modifier_vendor(u64::from(byte) << 56);
        assert!(!name.is_empty(), "vendor byte {byte} has no name");
    }
}

/// The report is once per session because the same pipeline negotiates the
/// same modifier for every sample: per-frame logging would bury it.
#[test]
fn the_tiled_fallback_is_reported_once() {
    let tiled = DmaBufSurface {
        fourcc: DmaBufSurface::NV12,
        modifier: 0x03 << 56 | 0x10,
        plane_count: 2,
        fds: [7, 8, 0, 0],
        strides: [64, 64, 0, 0],
        offsets: [0, 256, 0, 0],
    };
    assert!(!tiled.is_linear_nv12(), "fixture must be unimportable");

    assert!(
        report_tiled_fallback_for_source(&tiled, "test_session_1"),
        "the first tiled surface must be reported"
    );
    assert!(
        !report_tiled_fallback_for_source(&tiled, "test_session_1"),
        "the same fallback was reported twice"
    );
}

#[test]
fn two_tiled_sources_both_log() {
    let tiled = DmaBufSurface {
        fourcc: DmaBufSurface::NV12,
        modifier: 0x03 << 56 | 0x10,
        plane_count: 2,
        fds: [7, 8, 0, 0],
        strides: [64, 64, 0, 0],
        offsets: [0, 256, 0, 0],
    };
    assert!(
        report_tiled_fallback_for_source(&tiled, "stream_alpha"),
        "first source must log"
    );
    assert!(
        !report_tiled_fallback_for_source(&tiled, "stream_alpha"),
        "first source must not duplicate log"
    );
    assert!(
        report_tiled_fallback_for_source(&tiled, "stream_beta"),
        "second source must also log"
    );
    assert!(
        !report_tiled_fallback_for_source(&tiled, "stream_beta"),
        "second source must not duplicate log"
    );
}

/// WHY: a failed DMABuf attempt on NVDEC opened `nvh264dec`, failed to link,
/// and tore the CUDA decoder down again: 265 ms on an RTX 5090 before the CPU
/// open even started. A plugged decoder with no DMABuf output now stops the
/// attempt before it opens, within a few state polls. Covers the stop on a real
/// file and its bound; the probe itself is covered in
/// `gst_dmabuf_watch_tests`. A host whose decoder offers DMABuf is not stopped.
#[test]
fn a_dmabuf_attempt_stops_once_its_decoder_has_no_dmabuf_output() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4");
    gst::init().expect("GStreamer initializes");
    let uri = path_uri(&fixture).expect("fixture uri");
    let watch = Arc::new(DmabufWatch::default());
    let started = Instant::now();
    let result = try_open(&uri, Some(&watch));
    let took = started.elapsed();
    let decoders = watch.decoders();
    assert!(!decoders.is_empty(), "the attempt plugged no video decoder");
    if decoders
        .iter()
        .any(|(_, caps)| crate::gst_dmabuf_watch::offers_dmabuf(caps))
    {
        assert!(!watch.stopped(), "a decoder offering DMABuf was starved");
        if let Ok(opened) = result {
            let _ = opened.pipeline.set_state(gst::State::Null);
        }
        return;
    }
    match result {
        Ok(opened) => panic!("DMABuf opened from {decoders:?} in {:?}", opened.mode),
        Err(err) => assert!(
            err.to_string().contains(STOPPED),
            "the attempt failed without stopping: {err}"
        ),
    }
    assert!(watch.stopped());
    assert!(
        took < Duration::from_secs(2),
        "the stopped attempt took {took:?}"
    );
}

/// The cause a real failed open records names the decoder `uridecodebin`
/// plugged, so the watch on the pipeline sees the decoder it plugs. A host
/// whose decoder hands out LINEAR NV12 DMABuf opens and records nothing.
#[test]
fn a_failed_dmabuf_open_records_the_decoder_it_plugged() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4");
    gst::init().expect("GStreamer initializes");
    let opened = open_pipeline(&fixture, OutputMode::Gpu).expect("the fixture opens");
    let _ = opened.pipeline.set_state(gst::State::Null);
    if let Some(reason) = DMABUF_FAILED.get() {
        assert!(
            !reason.starts_with("no video decoder was plugged"),
            "the recorded cause names no decoder: {reason}"
        );
        // Each clause opens with the factory it describes, and every one is a
        // video decoder: the watch records no demuxer, parser, or queue.
        let causes = reason.split(" (").next().unwrap_or_default();
        for cause in causes.split("; ") {
            let name = cause.split(' ').next().unwrap_or_default();
            let factory = gst::ElementFactory::find(name)
                .unwrap_or_else(|| panic!("{name:?} in {reason:?} is no element factory"));
            let klass = factory
                .metadata(gst::ELEMENT_METADATA_KLASS)
                .unwrap_or_default();
            assert!(
                klass.contains("Decoder") && klass.contains("Video"),
                "{name} ({klass}) is not a video decoder: {reason}"
            );
        }
    } else {
        assert_eq!(
            opened.mode,
            OutputMode::Gpu,
            "a DMABuf open that did not fail"
        );
    }
}

/// WHY: the DMABuf latch turns DMABuf off for the rest of the process, so only
/// a failure that is DMABuf's may set it: one on a file the CPU path then
/// opens. An unreadable, empty, or audio-only source fails both paths; when it
/// latched, the first such import decoded every later GPU open of the session
/// to CPU NV12. Covers each outcome of the attempt (DMABuf opens, only the CPU
/// path opens, neither opens), a latch already set, a CPU caller, and a real
/// unreadable file through `try_open`. Does not catch a transient DMABuf
/// failure on a file the CPU path opens; that latches by design.
#[test]
fn only_a_file_the_cpu_path_opens_turns_dmabuf_off() {
    use std::cell::RefCell;
    use std::sync::OnceLock;

    // (gpu, latch set before, DMABuf opens, CPU opens) -> (attempts, result, latched)
    let cases: [(bool, bool, bool, bool, &[&str], Result<&str, &str>, bool); 5] = [
        (true, false, true, true, &["dmabuf"], Ok("dmabuf"), false),
        (
            true,
            false,
            false,
            true,
            &["dmabuf", "cpu"],
            Ok("cpu"),
            true,
        ),
        (
            true,
            false,
            false,
            false,
            &["dmabuf", "cpu"],
            Err("cpu"),
            false,
        ),
        (true, true, true, true, &["cpu"], Ok("cpu"), true),
        (false, false, true, true, &["cpu"], Ok("cpu"), false),
    ];
    for (case, (gpu, preset, dmabuf_opens, cpu_opens, attempts, expected, latched)) in
        cases.into_iter().enumerate()
    {
        let latch = OnceLock::new();
        if preset {
            let _ = latch.set("set before".to_owned());
        }
        let tried = RefCell::new(Vec::new());
        let got = open_latched(&latch, gpu, |watch| {
            let what = if watch.is_some() { "dmabuf" } else { "cpu" };
            tried.borrow_mut().push(what);
            if (watch.is_some() && dmabuf_opens) || (watch.is_none() && cpu_opens) {
                Ok(what)
            } else {
                Err(DecodeError::Open(format!("{what} could not open")))
            }
        });
        assert_eq!(
            tried.into_inner(),
            attempts,
            "case {case}: the attempts made"
        );
        match (got, expected) {
            (Ok(got), Ok(expected)) => assert_eq!(got, expected, "case {case}"),
            (Err(err), Err(expected)) => assert!(
                err.to_string()
                    .contains(&format!("{expected} could not open")),
                "case {case}: returned {err}, not the {expected} path's error"
            ),
            (got, expected) => panic!("case {case}: returned {got:?}, expected {expected:?}"),
        }
        assert_eq!(latch.get().is_some(), latched, "case {case}: the latch");
        if latched && !preset {
            let reason = latch.get().map(String::as_str).unwrap_or_default();
            assert!(
                reason.contains("dmabuf could not open"),
                "case {case}: the latch reason {reason:?} names the DMABuf failure"
            );
        }
    }

    crate::gst::ensure_init().expect("GStreamer initializes");
    let unreadable = path_uri(std::path::Path::new("/dev/null")).expect("a file URI");
    let latch = OnceLock::new();
    assert!(open_latched(&latch, true, |watch| try_open(&unreadable, watch)).is_err());
    assert_eq!(
        latch.get(),
        None,
        "an unreadable file turned DMABuf off for the process"
    );
}
