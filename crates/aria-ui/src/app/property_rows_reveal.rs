//! Property reveal chords: `Alt+P/S/R/T/A/E/M/L/U` and ``Alt+` `` open, toggle,
//! or close explicit property rows on the targeted clips.

use std::sync::Arc;

use aria_models::{Clip, ClipId, ClipParam, ParamValue};

use super::{ClipRevealMode, PropertyGroup, RevealAction, lane_model};
use crate::app::AppState;
use crate::app::clip_keys::{KeyParam, key_params_of};
use crate::app::decline::Decline;
use crate::app::mask_path_keys::path_masks;

impl AppState {
    /// Reveal or toggle timeline property rows via `Alt+P/S/R/T/A/E/M/L/U` or
    /// close every open property row via ``Alt+` ``.
    pub fn reveal_properties(&mut self, action: RevealAction, shift: bool) {
        if action == RevealAction::CloseAll {
            Arc::make_mut(&mut self.expanded_clips).clear();
            self.with_prop_rows_mut(|s| {
                s.clip_modes.clear();
            });
            self.refresh_expanded_models();
            return;
        }
        let targets: Vec<ClipId> = if !self.selection.is_empty() {
            self.selection.clone()
        } else {
            let at = self.playhead.value;
            self.store
                .sequence
                .tracks
                .iter()
                .filter(|t| t.targeted && !t.locked)
                .flat_map(|t| t.clips.iter())
                .filter(|c| !c.locked && c.timeline_start_ticks <= at && at < c.timeline_end_ticks)
                .map(|c| ClipId::from_raw(c.id))
                .collect()
        };
        if targets.is_empty() {
            self.decline(Decline::NoSelection);
            return;
        }

        for clip_id in targets {
            let raw = clip_id.raw();
            let prev_animated_u = self.with_prop_rows(|s| {
                matches!(
                    s.clip_modes.get(&raw),
                    Some(ClipRevealMode::Explicit {
                        last_was_animated_u: true,
                        ..
                    })
                ) && self.expanded_clips.contains(&raw)
            });
            // A clip not yet open has no projected model, and this is an
            // edit of the view rather than a frame, so it reads the session.
            let held = {
                let engine = self.engine.lock();
                lane_model(engine.project(), clip_id)
            };
            let (resolved, is_animated_u) = match &held {
                Some((clip, audio_lane)) => {
                    resolve_reveal_params(clip, *audio_lane, action, prev_animated_u)
                }
                None => (Revealed::default(), false),
            };

            let currently_open = self.expanded_clips.contains(&raw);

            if shift {
                let mut next = if currently_open {
                    Revealed {
                        params: self.visible_params_for_clip(clip_id),
                        paths: self.visible_mask_paths(clip_id),
                    }
                } else {
                    Revealed::default()
                };
                if !resolved.is_empty() && next.contains_all(&resolved) {
                    next.remove(&resolved);
                } else {
                    next.add(resolved);
                }
                if next.is_empty() {
                    Arc::make_mut(&mut self.expanded_clips).retain(|id| *id != raw);
                    self.with_prop_rows_mut(|s| {
                        s.clip_modes.remove(&raw);
                    });
                } else {
                    let open = Arc::make_mut(&mut self.expanded_clips);
                    if !open.contains(&raw) {
                        open.push(raw);
                    }
                    self.with_prop_rows_mut(|s| {
                        s.clip_modes.insert(
                            raw,
                            ClipRevealMode::Explicit {
                                params: next.params,
                                paths: next.paths,
                                last_was_animated_u: false,
                            },
                        );
                    });
                }
            } else {
                let same_already_open = currently_open
                    && action != RevealAction::AnimatedOrModified
                    && self.with_prop_rows(|s| {
                        matches!(
                            s.clip_modes.get(&raw),
                            Some(ClipRevealMode::Explicit { params, paths, .. })
                                if *params == resolved.params && *paths == resolved.paths
                        )
                    });
                if same_already_open {
                    Arc::make_mut(&mut self.expanded_clips).retain(|id| *id != raw);
                    self.with_prop_rows_mut(|s| {
                        s.clip_modes.remove(&raw);
                    });
                } else {
                    let open = Arc::make_mut(&mut self.expanded_clips);
                    if !open.contains(&raw) {
                        open.push(raw);
                    }
                    self.with_prop_rows_mut(|s| {
                        s.clip_modes.insert(
                            raw,
                            ClipRevealMode::Explicit {
                                params: resolved.params,
                                paths: resolved.paths,
                                last_was_animated_u: is_animated_u,
                            },
                        );
                    });
                }
            }
        }
        self.refresh_expanded_models();
    }
}

/// `clip` with every property that has a defined rest value put back at it:
/// the transform, the crop, gain and pan, the colour adjustments, and each
/// effect's parameters (an unwritten effect parameter reads its catalog
/// default). Blocks without a rest value (mask, key, matte, filter, LUT, layer
/// styles, generator geometry) keep their values, so their properties count as
/// modified only when animated or driven.
fn rest_state(clip: &Clip) -> Clip {
    let mut rest = clip.clone();
    rest.transform = Default::default();
    rest.crop = aria_models::Param::Constant(aria_models::CropRect::FULL);
    rest.volume = aria_models::Param::Constant(1.0);
    rest.pan = aria_models::Param::Constant(0.0);
    rest.adjust = Default::default();
    for effect in &mut rest.effects {
        effect.params.clear();
        effect.color_params.clear();
        effect.linear_color_params.clear();
        effect.vec2_params.clear();
        effect.vec3_params.clear();
        effect.bool_params.clear();
        effect.choice_params.clear();
        effect.curve_params.clear();
        effect.gradient_params.clear();
        effect.text_params.clear();
        effect.layer_params.clear();
    }
    rest
}

