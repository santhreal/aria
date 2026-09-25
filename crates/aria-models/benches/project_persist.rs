//! Criterion save/load benchmarks and deterministic allocation measurement for Aria projects.
//!
//! Exercises the production `.aria` persistence write and read paths:
//! - `Project::save_to_file` (atomic write with fsync and rename)
//! - `Project::load_from_file` (disk read, schema validation, migration, lane normalization)
//! - `Project::save_to_string` (CPU serialization with current schema stamp)
//! - `Project::load_from_string` (CPU deserialization, schema validation, migration)
//!
//! Benchmark targets:
//! - `project_save/save_to_file`: File write path on representative nontrivial project.
//! - `project_save/save_to_string`: Serialization path in memory.
//! - `project_load/load_from_file`: File load path on representative nontrivial project.
//! - `project_load/load_from_string`: Deserialization path in memory.
//! - `project_allocation/*`: Deterministic allocation counts and memory metrics.
//!
//! Run:
//!   `cargo bench -p aria-models --bench project_persist`

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_models::{
    AnimatedTransform, AudioRole, BlendMode, CanvasAspect, CanvasSettings, ColorAdjustments,
    CropRect, Easing, Generator, Keyframe, LayerShadow, LayerStyles, Marker, MarkerColor,
    MediaSource, MotionBlur, Param, PoolPosition, Project, Rational, RationalTime, SpatialTangents,
    TextStyle, TimeRange, TrackKind,
};
use criterion::{Criterion, criterion_group, criterion_main};

// --- Deterministic Allocation Counting ---------------------------------------

struct CountingAlloc;

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static DEALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static DEALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        DEALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        if new_size > layout.size() {
            ALLOC_BYTES.fetch_add(new_size - layout.size(), Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocStats {
    pub allocations: usize,
    pub allocated_bytes: usize,
    pub deallocations: usize,
    pub deallocated_bytes: usize,
}

impl AllocStats {
    pub fn snapshot() -> Self {
        Self {
            allocations: ALLOC_COUNT.load(Ordering::SeqCst),
            allocated_bytes: ALLOC_BYTES.load(Ordering::SeqCst),
            deallocations: DEALLOC_COUNT.load(Ordering::SeqCst),
            deallocated_bytes: DEALLOC_BYTES.load(Ordering::SeqCst),
        }
    }

    pub fn delta_from(&self, before: &AllocStats) -> Self {
        Self {
            allocations: self.allocations.saturating_sub(before.allocations),
            allocated_bytes: self.allocated_bytes.saturating_sub(before.allocated_bytes),
            deallocations: self.deallocations.saturating_sub(before.deallocations),
            deallocated_bytes: self
                .deallocated_bytes
                .saturating_sub(before.deallocated_bytes),
        }
    }
}

fn measure_allocs<R>(f: impl FnOnce() -> R) -> (R, AllocStats) {
    let before = AllocStats::snapshot();
    let result = f();
    let after = AllocStats::snapshot();
    (result, after.delta_from(&before))
}

// --- Workspace Scratch Directory (never /tmp) --------------------------------

fn workspace_scratch_dir() -> (tempfile::TempDir, PathBuf) {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            manifest.join(".bench_scratch")
        });
    std::fs::create_dir_all(&base).expect("create workspace scratch dir");
    let tempdir = tempfile::Builder::new()
        .prefix("aria_persist_bench_")
        .tempdir_in(&base)
        .expect("create workspace-local tempdir");
    let path = tempdir.path().to_path_buf();
    (tempdir, path)
}

// --- Nontrivial Representative Project Builder -------------------------------

const FPS: Rational = Rational::FPS_24;

fn rt(tick: i64) -> RationalTime {
    RationalTime::new(tick, FPS)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS)
}

/// A source window on media whose own rate is not the sequence rate. `add_clip`
/// states the source range at the media's rate and the timeline start at the
/// sequence rate, and resamples the duration between them; the b-roll here is
/// 60 fps against a 24 fps sequence.
fn tr_at(start: i64, duration: i64, rate: Rational) -> TimeRange {
    TimeRange::at_rate(start, duration, rate)
}

fn keyframed_opacity() -> Param<f32> {
    Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: 0.0,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: 24,
                value: 1.0,
                easing: Easing::EaseInOut,
                tangents: None,
            },
            Keyframe {
                tick: 72,
                value: 1.0,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: 96,
                value: 0.0,
                easing: Easing::EaseIn,
                tangents: None,
            },
        ],
    }
}

