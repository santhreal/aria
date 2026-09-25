//! Production scene resolution hot path (`resolve`, `resolve_with`, `resolve_gesture_partitions`).
//!
//! Run: `cargo bench -p aria-render --bench resolve`

use aria_models::{
    AnimatedTransform, ClipId, ColorAdjustments, CropRect, Easing, Generator, Keyframe, LayerGlow,
    LayerOutline, LayerShadow, LayerStyles, MediaSource, Param, Project, Rational, RationalTime,
    Scale2, Shape, TextStyle, TimeRange, TrackKind, TrackMatte,
};
use aria_render::{
    GESTURE_IDENTITY_TRANSFORM, ResolveOverrides, resolve, resolve_gesture_partitions, resolve_with,
};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

fn rt(tick: i64) -> RationalTime {
    RationalTime::new(tick, Rational::FPS_24)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, Rational::FPS_24)
}

fn video(width: u32, height: u32) -> MediaSource {
    MediaSource::new(
        "/media/footage.mp4",
        width,
        height,
        Rational::FPS_24,
        600,
        false,
    )
}

/// Build a representative multi-layer composite project with `layer_count` visual tracks.
fn build_multi_layer_project(layer_count: usize) -> (Project, ClipId) {
    let mut project = Project::new("multi_layer_bench", Rational::FPS_24);

    // Track 1 (V1): Base video background with color grade
    let v1 = project.add_track(TrackKind::Video, "V1");
    let media1 = project.add_media(video(1920, 1080));
    let clip_v1 = project.add_clip(v1, media1, tr(0, 120), rt(0)).unwrap();
    let mut adjust = ColorAdjustments::default();
    adjust.brightness = Param::Constant(0.1);
    adjust.contrast = Param::Constant(0.15);
    adjust.saturation = Param::Constant(0.05);
    project.set_clip_adjustments(clip_v1, adjust).unwrap();

    let mut primary_clip = clip_v1;

    if layer_count >= 2 {
        // Track 2 (V2): Overlay video (Picture-in-Picture) with drop shadow and transform
        let v2 = project.add_track(TrackKind::Video, "V2");
        let media2 = project.add_media(video(1280, 720));
        let clip_v2 = project.add_clip(v2, media2, tr(0, 120), rt(0)).unwrap();
        project
            .set_layer_styles(
                clip_v2,
                LayerStyles {
                    shadow: Some(LayerShadow {
                        blur: Param::Constant(12.0),
                        rgba: Param::Constant([0, 0, 0, 160]),
                        offset: Param::Constant([6.0, 6.0]),
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
        primary_clip = clip_v2;
    }

    if layer_count >= 3 {
        // Track 3 (T1): Main title with styled text, outline, and shadow
        let t1 = project.add_track(TrackKind::Text, "T1");
        let text_style = TextStyle {
            size: Param::Constant(64.0),
            bold: true,
            ..Default::default()
        };
        let clip_t1 = project
            .add_generated(
                t1,
                Generator::Text {
                    content: "Production Scene Resolution".into(),
                    style: text_style,
                },
                tr(0, 120),
            )
            .unwrap();
        project
            .set_layer_styles(
                clip_t1,
                LayerStyles {
                    glow: Some(LayerGlow {
                        radius: Param::Constant(16.0),
                        intensity: Param::Constant(1.2),
                        rgba: Param::Constant([255, 220, 100, 200]),
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
    }

    if layer_count >= 4 {
        // Track 4 (S1): Lower-third graphic shape
        let s1 = project.add_track(TrackKind::Sticker, "S1");
        project
            .add_generated(
                s1,
                Generator::Shape {
                    shape: Shape::Rectangle,
                    rgba: Param::Constant([20, 30, 40, 220]),
                    width: Param::Constant(600.0),
                    height: Param::Constant(120.0),
                    corner_radius: Param::Constant(8.0),
                    stroke: None,
                },
                tr(0, 120),
            )
            .unwrap();
    }

    if layer_count >= 5 {
        // Track 5 (T2): Subtitle / caption
        let t2 = project.add_track(TrackKind::Text, "T2");
        let sub_style = TextStyle {
            size: Param::Constant(32.0),
            ..Default::default()
        };
        project
            .add_generated(
                t2,
                Generator::Text {
                    content: "Multi-track timeline resolution benchmark".into(),
                    style: sub_style,
                },
                tr(0, 120),
            )
            .unwrap();
    }

    if layer_count >= 6 {
        // Track 6 (S2): Star / icon sticker
        let s2 = project.add_track(TrackKind::Sticker, "S2");
        project
            .add_generated(s2, Generator::sticker("star_spin"), tr(0, 120))
            .unwrap();
    }

    if layer_count >= 7 {
        // Track 7 (M1): Track matte on solid layer (consumed by upper stencil)
        let m1 = project.add_track(TrackKind::Sticker, "M1");
        let clip_m1 = project
            .add_generated(
                m1,
                Generator::SolidColor {
                    rgba: [200, 50, 50, 255],
                },
                tr(0, 120),
            )
            .unwrap();
        project
            .set_clip_matte(clip_m1, Some(TrackMatte::alpha()))
            .unwrap();
    }

    if layer_count >= 8 {
        // Track 8 (A1): Canvas-wide adjustment layer
        let a1 = project.add_track(TrackKind::Adjustment, "A1");
        project
            .add_generated(a1, Generator::Adjustment, tr(0, 120))
            .unwrap();
    }

    // Add extra layers for high track-count scaling tests (up to layer_count)
    for i in 9..=layer_count {
        let kind = if i % 2 == 0 {
            TrackKind::Sticker
        } else {
            TrackKind::Text
        };
        let track = project.add_track(kind, &format!("Extra_{i}"));
        if kind == TrackKind::Text {
            project
                .add_generated(
                    track,
                    Generator::Text {
                        content: format!("Layer {i}"),
                        style: TextStyle::default(),
                    },
                    tr(0, 120),
                )
                .unwrap();
        } else {
            project
                .add_generated(
                    track,
                    Generator::Shape {
                        shape: Shape::Ellipse,
                        rgba: Param::Constant([100, 150, 200, 255]),
                        width: Param::Constant(200.0),
                        height: Param::Constant(200.0),
                        corner_radius: Param::Constant(0.0),
                        stroke: None,
                    },
                    tr(0, 120),
                )
                .unwrap();
        }
    }

    (project, primary_clip)
}

/// Build a project with dense keyframed animations on transforms, opacity, crop, and styles.
fn build_keyframed_project(layer_count: usize) -> Project {
    let mut project = Project::new("keyframed_bench", Rational::FPS_24);

    for i in 0..layer_count {
        let track = project.add_track(TrackKind::Video, &format!("KV_{i}"));
        let media = project.add_media(video(1920, 1080));
        let clip = project.add_clip(track, media, tr(0, 120), rt(0)).unwrap();

        // 32 keyframes along a 120-frame clip
        let mut pos_kfs = Vec::with_capacity(32);
        let mut opacity_kfs = Vec::with_capacity(32);
        let mut scale_kfs = Vec::with_capacity(32);
        for k in 0..32 {
            let tick = k * 3;
            let phase = (k as f32) * 0.2;
            pos_kfs.push(Keyframe::new(
                tick,
                [phase.sin() * 100.0, phase.cos() * 50.0],
                Easing::EaseInOut,
            ));
            opacity_kfs.push(Keyframe::new(
                tick,
                0.5 + 0.5 * phase.sin().abs(),
                Easing::Linear,
            ));
            scale_kfs.push(Keyframe::new(
                tick,
                Scale2 {
                    x: 1.0 + 0.2 * phase.cos(),
                    y: 1.0 + 0.2 * phase.cos(),
                },
                Easing::EaseInOut,
            ));
        }

        let mut anim_transform = AnimatedTransform::identity();
        anim_transform.position = Param::Keyframed { keyframes: pos_kfs };
        anim_transform.opacity = Param::Keyframed {
            keyframes: opacity_kfs,
        };
        anim_transform.scale = Param::Keyframed {
            keyframes: scale_kfs,
        };
        project
            .set_animated_transform(clip, anim_transform)
            .unwrap();

        // Keyframed crop
        project.clip_mut(clip).unwrap().crop = Param::Keyframed {
            keyframes: vec![
                Keyframe::new(
                    0,
                    CropRect {
                        x: 0.0,
                        y: 0.0,
                        w: 1.0,
                        h: 1.0,
                    },
                    Easing::Linear,
                ),
                Keyframe::new(
                    96,
                    CropRect {
                        x: 0.05,
                        y: 0.05,
                        w: 0.9,
                        h: 0.9,
                    },
                    Easing::EaseInOut,
                ),
            ],
        };

        // Keyframed layer styles
        let mut blur_kfs = Vec::with_capacity(16);
        for k in 0..16 {
            blur_kfs.push(Keyframe::new(k * 6, 4.0 + (k as f32) * 1.5, Easing::Linear));
        }
        project
            .set_layer_styles(
                clip,
                LayerStyles {
                    shadow: Some(LayerShadow {
                        blur: Param::Keyframed {
                            keyframes: blur_kfs,
                        },
                        rgba: Param::Constant([0, 0, 0, 180]),
                        offset: Param::Constant([4.0, 4.0]),
                    }),
                    outline: Some(LayerOutline {
                        width: Param::Constant(2.0),
                        rgba: Param::Constant([255, 255, 255, 255]),
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
    }

    project
}

/// Build a project with abutting clips and a crossfade transition.
fn build_transition_project() -> Project {
    let mut project = Project::new("transition_bench", Rational::FPS_24);
    let track = project.add_track(TrackKind::Video, "V1");
    let media1 = project.add_media(video(1920, 1080));
    let media2 = project.add_media(video(1920, 1080));

    let left = project.add_clip(track, media1, tr(0, 48), rt(0)).unwrap();
    project.add_clip(track, media2, tr(0, 48), rt(48)).unwrap();
    project.add_transition(left, "crossfade").unwrap();
    project
}

fn bench_scene_resolve_multi_layer(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolve/multi_layer");

    for &layer_count in &[4, 8, 16] {
        let (project, _) = build_multi_layer_project(layer_count);
        let tick = rt(24);
        group.throughput(Throughput::Elements(layer_count as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{layer_count}_layers")),
            &layer_count,
            |b, _| {
                b.iter(|| {
                    black_box(resolve(black_box(&project), black_box(tick)).unwrap());
                })
            },
        );
    }

    group.finish();
}

fn bench_scene_resolve_keyframed(c: &mut Criterion) {
    let project = build_keyframed_project(8);
    let tick = rt(36);
    let mut group = c.benchmark_group("resolve/keyframed");
    group.throughput(Throughput::Elements(8));

    group.bench_function("8_layers_32_kfs", |b| {
        b.iter(|| {
            black_box(resolve(black_box(&project), black_box(tick)).unwrap());
        })
    });

    group.finish();
}

fn bench_scene_resolve_transition(c: &mut Criterion) {
    let project = build_transition_project();
    // Resolving at midpoint of transition window (cut at tick 48, default window duration 24 ticks -> midpoint at tick 48)
    let tick = rt(48);
    let mut group = c.benchmark_group("resolve/transition");
    group.throughput(Throughput::Elements(2));

    group.bench_function("crossfade_midpoint", |b| {
        b.iter(|| {
            black_box(resolve(black_box(&project), black_box(tick)).unwrap());
        })
    });

    group.finish();
}

fn bench_scene_resolve_gesture_partitions(c: &mut Criterion) {
    let (project, clip_id) = build_multi_layer_project(8);
    let tick = rt(24);
    let mut group = c.benchmark_group("resolve/gesture_partitions");
    group.throughput(Throughput::Elements(8));

    group.bench_function("8_layers_sprite_split", |b| {
        b.iter(|| {
            black_box(
                resolve_gesture_partitions(
                    black_box(&project),
                    black_box(tick),
                    black_box(clip_id),
                )
                .unwrap(),
            );
        })
    });

    group.finish();
}

fn bench_scene_resolve_with_overrides(c: &mut Criterion) {
    let (project, clip_id) = build_multi_layer_project(8);
    let tick = rt(24);
    let live_styles = LayerStyles {
        shadow: Some(LayerShadow {
            blur: Param::Constant(24.0),
            rgba: Param::Constant([0, 0, 0, 200]),
            offset: Param::Constant([8.0, 8.0]),
        }),
        ..Default::default()
    };
    let overrides = ResolveOverrides {
        transform: Some((clip_id, GESTURE_IDENTITY_TRANSFORM)),
        styles: Some((clip_id, &live_styles)),
        ..Default::default()
    };

    let mut group = c.benchmark_group("resolve/overrides");
    group.throughput(Throughput::Elements(8));

    group.bench_function("8_layers_live_override", |b| {
        b.iter(|| {
            black_box(
                resolve_with(black_box(&project), black_box(tick), black_box(overrides)).unwrap(),
            );
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_scene_resolve_multi_layer,
    bench_scene_resolve_keyframed,
    bench_scene_resolve_transition,
    bench_scene_resolve_gesture_partitions,
    bench_scene_resolve_with_overrides,
);
criterion_main!(benches);
