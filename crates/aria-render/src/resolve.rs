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

use aria_core::{RationalTime, resample};
use aria_models::{
    AnimationSlot, ClipId, ClipSource, ClipTransform, ColorAdjustments, CropRect, Easing,
    EffectInstance, Filter, Generator, LayerStyles, LookAnimations, MatteSource, MediaKind,
    Project, look_animation_window_ticks,
};

use crate::animation::{apply_look_animations, is_per_character, scaled_ticks, text_knobs};
use crate::grade::resolve_color_grade_at;
use crate::scene::{
    LayerSource, ResolvedPass, Scene, SceneBackground, SceneChromaKey, SceneGlow, SceneLayer,
    SceneLut, SceneMask, SceneOutline, SceneShadow, SceneStyles, SizeSpec, TextAnimation,
};

mod clip;
mod generator;
mod matte;
// Two more pure moves out of this file: the matte fold and the per-tick
// samplers. Both start `use super::*`, so the imports above still own what the
// resolver names.
mod sample;
mod shape;

use clip::resolve_clip;
#[cfg(test)]
use generator::map_text_style;
pub(crate) use generator::resolve_generator;
use matte::{MatteRequest, fold_mattes, matte_request};
use sample::{crop_flip_uv, fit_scale, resolve_effects, resolve_styles, sample_text_animation};

/// Vertical reference height that a generator's reference-pixel sizes (text
/// `size`, shape `width`/`height`) are authored against. Matches the model's
/// `canvas_height / 1080` convention.
const REFERENCE_HEIGHT: f32 = 1080.0;

/// Fallback canvas size when `Auto` aspect can't find any video media.
const DEFAULT_CANVAS: (u32, u32) = (1920, 1080);

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
    let overrides = ResolveOverrides {
        transform: Some((clip_id, GESTURE_IDENTITY_TRANSFORM)),
        generator: None,
        look: None,
        styles: None,
        animation: None,
        crop: None,
    };
    Ok(partition_gesture_scene(
        resolve_with(project, t, overrides)?,
        clip_id,
    ))
}

fn partition_gesture_scene(mut scene: Scene, clip_id: ClipId) -> Option<GestureScenePartition> {
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
        },
        sprite: Scene {
            width: scene.width,
            height: scene.height,
            background: [0, 0, 0, 0],
            tick: scene.tick,
            layers: vec![sprite],
        },
        above: Scene {
            width: scene.width,
            height: scene.height,
            background: [0, 0, 0, 0],
            tick: scene.tick,
            layers: above,
        },
    })
}

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
    let timeline = project.timeline();
    let rate = timeline.frame_rate;
    let t = resample(t, rate);

    let (width, height) = canvas_size(project);
    let bg = timeline.canvas().background;
    let mut scene = Scene::empty(width, height, [bg[0], bg[1], bg[2], 255]);
    scene.tick = t;

    let cw = width as f32;
    let ch = height as f32;

    let mut requests: Vec<Option<MatteRequest>> = Vec::new();
    for track in timeline.tracks_ordered() {
        if !track.kind.is_visual() || !timeline.track_is_visible(track) {
            continue;
        }
        if let Some(layer) = resolve_track_at(project, track, t, cw, ch, overrides)? {
            requests.push(matte_request(project, &layer, t));
            scene.layers.push(layer);
        }
    }
    scene.layers = fold_mattes(std::mem::take(&mut scene.layers), requests, cw, ch);

    Ok(scene)
}

/// Resolve one visual track at timeline instant `t`.
fn resolve_track_at(
    project: &Project,
    track: &aria_models::Track,
    t: RationalTime,
    cw: f32,
    ch: f32,
    overrides: ResolveOverrides<'_>,
) -> Result<Option<SceneLayer>, aria_models::ModelError> {
    // Transition window takes precedence over single-clip resolve.
    for transition in track.transitions() {
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
        let half = transition.duration / 2;
        let window_start = cut - half;
        let window_end = window_start + transition.duration;
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
            let outgoing = resolve_clip(project, left, outgoing_t, cw, ch, overrides)?;
            let incoming = resolve_clip(project, right, incoming_t, cw, ch, overrides)?;
            let (Some(outgoing), Some(incoming)) = (outgoing, incoming) else {
                break;
            };
            if outgoing.source.is_canvas_pass() || incoming.source.is_canvas_pass() {
                break;
            }
            let outgoing = Box::new(outgoing);
            let incoming = Box::new(incoming);
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
                effects: Vec::new(),
                mask: None,
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
    resolve_clip(project, clip, t, cw, ch, overrides)
}

/// Canvas pixel size for `project`: an explicit size wins; otherwise fixed
/// presets resolve to a 1080-baseline box on the shorter side, and `Auto`
/// follows the largest video media (falling back to 1920×1080 when there is
/// none).
pub fn canvas_size(project: &Project) -> (u32, u32) {
    let canvas = project.timeline().canvas();
    if let Some([w, h]) = canvas.resolution {
        return (even(w), even(h));
    }
    derived_canvas_size(project)
}

/// [`canvas_size`] as if no explicit size were set: the shape the aspect preset
/// or the footage implies.
///
/// The delivery-size chooser scales this rather than the current size, so a
/// project that already carries an explicit size still offers the shape its
/// footage produced instead of scaling the size it was handed. Asking for it
/// directly is also the difference between a read and a copy of the whole
/// project: the caller used to clear the resolution on a clone.
pub fn derived_canvas_size(project: &Project) -> (u32, u32) {
    let canvas = project.timeline().canvas();
    match canvas.aspect.pixels_for_baseline(REFERENCE_HEIGHT as u32) {
        Some([w, h]) => (w, h),
        None => auto_canvas_size(project),
    }
}

/// The largest video media used anywhere on the timeline, or the default.
fn auto_canvas_size(project: &Project) -> (u32, u32) {
    project
        .largest_used_video_dimensions()
        .map_or(DEFAULT_CANVAS, |(width, height)| {
            (even(width), even(height))
        })
}

/// Round `v` down to the nearest even value (≥ 2): H.264 needs even dimensions.
fn even(v: u32) -> u32 {
    (v & !1).max(2)
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