fn keyframed_position() -> Param<[f32; 2]> {
    Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: [-200.0, 0.0],
                easing: Easing::Bezier {
                    points: [0.25, 0.1, 0.25, 1.0],
                },
                tangents: Some(SpatialTangents {
                    in_t: [0.0, 0.0],
                    out_t: [2.5, 1.0],
                }),
            },
            Keyframe {
                tick: 48,
                value: [0.0, 0.0],
                easing: Easing::Linear,
                tangents: Some(SpatialTangents {
                    in_t: [-2.5, -1.0],
                    out_t: [2.5, -1.0],
                }),
            },
            Keyframe {
                tick: 96,
                value: [200.0, 0.0],
                easing: Easing::EaseOut,
                tangents: Some(SpatialTangents {
                    in_t: [-2.5, 1.0],
                    out_t: [0.0, 0.0],
                }),
            },
        ],
    }
}

fn sample_color_adjustments() -> ColorAdjustments {
    let mut adj = ColorAdjustments::default();
    adj.brightness = Param::Constant(0.12);
    adj.contrast = Param::Constant(0.25);
    adj.saturation = Param::Constant(0.08);
    adj.temperature = Param::Constant(0.15);
    adj.tint = Param::Constant(-0.05);
    adj.highlights = Param::Constant(-0.10);
    adj.shadows = Param::Constant(0.18);
    adj.vignette = Param::Constant(0.20);
    adj
}

fn sample_layer_styles() -> LayerStyles {
    LayerStyles {
        shadow: Some(LayerShadow {
            rgba: Param::Constant([0, 0, 0, 180]),
            offset: Param::Constant([6.0, 6.0]),
            blur: Param::Constant(12.0),
        }),
        glow: None,
        outline: None,
        background: None,
    }
}

