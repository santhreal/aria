//! DMA-BUF surface checks, the tiled-fallback report, and the cause named
//! when a DMABuf pipeline cannot open.

use super::*;

#[test]
fn packed_nv12_bytes_split() {
    let w = 4u32;
    let h = 4u32;
    let mut bytes = vec![16u8; (w * h) as usize];
    bytes.extend(std::iter::repeat_n(128u8, (w * h / 2) as usize));
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
    let result = try_open(&uri, true, Some(&watch));
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
    assert!(
        open_latched(&latch, true, |watch| try_open(
            &unreadable,
            watch.is_some(),
            watch
        ))
        .is_err()
    );
    assert_eq!(
        latch.get(),
        None,
        "an unreadable file turned DMABuf off for the process"
    );

    // When no ranked video decoder in the registry offers memory:DMABuf,
    // `open_latched_for` skips the doomed DMABuf pipeline, opens CPU directly,
    // and latches only when the CPU open succeeds.
    for (cpu_opens, expected_latched) in [(true, true), (false, false)] {
        let latch = OnceLock::new();
        let tried = RefCell::new(Vec::new());
        let got = open_latched_for(&latch, true, false, |dmabuf, watch| {
            tried.borrow_mut().push((dmabuf, watch.is_some()));
            if cpu_opens {
                Ok("cpu")
            } else {
                Err(DecodeError::Open("cpu could not open".into()))
            }
        });
        assert_eq!(
            tried.into_inner(),
            vec![(false, true)],
            "has_dmabuf_decoder=false must attempt only CPU with decoder recording"
        );
        assert_eq!(got.is_ok(), cpu_opens);
        assert_eq!(latch.get().is_some(), expected_latched);
    }
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
    let settle_deadline = Instant::now() + Duration::from_secs(10);
    while ACTIVE_VIDEO_PIPELINES.load(Ordering::Relaxed) > 0 && Instant::now() < settle_deadline {
        std::thread::sleep(Duration::from_millis(5));
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

    // Intra-frame / WPP slice-parallel 4K codecs (`avdec_h265`, `avdec_prores`)
    // must enable slice-only threading so open and seek preroll do not buffer
    // `max-threads` frames, while `avdec_h264` must keep frame threading so
    // single-slice H.264 streams decode across multiple threads during forward
    // roll and renderer lookahead.
    let probe_bin = gst::ElementFactory::make("uridecodebin")
        .build()
        .expect("uridecodebin")
        .downcast::<gst::Bin>()
        .expect("uridecodebin is a bin");
    configure_video_uridecodebin(&probe_bin, single_budget);
    let mut checked_codecs = 0usize;
    for (codec, expect_slice_only) in [
        ("avdec_h264", false),
        ("avdec_h265", true),
        ("avdec_prores", true),
    ] {
        let Ok(dec) = gst::ElementFactory::make(codec).build() else {
            continue;
        };
        if dec.find_property("thread-type").is_none() {
            continue;
        }
        probe_bin.add(&dec).expect("add decoder to configured bin");
        let value = dec.property_value("thread-type");
        let flags = gst::glib::FlagsClass::with_type(value.type_())
            .expect("thread-type is a GFlags property");
        let slice_only =
            flags.is_set_by_nick(&value, "slice") && !flags.is_set_by_nick(&value, "frame");
        assert_eq!(
            slice_only, expect_slice_only,
            "{codec} slice-only threading mismatch (expected {expect_slice_only})"
        );
        let _ = probe_bin.remove(&dec);
        checked_codecs += 1;
    }
    assert_eq!(
        checked_codecs, 3,
        "expected avdec_h264, avdec_h265, and avdec_prores to be checked"
    );
}

#[test]
fn nv12_from_video_frame_packs_i420_and_i420_10le_without_videoconvert_unpack() {
    gst::init().expect("GStreamer initializes");

    // 4x2 I420 frame with distinct Y, U, V values.
    let info_i420 = gst_video::VideoInfo::builder(gst_video::VideoFormat::I420, 4, 2)
        .build()
        .expect("i420 info");
    let mut buf_i420 = gst::Buffer::with_size(info_i420.size()).expect("i420 buffer");
    {
        let buf_mut = buf_i420.get_mut().expect("mutable i420 buffer");
        let mut frame =
            gst_video::VideoFrameRef::from_buffer_ref_writable(buf_mut, &info_i420).expect("map");
        let y_stride = frame.plane_stride()[0] as usize;
        let u_stride = frame.plane_stride()[1] as usize;
        let v_stride = frame.plane_stride()[2] as usize;
        frame.plane_data_mut(0).unwrap()[..4].copy_from_slice(&[10, 20, 30, 40]);
        frame.plane_data_mut(0).unwrap()[y_stride..y_stride + 4].copy_from_slice(&[50, 60, 70, 80]);
        frame.plane_data_mut(1).unwrap()[..2].copy_from_slice(&[111, 122]);
        let _ = u_stride;
        frame.plane_data_mut(2).unwrap()[..2].copy_from_slice(&[211, 222]);
        let _ = v_stride;
    }
    let frame_i420 =
        gst_video::VideoFrameRef::from_buffer_ref_readable(buf_i420.as_ref(), &info_i420)
            .expect("read i420");
    let planes_i420 = nv12_from_video_frame(&frame_i420, 2).expect("i420 -> nv12");
    assert_eq!(planes_i420.len(), 2);
    assert_eq!(planes_i420[0].rows, 2);
    assert_eq!(&planes_i420[0].data[..4], &[10, 20, 30, 40]);
    assert_eq!(
        &planes_i420[0].data[planes_i420[0].stride..planes_i420[0].stride + 4],
        &[50, 60, 70, 80]
    );
    assert_eq!(planes_i420[1].stride, 4);
    assert_eq!(planes_i420[1].rows, 1);
    assert_eq!(&planes_i420[1].data, &[111, 211, 122, 222]);

    // 4x2 I420_10LE frame: 10-bit values (val << 2) must shift down by 2 into 8-bit NV12.
    let info_10le = gst_video::VideoInfo::builder(gst_video::VideoFormat::I42010le, 4, 2)
        .build()
        .expect("i420_10le info");
    let mut buf_10le = gst::Buffer::with_size(info_10le.size()).expect("i420_10le buffer");
    {
        let buf_mut = buf_10le.get_mut().expect("mutable i420_10le buffer");
        let mut frame =
            gst_video::VideoFrameRef::from_buffer_ref_writable(buf_mut, &info_10le).expect("map");
        let y_stride = frame.plane_stride()[0] as usize;
        for (i, px) in [16u16, 64, 128, 235].into_iter().enumerate() {
            let bytes = (px << 2).to_le_bytes();
            frame.plane_data_mut(0).unwrap()[i * 2..i * 2 + 2].copy_from_slice(&bytes);
        }
        for (i, px) in [32u16, 96, 160, 240].into_iter().enumerate() {
            let bytes = (px << 2).to_le_bytes();
            frame.plane_data_mut(0).unwrap()[y_stride + i * 2..y_stride + i * 2 + 2]
                .copy_from_slice(&bytes);
        }
        for (i, u_px) in [90u16, 180].into_iter().enumerate() {
            let bytes = (u_px << 2).to_le_bytes();
            frame.plane_data_mut(1).unwrap()[i * 2..i * 2 + 2].copy_from_slice(&bytes);
        }
        for (i, v_px) in [100u16, 200].into_iter().enumerate() {
            let bytes = (v_px << 2).to_le_bytes();
            frame.plane_data_mut(2).unwrap()[i * 2..i * 2 + 2].copy_from_slice(&bytes);
        }
    }
    let frame_10le =
        gst_video::VideoFrameRef::from_buffer_ref_readable(buf_10le.as_ref(), &info_10le)
            .expect("read i420_10le");
    let planes_10le = nv12_from_video_frame(&frame_10le, 2).expect("i420_10le -> nv12");
    assert_eq!(planes_10le[0].stride, 4);
    assert_eq!(planes_10le[0].rows, 2);
    assert_eq!(&planes_10le[0].data, &[16, 64, 128, 235, 32, 96, 160, 240]);
    assert_eq!(planes_10le[1].stride, 4);
    assert_eq!(planes_10le[1].rows, 1);
    assert_eq!(&planes_10le[1].data, &[90, 100, 180, 200]);
}
