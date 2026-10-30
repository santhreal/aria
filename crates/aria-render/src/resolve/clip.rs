//! One clip → one placed [`SceneLayer`]: sampled transform, crop, look, mask,
//! chroma key, styles, and the classified pixel source behind it.

use super::nest::{resolve_nest, uv_in_frame};
use super::*;

/// Which picture a clip reads at `t`: the media, its probed source, and the
/// shift between the clip's source clock and that file's own clock.
///
/// A plain media clip reads itself, so the shift is zero. A multicam clip reads
/// whichever angle its cut list makes live at that tick, and the shift is that
/// angle's sync offset: the offset is where the camera's first frame sits on the
/// group's common clock, so a camera that started late (positive offset) is read
/// that much earlier in its own file. The cut list is addressed in the group's
/// clock, which is the clip's source time, so moving the clip along the lane
/// does not move which camera is live under it.
fn read_at<'p>(
    project: &'p Project,
    clip: &aria_models::Clip,
    t: RationalTime,
) -> Result<
    Option<(aria_models::MediaId, &'p aria_models::MediaSource, i64)>,
    aria_models::ModelError,
> {
    match &clip.content {
        ClipSource::Media { media, .. } => Ok(project.media(*media).map(|src| (*media, src, 0))),
        ClipSource::Multicam { group, .. } => {
            let Some(group) = project.multicam_group(*group) else {
                return Ok(None);
            };
            let Some(source_time) = clip.source_time_at(t)? else {
                return Ok(None);
            };
            let Some(angle) = group.live_at(source_time.value) else {
                return Ok(None);
            };
            Ok(project
                .media(angle.media)
                .map(|src| (angle.media, src, angle.sync_offset)))
        }
        ClipSource::Generated(_) | ClipSource::Sequence { .. } => Ok(None),
    }
}

pub(super) fn resolve_clip(
    project: &Project,
    clip: &aria_models::Clip,
    t: RationalTime,
    cw: f32,
    ch: f32,
    overrides: ResolveOverrides<'_>,
    nesting: &Nesting<'_>,
) -> Result<Option<SceneLayer>, aria_models::ModelError> {
    let mut pool = super::sample::ResolvePool::default();
    resolve_clip_pooled(project, clip, t, cw, ch, overrides, nesting, &mut pool)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_clip_pooled(
    project: &Project,
    clip: &aria_models::Clip,
    t: RationalTime,
    cw: f32,
    ch: f32,
    overrides: ResolveOverrides<'_>,
    nesting: &Nesting<'_>,
    pool: &mut super::sample::ResolvePool,
) -> Result<Option<SceneLayer>, aria_models::ModelError> {
    if !clip.enabled {
        return Ok(None);
    }
    // The tick this clip's parameters are addressed at: clip-local for
    // anything that paints, absolute for a lane bar, as its catalog row
    // declares. Both `t` and the clip start are expressed at the timeline rate.
    let local_tick = clip.param_tick(t.value);
    let local_tick_f = clip.param_tick_f(t.value as f64);
    // A hover preview substitutes this clip's animation slots; anything else renders
    // what the clip carries.
    let anims = match overrides.animation {
        Some((id, anims)) if id == clip.id => anims,
        _ => LookAnimations::of(clip),
    };
    // A live gesture replaces the whole sampled transform for its clip.
    let xf = match overrides.transform {
        Some((id, xf)) if id == clip.id => xf,
        _ => {
            let base = clip.transform.sample(local_tick);
            apply_look_animations(
                anims,
                clip.timeline.duration.value,
                base,
                // An edge animation is defined against the clip's own span
                // (fade over its first N ticks), so it reads a clip-local tick
                // even where the clip's parameters are addressed absolutely.
                clip.animation_tick(t.value),
                clip.animation_tick_f(t.value as f64),
                t.rate,
            )
        }
    };
    // A parent that covers this instant places the clip's own pose in its
    // frame, a gesture on the parent included.
    let xf = crate::parent::world_pose(
        project,
        clip,
        xf,
        t.value as f64,
        t.rate,
        [cw, ch],
        overrides.transform,
    );

    // `position` is the anchor's offset from the canvas center, as a fraction
    // of canvas width/height. The layer carries the anchor position plus the
    // normalized `anchor_point`; the renderer derives the quad center once the
    // final pixel size is known (identity for the default center anchor).
    let center = [cw * (0.5 + xf.position[0]), ch * (0.5 + xf.position[1])];
    let anchor_point = xf.anchor_point;
    let rotation = xf.rotation.to_radians();
    let opacity = xf.opacity.clamp(0.0, 1.0);
    let uv = crop_flip_uv(clip, local_tick, overrides);
    let seconds = RationalTime::new(clip.animation_tick(t.value), t.rate).seconds() as f32;
    let effects = resolve_effects_pooled(clip, local_tick, seconds, overrides.effect_vec2, pool);
    let (filter, adjust) = match overrides.look {
        Some((id, filter, adjust)) if id == clip.id => (filter, adjust),
        _ => (clip.filter.as_ref(), &clip.adjust),
    };
    let color_grade = resolve_color_grade_at(filter, adjust, local_tick);
    // File-backed `.cube` LUT (applied after the grade). Zero intensity is
    // identity — drop it here so downstream stages keep their fast paths.
    let lut = clip
        .lut
        .as_ref()
        .filter(|l| l.intensity.sample(local_tick) > 0.0)
        .map(|l| SceneLut {
            path: l.path.clone(),
            intensity: l.intensity.sample(local_tick),
        });
    let chroma_key = clip.chroma_key.as_ref().map(|chroma| SceneChromaKey {
        rgb: chroma.rgb,
        strength: chroma.strength.sample(local_tick),
        shadow: chroma.shadow.sample(local_tick),
    });
    // Isotropic: stroke / blur / style ref-px → canvas px. Geometric mean
    // keeps widths stable under uniform scale and splits the difference
    // under stretch.
    let px = (ch / REFERENCE_HEIGHT) * xf.scale.isotropic();
    let masks = resolve_masks(&clip.masks, local_tick, px);
    let styles_src = match overrides.styles {
        Some((id, s)) if id == clip.id => s,
        _ => &clip.styles,
    };
    let styles = resolve_styles(styles_src, local_tick, px);

    match &clip.content {
        ClipSource::Media { .. } | ClipSource::Multicam { .. } => {
            let Some((media, src, shift)) = read_at(project, clip, t)? else {
                return Ok(None);
            };
            // Both picture kinds aspect-fit into the canvas at their probed
            // size; audio-only media places nothing. A multicam angle fits at
            // its own size, so a group of mixed formats is not letterboxed to
            // whichever camera happens to be first.
            let source = match src.kind() {
                MediaKind::Video => {
                    let Some(source_time) = clip.source_time_at(t)? else {
                        return Ok(None);
                    };
                    LayerSource::Media {
                        media,
                        // A decoder cannot answer for a negative tick, so a read
                        // before this camera's first frame stalls on it: a held
                        // frame beats a hole in the middle of a cut.
                        source_time: RationalTime::new(
                            (source_time.value - shift).max(0),
                            source_time.rate,
                        ),
                    }
                }
                // One frame for the clip's whole extent: no source time, and
                // retime/reverse are irrelevant by construction.
                MediaKind::Image => LayerSource::Still { media },
                MediaKind::Audio => return Ok(None),
            };
            let fit = fit_scale(src.width as f32, src.height as f32, cw, ch);
            // Per-axis placement: width × x, height × y.
            let size = SizeSpec::Fixed([
                src.width as f32 * fit * xf.scale.x,
                src.height as f32 * fit * xf.scale.y,
            ]);
            Ok(Some(SceneLayer {
                clip: Some(clip.id),
                source,
                center,
                anchor_point,
                size,
                rotation,
                opacity,
                uv,
                effects,
                masks,
                chroma_key,
                color_grade,
                lut,
                blend_mode: clip.blend_mode,
                styles,
                blur_passes: Vec::new(),
            }))
        }
        ClipSource::Generated(generator) => {
            // A live inspector edit replaces the clip's generator content.
            let generator = match overrides.generator {
                Some((id, live)) if id == clip.id => live,
                _ => generator,
            };
            // A generator whose plugin is missing, failed, or disabled draws a
            // slate stating its id where its picture was, unless it is bypassed.
            let slate = clip
                .effects
                .iter()
                .find(|fx| !fx.bypass && fx.is_missing_generator())
                .map(|fx| missing_generator_slate(&fx.effect_id));
            let generator = slate.as_ref().unwrap_or(generator);
            Ok(resolve_generator_pooled(
                generator,
                center,
                anchor_point,
                rotation,
                opacity,
                uv,
                color_grade,
                lut,
                cw,
                ch,
                xf.scale,
                local_tick,
                local_tick as f64 * t.rate.seconds_per_unit(),
                effects,
                pool,
            )
            .map(|mut layer| {
                layer.clip = Some(clip.id);
                // A mask cuts what the generator draws: a title, a shape, a
                // sticker, or, for an adjustment layer, the region the pass
                // touches.
                layer.masks = masks;
                // Canvas passes grade/effect the whole stack — blend modes
                // and layer styles only apply to layer quads.
                if layer.source.takes_layer_styles() {
                    layer.blend_mode = clip.blend_mode;
                    layer.styles = styles;
                }
                if let LayerSource::Text { animation, .. } = &mut layer.source {
                    *animation = sample_text_animation(
                        anims,
                        clip.timeline.duration.value,
                        local_tick,
                        local_tick_f,
                        t.rate,
                    );
                }
                layer
            }))
        }
        ClipSource::Sequence { sequence, .. } => {
            let Some(source_time) = clip.source_time_at(t)? else {
                return Ok(None);
            };
            let Some(nest) = resolve_nest(
                project,
                *sequence,
                source_time,
                [cw, ch],
                overrides,
                nesting,
            )?
            else {
                return Ok(None);
            };
            let [width, height] = nest.size;
            Ok(Some(SceneLayer {
                clip: Some(clip.id),
                source: LayerSource::Nest {
                    scene: Box::new(nest.scene),
                    frame: nest.frame,
                },
                center,
                anchor_point,
                size: SizeSpec::Fixed([width * xf.scale.x, height * xf.scale.y]),
                rotation,
                opacity,
                uv: uv_in_frame(uv, nest.frame),
                effects,
                masks,
                chroma_key,
                color_grade,
                lut,
                blend_mode: clip.blend_mode,
                styles,
                blur_passes: Vec::new(),
            }))
        }
    }
}

/// Sample a clip's mask stack at clip-local `tick`, top first, with
/// expansion in canvas pixels at `px` canvas pixels per reference pixel. A
/// mask whose mode is `None` is left out of the stack.
fn resolve_masks(masks: &[aria_models::Mask], tick: i64, px: f32) -> Vec<SceneMask> {
    masks
        .iter()
        .filter_map(|mask| {
            let op = match mask.mode {
                MaskMode::None => return None,
                MaskMode::Add => MaskOp::Add,
                MaskMode::Subtract => MaskOp::Subtract,
                MaskMode::Intersect => MaskOp::Intersect,
                MaskMode::Lighten => MaskOp::Lighten,
                MaskMode::Darken => MaskOp::Darken,
                MaskMode::Difference => MaskOp::Difference,
            };
            Some(SceneMask {
                kind: mask.kind,
                feather: mask.feather.sample(tick),
                op,
                opacity: mask.opacity.sample(tick).clamp(0.0, 1.0),
                expansion: mask.expansion.sample(tick) * px,
                center: mask.center.sample(tick),
                size: mask.size.sample(tick),
                rotation_rad: mask.rotation.sample(tick).to_radians(),
                roundness: mask.roundness.sample(tick),
                invert: mask.invert,
                path: mask
                    .path_at(tick as f64)
                    .iter()
                    .map(|p| aria_shapes::PathPoint {
                        anchor: p.anchor,
                        handle_in: p.handle_in,
                        handle_out: p.handle_out,
                    })
                    .collect(),
                path_closed: mask.path_closed,
            })
        })
        .collect()
}

/// The card drawn in place of a generator effect that no catalog lists: its id
/// in light text on an opaque dark slate. One text layer, so the clip's
/// transform, opacity, and remaining effects apply to it as they did to the
/// generator's picture.
fn missing_generator_slate(effect_id: &str) -> Generator {
    Generator::Text {
        content: format!("Missing generator\n{effect_id}"),
        style: aria_models::TextStyle {
            fill: aria_models::Param::Constant([232, 232, 232, 255]),
            background: Some(aria_models::TextBackground {
                rgba: aria_models::Param::Constant([38, 38, 38, 255]),
                radius: aria_models::Param::Constant(0.0),
            }),
            ..aria_models::TextStyle::default()
        },
    }
}