/// Builds a representative, non-trivial Aria editing project containing:
/// - 8 media sources across 3 bins with freeform positions
/// - 7 tracks (Video A-Roll, Video B-Roll, Text, Stickers, Dialogue, Score, SFX)
/// - 42 clips total with various parameters, transforms, keyframes, transitions,
///   layer styles, color grades, text styling, and retiming
/// - 8 timeline ruler markers with varied colors and notes
/// - Configured canvas and audio layout
fn build_nontrivial_project() -> Project {
    let mut project = Project::new("Aria Feature Film Montage - 4K Master", FPS);
    project.metadata_mut().author = Some("Aria Benchmark Suite".into());
    project.metadata_mut().description =
        "Representative non-trivial production project for persistence".into();

    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Wide16x9,
        background: [10, 12, 16],
        resolution: Some([3840, 2160]),
    });

    // 1. Media Pool & Bins
    let bin_aroll = project.add_bin("A-Roll Primary", None).expect("add bin");
    let bin_broll = project.add_bin("B-Roll Cutaways", None).expect("add bin");
    let bin_audio = project
        .add_bin("Soundtracks & Foley", None)
        .expect("add bin");

    let media_v1 = project.add_media(MediaSource::new(
        "media/scene1_cam_a.mp4",
        3840,
        2160,
        FPS,
        4800,
        true,
    ));
    let media_v2 = project.add_media(MediaSource::new(
        "media/scene1_cam_b.mp4",
        3840,
        2160,
        FPS,
        4800,
        true,
    ));
    let media_v3 = project.add_media(MediaSource::new(
        "media/scene2_dolly.mp4",
        3840,
        2160,
        FPS,
        3600,
        true,
    ));
    let media_broll1 = project.add_media(MediaSource::new(
        "media/city_timelapse.mp4",
        3840,
        2160,
        Rational::FPS_60,
        7200,
        false,
    ));
    let media_broll2 = project.add_media(MediaSource::new(
        "media/drone_overhead.mp4",
        3840,
        2160,
        Rational::FPS_60,
        6000,
        false,
    ));
    let media_audio_score = project.add_media(MediaSource::new(
        "audio/orchestral_theme.wav",
        0,
        0,
        FPS,
        9600,
        true,
    ));
    let media_audio_dialogue = project.add_media(MediaSource::new(
        "audio/dialogue_take3.wav",
        0,
        0,
        FPS,
        4800,
        true,
    ));
    let media_audio_sfx = project.add_media(MediaSource::new(
        "audio/whoosh_impact.wav",
        0,
        0,
        FPS,
        1200,
        true,
    ));

    let _ = project.set_pool_item_position(
        Some(bin_aroll),
        media_v1,
        PoolPosition { x: 50.0, y: 100.0 },
    );
    let _ = project.set_pool_item_position(
        Some(bin_aroll),
        media_v2,
        PoolPosition { x: 250.0, y: 100.0 },
    );
    let _ = project.set_pool_item_position(
        Some(bin_aroll),
        media_v3,
        PoolPosition { x: 450.0, y: 100.0 },
    );
    let _ = project.set_pool_item_position(
        Some(bin_broll),
        media_broll1,
        PoolPosition { x: 50.0, y: 100.0 },
    );
    let _ = project.set_pool_item_position(
        Some(bin_broll),
        media_broll2,
        PoolPosition { x: 250.0, y: 100.0 },
    );
    let _ = project.set_pool_item_position(
        Some(bin_audio),
        media_audio_score,
        PoolPosition { x: 50.0, y: 100.0 },
    );
    let _ = project.set_pool_item_position(
        Some(bin_audio),
        media_audio_dialogue,
        PoolPosition { x: 250.0, y: 100.0 },
    );
    let _ = project.set_pool_item_position(
        Some(bin_audio),
        media_audio_sfx,
        PoolPosition { x: 450.0, y: 100.0 },
    );

    // 2. Tracks
    let track_v1 = project.add_track(TrackKind::Video, "V1 A-Roll");
    let track_v2 = project.add_track(TrackKind::Video, "V2 B-Roll");
    let track_text = project.add_track(TrackKind::Text, "Titles");
    let track_sticker = project.add_track(TrackKind::Sticker, "Overlays");
    let track_a1 = project.add_track(TrackKind::Audio, "A1 Dialogue");
    let track_a2 = project.add_track(TrackKind::Audio, "A2 Score");
    let track_a3 = project.add_track(TrackKind::Audio, "A3 SFX");

    // 3. Clips & Transitions
    // A-Roll clips (abutting clips with transitions)
    let clip_v1_1 = project
        .add_clip(track_v1, media_v1, tr(0, 240), rt(0))
        .expect("add clip v1_1");
    let clip_v1_2 = project
        .add_clip(track_v1, media_v2, tr(100, 240), rt(240))
        .expect("add clip v1_2");
    let clip_v1_3 = project
        .add_clip(track_v1, media_v3, tr(50, 360), rt(480))
        .expect("add clip v1_3");
    let clip_v1_4 = project
        .add_clip(track_v1, media_v1, tr(300, 240), rt(840))
        .expect("add clip v1_4");
    let clip_v1_5 = project
        .add_clip(track_v1, media_v2, tr(400, 480), rt(1080))
        .expect("add clip v1_5");
    let clip_v1_6 = project
        .add_clip(track_v1, media_v3, tr(500, 360), rt(1560))
        .expect("add clip v1_6");

    let _ = project.add_transition(clip_v1_1, "crossfade");
    let _ = project.add_transition(clip_v1_3, "dip_to_black");
    let _ = project.add_transition(clip_v1_5, "crossfade");

    // Color adjustments and layer styles on A-Roll
    project
        .set_clip_adjustments(clip_v1_1, sample_color_adjustments())
        .expect("set adj v1_1");
    project
        .set_clip_adjustments(clip_v1_2, sample_color_adjustments())
        .expect("set adj v1_2");
    project
        .set_layer_styles(clip_v1_3, sample_layer_styles())
        .expect("set styles v1_3");
    project
        .set_blend_mode(clip_v1_4, BlendMode::Normal)
        .expect("set blend");
    project
        .set_motion_blur(
            clip_v1_5,
            MotionBlur {
                enabled: true,
                shutter_deg: 180.0,
                samples: 8,
            },
        )
        .expect("set mb");
    project
        .set_clip_crop(
            clip_v1_6,
            CropRect {
                x: 0.05,
                y: 0.05,
                w: 0.90,
                h: 0.90,
            },
            false,
            false,
            None,
        )
        .expect("set crop");

    // B-Roll clips (with animated keyframed transforms)
    let clip_v2_1 = project
        .add_clip(
            track_v2,
            media_broll1,
            tr_at(0, 120, Rational::FPS_60),
            rt(60),
        )
        .expect("add clip v2_1");
    let clip_v2_2 = project
        .add_clip(
            track_v2,
            media_broll2,
            tr_at(100, 180, Rational::FPS_60),
            rt(300),
        )
        .expect("add clip v2_2");
    let clip_v2_3 = project
        .add_clip(
            track_v2,
            media_broll1,
            tr_at(300, 150, Rational::FPS_60),
            rt(600),
        )
        .expect("add clip v2_3");
    let clip_v2_4 = project
        .add_clip(
            track_v2,
            media_broll2,
            tr_at(400, 200, Rational::FPS_60),
            rt(900),
        )
        .expect("add clip v2_4");

    let mut anim_transform = AnimatedTransform::identity();
    anim_transform.opacity = keyframed_opacity();
    anim_transform.position = keyframed_position();
    project
        .set_animated_transform(clip_v2_1, anim_transform.clone())
        .expect("set anim v2_1");
    project
        .set_animated_transform(clip_v2_3, anim_transform)
        .expect("set anim v2_3");

    // Retiming / speed curve
    let speed_curve = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: 1.0,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: 48,
                value: 2.5,
                easing: Easing::EaseInOut,
                tangents: None,
            },
            Keyframe {
                tick: 96,
                value: 0.5,
                easing: Easing::Linear,
                tangents: None,
            },
        ],
    };
    let _ = project.set_clip_speed_curve(clip_v2_2, Some(speed_curve));
    let _ = project.set_clip_speed(clip_v2_4, Rational::new(2, 1), false);

    // Text clips (Titles and lower thirds)
    for i in 0..6 {
        let start = i * 300;
        let mut style = TextStyle::default();
        style.font = "Inter-Bold".into();
        style.size = Param::Constant(64.0);
        style.bold = true;
        style.fill = Param::Constant([240, 245, 255, 255]);

        let gen_clip = project
            .add_generated(
                track_text,
                Generator::Text {
                    content: format!("Chapter {i}: Scene Title Narrative Sequence"),
                    style,
                },
                tr(start, 120),
            )
            .expect("add text clip");

        let _ = project.set_layer_styles(gen_clip, sample_layer_styles());
    }

    // Sticker / generated shapes
    for i in 0..6 {
        let start = i * 300 + 120;
        let _ = project.add_generated(
            track_sticker,
            Generator::SolidColor {
                rgba: [220, 160, 40, 180],
            },
            tr(start, 90),
        );
    }

    // Audio Dialogue track
    for i in 0..6 {
        let start = i * 300;
        let clip = project
            .add_clip(track_a1, media_audio_dialogue, tr(i * 200, 240), rt(start))
            .expect("add audio d");
        project
            .set_clip_audio(clip, Some(1.0), rt(12), rt(12))
            .expect("set audio");
        project
            .set_clip_audio_role(clip, Some(AudioRole::Voiceover))
            .expect("set role");
        let _ = project.set_clip_denoise(clip, true);
    }

    // Audio Score track
    let clip_score1 = project
        .add_clip(track_a2, media_audio_score, tr(0, 960), rt(0))
        .expect("add score 1");
    let clip_score2 = project
        .add_clip(track_a2, media_audio_score, tr(960, 960), rt(960))
        .expect("add score 2");
    project
        .set_clip_audio(clip_score1, Some(0.75), rt(24), rt(48))
        .expect("set audio score 1");
    project
        .set_clip_audio(clip_score2, Some(0.75), rt(48), rt(72))
        .expect("set audio score 2");
    project
        .set_clip_audio_role(clip_score1, Some(AudioRole::Music))
        .expect("set role");
    project
        .set_clip_audio_role(clip_score2, Some(AudioRole::Music))
        .expect("set role");

    // Audio SFX track
    for i in 0..6 {
        let start = i * 300 + 60;
        let clip = project
            .add_clip(track_a3, media_audio_sfx, tr(0, 72), rt(start))
            .expect("add sfx");
        project
            .set_clip_audio(clip, Some(0.9), rt(4), rt(12))
            .expect("set audio sfx");
        project
            .set_clip_audio_role(clip, Some(AudioRole::Sfx))
            .expect("set role");
    }

    // 4. Markers
    let marker_specs = [
        (
            0,
            "Intro Scene",
            MarkerColor::Blue,
            "Opening sequence start",
        ),
        (
            240,
            "Cut to Angle B",
            MarkerColor::Green,
            "Camera switch point",
        ),
        (
            480,
            "Dolly Movement",
            MarkerColor::Purple,
            "Slow dolly forward on character",
        ),
        (
            720,
            "Midpoint Climax",
            MarkerColor::Red,
            "Dramatic beat in music",
        ),
        (
            960,
            "Score Transition",
            MarkerColor::Yellow,
            "Orchestral movement bridge",
        ),
        (
            1200,
            "Dialogue Beat",
            MarkerColor::Teal,
            "Crucial exposition line",
        ),
        (
            1560,
            "Outro Montage",
            MarkerColor::Orange,
            "Fast cut sequence leading to close",
        ),
        (
            1920,
            "Fade Out End",
            MarkerColor::Pink,
            "Timeline end marker",
        ),
    ];

    for (tick, name, color, note) in marker_specs {
        let marker = Marker::new(rt(tick), name, color).with_comment(note);
        let _ = project.timeline_mut().add_marker(marker);
    }

    project
}

