//! Unit and integration tests for the audio analyzer contribution point.
//!
//! WHY: Closes the defect classes where:
//! - malformed or duplicate analyzer registrations are accepted or outlive an
//!   unmounted/failed plugin;
//! - `run_analyzer` fails to convert sample frames to exact sequence/clip ticks
//!   on a click track, ignores per-block cancellation, or lets a panicking
//!   analyzer corrupt the project or stay mounted;
//! - applying an `AnalyzerResult` splits beats, markers, and silence ranges
//!   across multiple undo steps instead of committing one reversible step named
//!   after the analyzer.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aria_core::{AudioEncoderConfig, Rational, RationalTime, TimeRange};
use aria_engine::{Engine, EngineConfig};
use aria_models::{ClipId, MarkerColor, MarkerKind, MediaSource, Project, TrackKind};
use aria_render::{EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE};
use parking_lot::Mutex;

use crate::analyzers::{
    AnalyzerError, AnalyzerMarker, AnalyzerResult, AnalyzerRun, AnalyzerScope, AnalyzerSpec,
    BeatAnalyzer, SHIPPED_BEAT_ANALYZER_ID, SHIPPED_LOUDNESS_ANALYZER_ID,
    SHIPPED_SILENCE_ANALYZER_ID, apply_analyzer_result, frame_to_tick, register_shipped_analyzers,
};
use crate::audio_host::AudioSource;
use crate::context::{PluginContext, PluginFault};
use crate::error::PluginError;
use crate::plugin::{Plugin, PluginManifest};
use crate::project_host::{Origin, ProjectHost};
use crate::registration::ContributionKind;

const R24: Rational = Rational::FPS_24;
const RATE: u32 = EXPORT_AUDIO_RATE;
const CHANNELS: usize = EXPORT_AUDIO_CHANNELS as usize;

/// Write a stereo WAV file of `total_frames` at 48 kHz where each frame in
/// `click_frames` starts a 10 ms 1 kHz burst at 0.9 amplitude and all other
/// frames are silent.
fn write_click_track(path: &Path, total_frames: i64, click_frames: &[i64]) {
    let mut sink =
        aria_encoder::open_audio_encoder(path, AudioEncoderConfig::new(RATE, CHANNELS as u16))
            .expect("open WAV encoder");
    let burst_frames = (RATE as i64) / 100; // 10 ms = 480 frames
    let chunk_frames = 2_400i64;
    let mut block = Vec::with_capacity(chunk_frames as usize * CHANNELS);
    let mut written = 0i64;
    while written < total_frames {
        let count = (total_frames - written).min(chunk_frames);
        block.clear();
        for offset in 0..count {
            let frame = written + offset;
            let in_click = click_frames
                .iter()
                .any(|&start| frame >= start && frame < start + burst_frames);
            let sample = if in_click {
                let phase = std::f32::consts::TAU * 1_000.0 * (frame as f32 / RATE as f32);
                0.9 * phase.sin()
            } else {
                0.0
            };
            for _ in 0..CHANNELS {
                block.push(sample);
            }
        }
        sink.push_audio(
            &block,
            RationalTime::new(written, Rational::new(RATE as i32, 1)),
        )
        .expect("push audio block");
        written += count;
    }
    sink.finish().expect("finish WAV");
}

/// Write a stereo WAV file with `segments` of `(duration_frames, amplitude)`.
fn write_envelope_track(path: &Path, segments: &[(i64, f32)]) {
    let mut sink =
        aria_encoder::open_audio_encoder(path, AudioEncoderConfig::new(RATE, CHANNELS as u16))
            .expect("open WAV encoder");
    let mut written = 0i64;
    for &(frames, amp) in segments {
        let mut block = Vec::with_capacity(frames as usize * CHANNELS);
        for offset in 0..frames {
            let frame = written + offset;
            let phase = std::f32::consts::TAU * 440.0 * (frame as f32 / RATE as f32);
            let sample = amp * phase.sin();
            for _ in 0..CHANNELS {
                block.push(sample);
            }
        }
        sink.push_audio(
            &block,
            RationalTime::new(written, Rational::new(RATE as i32, 1)),
        )
        .expect("push audio segment");
        written += frames;
    }
    sink.finish().expect("finish WAV");
}

