use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::{Duration, Instant};

struct CountingAlloc;

thread_local! {
    static THREAD_ALLOCS: Cell<u64> = const { Cell::new(0) };
}

// SAFETY: Forwards every allocation, deallocation, and reallocation directly
// to `std::alloc::System` with identical layout and pointer arguments, only
// incrementing a thread-local counter on successful heap allocations.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let _ = THREAD_ALLOCS.try_with(|c| c.set(c.get().saturating_add(1)));
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let out = unsafe { System.realloc(ptr, layout, new_size) };
        if !out.is_null() {
            let _ = THREAD_ALLOCS.try_with(|c| c.set(c.get().saturating_add(1)));
        }
        out
    }
}

#[global_allocator]
static COUNTING_ALLOC: CountingAlloc = CountingAlloc;

fn count_thread_allocs<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let before = THREAD_ALLOCS.with(Cell::get);
    let out = f();
    let after = THREAD_ALLOCS.with(Cell::get);
    (out, after.saturating_sub(before))
}

use super::*;
use crate::scene::SizeSpec;

#[test]
fn frame_scratch_retains_its_container_allocations() {
    let mut scratch = RealizeScratch::default();
    scratch.realized.reserve(8);
    scratch.scene_indices.reserve(8);
    scratch.levels.push(0..1);
    scratch.item_levels.push(0..1);
    let realized = scratch.realized.as_ptr();
    let scene_indices = scratch.scene_indices.as_ptr();
    let levels = scratch.levels.as_ptr();
    let item_levels = scratch.item_levels.as_ptr();

    scratch.begin_frame();

    assert_eq!(scratch.realized.as_ptr(), realized);
    assert_eq!(scratch.scene_indices.as_ptr(), scene_indices);
    assert_eq!(scratch.levels.as_ptr(), levels);
    assert_eq!(scratch.item_levels.as_ptr(), item_levels);
    assert!(scratch.realized.is_empty() && scratch.scene_indices.is_empty());
    assert!(scratch.levels.is_empty() && scratch.item_levels.is_empty());
}

fn layer(source: LayerSource, effect: f32) -> SceneLayer {
    SceneLayer {
        clip: None,
        source,
        center: [8.0, 8.0],
        anchor_point: [0.5, 0.5],
        size: SizeSpec::Fixed([16.0, 16.0]),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: vec![crate::scene::ResolvedPass::new(
            "gaussian_blur",
            vec![effect],
        )],
        masks: Vec::new(),
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: aria_models::BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    }
}

fn effect_value(effects: &[PassInstance]) -> f32 {
    effects[0].params[0]
}

#[test]
fn omitted_text_does_not_shift_the_next_plain_layers_effects() {
    let empty_text = layer(
        LayerSource::Text {
            content: String::new(),
            style: aria_text::TextStyle::new(24.0),
            animation: None,
        },
        1.0,
    );
    let visible = layer(LayerSource::Solid([255, 255, 255, 255]), 2.0);
    let mut scene = crate::scene::Scene::empty(16, 16, [0, 0, 0, 255]);
    scene.layers = vec![empty_text, visible];

    let effects = scene.drawn(1).effects.as_slice();

    assert_eq!(effect_value(effects), 2.0);
    assert_eq!(
        effects[0].params.as_ptr(),
        scene.layers[1].effects[0].params.as_ptr()
    );

    // A reference draws first, so every canvas layer's draw index moves by one.
    scene.references = vec![layer(LayerSource::Solid([0, 0, 0, 255]), 3.0)];
    assert_eq!(effect_value(&scene.drawn(0).effects), 3.0);
    assert_eq!(effect_value(&scene.drawn(2).effects), 2.0);
}

#[test]
fn transition_side_effects_remain_associated() {
    let outgoing = layer(LayerSource::Solid([255, 0, 0, 255]), 3.0);
    let incoming = layer(LayerSource::Solid([0, 0, 255, 255]), 4.0);
    let wrapper = layer(
        LayerSource::Transition {
            outgoing: Box::new(outgoing),
            incoming: Box::new(incoming),
            transition_id: "dissolve".into(),
            progress: 0.5,
        },
        99.0,
    );

    let (outgoing, incoming) = transition_effects(&wrapper);

    assert_eq!(effect_value(outgoing), 3.0);
    assert_eq!(effect_value(incoming), 4.0);
}

