//! DMA-BUF surface checks, the tiled-fallback report, and the cause named
//! when a DMABuf pipeline cannot open.

use super::*;

#[test]
fn packed_nv12_and_i420_bytes_split() {
    let w = 4u32;
    let h = 4u32;
    let mut nv12 = vec![16u8; (w * h) as usize];
    nv12.extend(std::iter::repeat_n(128u8, (w * h / 2) as usize));
    let planes = nv12_from_bytes(&nv12, w, h).expect("split nv12");
    assert_eq!(planes.len(), 2);
    assert_eq!(planes[0].rows, 4);
    assert_eq!(planes[1].rows, 2);

    let mut i420 = vec![16u8; (w * h) as usize];
    i420.extend(std::iter::repeat_n(64u8, (w * h / 4) as usize));
    i420.extend(std::iter::repeat_n(192u8, (w * h / 4) as usize));
    let planes = i420_from_bytes(&i420, w, h).expect("split i420");
    assert_eq!(planes.len(), 3);
    assert_eq!((planes[0].stride, planes[0].rows), (4, 4));
    assert_eq!((planes[1].stride, planes[1].rows), (2, 2));
    assert_eq!((planes[2].stride, planes[2].rows), (2, 2));
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

/// WHY: `uridecodebin` defaulted to `expose-all-streams = true` with unbounded
/// caps and left `avdec_*` `max-threads` at `0` (auto = one frame thread per
/// logical CPU), so every stacked video decoder on an A/V container also
/// demuxed audio into `multiqueue` and spawned 16 libav frame threads + frame
/// pools on a 16-core host, while an audio-only `.wav` prerolled the audio pad
/// and stalled video preroll for `PREROLL_WAIT`. Closes the class of video
/// decode pipelines exposing non-video branches or spawning unbounded software
/// decoder thread pools under concurrency.
#[test]
fn video_pipeline_ignores_audio_streams_and_bounds_decoder_threads() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let av_fixture = manifest.join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4");
    let wav_fixture = manifest.join("../../tests/fixtures/fixture-stereo-48k-2s.wav");
    crate::gst::ensure_init().expect("GStreamer initializes");

    let wav_started = Instant::now();
    let wav_res = crate::gst::GstDecoder::open(&wav_fixture, OutputMode::Cpu);
    let wav_ms = wav_started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "DECODER_WAV_REJECT wav_ok={} wav_ms={wav_ms:.2}",
        wav_res.is_ok()
    );

    let inspect_pipeline = |pipeline: &gst::Pipeline| -> (bool, u32, Vec<(String, i32)>) {
        let src = pipeline.by_name("src").expect("uridecodebin src");
        let expose_all: bool = src.property("expose-all-streams");
        let pads = src.num_src_pads() as u32;
        let mut max_threads = Vec::new();
        let mut iter = pipeline.iterate_recurse();
        while let Ok(Some(element)) = iter.next() {
            let Some(factory) = element.factory() else {
                continue;
            };
            let klass = factory
                .metadata(gst::ELEMENT_METADATA_KLASS)
                .unwrap_or_default();
            if klass.contains("Decoder")
                && klass.contains("Video")
                && element.find_property("max-threads").is_some()
            {
                max_threads.push((
                    factory.name().to_string(),
                    element.property::<i32>("max-threads"),
                ));
            }
        }
        if max_threads.is_empty()
            && let Ok(src_bin) = src.downcast::<gst::Bin>()
            && let Ok(avdec) = gst::ElementFactory::make("avdec_h264").build()
        {
            src_bin.add(&avdec).expect("attach probe avdec_h264");
            max_threads.push((
                "avdec_h264".to_string(),
                avdec.property::<i32>("max-threads"),
            ));
            let _ = src_bin.remove(&avdec);
        }
        (expose_all, pads, max_threads)
    };

    let mut opened = Vec::new();
    let mut thread_budgets = Vec::new();
    for idx in 0..8 {
        let pipe = open_pipeline(&av_fixture, OutputMode::Cpu).expect("av fixture opens");
        let (expose_all, pads, threads) = inspect_pipeline(&pipe.pipeline);
        println!("DECODER_PIPE idx={idx} expose_all={expose_all} pads={pads} threads={threads:?}");
        opened.push(pipe);
        thread_budgets.push((expose_all, pads, threads));
    }

    for pipe in opened.drain(..) {
        let _ = pipe.pipeline.set_state(gst::State::Null);
        drop(pipe);
    }
    let settle_deadline = Instant::now() + Duration::from_secs(2);
    while ACTIVE_VIDEO_PIPELINES.load(Ordering::Relaxed) > 0 && Instant::now() < settle_deadline {
        std::thread::yield_now();
    }
    let after_drop = open_pipeline(&av_fixture, OutputMode::Cpu).expect("av fixture reopens");
    let (_, _, restored_threads) = inspect_pipeline(&after_drop.pipeline);
    let _ = after_drop.pipeline.set_state(gst::State::Null);
    drop(after_drop);

    assert!(wav_res.is_err(), "audio-only wav must not open as video");
    assert!(
        wav_ms < 1000.0,
        "audio-only wav rejection took {wav_ms:.1} ms (expected < 1000 ms without audio pad preroll)"
    );
    for (idx, (expose_all, pads, threads)) in thread_budgets.iter().enumerate() {
        assert!(
            !expose_all,
            "pipeline {idx} left expose-all-streams enabled on uridecodebin"
        );
        assert_eq!(
            *pads, 1,
            "pipeline {idx} exposed {pads} pads on an A/V file instead of 1 video pad"
        );
        assert!(
            !threads.is_empty(),
            "pipeline {idx} plugged no video decoder with max-threads"
        );
        for (name, mt) in threads {
            assert!(
                (1..=MAX_DECODER_THREADS as i32).contains(mt),
                "pipeline {idx} decoder {name} has unbounded max-threads={mt} (expected 1..={MAX_DECODER_THREADS})"
            );
        }
    }
    let cpus = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4);
    let single_budget = cpus.clamp(1, MAX_DECODER_THREADS) as i32;
    let multi_floor = cpus.clamp(1, MIN_DECODER_THREADS) as i32;
    let eighth_cap = (cpus / 8).clamp(multi_floor as usize, MAX_DECODER_THREADS) as i32;
    let last_mt = thread_budgets[7].2[0].1;
    assert!(
        last_mt <= eighth_cap,
        "eighth concurrent decoder max-threads={last_mt} exceeded cap={eighth_cap} on {cpus}-CPU host"
    );
    if single_budget > multi_floor {
        assert!(
            restored_threads[0].1 > last_mt,
            "dropping concurrent pipelines did not restore a larger thread budget: eighth={last_mt}, restored={}",
            restored_threads[0].1
        );
    }
}