fn project_with_wav(path: &Path, duration_ticks: i64) -> (Project, ClipId) {
    let mut project = Project::new("analyzer test", R24);
    let media = project.add_media(MediaSource::new(path, 0, 0, R24, duration_ticks, true));
    let lane = project.add_track(TrackKind::Audio, "A1");
    let clip = project
        .add_clip(
            lane,
            media,
            TimeRange::at_rate(0, duration_ticks, R24),
            RationalTime::new(0, R24),
        )
        .expect("add audio clip");
    (project, clip)
}

#[test]
fn registration_validates_id_label_duplicates_and_unmount_removes_entry() {
    let ctx = PluginContext::new();
    let noop = |_rate: u32, _channels: u16| -> Box<dyn AnalyzerRun> {
        struct Empty;
        impl AnalyzerRun for Empty {
            fn block(&mut self, _: i64, _: &[f32]) -> ControlFlow<()> {
                ControlFlow::Continue(())
            }
            fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
                Ok(AnalyzerResult::default())
            }
        }
        Box::new(Empty)
    };

    // Empty id refused.
    assert!(matches!(
        ctx.register_analyzer(AnalyzerSpec::new("", "Label", AnalyzerScope::Clip), noop),
        Err(PluginError::AnalyzerRefused(_))
    ));
    // Un-namespaced id refused.
    assert!(matches!(
        ctx.register_analyzer(
            AnalyzerSpec::new("beats", "Label", AnalyzerScope::Clip),
            noop
        ),
        Err(PluginError::AnalyzerRefused(_))
    ));
    // Invalid characters refused.
    assert!(matches!(
        ctx.register_analyzer(
            AnalyzerSpec::new("bad.id/slash", "Label", AnalyzerScope::Clip),
            noop
        ),
        Err(PluginError::AnalyzerRefused(_))
    ));
    // Blank label refused.
    assert!(matches!(
        ctx.register_analyzer(
            AnalyzerSpec::new("example.blank", "   ", AnalyzerScope::Clip),
            noop
        ),
        Err(PluginError::AnalyzerRefused(_))
    ));

    // Valid registration succeeds and lists under ContributionKind::Analyzer.
    let guard = ctx
        .register_analyzer(
            AnalyzerSpec::new("example.beats", "  Custom Beats  ", AnalyzerScope::Clip)
                .with_category("  Rhythm  "),
            noop,
        )
        .expect("valid registration");
    assert_eq!(
        guard.contribution().map(|c| (c.kind, c.id.as_str())),
        Some((ContributionKind::Analyzer, "example.beats"))
    );
    assert_eq!(ctx.analyzers().len(), 1);
    assert_eq!(ctx.analyzers()[0].spec.label, "Custom Beats");
    assert_eq!(ctx.analyzers()[0].spec.category.as_deref(), Some("Rhythm"));

    // Duplicate id refused while registered.
    let dup = ctx.register_analyzer(
        AnalyzerSpec::new("example.beats", "Second", AnalyzerScope::Sequence),
        noop,
    );
    assert!(
        matches!(&dup, Err(PluginError::AnalyzerRefused(msg)) if msg.contains("already registered")),
        "unexpected duplicate result: {dup:?}"
    );

    // Dropping the registration guard removes the analyzer and allows re-registration.
    drop(guard);
    assert!(ctx.analyzers().is_empty());

    // Mounting a plugin retains its analyzer and unmounting removes it.
    struct TestPlugin;
    impl Plugin for TestPlugin {
        fn manifest(&self) -> &PluginManifest {
            static MANIFEST: PluginManifest =
                PluginManifest::new("test.analyzer_mount", "Analyzer Mount", "1.0.0", &[], &[]);
            &MANIFEST
        }
        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            register_shipped_analyzers(
                ctx,
                aria_analysis::BeatConfig::default(),
                aria_analysis::SilenceConfig::default(),
                aria_analysis::LoudnessConfig::default(),
            )?;
            Ok(())
        }
    }

    ctx.mount(&TestPlugin).expect("mounts plugin");
    assert_eq!(ctx.analyzers().len(), 3);
    assert_eq!(ctx.analyzers_for_scope(AnalyzerScope::Clip).len(), 1);
    assert_eq!(ctx.analyzers_for_scope(AnalyzerScope::Sequence).len(), 1);
    assert_eq!(ctx.analyzers_for_scope(AnalyzerScope::InOut).len(), 1);
    let mounted = ctx.mounted();
    assert_eq!(mounted.len(), 1);
    assert_eq!(
        mounted[0]
            .contributions
            .iter()
            .filter(|c| c.kind == ContributionKind::Analyzer)
            .count(),
        3
    );

    ctx.unmount("test.analyzer_mount").expect("unmounts plugin");
    assert!(ctx.analyzers().is_empty());
}