// --- Benchmark Functions -----------------------------------------------------

fn bench_save(c: &mut Criterion) {
    let project = build_nontrivial_project();
    let (_tempdir, scratch_path) = workspace_scratch_dir();
    let target_file = scratch_path.join("bench_save_target.aria");

    let mut group = c.benchmark_group("project_save");
    group.bench_function("save_to_file", |b| {
        b.iter(|| {
            project
                .save_to_file(black_box(&target_file))
                .expect("save_to_file");
        });
    });
    group.bench_function("save_to_string", |b| {
        b.iter(|| {
            let s = project.save_to_string().expect("save_to_string");
            black_box(s);
        });
    });
    group.finish();
}

fn bench_load(c: &mut Criterion) {
    let project = build_nontrivial_project();
    let (_tempdir, scratch_path) = workspace_scratch_dir();
    let fixture_file = scratch_path.join("bench_load_fixture.aria");
    project.save_to_file(&fixture_file).expect("save_to_file");
    let json_str = project.save_to_string().expect("save_to_string");

    let mut group = c.benchmark_group("project_load");
    group.bench_function("load_from_file", |b| {
        b.iter(|| {
            let p = Project::load_from_file(black_box(&fixture_file)).expect("load_from_file");
            black_box(p);
        });
    });
    group.bench_function("load_from_string", |b| {
        b.iter(|| {
            let p = Project::load_from_string(black_box(&json_str)).expect("load_from_string");
            black_box(p);
        });
    });
    group.finish();
}