#[test]
fn matte_side_effects_remain_associated() {
    let content = layer(LayerSource::Solid([255, 0, 0, 255]), 5.0);
    let stencil = layer(LayerSource::Solid([0, 0, 0, 255]), 6.0);
    let wrapper = layer(
        LayerSource::Matte {
            layers: Box::new([content, stencil]),
            read: MatteSource::Alpha,
            invert: false,
            softness: 0.0,
        },
        99.0,
    );

    let (content, stencil) = matte_effects(&wrapper);

    assert_eq!(effect_value(content), 5.0);
    assert_eq!(effect_value(stencil), 6.0);
}

fn build_steady_alloc_project(case: aria_models::TextCase) -> Project {
    use aria_core::Rational;
    use aria_models::{
        AnimationRef, AnimationSlot, CanvasAspect, CanvasSettings, Generator, TextStyle, TimeRange,
        TrackKind,
    };

    let fps = Rational::FPS_60;
    let mut project = Project::new("steady-alloc", fps);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Auto,
        background: [0, 0, 0],
        resolution: Some([64, 64]),
    });
    let entrance = aria_models::animation_catalog()
        .iter()
        .find(|spec| spec.slot == AnimationSlot::In && !spec.text_only)
        .expect("whole-layer entrance preset");
    let exit = aria_models::animation_catalog()
        .iter()
        .find(|spec| spec.slot == AnimationSlot::Out && !spec.text_only)
        .expect("whole-layer exit preset");

    for (idx, rgba) in [[220, 40, 40, 255], [40, 220, 40, 255], [40, 40, 220, 255]]
        .into_iter()
        .enumerate()
    {
        let track = project.add_track(TrackKind::Sticker, format!("S{idx}"));
        let clip = project
            .add_generated(
                track,
                Generator::SolidColor { rgba },
                TimeRange::at_rate(0, 120, fps),
            )
            .expect("add solid clip");
        project
            .add_effect(clip, "gaussian_blur")
            .expect("add gaussian_blur");
        project
            .set_effect_param(clip, 0, 0, 4.0)
            .expect("set radius");
        project
            .set_clip_animation(
                clip,
                AnimationSlot::In,
                Some(AnimationRef::new(&entrance.id)),
            )
            .expect("set entrance animation");
        project
            .set_clip_animation(clip, AnimationSlot::Out, Some(AnimationRef::new(&exit.id)))
            .expect("set exit animation");
    }

    let text_track = project.add_track(TrackKind::Text, "T0");
    let style = TextStyle {
        case,
        ..TextStyle::default()
    };
    project
        .add_generated(
            text_track,
            Generator::Text {
                content: "Aria Steady Frame".into(),
                style,
            },
            TimeRange::at_rate(0, 120, fps),
        )
        .expect("add text clip");
    project
}