#[test]
fn beat_analyzer_over_click_track_produces_exact_beat_ticks_and_undoes_in_one_step() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("clicks.wav");
    // 2 seconds at 48 kHz = 96,000 frames (48 ticks at 24 fps).
    // Clicks at 0.0 s (tick 0), 0.5 s (tick 12), 1.0 s (tick 24), 1.5 s (tick 36).
    write_click_track(&wav, 96_000, &[0, 24_000, 48_000, 72_000]);
    let (project, clip_id) = project_with_wav(&wav, 48);

    let ctx = PluginContext::new();
    let _reg = ctx
        .register_analyzer(BeatAnalyzer::spec(), BeatAnalyzer::default())
        .expect("register beat analyzer");
    let engine = Arc::new(Mutex::new(
        Engine::with_project(EngineConfig::default(), project).expect("engine"),
    ));
    let host = Arc::new(ProjectHost::new(Arc::clone(&engine), &ctx));

    let cancel = AtomicBool::new(false);
    let mut progress_steps = Vec::new();
    let snapshot = host.snapshot();
    let result = ctx
        .run_analyzer_with_progress(
            SHIPPED_BEAT_ANALYZER_ID,
            &snapshot,
            AudioSource::Clip(clip_id),
            TimeRange::at_rate(0, 48, R24),
            &cancel,
            |mixed, total| progress_steps.push((mixed, total)),
        )
        .expect("beat analyzer succeeds");

    assert_eq!(
        result.beat_ticks(R24),
        vec![0, 12, 24, 36],
        "expected exact beat ticks [0, 12, 24, 36], got frames {:?}",
        result.beats
    );
    assert_eq!(
        result.clip_beat_ticks(&snapshot, clip_id),
        Some(vec![0, 12, 24, 36])
    );
    assert_eq!(progress_steps.first().copied(), Some((0, 96_000)));
    assert_eq!(progress_steps.last().copied(), Some((96_000, 96_000)));
    assert_eq!(result.summary(), "4 beats");

    // Apply result as one undo step named "Detect Beats".
    ctx.apply_analyzer(
        &host,
        Origin::Editor,
        SHIPPED_BEAT_ANALYZER_ID,
        AudioSource::Clip(clip_id),
        &result,
    )
    .expect("apply beat result");

    assert_eq!(
        host.snapshot()
            .timeline()
            .clip(clip_id)
            .expect("clip")
            .beats,
        vec![0, 12, 24, 36]
    );
    assert_eq!(
        engine
            .lock()
            .history_entries()
            .into_iter()
            .filter(|e| e.applied)
            .map(|e| e.label.into_owned())
            .collect::<Vec<_>>(),
        vec!["Detect Beats".to_owned()]
    );

    // One undo step restores the empty beat grid.
    assert!(host.undo().expect("undo"));
    assert!(
        host.snapshot()
            .timeline()
            .clip(clip_id)
            .expect("clip")
            .beats
            .is_empty()
    );

    // One redo step restores the exact beat grid.
    assert!(host.redo().expect("redo"));
    assert_eq!(
        host.snapshot()
            .timeline()
            .clip(clip_id)
            .expect("clip")
            .beats,
        vec![0, 12, 24, 36]
    );
}

