//! The pure timeline → [`Scene`] resolver.
//!
//! Given a [`Project`] and a timeline instant, [`resolve`] walks the visual
//! track stack bottom-to-top, finds the clip active on each lane, and turns it
//! into a placed [`SceneLayer`]: canvas geometry, transform, crop/mirror, and a
//! classified pixel source. It decodes nothing and touches no GPU, so the
//! geometry is deterministic and unit-testable on any platform.
//!
//! ## Coverage (v1)
//!
//! - **Media**: video sources and still images (both aspect-fit into the
//!   canvas, then scaled; stills place one cached frame for the clip's whole
//!   extent).
//! - **Generators**: text, solid fills, and every shape kind — parametric
//!   shapes resolve to sampled SDF layers (animated geometry/colors are
//!   sampled per instant here, evaluated on the GPU), pen paths to CPU-raster
//!   layers.
//! - **Lane passes**: effect, filter, and adjustment generator bars resolve to
//!   canvas-wide passes over everything below their track.
//! - **Track mattes**: a clip carrying a matte consumes the layer above it as
//!   its stencil, leaving one matte wrapper where two layers were.
//! - **Stickers & Lottie**: bundled stickers and Lottie compositions resolve
//!   to placed layers, sized by their intrinsic reference-pixel dimensions.

use aria_compositor::MaskOp;
use aria_core::{RationalTime, resample};
use aria_models::{
    AnimationSlot, CanvasSettings, ClipId, ClipSource, ClipTransform, ColorAdjustments, CropRect,
    Easing, EffectInstance, Filter, Generator, LayerStyles, LookAnimations, MaskMode, MatteSource,
    MediaKind, Project, Timeline, look_animation_window_ticks,
};

use crate::animation::{apply_look_animations, is_per_character, scaled_ticks, text_knobs};
use crate::driver::DriverFrame;
use crate::grade::resolve_color_grade_at;
use crate::scene::{
    LayerSource, ResolvedPass, Scene, SceneBackground, SceneChromaKey, SceneGlow, SceneLayer,
    SceneLut, SceneMask, SceneOutline, SceneShadow, SceneStyles, SizeSpec, TextAnimation,
};

mod canvas;
mod clip;
mod generator;
mod matte;
mod nest;
mod reference;
// Two more pure moves out of this file: the matte fold and the per-tick
// samplers. Both start `use super::*`, so the imports above still own what the
// resolver names.
mod sample;
mod shape;

use canvas::timeline_canvas;
pub use canvas::{canvas_size, derived_canvas_size};
use clip::{resolve_clip, resolve_clip_pooled};
#[cfg(test)]
use generator::map_text_style;
pub(crate) use generator::{resolve_generator, resolve_generator_pooled};
use matte::{MatteRequest, fold_mattes, matte_request};
use nest::Nesting;
use sample::{
    crop_flip_uv, fit_scale, resolve_effects_pooled, resolve_styles, sample_text_animation,
};

/// Vertical reference height that a generator's reference-pixel sizes (text
/// `size`, shape `width`/`height`) are authored against. Matches the model's
/// `canvas_height / 1080` convention.
const REFERENCE_HEIGHT: f32 = 1080.0;