/// WHY: Closes the per-frame heap-allocation class across timeline resolve,
/// look-ahead resolve, shipped `PassId` lookup, effect parameter sampling,
/// matte folding when no clip requests a matte, whole-layer look animations,
/// and `TextCase` application across all variants (`Normal`, `Upper`, `Lower`,
/// `Title`). Does not catch GPU driver or `wgpu` command-encoding allocations,
/// which are covered by `steady_frame_render_allocations_stay_at_wgpu_floor`.
#[test]
fn steady_resolve_and_look_ahead_allocate_zero_heap_blocks() {
    use crate::resolve::{ResolveOverrides, ResolvePool, resolve_with_into};
    use aria_core::{Rational, RationalTime};
    use aria_models::TextCase;

    let started = Instant::now();
    for case in [
        TextCase::Normal,
        TextCase::Upper,
        TextCase::Lower,
        TextCase::Title,
    ] {
        let project = build_steady_alloc_project(case);
        let mut scene = Scene::empty(0, 0, [0, 0, 0, 255]);
        let mut pool = ResolvePool::default();

        // Warm the pool on tick 0, then recycle the resolved scene.
        resolve_with_into(
            &project,
            RationalTime::new(0, Rational::FPS_60),
            ResolveOverrides::default(),
            &mut scene,
            &mut pool,
        )
        .expect("warmup resolve");
        pool.recycle_scene(&mut scene);

        for tick in [1_i64, 2, 3, 4, 60, 118, 119] {
            let (res, allocs) = count_thread_allocs(|| {
                let r = resolve_with_into(
                    &project,
                    RationalTime::new(tick, Rational::FPS_60),
                    ResolveOverrides::default(),
                    &mut scene,
                    &mut pool,
                );
                pool.recycle_scene(&mut scene);
                r
            });
            res.expect("steady resolve");
            assert_eq!(
                allocs, 0,
                "steady resolve_with_into allocated heap blocks: {allocs}, at tick {tick} for {case:?}"
            );
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "steady resolve allocation sweep exceeded 2s bound: {:?}",
        started.elapsed()
    );
}

/// WHY: Closes the per-frame render-thread heap-allocation class across
/// `Renderer::render_frame_to_texture_with`, `RealizeScratch` / `InlineVec`
/// realization tables, static text bitmap memoization (`Renderer::text_memo`),
/// look-ahead scene pooling, and `wgpu` command-encoder / render-pass label
/// `String` allocations. Each frame is counted on its own and compared with a
/// pre-resolved `render_scene_to_texture` frame, so one allocation on one frame
/// fails. Every counted frame starts on an idle device: `Queue::submit` retires
/// the finished submissions it finds, and while earlier frames are still in
/// flight `wgpu` grows its submission and encoder pools by a count that depends
/// on GPU timing. The device is the test's own: the process-wide headless
/// device also takes the other tests' submissions, and the poll that ends
/// `Queue::submit` retires whichever of them have finished on the submitting
/// thread, inside this test's count. The absolute floor is checked on
/// lavapipe, the adapter CI renders on.
/// Does not catch allocations inside hardware-only video decoder backends
/// (NVDEC/VideoToolbox), or allocations made only while earlier frames are
/// still in flight.
#[test]
fn steady_frame_render_allocations_stay_at_wgpu_floor() {
    use crate::resolve::ResolveOverrides;
    use aria_core::{Rational, RationalTime};
    use aria_models::TextCase;

    const STEADY_FRAMES: usize = 8;
    let started = Instant::now();
    let shared = aria_compositor::GpuContext::new_headless_blocking().expect("headless GPU");
    // `new_headless_blocking` hands a GL device out unshared already.
    let gpu = if shared.adapter.get_info().backend == wgpu::Backend::Gl {
        shared
    } else {
        let (device, queue) =
            pollster::block_on(shared.adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("steady-frame-allocations"),
                required_limits: shared.adapter.limits(),
                ..Default::default()
            }))
            .expect("a device of the test's own");
        aria_compositor::GpuContext::from_parts(shared.instance, shared.adapter, device, queue)
    };
    let mut renderer = Renderer::with_gpu(gpu);
    let adapter = renderer.gpu().adapter.get_info();
    let device = renderer.gpu().device.clone();
    let idle = || {
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device finishes its submitted frames");
    };
    let project = build_steady_alloc_project(TextCase::Title);
    let target = aria_compositor::create_frame_target(&device, 64, 64);

    // Warm up caches and pools across 2 frames before the 1s mark (tick 60).
    for tick in 58..60 {
        renderer
            .render_frame_to_texture_with(
                &project,
                RationalTime::new(tick, Rational::FPS_60),
                ResolveOverrides::default(),
                SeekPolicy::Exact,
                |_, _| target.clone(),
            )
            .expect("warmup frame render");
    }

    let mut frame_allocs = [0_u64; STEADY_FRAMES];
    for (allocs, tick) in frame_allocs.iter_mut().zip(60_i64..) {
        idle();
        let (res, count) = count_thread_allocs(|| {
            renderer.render_frame_to_texture_with(
                &project,
                RationalTime::new(tick, Rational::FPS_60),
                ResolveOverrides::default(),
                SeekPolicy::Exact,
                |_, _| target.clone(),
            )
        });
        res.expect("steady frame render");
        *allocs = count;
    }

    let warm_scene = crate::resolve::resolve(&project, RationalTime::new(60, Rational::FPS_60))
        .expect("resolve warm scene");
    let mut scene_allocs = [0_u64; STEADY_FRAMES];
    for allocs in &mut scene_allocs {
        idle();
        let (res, count) = count_thread_allocs(|| {
            renderer.render_scene_to_texture(&project, &warm_scene, &target)
        });
        res.expect("steady scene render");
        *allocs = count;
    }
    let floor = scene_allocs[0];
    assert_eq!(
        scene_allocs, [floor; STEADY_FRAMES],
        "pre-resolved render_scene_to_texture allocated a different count per frame on an idle device"
    );
    assert_eq!(
        frame_allocs, [floor; STEADY_FRAMES],
        "render_frame_to_texture_with allocated these blocks per frame against {floor} for pre-resolved render_scene_to_texture"
    );
    // For 1 CommandEncoder + 12 RenderPasses (3 × (layer-into + 2 gaussian_blur
    // + canvas blit), with the initial canvas clear fused into the first canvas
    // blit and the final text layer fused into the last canvas blit) + 1
    // Queue::submit with every descriptor label `None`, wgpu 29.0.4 on lavapipe
    // allocates 158 blocks. An allocation on the path both renders share
    // (realize InlineVec, text_memo, PassId) or a descriptor label String
    // raises it. Other adapters allocate their own counts (wgpu's GL backend on
    // an Intel iGPU: 222), so the floor holds for lavapipe only.
    const LAVAPIPE_STEADY_FLOOR: u64 = 158;
    if adapter.backend == wgpu::Backend::Vulkan && adapter.name.contains("llvmpipe") {
        assert!(
            floor <= LAVAPIPE_STEADY_FLOOR,
            "steady frame allocated {floor} blocks/frame on {}, exceeding the lavapipe floor {LAVAPIPE_STEADY_FLOOR}",
            adapter.name
        );
    } else {
        eprintln!(
            "lavapipe floor not checked on {} ({:?}): {floor} blocks/frame",
            adapter.name, adapter.backend
        );
    }
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "steady frame render test exceeded 10s bound: {:?}",
        started.elapsed()
    );
}