/// WHY: Hardcoding `video/x-raw,format=NV12` on the CPU `appsink` forced
/// `videoconvert` to repack every planar `I420` frame from software decoders
/// (`avdec_h264`, `avdec_h265`) into `NV12` on the CPU, and hardcoding
/// `PixelFormat::Nv12` in `sample_source_info` and `sample_to_cpu_frame`
/// dropped plane 2 or mislabelled non-NV12 samples. Closes the class of CPU
/// decode format negotiation and plane-extraction mismatches across `I420`,
/// `NV12`, padded `GstVideoMeta` strides and offsets, and 10-bit or non-4:2:0
/// formats. Does not cover GPU DMABuf import; `a_failed_dmabuf_open_records_the_decoder_it_plugged`
/// and `linear_nv12_surface_is_importable` cover that path.
#[test]
fn cpu_pipeline_passes_i420_and_nv12_with_exact_plane_strides_and_offsets_and_rejects_unconverted_10bit() {
    use aria_core::{ColorSpace, Rotation, SourceInfo};

    crate::gst::ensure_init().expect("GStreamer initializes");
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let av_fixture = manifest.join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4");

    let opened = open_pipeline(&av_fixture, OutputMode::Cpu).expect("CPU pipeline opens");
    let mut filter_caps_str = None;
    let mut converter_passthrough = None;
    let mut iter = opened.pipeline.iterate_elements();
    while let Ok(Some(element)) = iter.next() {
        let Some(factory) = element.factory() else {
            continue;
        };
        if factory.name() == "capsfilter" && element.name() != "src" {
            let caps: gst::Caps = element.property("caps");
            let s = caps.to_string();
            if s.contains("I420") || s.contains("NV12") {
                filter_caps_str = Some(s);
            }
        } else if factory.name() == "videoconvert" {
            let sink_fmt = element
                .static_pad("sink")
                .and_then(|p| p.current_caps())
                .and_then(|c| gst_video::VideoInfo::from_caps(&c).ok())
                .map(|v| v.format());
            let src_fmt = element
                .static_pad("src")
                .and_then(|p| p.current_caps())
                .and_then(|c| gst_video::VideoInfo::from_caps(&c).ok())
                .map(|v| v.format());
            converter_passthrough = Some((sink_fmt, src_fmt));
        }
    }
    let _ = opened.pipeline.set_state(gst::State::Null);
    let caps_str = filter_caps_str.expect("CPU capsfilter configured");
    assert!(
        caps_str.contains("I420") && caps_str.contains("NV12"),
        "CPU capsfilter must accept both I420 and NV12, got: {caps_str}"
    );
    assert!(
        !caps_str.contains("I420_10LE") && !caps_str.contains("P010"),
        "CPU capsfilter must keep 10-bit formats on videoconvert, got: {caps_str}"
    );
    if let Some((Some(gst_video::VideoFormat::I420), src_fmt)) = converter_passthrough {
        assert_eq!(
            src_fmt,
            Some(gst_video::VideoFormat::I420),
            "videoconvert converted an I420 software decode instead of passing it through"
        );
        assert_eq!(opened.info.pixel_format, PixelFormat::Yuv420p);
    }
    drop(opened);

    let source_info = SourceInfo {
        coded_size: (6, 4),
        display_size: (6, 4),
        rotation: Rotation::None,
        pixel_format: PixelFormat::Yuv420p,
        color: ColorSpace::BT709,
        frame_rate: Rational::new(30, 1),
        time_base: Rational::new(crate::gst::NS_PER_SEC as i32, 1),
        duration: None,
    };

    // 1. Padded I420 sample with non-contiguous offsets and sentinel bytes per row.
    let i420_caps = gst_video::VideoInfo::builder(gst_video::VideoFormat::I420, 6, 4)
        .fps(gst::Fraction::new(30, 1))
        .build()
        .expect("i420 video info")
        .to_caps()
        .expect("i420 caps");
    let i420_strides: [i32; 3] = [16, 12, 10];
    let i420_offsets: [usize; 3] = [32, 160, 224];
    let mut raw = vec![0xEEu8; 300];
    for r in 0..4usize {
        for c in 0..16usize {
            raw[i420_offsets[0] + r * 16 + c] = (0x10 + r * 16 + c) as u8;
        }
    }
    for r in 0..2usize {
        for c in 0..12usize {
            raw[i420_offsets[1] + r * 12 + c] = (0x80 + r * 12 + c) as u8;
        }
        for c in 0..10usize {
            raw[i420_offsets[2] + r * 10 + c] = (0xC0 + r * 10 + c) as u8;
        }
    }
    let mut gst_buf = gst::Buffer::from_mut_slice(raw.clone());
    {
        let buf_mut = gst_buf.get_mut().expect("mutable buffer");
        buf_mut.set_pts(gst::ClockTime::from_nseconds(33_333_333));
        gst_video::VideoMeta::add_full(
            buf_mut,
            gst_video::VideoFrameFlags::empty(),
            gst_video::VideoFormat::I420,
            6,
            4,
            &i420_offsets,
            &i420_strides,
        )
        .expect("attach I420 VideoMeta");
    }
    let i420_sample = gst::Sample::builder()
        .buffer(&gst_buf)
        .caps(&i420_caps)
        .build();
    let decoded = crate::gst::sample_to_cpu_frame(&i420_sample, &source_info)
        .expect("decode padded I420 sample");
    assert_eq!(decoded.format, PixelFormat::Yuv420p);
    let cpu = decoded.cpu().expect("CPU image");
    assert_eq!(cpu.planes.len(), 3);
    assert_eq!((cpu.planes[0].stride, cpu.planes[0].rows), (16, 4));
    assert_eq!((cpu.planes[1].stride, cpu.planes[1].rows), (12, 2));
    assert_eq!((cpu.planes[2].stride, cpu.planes[2].rows), (10, 2));
    assert_eq!(
        cpu.planes[0].data,
        &raw[i420_offsets[0]..i420_offsets[0] + 16 * 4]
    );
    assert_eq!(
        cpu.planes[1].data,
        &raw[i420_offsets[1]..i420_offsets[1] + 12 * 2]
    );
    assert_eq!(
        cpu.planes[2].data,
        &raw[i420_offsets[2]..i420_offsets[2] + 10 * 2]
    );

    let from_bytes = cpu_planes_from_bytes(
        &raw,
        PixelFormat::Yuv420p,
        6,
        4,
        &i420_strides,
        &i420_offsets,
    )
    .expect("extract padded I420 from bytes");
    assert_eq!(from_bytes[0].data, cpu.planes[0].data);
    assert_eq!(from_bytes[1].data, cpu.planes[1].data);
    assert_eq!(from_bytes[2].data, cpu.planes[2].data);

    // 2. Padded NV12 sample with non-contiguous offsets.
    let nv12_caps = gst_video::VideoInfo::builder(gst_video::VideoFormat::Nv12, 6, 4)
        .fps(gst::Fraction::new(30, 1))
        .build()
        .expect("nv12 video info")
        .to_caps()
        .expect("nv12 caps");
    let nv12_strides: [i32; 2] = [16, 12];
    let nv12_offsets: [usize; 2] = [16, 112];
    let mut nv12_buf = gst::Buffer::from_mut_slice(raw.clone());
    {
        let buf_mut = nv12_buf.get_mut().expect("mutable buffer");
        gst_video::VideoMeta::add_full(
            buf_mut,
            gst_video::VideoFrameFlags::empty(),
            gst_video::VideoFormat::Nv12,
            6,
            4,
            &nv12_offsets,
            &nv12_strides,
        )
        .expect("attach NV12 VideoMeta");
    }
    let nv12_sample = gst::Sample::builder()
        .buffer(&nv12_buf)
        .caps(&nv12_caps)
        .build();
    let nv12_decoded = crate::gst::sample_to_cpu_frame(&nv12_sample, &source_info)
        .expect("decode padded NV12 sample");
    assert_eq!(nv12_decoded.format, PixelFormat::Nv12);
    let nv12_cpu = nv12_decoded.cpu().expect("NV12 CPU image");
    assert_eq!(nv12_cpu.planes.len(), 2);
    assert_eq!((nv12_cpu.planes[0].stride, nv12_cpu.planes[0].rows), (16, 4));
    assert_eq!((nv12_cpu.planes[1].stride, nv12_cpu.planes[1].rows), (12, 2));
    assert_eq!(
        nv12_cpu.planes[0].data,
        &raw[nv12_offsets[0]..nv12_offsets[0] + 16 * 4]
    );
    assert_eq!(
        nv12_cpu.planes[1].data,
        &raw[nv12_offsets[1]..nv12_offsets[1] + 12 * 2]
    );

    // 3. Every 10-bit or non-4:2:0 GStreamer format is rejected rather than
    // truncated or mislabelled as 8-bit Nv12 or Yuv420p.
    for unsupported in [
        gst_video::VideoFormat::I42010le,
        gst_video::VideoFormat::P01010le,
        gst_video::VideoFormat::Y444,
        gst_video::VideoFormat::Rgba,
    ] {
        assert!(
            gst_cpu_format(unsupported).is_err(),
            "unsupported format {unsupported:?} was accepted as 8-bit CPU YUV"
        );
        let bad_caps = gst_video::VideoInfo::builder(unsupported, 6, 4)
            .fps(gst::Fraction::new(30, 1))
            .build()
            .expect("video info")
            .to_caps()
            .expect("caps");
        let bad_buf = gst::Buffer::from_mut_slice(vec![0u8; 512]);
        let bad_sample = gst::Sample::builder()
            .buffer(&bad_buf)
            .caps(&bad_caps)
            .build();
        assert!(
            crate::gst::sample_to_cpu_frame(&bad_sample, &source_info).is_err(),
            "sample_to_cpu_frame accepted {unsupported:?} without conversion"
        );
    }
}
