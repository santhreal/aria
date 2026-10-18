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
                "steady resolve_with_into allocated {allocs} heap block(s) at tick {tick} for {case:?}"
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
/// `String` allocations on the lavapipe software Vulkan path. Does not catch
/// allocations inside hardware-only video decoder backends (NVDEC/VideoToolbox).
#[test]
fn steady_frame_render_allocations_stay_at_wgpu_floor() {
    use crate::resolve::ResolveOverrides;
    use aria_core::{Rational, RationalTime};
    use aria_models::TextCase;

    let started = Instant::now();
    let mut renderer = Renderer::new_headless().expect("lavapipe headless renderer");
    let project = build_steady_alloc_project(TextCase::Title);
    let target = aria_compositor::create_frame_target(&renderer.gpu().device, 64, 64);

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

    let steady_frames = 4_u64;
    let (res, total_allocs) = count_thread_allocs(|| -> Result<(), RenderError> {
        for tick in 60..(60 + steady_frames as i64) {
            renderer.render_frame_to_texture_with(
                &project,
                RationalTime::new(tick, Rational::FPS_60),
                ResolveOverrides::default(),
                SeekPolicy::Exact,
                |_, _| target.clone(),
            )?;
        }
        Ok(())
    });
    res.expect("steady frame renders");

    let allocs_per_frame = total_allocs / steady_frames;
    let warm_scene = crate::resolve::resolve(&project, RationalTime::new(60, Rational::FPS_60))
        .expect("resolve warm scene");
    let (scene_res, scene_total_allocs) = count_thread_allocs(|| -> Result<(), RenderError> {
        for _ in 0..steady_frames {
            renderer.render_scene_to_texture(&project, &warm_scene, &target)?;
        }
        Ok(())
    });
    scene_res.expect("steady scene renders");
    let scene_allocs_per_frame = scene_total_allocs / steady_frames;
    assert_eq!(
        allocs_per_frame, scene_allocs_per_frame,
        "render_frame_to_texture_with allocated {allocs_per_frame}/frame vs {scene_allocs_per_frame}/frame for pre-resolved render_scene_to_texture"
    );
    // For 1 CommandEncoder + 8 RenderPasses (canvas clear + 3 layer-into +
    // 3 gaussian_blur + 1 text blit) + 1 Queue::submit, wgpu 29.0.4 + lavapipe
    // allocates 177 internal blocks on a steady frame when all descriptor
    // labels are `None`. Any re-introduced aria-side allocation (resolve,
    // realize InlineVec, text_memo, PassId, look_ahead) or wgpu descriptor
    // label String pushes this above the 177 wgpu floor.
    const WGPU_STEADY_FLOOR: u64 = 177;
    assert!(
        allocs_per_frame <= WGPU_STEADY_FLOOR,
        "steady frame allocated {allocs_per_frame} blocks/frame (total {total_allocs} over {steady_frames} frames), exceeding wgpu floor {WGPU_STEADY_FLOOR}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "steady frame render test exceeded 10s bound: {:?}",
        started.elapsed()
    );
}
