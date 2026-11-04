use std::time::{Duration, Instant};

use aria_core::{Rational, RationalTime};
use aria_models::{
    CanvasAspect, CanvasSettings, Generator, Mask, MaskKind, MatteSource, MediaSource, Param,
    Project, Shape, ShapePath, ShapePathPoint, ShapeStroke, TEXT_RUN_VERSION, TextStyle,
    TextStyleRun, TimeRange, TrackKind, TrackMatte, generator_specs, shipped_transition_catalog,
    sticker_catalog,
};

use super::scratch_tests::count_thread_allocs;
use crate::render::{Renderer, SeekPolicy};
use crate::resolve::ResolveOverrides;

/// WHY: Closes the per-frame render-thread heap-allocation class across Aria's
/// own scene resolution, realization, and compositor layer construction
/// (`Renderer::realize_frame_with`, which executes `take_resolved_scene`,
/// `resolve_with_into`, `prepare_motion_blur`, `realize_scene_layers`,
/// `composite_leaf`, `finish_pending_on_gpu`, `return_scratch`,
/// `return_resolved_scene`, and `look_ahead`) for every generator variant and
/// topology derived at run time from `aria_models::generator_specs()`, every
/// transition in `aria_models::shipped_transition_catalog()`, video, still
/// images, audio, track mattes (`Alpha` and `Luma`), layer masks (`Rectangle`,
/// `Circle`, `Path`), and effect masks. Adding a new generator or topology to
/// `generator_specs()` panics until covered here. Does not cover `wgpu` command
/// encoding or `Queue::submit` allocations, which are covered by
/// `steady_frame_render_allocations_stay_at_wgpu_floor`.
#[test]
fn steady_realize_allocates_zero_heap_blocks_across_all_clip_and_transition_kinds() {
    const STEADY_SOLO_LOTTIE: &str = r#"{
      "v": "5.7.0", "fr": 30, "ip": 0, "op": 60, "w": 64, "h": 64,
      "layers": [{
        "ty": 4, "ip": 0, "op": 60, "st": 0,
        "ks": {"o": {"a": 0, "k": 100}, "p": {"a": 0, "k": [32, 32]},
               "a": {"a": 0, "k": [0, 0, 0]}, "s": {"a": 0, "k": [100, 100, 100]},
               "r": {"a": 0, "k": 0}},
        "shapes": [
          {"ty": "rc", "p": {"a": 0, "k": [0, 0]}, "s": {"a": 0, "k": [48, 48]}, "r": {"a": 0, "k": 0}},
          {"ty": "fl", "c": {"a": 0, "k": [1, 0, 0, 1]}, "o": {"a": 0, "k": 100}}
        ]
      }]
    }"#;

    let started = Instant::now();
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no GPU adapter for the headless renderer");
        return;
    };
    renderer.decode_mode = aria_decoder::OutputMode::Cpu;
    let fps = Rational::FPS_30;
    // One fixed name: every run writes the same bytes, so a run reuses the
    // file the last run left instead of adding one.
    let lottie_path = std::env::temp_dir().join("aria-steady-lottie.json");
    std::fs::write(&lottie_path, STEADY_SOLO_LOTTIE).expect("write temporary lottie fixture");
    let lottie_path_str = lottie_path.to_string_lossy().into_owned();
    let fixtures_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let video_path = fixtures_dir.join("fixture-160x90-30fps-cuts.mp4");
    let still_path = fixtures_dir.join("texture.png");
    let audio_path = fixtures_dir.join("fixture-sine-48k-2s.wav");

    let base_project = || {
        let mut project = Project::new("steady-kind", fps);
        project.timeline_mut().set_canvas(CanvasSettings {
            aspect: CanvasAspect::Auto,
            background: [0, 0, 0],
            resolution: Some([64, 64]),
        });
        project
    };

    let assert_zero_steady_realize = |renderer: &mut Renderer,
                                      project: &Project,
                                      tick: i64,
                                      label: &str| {
        let t = RationalTime::new(tick, fps);
        for _ in 0..2 {
            renderer
                .realize_frame_with(project, t, ResolveOverrides::default(), SeekPolicy::Exact)
                .unwrap_or_else(|e| panic!("warmup realize failed for {label}: {e}"));
        }
        for pass in 0..3 {
            let (res, allocs) = count_thread_allocs(|| {
                renderer.realize_frame_with(
                    project,
                    t,
                    ResolveOverrides::default(),
                    SeekPolicy::Exact,
                )
            });
            res.unwrap_or_else(|e| panic!("steady realize failed for {label}: {e}"));
            assert_eq!(
                allocs, 0,
                "steady realize_frame_with allocated {allocs} heap blocks for {label} (pass {pass})"
            );
        }
    };

    // 1. Every generator variant and every topology it renders with.
    let sticker_id = sticker_catalog()
        .first()
        .expect("shipped sticker catalog non-empty")
        .id
        .clone();
    for spec in generator_specs() {
        for &topology in spec.renders_with {
            let mut project = base_project();
            if !spec.paints {
                let base_track = project.add_track(TrackKind::Sticker, "Base");
                project
                    .add_generated(
                        base_track,
                        Generator::SolidColor {
                            rgba: [100, 120, 140, 255],
                        },
                        TimeRange::at_rate(0, 60, fps),
                    )
                    .expect("add base solid");
            }
            let track = project.add_track(spec.lane, spec.label);
            let generator = match (spec.id, topology) {
                ("text", "text") => {
                    let mut style = TextStyle {
                        font: "Inter".into(),
                        ..TextStyle::default()
                    };
                    style.runs.push(TextStyleRun {
                        start: 0,
                        end: 4,
                        font: Some("Inter".into()),
                        size: Some(36.0),
                        bold: Some(true),
                        italic: None,
                        underline: None,
                        fill: Some([255, 200, 50, 255]),
                        letter_spacing: None,
                        version: TEXT_RUN_VERSION,
                    });
                    Generator::Text {
                        content: "Aria Steady".into(),
                        style,
                    }
                }
                ("solid", "solid") => Generator::SolidColor {
                    rgba: [200, 80, 40, 255],
                },
                ("shape", "shape") => Generator::Shape {
                    shape: Shape::Ellipse,
                    rgba: Param::Constant([40, 180, 220, 255]),
                    width: Param::Constant(48.0),
                    height: Param::Constant(32.0),
                    corner_radius: Param::Constant(4.0),
                    stroke: Some(ShapeStroke {
                        rgba: Param::Constant([255, 255, 255, 255]),
                        width: Param::Constant(2.0),
                    }),
                },
                ("shape", "path-shape") => Generator::Shape {
                    shape: Shape::Path(ShapePath {
                        points: vec![
                            ShapePathPoint::corner([8.0, 8.0]),
                            ShapePathPoint::corner([56.0, 16.0]),
                            ShapePathPoint::corner([32.0, 56.0]),
                        ],
                        closed: true,
                    }),
                    rgba: Param::Constant([240, 120, 60, 255]),
                    width: Param::Constant(64.0),
                    height: Param::Constant(64.0),
                    corner_radius: Param::Constant(0.0),
                    stroke: Some(ShapeStroke {
                        rgba: Param::Constant([255, 255, 255, 255]),
                        width: Param::Constant(2.0),
                    }),
                },
                ("sticker", "sticker") => Generator::Sticker {
                    asset: sticker_id.clone(),
                },
                ("lottie", "lottie") => Generator::lottie(&lottie_path_str, 64, 64),
                ("effect" | "filter" | "adjustment", "canvas-pass") => (spec.census)(),
                (id, topo) => panic!(
                    "unhandled generator_specs() entry id='{id}' topology='{topo}'; add a zero-allocation test case"
                ),
            };
            let clip = project
                .add_generated(track, generator, TimeRange::at_rate(0, 60, fps))
                .expect("add generated clip");
            if !spec.paints {
                project
                    .add_effect(clip, "gaussian_blur")
                    .expect("add effect to canvas pass");
                project
                    .set_effect_param(clip, 0, 0, 2.0)
                    .expect("set blur radius");
            }
            let label = format!("generator:{}:{}", spec.id, topology);
            assert_zero_steady_realize(&mut renderer, &project, 5, &label);
        }
    }

    // 2. Video, Still, Audio, Masks (Rectangle, Circle, Path), Effect Mask, and Track Mattes.
    {
        let mut project = base_project();
        let v_media = project.add_media(MediaSource::new(&video_path, 160, 90, fps, 90, false));
        let s_media = project.add_media(MediaSource::image(&still_path, 64, 64));
        let a_media = project.add_media(MediaSource::new(&audio_path, 0, 0, fps, 60, true));
        let v_track = project.add_track(TrackKind::Video, "V1");
        let v_clip = project
            .add_clip(
                v_track,
                v_media,
                TimeRange::at_rate(0, 60, fps),
                RationalTime::new(0, fps),
            )
            .expect("add video clip");
        let mut rect_mask = Mask::new(MaskKind::Rectangle);
        rect_mask.feather = Param::Constant(0.1);
        let mut path_mask = Mask::new(MaskKind::Path);
        path_mask.set_path_at(
            0,
            vec![
                ShapePathPoint::corner([0.1, 0.1]),
                ShapePathPoint::corner([0.9, 0.1]),
                ShapePathPoint::corner([0.5, 0.9]),
            ],
        );
        let mut fx_mask = Mask::new(MaskKind::Circle);
        fx_mask.feather = Param::Constant(0.05);
        project.clip_mut(v_clip).expect("video clip").masks = vec![rect_mask, path_mask, fx_mask];
        project
            .add_effect(v_clip, "gaussian_blur")
            .expect("add effect");
        project
            .set_effect_param(v_clip, 0, 0, 3.0)
            .expect("set effect param");
        project.clip_mut(v_clip).expect("video clip").effects[0].masks = vec![2];

        let s_track = project.add_track(TrackKind::Video, "Still");
        let s_clip = project
            .add_clip(
                s_track,
                s_media,
                TimeRange::at_rate(0, 2_000, aria_models::STILL_TICK_RATE),
                RationalTime::new(0, fps),
            )
            .expect("add still clip");
        project.clip_mut(s_clip).expect("still clip").matte = Some(TrackMatte {
            source: MatteSource::Alpha,
            invert: false,
            softness: Param::Constant(0.1),
        });
        let stencil_track = project.add_track(TrackKind::Sticker, "MatteStencil");
        project
            .add_generated(
                stencil_track,
                Generator::SolidColor {
                    rgba: [255, 255, 255, 255],
                },
                TimeRange::at_rate(0, 60, fps),
            )
            .expect("add stencil clip");

        let a_track = project.add_track(TrackKind::Audio, "A1");
        project
            .add_clip(
                a_track,
                a_media,
                TimeRange::at_rate(0, 60, fps),
                RationalTime::new(0, fps),
            )
            .expect("add audio clip");

        assert_zero_steady_realize(
            &mut renderer,
            &project,
            5,
            "video+still+audio+masks+effect_mask+matte",
        );
    }

    // 3. Every transition in shipped_transition_catalog().
    for tspec in shipped_transition_catalog() {
        let mut project = base_project();
        let track = project.add_track(TrackKind::Sticker, "TransTrack");
        let left = project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [220, 50, 50, 255],
                },
                TimeRange::at_rate(0, 30, fps),
            )
            .expect("add left clip");
        let _right = project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [50, 120, 220, 255],
                },
                TimeRange::at_rate(30, 30, fps),
            )
            .expect("add right clip");
        project
            .add_transition(left, &tspec.id)
            .expect("set transition");
        let label = format!("transition:{}", tspec.id);
        assert_zero_steady_realize(&mut renderer, &project, 30, &label);
    }

    let _ = std::fs::remove_file(&lottie_path);
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "clip and transition zero-allocation sweep exceeded 15s bound: {:?}",
        started.elapsed()
    );
}