/// WHY: A full-canvas unmodified 1080p BT.709 CPU NV12 media frame was
/// uploaded to the GPU, composited on `llvmpipe`, read back as 8.29 MB of
/// RGBA, and converted back to NV12 on the CPU for every export frame.
/// `scene_is_nv12_passthrough` passes the decoded planes to the encoder only
/// when the scene and the decoded frame need no GPU work. Closes taking the
/// passthrough when any single `SceneLayer` field, a second layer, a
/// reference, a non-media source, or the frame format, visible rect, rotation
/// or colour space asks for composition. A `SceneLayer` field added later
/// fails to compile in the predicate. Does not cover a GPU-resident frame or
/// a nested sequence, which need a device and a project to build.
#[test]
fn scene_is_nv12_passthrough_accepts_only_unmodified_full_canvas_bt709_cpu_nv12() {
    use aria_core::{
        ColorSpace, CpuImage, FrameData, PixelFormat, Plane, Rational, RationalTime, Rect,
        Rotation, VideoFrame,
    };
    use aria_models::{BlendMode, MediaId};

    let make_state = |modify_scene: &dyn Fn(&mut Scene), modify_frame: &dyn Fn(&mut VideoFrame)| {
        let media = MediaId::next();
        let mut scene = crate::scene::Scene::empty(1920, 1080, [0, 0, 0, 255]);
        scene.layers = vec![SceneLayer {
            clip: None,
            source: LayerSource::Media {
                media,
                source_time: RationalTime::new(0, Rational::FPS_60),
            },
            center: [960.0, 540.0],
            anchor_point: [0.5, 0.5],
            size: SizeSpec::Fixed([1920.0, 1080.0]),
            rotation: 0.0,
            opacity: 1.0,
            uv: [0.0, 0.0, 1.0, 1.0],
            effects: Vec::new(),
            color_grade: None,
            lut: None,
            chroma_key: None,
            blend_mode: BlendMode::Normal,
            masks: Vec::new(),
            styles: None,
            blur_passes: Vec::new(),
        }];
        modify_scene(&mut scene);
        let mut frame = VideoFrame::new(
            RationalTime::new(0, Rational::FPS_60),
            PixelFormat::Nv12,
            ColorSpace::BT709,
            (1920, 1080),
            Rect::from_size(1920, 1080),
            Rotation::None,
            FrameData::Cpu(CpuImage {
                planes: vec![
                    Plane {
                        data: vec![16; 1920 * 4],
                        stride: 1920,
                        rows: 4,
                    },
                    Plane {
                        data: vec![128; 1920 * 2],
                        stride: 1920,
                        rows: 2,
                    },
                ],
            }),
        );
        modify_frame(&mut frame);
        let mut scratch = RealizeScratch::default();
        scratch.realized.push(Realized::Layer(Leaf {
            pixels: Pixels::Frame(Shown {
                key: super::super::look_ahead::DecoderKey { media, clip: None },
                source_time: RationalTime::new(0, Rational::FPS_60),
                policy: SeekPolicy::Exact,
                frame,
            }),
            placement: LayerPlacement::full_canvas(&CompositorConfig::new(1920, 1080)),
            uv: [0.0, 0.0, 1.0, 1.0],
            fx: aria_compositor::LayerEffects::default(),
            color_grade: None,
            lut: None,
            blend_mode: aria_compositor::BlendMode::Normal,
            styles: aria_compositor::LayerStyles::default(),
            blur: Vec::new(),
            stencil: None,
        }));
        let nests = Nests::new(&scene);
        scene_is_nv12_passthrough(&scene, &nests, &scratch)
    };

    assert!(
        make_state(&|_| {}, &|_| {}),
        "unmodified full-canvas BT.709 CPU NV12 frame must qualify for passthrough"
    );
    let mask = crate::scene::SceneMask {
        kind: aria_models::MaskKind::Rectangle,
        feather: 0.0,
        op: aria_compositor::MaskOp::Add,
        opacity: 1.0,
        expansion: 0.0,
        center: [0.0, 0.0],
        size: [1.0, 1.0],
        rotation_rad: 0.0,
        roundness: 0.0,
        invert: false,
        path: Vec::new(),
        path_closed: false,
    };
    type SceneEdit = Box<dyn Fn(&mut Scene)>;
    let scene_cases: Vec<(&str, SceneEdit)> = vec![
        (
            "still source",
            Box::new(|s: &mut Scene| {
                s.layers[0].source = LayerSource::Still {
                    media: MediaId::next(),
                }
            }),
        ),
        (
            "off-centre",
            Box::new(|s: &mut Scene| s.layers[0].center = [961.0, 540.0]),
        ),
        (
            "moved anchor",
            Box::new(|s: &mut Scene| s.layers[0].anchor_point = [0.0, 0.0]),
        ),
        (
            "scaled size",
            Box::new(|s: &mut Scene| s.layers[0].size = SizeSpec::Fixed([1910.0, 1080.0])),
        ),
        (
            "rotation",
            Box::new(|s: &mut Scene| s.layers[0].rotation = 0.1),
        ),
        (
            "opacity",
            Box::new(|s: &mut Scene| s.layers[0].opacity = 0.5),
        ),
        (
            "cropped uv",
            Box::new(|s: &mut Scene| s.layers[0].uv = [0.1, 0.0, 0.9, 1.0]),
        ),
        (
            "mirrored uv",
            Box::new(|s: &mut Scene| s.layers[0].uv = [1.0, 0.0, 0.0, 1.0]),
        ),
        (
            "effect",
            Box::new(|s: &mut Scene| {
                s.layers[0]
                    .effects
                    .push(crate::scene::ResolvedPass::new("gaussian_blur", vec![2.0]))
            }),
        ),
        (
            "mask",
            Box::new(move |s: &mut Scene| s.layers[0].masks.push(mask.clone())),
        ),
        (
            "chroma key",
            Box::new(|s: &mut Scene| {
                s.layers[0].chroma_key = Some(crate::scene::SceneChromaKey {
                    rgb: [0, 255, 0],
                    strength: 0.5,
                    shadow: 0.0,
                })
            }),
        ),
        (
            "colour grade",
            Box::new(|s: &mut Scene| {
                s.layers[0].color_grade = Some(aria_compositor::ColorGrade::default())
            }),
        ),
        (
            "lut",
            Box::new(|s: &mut Scene| {
                s.layers[0].lut = Some(SceneLut {
                    path: "look.cube".into(),
                    intensity: 1.0,
                })
            }),
        ),
        (
            "blend mode",
            Box::new(|s: &mut Scene| s.layers[0].blend_mode = BlendMode::Multiply),
        ),
        (
            "styles",
            Box::new(|s: &mut Scene| {
                s.layers[0].styles = Some(crate::scene::SceneStyles {
                    shadow: None,
                    glow: None,
                    outline: Some(crate::scene::SceneOutline {
                        rgba: [255; 4],
                        width: 2.0,
                    }),
                    background: None,
                })
            }),
        ),
        (
            "motion blur",
            Box::new(|s: &mut Scene| {
                s.layers[0].blur_passes.push(crate::scene::SceneBlurPass {
                    center: [960.0, 540.0],
                    size: [1920.0, 1080.0],
                    rotation: 0.0,
                    opacity: 0.5,
                })
            }),
        ),
        (
            "second layer",
            Box::new(|s: &mut Scene| {
                let top = s.layers[0].clone();
                s.layers.push(top);
            }),
        ),
        (
            "reference",
            Box::new(|s: &mut Scene| {
                let read = s.layers[0].clone();
                s.references.push(read);
            }),
        ),
    ];
    for (name, edit) in &scene_cases {
        assert!(
            !make_state(edit.as_ref(), &|_| {}),
            "{name}: the scene must composite on the GPU"
        );
    }
    type FrameEdit = Box<dyn Fn(&mut VideoFrame)>;
    let frame_cases: Vec<(&str, FrameEdit)> = vec![
        (
            "RGBA format",
            Box::new(|f: &mut VideoFrame| f.format = PixelFormat::Rgba8),
        ),
        (
            "smaller visible rect",
            Box::new(|f: &mut VideoFrame| f.visible = Rect::from_size(1280, 720)),
        ),
        (
            "offset visible rect",
            Box::new(|f: &mut VideoFrame| f.visible = Rect::new(8, 0, 1920, 1080)),
        ),
        (
            "container rotation",
            Box::new(|f: &mut VideoFrame| f.rotation = Rotation::Cw90),
        ),
        (
            "BT.601 colour",
            Box::new(|f: &mut VideoFrame| f.color = ColorSpace::BT601),
        ),
    ];
    for (name, edit) in &frame_cases {
        assert!(
            !make_state(&|_| {}, edit.as_ref()),
            "{name}: the frame must composite on the GPU"
        );
    }
}