/// Live-preview substitutions resolved in place of committed clip state.
///
/// A drag/scale/rotate gesture overrides one clip's transform; a live
/// inspector edit (font-size slider, shape color) overrides one clip's
/// generator. Both are session-side only: the project, history, and export
/// never see them — release commits one real edit and clears the override.
#[derive(Debug, Default, Clone, Copy)]
pub struct ResolveOverrides<'a> {
    pub transform: Option<(ClipId, ClipTransform)>,
    pub generator: Option<(ClipId, &'a Generator)>,
    pub look: Option<(ClipId, Option<&'a Filter>, &'a ColorAdjustments)>,
    pub styles: Option<(ClipId, &'a LayerStyles)>,
    /// One clip's animation slots, for a catalog hover preview: the entry is shown on
    /// the selected clip before any edit exists, so moving off restores it with no
    /// history entry to undo.
    pub animation: Option<(ClipId, LookAnimations<'a>)>,
    /// One clip's crop window and mirror flags. A crop is sampled as a UV
    /// sub-rect stretched across the layer quad, so the composite already hides
    /// what a crop removed; an on-canvas crop drag overrides it to the full
    /// frame for the duration of the drag, and the handles are then drawn over
    /// the frame they are cutting into.
    pub crop: Option<(ClipId, CropRect, bool, bool)>,
    /// One vec2 effect parameter on one clip: the clip, the effect's index in
    /// its chain, the parameter's slot in the effect's catalog row, and the
    /// value. A handle drag on the Viewer shows the point it is placing before
    /// the release commits it.
    pub effect_vec2: Option<(ClipId, usize, usize, [f32; 2])>,
    /// Clear the canvas to transparent black instead of the sequence's
    /// background colour, so whatever is drawn behind the frame shows where
    /// no layer covers the canvas. The frame stays premultiplied. The
    /// Viewer's transparency grid sets it; export never does.
    pub transparent: bool,
}

impl ResolveOverrides<'_> {
    /// Whether no live-preview substitution is active.
    pub const fn is_empty(&self) -> bool {
        self.transform.is_none()
            && self.generator.is_none()
            && self.look.is_none()
            && self.styles.is_none()
            && self.animation.is_none()
            && self.crop.is_none()
            && self.effect_vec2.is_none()
            && !self.transparent
    }
}

/// Identity transform used when rasterizing the gesture sprite: the clip's
/// pixels at scale 1, centered on the canvas, with no rotation.
pub const GESTURE_IDENTITY_TRANSFORM: ClipTransform = ClipTransform {
    position: [0.0, 0.0],
    anchor_point: [0.5, 0.5],
    scale: aria_models::Scale2::ONE,
    rotation: 0.0,
    opacity: 1.0,
};

/// Three scene partitions for zero-drift preview transform gestures.
#[derive(Debug, Clone, PartialEq)]
pub struct GestureScenePartition {
    /// Layers below the dragged clip over the canvas background.
    pub below: Scene,
    /// The dragged clip alone at [`GESTURE_IDENTITY_TRANSFORM`].
    pub sprite: Scene,
    /// Layers above the dragged clip (may be empty).
    pub above: Scene,
}

/// Resolve the scene at `t`, force `clip_id`'s transform to identity, and
/// split the stack into below / sprite / above partitions. Returns `None` when
/// the clip isn't composited at `t`, sits inside a transition window, or
/// otherwise can't be sprite-partitioned (caller falls back to per-move
/// override rendering).
pub fn resolve_gesture_partitions(
    project: &Project,
    t: RationalTime,
    clip_id: ClipId,
) -> Result<Option<GestureScenePartition>, aria_models::ModelError> {
    // A clip placed in a parent's frame, or one that places others, moves more
    // than its own sprite: the caller renders each move whole.
    if parenting_involves(project, clip_id) {
        return Ok(None);
    }
    let overrides = ResolveOverrides {
        transform: Some((clip_id, GESTURE_IDENTITY_TRANSFORM)),
        generator: None,
        look: None,
        styles: None,
        animation: None,
        crop: None,
        effect_vec2: None,
        transparent: false,
    };
    Ok(partition_gesture_scene(
        resolve_with(project, t, overrides)?,
        clip_id,
    ))
}

/// Whether `clip_id` has a parent or is the parent of a clip on the open
/// timeline.
fn parenting_involves(project: &Project, clip_id: ClipId) -> bool {
    project
        .clip(clip_id)
        .is_some_and(|clip| clip.parent.is_some())
        || project
            .timeline()
            .tracks_ordered()
            .flat_map(|track| track.clips())
            .any(|clip| clip.parent == Some(clip_id))
}

fn partition_gesture_scene(mut scene: Scene, clip_id: ClipId) -> Option<GestureScenePartition> {
    // A reference draws a clip into what an effect reads: moving the sprite
    // alone would leave the effects reading it, or read by it, stale.
    if !scene.references.is_empty() {
        return None;
    }
    let index = scene
        .layers
        .iter()
        .position(|layer| layer.clip == Some(clip_id))?;
    if !scene.layers[index].source.is_sprite()
        || scene.layers[index + 1..]
            .iter()
            .any(|layer| layer.source.is_canvas_pass())
    {
        return None;
    }

    let above = scene.layers.split_off(index + 1);
    let sprite = scene
        .layers
        .pop()
        .expect("the partition index identifies a layer");
    Some(GestureScenePartition {
        below: Scene {
            width: scene.width,
            height: scene.height,
            background: scene.background,
            tick: scene.tick,
            layers: scene.layers,
            references: Vec::new(),
        },
        sprite: Scene {
            width: scene.width,
            height: scene.height,
            background: [0, 0, 0, 0],
            tick: scene.tick,
            layers: vec![sprite],
            references: Vec::new(),
        },
        above: Scene {
            width: scene.width,
            height: scene.height,
            background: [0, 0, 0, 0],
            tick: scene.tick,
            layers: above,
            references: Vec::new(),
        },
    })
}

pub(crate) use sample::ResolvePool;

/// Resolve `project` at timeline instant `t` into a [`Scene`].
///
/// `t` is interpreted at the timeline frame rate (it is resampled to it first),
/// so callers may pass a tick at any rate.
pub fn resolve(project: &Project, t: RationalTime) -> Result<Scene, aria_models::ModelError> {
    resolve_with(project, t, ResolveOverrides::default())
}

/// [`resolve`] with live-preview [`ResolveOverrides`] applied.
pub fn resolve_with(
    project: &Project,
    t: RationalTime,
    overrides: ResolveOverrides<'_>,
) -> Result<Scene, aria_models::ModelError> {
    let mut scene = Scene::empty(0, 0, [0, 0, 0, 255]);
    let mut pool = ResolvePool::default();
    resolve_with_into(project, t, overrides, &mut scene, &mut pool)?;
    Ok(scene)
}

pub(crate) fn resolve_with_into(
    project: &Project,
    t: RationalTime,
    overrides: ResolveOverrides<'_>,
    scene: &mut Scene,
    pool: &mut ResolvePool,
) -> Result<(), aria_models::ModelError> {
    let timeline = project.timeline();
    let (width, height) = canvas_size(project);
    let bg = timeline.canvas().background;
    let background = if overrides.transparent {
        [0, 0, 0, 0]
    } else {
        [bg[0], bg[1], bg[2], 255]
    };
    resolve_timeline_into(
        project,
        timeline,
        t,
        [width, height],
        background,
        overrides,
        &Nesting::root(project),
        scene,
        pool,
    )
}

/// The scene of `timeline` at `t`, resampled to its rate, on a
/// `width`×`height` canvas over `background`. `nesting` holds the sequences
/// this resolve is inside, so a nest that would show itself is a cycle.
fn resolve_timeline<'p>(
    project: &'p Project,
    timeline: &'p Timeline,
    t: RationalTime,
    [width, height]: [u32; 2],
    background: [u8; 4],
    overrides: ResolveOverrides<'_>,
    nesting: &Nesting<'_>,
) -> Result<Scene, aria_models::ModelError> {
    let mut scene = Scene::empty(width, height, background);
    let mut pool = ResolvePool::default();
    resolve_timeline_into(
        project,
        timeline,
        t,
        [width, height],
        background,
        overrides,
        nesting,
        &mut scene,
        &mut pool,
    )?;
    Ok(scene)
}