#[test]
fn silence_and_loudness_analyzers_detect_ranges_and_loudness_and_undo_in_one_step() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("envelope.wav");
    // 0.0..0.5s (24,000 frames): 0.5 amp tone
    // 0.5..1.5s (48,000 frames): silence (ticks 12..36 at 24 fps)
    // 1.5..2.0s (24,000 frames): 0.5 amp tone
    write_envelope_track(&wav, &[(24_000, 0.5), (48_000, 0.0), (24_000, 0.5)]);
    let (project, _clip_id) = project_with_wav(&wav, 48);

    let ctx = PluginContext::new();
    let _regs = register_shipped_analyzers(
        &ctx,
        aria_analysis::BeatConfig::default(),
        aria_analysis::SilenceConfig::default(),
        aria_analysis::LoudnessConfig::default(),
    )
    .expect("register shipped analyzers");
    let engine = Arc::new(Mutex::new(
        Engine::with_project(EngineConfig::default(), project).expect("engine"),
    ));
    let host = Arc::new(ProjectHost::new(Arc::clone(&engine), &ctx));
    let cancel = AtomicBool::new(false);
    let range = TimeRange::at_rate(0, 48, R24);

    // 1. Silence analyzer finds the 0.5..1.5s silent region and applies it as
    // a range marker on the sequence in one undo step.
    let silence_res = ctx
        .run_analyzer(
            SHIPPED_SILENCE_ANALYZER_ID,
            &host.snapshot(),
            AudioSource::Open,
            range,
            &cancel,
        )
        .expect("silence analyzer runs");
    assert_eq!(silence_res.ranges.len(), 1);
    assert_eq!(frame_to_tick(silence_res.ranges[0].start, RATE, R24), 12);
    assert_eq!(frame_to_tick(silence_res.ranges[0].end, RATE, R24), 36);

    ctx.apply_analyzer(
        &host,
        Origin::Editor,
        SHIPPED_SILENCE_ANALYZER_ID,
        AudioSource::Open,
        &silence_res,
    )
    .expect("apply silence ranges");

    let markers = host.snapshot().timeline().markers().to_vec();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].tick, RationalTime::new(12, R24));
    assert_eq!(markers[0].duration, RationalTime::new(24, R24));
    assert_eq!(markers[0].name, "Silence");
    assert_eq!(markers[0].kind, MarkerKind::Segmentation);
    assert_eq!(
        engine
            .lock()
            .history_entries()
            .into_iter()
            .filter(|e| e.applied)
            .map(|e| e.label.into_owned())
            .collect::<Vec<_>>(),
        vec!["Detect Silence".to_owned()]
    );

    assert!(host.undo().expect("undo silence marker"));
    assert!(host.snapshot().timeline().markers().is_empty());
    assert!(host.redo().expect("redo silence marker"));
    assert_eq!(host.snapshot().timeline().markers().len(), 1);
    assert_eq!(
        host.snapshot().timeline().markers()[0].duration,
        RationalTime::new(24, R24)
    );

    // 2. Loudness analyzer computes finite integrated LUFS and true peak dBFS.
    let loudness_res = ctx
        .run_analyzer(
            SHIPPED_LOUDNESS_ANALYZER_ID,
            &host.snapshot(),
            AudioSource::Open,
            range,
            &cancel,
        )
        .expect("loudness analyzer runs");
    let loudness = loudness_res.loudness.expect("loudness populated");
    assert!(loudness.integrated_lufs.is_finite());
    assert!(loudness.true_peak_db.is_finite());
    assert!((loudness.true_peak_db - (-6.0206)).abs() < 0.1);
    assert!(loudness_res.summary().contains("LUFS"));
}