/// WHY: the export loop selects the passthrough from the encoder's
/// `accepts_cpu_nv12`, retimes the decoded frame to the output timeline, and
/// returns it to its decoder after the push. Closes an encoder that takes RGBA
/// only receiving NV12 planes, an NV12 encoder receiving a composite it did
/// not need, a passed-through frame keeping its source timestamp, and the
/// look-ahead pairing a pending frame with the planes of a different source
/// frame. Does not cover the encoder packing the planes, which the
/// `aria-encoder` tests cover, or a GPU-resident decoded frame.
#[test]
fn export_passes_decoded_planes_only_to_an_encoder_that_takes_cpu_nv12() {
    use aria_core::{
        EncodeError, EncoderChoice, PixelFormat, Rational, RationalTime, VideoEncoder, VideoFrame,
    };
    use aria_models::{CanvasAspect, CanvasSettings, MediaSource, Project, TimeRange, TrackKind};

    /// Format, timestamp, and `(bytes, stride, rows)` of each plane pushed.
    type Pushed = (PixelFormat, RationalTime, Vec<(Vec<u8>, usize, usize)>);

    struct Recorder {
        nv12: bool,
        frames: Vec<Pushed>,
    }

    impl VideoEncoder for Recorder {
        fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
            let cpu = frame.cpu().expect("export pushes CPU frames");
            let planes = cpu
                .planes
                .iter()
                .map(|plane| (plane.data.clone(), plane.stride, plane.rows))
                .collect();
            self.frames.push((frame.format, frame.pts, planes));
            Ok(())
        }

        fn finish(&mut self) -> Result<(), EncodeError> {
            Ok(())
        }

        fn choice(&self) -> EncoderChoice {
            EncoderChoice::only("recorder", Some(false))
        }

        fn accepts_cpu_nv12(&self) -> bool {
            self.nv12
        }
    }

    const FRAMES: i64 = 3;
    // Source frames 30..33 at output frames 0..3: a frame that keeps its
    // source timestamp is a second off its output slot.
    const SOURCE_START: i64 = 30;
    let fps = Rational::FPS_30;
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4");
    let mut project = Project::new("passthrough", fps);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Auto,
        background: [0, 0, 0],
        resolution: Some([640, 360]),
    });
    let media = project.add_media(MediaSource::new(&path, 640, 360, fps, 90, false));
    let track = project.add_track(TrackKind::Video, "V1");
    project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(SOURCE_START, FRAMES, fps),
            RationalTime::new(0, fps),
        )
        .expect("place the fixture clip");

    let export = |nv12: bool| -> Option<Vec<Pushed>> {
        let Ok(mut renderer) = Renderer::new_headless() else {
            eprintln!("skipping: no GPU adapter for the headless renderer");
            return None;
        };
        renderer.decode_mode = aria_decoder::OutputMode::Cpu;
        let mut recorder = Recorder {
            nv12,
            frames: Vec::new(),
        };
        let settings = crate::ExportSettings::for_project(&project);
        let written = crate::export_observed(
            &mut renderer,
            &project,
            &mut recorder,
            settings,
            &mut |_, _| true,
        )
        .expect("export the lone clip");
        assert_eq!(written, FRAMES as u64);
        Some(recorder.frames)
    };
    let Some(nv12) = export(true) else {
        return;
    };
    let Some(rgba) = export(false) else {
        return;
    };

    let mut decoder = aria_decoder::open_video_decoder(&path, aria_decoder::OutputMode::Cpu)
        .expect("the fixture opens");
    for skipped in 0..SOURCE_START {
        decoder
            .next_frame()
            .expect("decode the fixture")
            .unwrap_or_else(|| panic!("the fixture holds frame {skipped}"));
    }
    assert_eq!(nv12.len(), FRAMES as usize);
    assert_eq!(rgba.len(), FRAMES as usize);
    for (index, ((format, pts, planes), (rgba_format, rgba_pts, _))) in
        nv12.iter().zip(&rgba).enumerate()
    {
        let at = RationalTime::new(index as i64, fps);
        let source = decoder
            .next_frame()
            .expect("decode the fixture")
            .unwrap_or_else(|| panic!("the fixture holds frame {}", SOURCE_START + index as i64));
        let expected: Vec<(Vec<u8>, usize, usize)> = source
            .cpu()
            .expect("CPU decode yields CPU planes")
            .planes
            .iter()
            .map(|plane| (plane.data.clone(), plane.stride, plane.rows))
            .collect();
        assert_eq!(*format, PixelFormat::Nv12, "frame {index} was composited");
        assert_eq!(
            *pts, at,
            "frame {index} kept a timestamp other than its output slot"
        );
        assert!(
            *planes == expected,
            "frame {index} carries planes other than source frame {}",
            SOURCE_START + index as i64
        );
        assert_eq!(
            *rgba_format,
            PixelFormat::Rgba8,
            "frame {index} reached an RGBA encoder as {rgba_format:?}"
        );
        assert_eq!(*rgba_pts, at);
    }
}