fn bench_allocation(c: &mut Criterion) {
    let project = build_nontrivial_project();
    let json_str = project.save_to_string().expect("save_to_string");

    let (_tempdir, scratch_path) = workspace_scratch_dir();
    let file_fixture = scratch_path.join("bench_alloc_fixture.aria");
    project.save_to_file(&file_fixture).expect("save_to_file");
    let file_save_dest = scratch_path.join("bench_alloc_save_target.aria");

    // Collect deterministic single-shot allocation evidence
    let (_saved_str, save_str_stats) =
        measure_allocs(|| project.save_to_string().expect("save_to_string"));
    let (_loaded_str, load_str_stats) =
        measure_allocs(|| Project::load_from_string(&json_str).expect("load_from_string"));
    let (_saved_file, save_file_stats) =
        measure_allocs(|| project.save_to_file(&file_save_dest).expect("save_to_file"));
    let (_loaded_file, load_file_stats) =
        measure_allocs(|| Project::load_from_file(&file_fixture).expect("load_from_file"));

    eprintln!(
        "\n=================================================================\n\
         DETERMINISTIC ALLOCATION COUNT EVIDENCE (aria-models project persistence)\n\
         =================================================================\n\
         - save_to_string:    {:>6} allocs, {:>10} bytes (deallocs: {:>6})\n\
         - load_from_string:  {:>6} allocs, {:>10} bytes (deallocs: {:>6})\n\
         - save_to_file:      {:>6} allocs, {:>10} bytes (deallocs: {:>6})\n\
         - load_from_file:    {:>6} allocs, {:>10} bytes (deallocs: {:>6})\n\
         =================================================================\n",
        save_str_stats.allocations,
        save_str_stats.allocated_bytes,
        save_str_stats.deallocations,
        load_str_stats.allocations,
        load_str_stats.allocated_bytes,
        load_str_stats.deallocations,
        save_file_stats.allocations,
        save_file_stats.allocated_bytes,
        save_file_stats.deallocations,
        load_file_stats.allocations,
        load_file_stats.allocated_bytes,
        load_file_stats.deallocations,
    );

    // Counts are a single-shot measurement, not a Criterion time sample:
    // reporting allocs as Duration makes Criterion's variance math panic
    // when every sample is identical (save_to_file) or sub-nanosecond.
    let _ = c;
}

criterion_group!(benches, bench_save, bench_load, bench_allocation);
criterion_main!(benches);