#[test]
fn combined_beats_and_clip_markers_apply_and_undo_as_one_step() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("short.wav");
    write_click_track(&wav, 48_000, &[0, 24_000]);
    let (project, clip_id) = project_with_wav(&wav, 24);

    let ctx = PluginContext::new();
    let engine = Arc::new(Mutex::new(
        Engine::with_project(EngineConfig::default(), project).expect("engine"),
    ));
    let host = Arc::new(ProjectHost::new(Arc::clone(&engine), &ctx));

    let result = AnalyzerResult {
        beats: vec![0, 24_000],
        markers: vec![
            AnalyzerMarker::new(0, 0, "Downbeat", MarkerColor::Teal),
            AnalyzerMarker::new(24_000, 12_000, "Accent", MarkerColor::Orange),
        ],
        ranges: Vec::new(),
        loudness: None,
    };

    apply_analyzer_result(
        &host,
        Origin::Editor,
        "Combined Clip Analysis",
        AudioSource::Clip(clip_id),
        &result,
    )
    .expect("apply combined result");

    let snap = host.snapshot();
    let clip = snap.timeline().clip(clip_id).expect("clip");
    assert_eq!(clip.beats, vec![0, 12]);
    assert_eq!(clip.markers().len(), 2);
    assert_eq!(clip.markers()[0].tick, RationalTime::new(0, R24));
    assert_eq!(clip.markers()[0].duration, RationalTime::new(0, R24));
    assert_eq!(clip.markers()[0].name, "Downbeat");
    assert_eq!(clip.markers()[1].tick, RationalTime::new(12, R24));
    assert_eq!(clip.markers()[1].duration, RationalTime::new(6, R24));
    assert_eq!(clip.markers()[1].name, "Accent");
    assert_eq!(
        engine
            .lock()
            .history_entries()
            .into_iter()
            .filter(|e| e.applied)
            .map(|e| e.label.into_owned())
            .collect::<Vec<_>>(),
        vec!["Combined Clip Analysis".to_owned()]
    );

    // Single undo removes both beats and clip markers.
    assert!(host.undo().expect("undo combined step"));
    let undone = host.snapshot();
    let undone_clip = undone.timeline().clip(clip_id).expect("clip");
    assert!(undone_clip.beats.is_empty());
    assert!(undone_clip.markers().is_empty());

    // Single redo restores both beats and clip markers (including marker duration).
    assert!(host.redo().expect("redo combined step"));
    let redone = host.snapshot();
    let redone_clip = redone.timeline().clip(clip_id).expect("clip");
    assert_eq!(redone_clip.beats, vec![0, 12]);
    assert_eq!(redone_clip.markers().len(), 2);
    assert_eq!(redone_clip.markers()[1].duration, RationalTime::new(6, R24));
}

#[test]
fn cancel_stops_analyzer_within_one_block_and_skips_finish() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("long.wav");
    // 16,384 frames = 4 blocks of 4,096 frames.
    write_click_track(&wav, 16_384, &[0]);
    let (project, clip_id) = project_with_wav(&wav, 8);

    let cancel = Arc::new(AtomicBool::new(false));
    let blocks_seen = Arc::new(AtomicUsize::new(0));
    let finish_called = Arc::new(AtomicBool::new(false));

    let cancel_in_block = Arc::clone(&cancel);
    let blocks_in_block = Arc::clone(&blocks_seen);
    let finish_in_block = Arc::clone(&finish_called);

    let ctx = PluginContext::new();
    let _reg = ctx
        .register_analyzer(
            AnalyzerSpec::new("test.cancel_one_block", "Cancel Test", AnalyzerScope::Clip),
            move |_rate: u32, _channels: u16| -> Box<dyn AnalyzerRun> {
                struct CancellingRun {
                    cancel: Arc<AtomicBool>,
                    blocks: Arc<AtomicUsize>,
                    finished: Arc<AtomicBool>,
                }
                impl AnalyzerRun for CancellingRun {
                    fn block(&mut self, _first_frame: i64, _samples: &[f32]) -> ControlFlow<()> {
                        self.blocks.fetch_add(1, Ordering::SeqCst);
                        self.cancel.store(true, Ordering::Release);
                        ControlFlow::Continue(())
                    }
                    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
                        self.finished.store(true, Ordering::SeqCst);
                        Ok(AnalyzerResult::default())
                    }
                }
                Box::new(CancellingRun {
                    cancel: Arc::clone(&cancel_in_block),
                    blocks: Arc::clone(&blocks_in_block),
                    finished: Arc::clone(&finish_in_block),
                })
            },
        )
        .expect("register cancelling analyzer");

    let res = ctx.run_analyzer(
        "test.cancel_one_block",
        &project,
        AudioSource::Clip(clip_id),
        TimeRange::at_rate(0, 8, R24),
        &cancel,
    );
    assert_eq!(res, Err(AnalyzerError::Cancelled));
    assert_eq!(
        blocks_seen.load(Ordering::SeqCst),
        1,
        "cancel set in block 0 must stop before block 1"
    );
    assert!(
        !finish_called.load(Ordering::SeqCst),
        "cancelled run must not call finish()"
    );
}