#[allow(clippy::too_many_arguments)]
fn resolve_timeline_into<'p>(
    project: &'p Project,
    timeline: &'p Timeline,
    t: RationalTime,
    [width, height]: [u32; 2],
    background: [u8; 4],
    overrides: ResolveOverrides<'_>,
    nesting: &Nesting<'_>,
    scene: &mut Scene,
    pool: &mut ResolvePool,
) -> Result<(), aria_models::ModelError> {
    let t = resample(t, timeline.frame_rate);
    pool.recycle_scene(scene);
    scene.width = width;
    scene.height = height;
    scene.background = background;
    scene.tick = t;

    let cw = width as f32;
    let ch = height as f32;

    let mut requests: Vec<Option<MatteRequest>> = Vec::new();
    let mut drivers = DriverFrame::in_timeline(project, timeline);
    for track in timeline.tracks_ordered() {
        if !track.kind.is_visual() || !timeline.track_is_visible(track) {
            continue;
        }
        if let Some(layer) = resolve_track_at(
            project,
            track,
            t,
            cw,
            ch,
            overrides,
            nesting,
            &mut drivers,
            pool,
        )? {
            if let Some(req) = matte_request(project, &layer, t, &mut drivers) {
                requests.resize_with(scene.layers.len(), || None);
                requests.push(Some(req));
            } else if !requests.is_empty() {
                requests.push(None);
            }
            scene.layers.push(layer);
        }
    }
    if !requests.is_empty() {
        scene.layers = fold_mattes(std::mem::take(&mut scene.layers), requests, cw, ch);
    }
    scene.references = reference::resolve_references(
        project,
        timeline,
        &scene.layers,
        t,
        [cw, ch],
        overrides,
        nesting,
        &mut drivers,
    )?;

    Ok(())
}