/// Whether `row` is animated, driven, or holds a value other than its value on
/// `rest` (see [`rest_state`]).
fn is_param_modified(clip: &Clip, rest: &Clip, row: &KeyParam) -> bool {
    if !row.marks.is_empty() || clip.driver(row.param()).is_some() {
        return true;
    }
    let local = clip.param_tick(clip.timeline.start.value);
    match (row.spec.sample(clip, local), row.spec.sample(rest, local)) {
        (Some(value), Some(rest)) => !same_value(value, rest),
        (Some(_), None) => true,
        _ => false,
    }
}

/// Whether two sampled values are equal within the drift a keyframe round trip
/// leaves in an `f32`.
fn same_value(a: ParamValue, b: ParamValue) -> bool {
    const EPSILON: f32 = 1e-4;
    let close = |x: &[f32], y: &[f32]| x.iter().zip(y).all(|(p, q)| (p - q).abs() <= EPSILON);
    match (a, b) {
        (ParamValue::Scalar(x), ParamValue::Scalar(y)) => (x - y).abs() <= EPSILON,
        (ParamValue::Vec2(x), ParamValue::Vec2(y)) => close(&x, &y),
        (ParamValue::Vec3(x), ParamValue::Vec3(y)) => close(&x, &y),
        (ParamValue::Rect(x), ParamValue::Rect(y)) => close(&x, &y),
        (ParamValue::Color(x), ParamValue::Color(y)) => x == y,
        _ => false,
    }
}

/// What a reveal chord opens on one clip: catalog properties, and the path
/// masks whose Path row shows.
#[derive(Debug, Clone, Default, PartialEq)]
struct Revealed {
    params: Vec<ClipParam>,
    /// Mask indices, ascending.
    paths: Vec<u32>,
}

impl Revealed {
    fn is_empty(&self) -> bool {
        self.params.is_empty() && self.paths.is_empty()
    }

    fn contains_all(&self, other: &Self) -> bool {
        other.params.iter().all(|param| self.params.contains(param))
            && other.paths.iter().all(|path| self.paths.contains(path))
    }

    fn remove(&mut self, other: &Self) {
        self.params.retain(|param| !other.params.contains(param));
        self.paths.retain(|path| !other.paths.contains(path));
    }

    fn add(&mut self, other: Self) {
        for param in other.params {
            if !self.params.contains(&param) {
                self.params.push(param);
            }
        }
        for path in other.paths {
            if !self.paths.contains(&path) {
                self.paths.push(path);
            }
        }
        self.paths.sort_unstable();
    }
}

/// The rows a reveal chord opens on `clip`, and whether this press was the
/// first `U` of a pair.
///
/// A Path row opens with the masks, with the animated properties when its path
/// is keyframed, and with the modified ones always: a path is drawn, so it
/// never sits at a default.
fn resolve_reveal_params(
    clip: &Clip,
    audio_lane: bool,
    action: RevealAction,
    second_u: bool,
) -> (Revealed, bool) {
    let rows = key_params_of(clip, audio_lane);
    let filter_in = |pred: &dyn Fn(&KeyParam) -> bool| -> Vec<ClipParam> {
        rows.iter()
            .filter(|r| pred(r))
            .map(KeyParam::param)
            .collect()
    };
    let only = |params: Vec<ClipParam>| Revealed {
        params,
        paths: Vec::new(),
    };
    let paths = |keyed: bool| -> Vec<u32> {
        if audio_lane {
            return Vec::new();
        }
        path_masks(clip)
            .filter(|&mask| !keyed || clip.masks[mask as usize].path.is_animated())
            .collect()
    };
    match action {
        RevealAction::Position => (
            only(filter_in(&|r| r.param() == ClipParam::Position)),
            false,
        ),
        RevealAction::Scale => (only(filter_in(&|r| r.param() == ClipParam::Scale)), false),
        RevealAction::Rotation => (
            only(filter_in(&|r| r.param() == ClipParam::Rotation)),
            false,
        ),
        RevealAction::Opacity => (only(filter_in(&|r| r.param() == ClipParam::Opacity)), false),
        RevealAction::Anchor => (
            only(filter_in(&|r| r.param() == ClipParam::AnchorPoint)),
            false,
        ),
        RevealAction::Effects => (
            only(filter_in(&|r| {
                PropertyGroup::of(&r.spec) == Some(PropertyGroup::Effects)
            })),
            false,
        ),
        RevealAction::Masks => (
            Revealed {
                params: filter_in(&|r| PropertyGroup::of(&r.spec) == Some(PropertyGroup::Masks)),
                paths: paths(false),
            },
            false,
        ),
        RevealAction::Audio => (
            only(filter_in(&|r| {
                matches!(r.param(), ClipParam::Volume | ClipParam::Pan)
            })),
            false,
        ),
        RevealAction::AnimatedOrModified if !second_u => (
            Revealed {
                params: filter_in(&|r| !r.marks.is_empty() || clip.driver(r.param()).is_some()),
                paths: paths(true),
            },
            true,
        ),
        RevealAction::AnimatedOrModified | RevealAction::Modified => {
            let rest = rest_state(clip);
            (
                Revealed {
                    params: filter_in(&|r| is_param_modified(clip, &rest, r)),
                    paths: paths(false),
                },
                false,
            )
        }
        RevealAction::CloseAll => (Revealed::default(), false),
    }
}