#[test]
fn panicking_analyzer_fails_plugin_and_leaves_project_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("panic.wav");
    write_click_track(&wav, 8_192, &[0]);
    let (project, clip_id) = project_with_wav(&wav, 4);

    struct PanicPlugin;
    impl Plugin for PanicPlugin {
        fn manifest(&self) -> &PluginManifest {
            static MANIFEST: PluginManifest =
                PluginManifest::new("test.panic_analyzer", "Panic Analyzer", "1.0.0", &[], &[]);
            &MANIFEST
        }
        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            ctx.register_analyzer(
                AnalyzerSpec::new("test.panic_analyzer.crash", "Crash", AnalyzerScope::Clip),
                |_rate: u32, _channels: u16| -> Box<dyn AnalyzerRun> {
                    struct CrashRun;
                    impl AnalyzerRun for CrashRun {
                        fn block(&mut self, _: i64, _: &[f32]) -> ControlFlow<()> {
                            panic!("boom in analyzer block");
                        }
                        fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
                            Ok(AnalyzerResult::default())
                        }
                    }
                    Box::new(CrashRun)
                },
            )?;
            Ok(())
        }
    }

    let ctx = PluginContext::new();
    ctx.mount(&PanicPlugin).expect("mounts panic plugin");
    let engine = Arc::new(Mutex::new(
        Engine::with_project(EngineConfig::default(), project).expect("engine"),
    ));
    let host = Arc::new(ProjectHost::new(Arc::clone(&engine), &ctx));
    let before_revision = host.revision();

    let cancel = AtomicBool::new(false);
    let res = ctx.run_analyzer(
        "test.panic_analyzer.crash",
        &host.snapshot(),
        AudioSource::Clip(clip_id),
        TimeRange::at_rate(0, 4, R24),
        &cancel,
    );
    assert_eq!(
        res,
        Err(AnalyzerError::Panicked {
            id: "test.panic_analyzer.crash".to_owned(),
            reason: "boom in analyzer block".to_owned(),
        })
    );
    assert!(!ctx.is_mounted("test.panic_analyzer"));
    assert!(ctx.analyzers().is_empty());
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, "test.panic_analyzer");
    assert_eq!(
        failures[0].reason,
        "analyzer test.panic_analyzer.crash panicked: boom in analyzer block"
    );
    assert_eq!(host.revision(), before_revision);
    assert!(
        host.snapshot()
            .timeline()
            .clip(clip_id)
            .expect("clip")
            .beats
            .is_empty()
    );
}

#[test]
fn returning_err_from_finish_records_fault_and_keeps_plugin_mounted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("err.wav");
    write_click_track(&wav, 4_096, &[0]);
    let (project, clip_id) = project_with_wav(&wav, 2);

    struct ErrPlugin;
    impl Plugin for ErrPlugin {
        fn manifest(&self) -> &PluginManifest {
            static MANIFEST: PluginManifest =
                PluginManifest::new("test.err_analyzer", "Err Analyzer", "1.0.0", &[], &[]);
            &MANIFEST
        }
        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            ctx.register_analyzer(
                AnalyzerSpec::new("test.err_analyzer.fail", "Fail", AnalyzerScope::Clip),
                |_rate: u32, _channels: u16| -> Box<dyn AnalyzerRun> {
                    struct FailRun;
                    impl AnalyzerRun for FailRun {
                        fn block(&mut self, _: i64, _: &[f32]) -> ControlFlow<()> {
                            ControlFlow::Continue(())
                        }
                        fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
                            Err("insufficient signal".to_owned())
                        }
                    }
                    Box::new(FailRun)
                },
            )?;
            Ok(())
        }
    }

    let ctx = PluginContext::new();
    ctx.mount(&ErrPlugin).expect("mounts err plugin");
    let cancel = AtomicBool::new(false);
    let res = ctx.run_analyzer(
        "test.err_analyzer.fail",
        &project,
        AudioSource::Clip(clip_id),
        TimeRange::at_rate(0, 2, R24),
        &cancel,
    );
    assert_eq!(
        res,
        Err(AnalyzerError::Failed {
            id: "test.err_analyzer.fail".to_owned(),
            reason: "insufficient signal".to_owned(),
        })
    );
    assert!(ctx.is_mounted("test.err_analyzer"));
    assert_eq!(
        ctx.faults(),
        vec![PluginFault {
            plugin: "test.err_analyzer".into(),
            message: "analyzer test.err_analyzer.fail failed: insufficient signal".to_owned(),
        }]
    );
}