/// Resolve one visual track at timeline instant `t`, its clips after their
/// drivers.
#[allow(clippy::too_many_arguments)]
fn resolve_track_at<'p>(
    project: &'p Project,
    track: &'p aria_models::Track,
    t: RationalTime,
    cw: f32,
    ch: f32,
    overrides: ResolveOverrides<'_>,
    nesting: &Nesting<'_>,
    drivers: &mut DriverFrame<'p>,
    pool: &mut ResolvePool,
) -> Result<Option<SceneLayer>, aria_models::ModelError> {
    // Transition window takes precedence over single-clip resolve.
    for transition in track.transitions() {
        // A transition whose plugin is missing, failed, or disabled is a cut:
        // the clip under the playhead draws alone, and the saved id stays on
        // the junction for when the plugin returns.
        if aria_models::transition_spec(&transition.transition_id).is_none() {
            continue;
        }
        let left = track
            .clip(transition.left)
            .ok_or(aria_models::ModelError::UnknownClip(transition.left))?;
        let right = track
            .clip(transition.right)
            .ok_or(aria_models::ModelError::UnknownClip(transition.right))?;
        if left.timeline.end_tick() != right.timeline.start.value {
            continue;
        }
        let cut = left.timeline.end_tick();
        let (window_start, window_end) = transition.window(cut);
        if t.value >= window_start && t.value < window_end {
            let progress = (t.value - window_start) as f32 / transition.duration as f32;
            // Each side plays live wherever it has material and holds its
            // boundary frame past it: the outgoing clip runs until the cut
            // then freezes on its last frame, the incoming holds its first
            // frame until the cut then runs — CapCut's motion, and the tail
            // frame is only requested for the window's back half. Clamped
            // into each clip's extent (not just at the cut) because the
            // model doesn't bound the duration by the clips' lengths.
            let outgoing_t = RationalTime::new(
                t.value
                    .clamp(left.timeline.start.value, left.timeline.end_tick() - 1),
                t.rate,
            );
            let incoming_t = RationalTime::new(
                t.value
                    .clamp(right.timeline.start.value, right.timeline.end_tick() - 1),
                t.rate,
            );
            // A side that produces no layer (e.g. empty text) or a canvas-wide
            // pass (effect/filter/adjustment segments) can't be composited as
            // a transition frame — the renderer rejects nested canvas passes.
            // Skip the transition and resolve the track normally so the
            // preview keeps updating instead of erroring every frame.
            let left = drivers.apply(left, outgoing_t.value as f64);
            let right = drivers.apply(right, incoming_t.value as f64);
            let outgoing =
                resolve_clip_pooled(project, &left, outgoing_t, cw, ch, overrides, nesting, pool)?;
            let incoming = resolve_clip_pooled(
                project, &right, incoming_t, cw, ch, overrides, nesting, pool,
            )?;
            let (Some(outgoing), Some(incoming)) = (outgoing, incoming) else {
                break;
            };
            if outgoing.source.is_canvas_pass() || incoming.source.is_canvas_pass() {
                break;
            }
            let outgoing = Box::new(outgoing);
            let incoming = Box::new(incoming);
            let transition_params = transition
                .sample_flattened_floats((t.value - window_start) as f64)
                .unwrap_or_default();
            let effects = if transition_params.is_empty() {
                Vec::new()
            } else {
                let id = aria_compositor::pass_id(&transition.transition_id)
                    .unwrap_or_else(|| aria_compositor::PassId::from("crossfade"));
                vec![ResolvedPass::new(id, transition_params)]
            };
            return Ok(Some(SceneLayer {
                clip: None,
                source: LayerSource::Transition {
                    outgoing,
                    incoming,
                    transition_id: transition.transition_id.clone(),
                    progress,
                },
                center: [cw * 0.5, ch * 0.5],
                anchor_point: [0.5, 0.5],
                size: SizeSpec::Fixed([cw, ch]),
                rotation: 0.0,
                opacity: 1.0,
                uv: [0.0, 0.0, 1.0, 1.0],
                effects,
                masks: Vec::new(),
                chroma_key: None,
                color_grade: None,
                lut: None,
                blend_mode: aria_models::BlendMode::Normal,
                styles: None,
                blur_passes: Vec::new(),
            }));
        }
    }

    let Some(clip) = track.clip_at(t)? else {
        return Ok(None);
    };
    let clip = drivers.apply(clip, t.value as f64);
    resolve_clip_pooled(project, &clip, t, cw, ch, overrides, nesting, pool)
}

#[cfg(test)]
mod matte_tests;
#[cfg(test)]
mod per_char_tests;
#[cfg(test)]
mod tests;
// Two suites are declared here rather than inside `tests`, so their names read
// `resolve::lift_gamma_gain` and `resolve::lane_keyframe`: the paths the
// grading and lane-keyframe work is checked by. They still borrow the shared
// fixtures, which `tests` exposes to this module and everything under it.
#[cfg(test)]
#[path = "resolve/tests_lane_keyframe.rs"]
mod lane_keyframe;
#[cfg(test)]
#[path = "resolve/tests_lift_gamma_gain.rs"]
mod lift_gamma_gain;
#[cfg(test)]
#[path = "resolve/tests_multicam.rs"]
mod multicam;
