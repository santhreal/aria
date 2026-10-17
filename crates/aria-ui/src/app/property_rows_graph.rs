//! Keyframe selection, time dragging, ease shortcuts, and timeline graph editor.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    Clip, ClipId, ClipParam, Easing, MIN_TEMPORAL_INFLUENCE, ParamKind, ParamSpec, ParamValue,
    RationalTime, SpatialTangents, TemporalEase,
};
use gpui::SharedString;

use super::{
    CopiedKeyframe, KeyframeContextMenu, KeyframeIconShape, KeyframeTimeDrag, SelectedKeyframe,
};
use crate::app::AppState;
use crate::app::clip_keys::key_params_of;
use crate::app::curve_geometry::{CurveBounds, build_curve_polyline, sample_param_scalar};

/// Which graph the timeline graph editor draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphMode {
    #[default]
    Value,
    Speed,
}

/// Incoming or outgoing side of a keyframe's temporal ease or spatial tangent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphHandleSide {
    Incoming,
    Outgoing,
}

/// One keyframe node in the timeline graph editor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphKeyNode {
    pub clip: ClipId,
    pub param: ClipParam,
    pub tick: i64,
    pub value: f32,
    pub speed: f32,
    pub x: f32,
    pub y: f32,
    pub shape: KeyframeIconShape,
    pub selected: bool,
    pub roving: bool,
    pub hold: bool,
    pub interior: bool,
}

/// One incoming or outgoing temporal influence + speed handle on a keyframe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphInfluenceHandle {
    pub clip: ClipId,
    pub param: ClipParam,
    pub tick: i64,
    pub side: GraphHandleSide,
    pub anchor_x: f32,
    pub anchor_y: f32,
    pub handle_x: f32,
    pub handle_y: f32,
    pub segment_px_w: f32,
    pub influence: f32,
    pub speed: f32,
}

/// One spatial tangent handle on a `Position` keyframe in the value graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphSpatialHandle {
    pub clip: ClipId,
    pub tick: i64,
    pub side: GraphHandleSide,
    pub anchor_x: f32,
    pub anchor_y: f32,
    pub handle_x: f32,
    pub handle_y: f32,
    pub tangent: [f32; 2],
}

/// One curve plotted in the timeline graph editor.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineGraphCurve {
    pub clip: ClipId,
    pub param: ClipParam,
    pub label: SharedString,
    pub samples: Vec<(f32, f32)>,
    pub keys: Vec<GraphKeyNode>,
    pub influence_handles: Vec<GraphInfluenceHandle>,
    pub spatial_handles: Vec<GraphSpatialHandle>,
}

/// Complete model prepared for one frame of the timeline graph editor.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineGraphModel {
    pub mode: GraphMode,
    pub min_y: f32,
    pub max_y: f32,
    pub top_label: SharedString,
    pub mid_label: SharedString,
    pub bottom_label: SharedString,
    pub curves: Vec<TimelineGraphCurve>,
}

/// In-flight pointer drag inside the timeline graph editor.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphDragTarget {
    Keyframe {
        clip: ClipId,
        param: ClipParam,
        origin_tick: i64,
        origin_value: f32,
        current_tick: i64,
        current_value: f32,
        easing: Easing,
    },
    Influence {
        clip: ClipId,
        param: ClipParam,
        tick: i64,
        side: GraphHandleSide,
        anchor_x: f32,
        anchor_y: f32,
        segment_px_w: f32,
        in_ease: TemporalEase,
        out_ease: TemporalEase,
        hold: bool,
        min_y: f32,
        max_y: f32,
        height: f32,
    },
    SpatialTangent {
        clip: ClipId,
        tick: i64,
        side: GraphHandleSide,
        origin_x: f32,
        origin_y: f32,
        origin_tangents: SpatialTangents,
        current_tangents: SpatialTangents,
        value: ParamValue,
        easing: Easing,
    },
}

impl AppState {
    /// Whether the timeline graph editor is open (`Shift+F3`).
    pub fn timeline_graph_open(&self) -> bool {
        self.with_prop_rows(|s| s.graph_open)
    }

    /// Toggle the timeline graph editor (`Shift+F3` or header button).
    pub fn toggle_timeline_graph_editor(&mut self) {
        let next = self.with_prop_rows_mut(|s| {
            s.graph_open = !s.graph_open;
            if s.graph_open && s.graph_params.is_empty() {
                if let Some(first) = s.selected_keyframes.first() {
                    s.graph_params.push((first.clip, first.param));
                }
            }
            s.graph_open
        });
        if next && self.with_prop_rows(|s| s.graph_params.is_empty()) {
            if let Some(clip) = self.selected_clip() {
                let vis = self.visible_params_for_clip(clip);
                let param = vis
                    .first()
                    .copied()
                    .or_else(|| self.curve_param())
                    .unwrap_or(ClipParam::Position);
                self.with_prop_rows_mut(|s| {
                    s.graph_params.push((clip, param));
                });
            }
        }
    }

    /// Current timeline graph mode (`Value` or `Speed`).
    pub fn timeline_graph_mode(&self) -> GraphMode {
        self.with_prop_rows(|s| s.graph_mode)
    }

    /// Set the timeline graph mode (`Value` or `Speed`).
    pub fn set_timeline_graph_mode(&mut self, mode: GraphMode) {
        self.with_prop_rows_mut(|s| {
            s.graph_mode = mode;
            s.graph_drag = None;
        });
    }

    /// Select a property row for display in the timeline graph editor.
    pub fn select_timeline_property(&mut self, clip: ClipId, param: ClipParam, additive: bool) {
        self.set_curve_param(param);
        self.with_prop_rows_mut(|s| {
            if additive {
                if let Some(pos) = s.graph_params.iter().position(|p| *p == (clip, param)) {
                    s.graph_params.remove(pos);
                } else {
                    s.graph_params.push((clip, param));
                }
            } else {
                s.graph_params.clear();
                s.graph_params.push((clip, param));
            }
        });
    }

    /// Currently selected timeline keyframes.
    pub fn selected_timeline_keyframes(&self) -> Vec<SelectedKeyframe> {
        self.with_prop_rows(|s| s.selected_keyframes.iter().copied().collect())
    }

    /// Whether the keyframe at absolute `tick` on `(clip, param)` is selected.
    pub fn is_timeline_keyframe_selected(&self, clip: ClipId, param: ClipParam, tick: i64) -> bool {
        self.with_prop_rows(|s| {
            s.selected_keyframes
                .contains(&SelectedKeyframe { clip, param, tick })
        })
    }

    /// Deselect every timeline keyframe.
    pub fn clear_timeline_keyframe_selection(&mut self) {
        self.with_prop_rows_mut(|s| s.selected_keyframes.clear());
    }

    /// Select a keyframe on a timeline property row (`Shift`/`Ctrl` extends or
    /// toggles selection).
    pub fn select_timeline_keyframe(
        &mut self,
        clip: ClipId,
        param: ClipParam,
        tick: i64,
        additive: bool,
    ) {
        let item = SelectedKeyframe { clip, param, tick };
        self.with_prop_rows_mut(|s| {
            if additive {
                if !s.selected_keyframes.remove(&item) {
                    s.selected_keyframes.insert(item);
                }
            } else if !s.selected_keyframes.contains(&item) {
                s.selected_keyframes.clear();
                s.selected_keyframes.insert(item);
            }
            if !s.graph_params.contains(&(clip, param)) {
                if !additive {
                    s.graph_params.clear();
                }
                s.graph_params.push((clip, param));
            }
        });
    }

    /// Select all keyframes on `(clip, param)` within `[low_tick, high_tick]`.
    pub fn marquee_select_keyframes(
        &mut self,
        clip: ClipId,
        param: ClipParam,
        low_tick: i64,
        high_tick: i64,
        additive: bool,
    ) {
        let (lo, hi) = if low_tick <= high_tick {
            (low_tick, high_tick)
        } else {
            (high_tick, low_tick)
        };
        let Some((model, audio_lane)) = self.clip_and_lane_kind(clip) else {
            return;
        };
        let rows = key_params_of(&model, audio_lane);
        let Some(row) = rows.iter().find(|r| r.param() == param) else {
            return;
        };
        let hits: Vec<SelectedKeyframe> = row
            .marks
            .iter()
            .filter(|m| m.tick >= lo && m.tick <= hi)
            .map(|m| SelectedKeyframe {
                clip,
                param,
                tick: m.tick,
            })
            .collect();
        self.with_prop_rows_mut(|s| {
            if !additive {
                s.selected_keyframes.clear();
            }
            for hit in hits {
                s.selected_keyframes.insert(hit);
            }
        });
    }

    /// Open or close the right-click context menu on a timeline keyframe.
    pub fn open_keyframe_context_menu(&mut self, menu: KeyframeContextMenu) {
        self.select_timeline_keyframe(menu.clip, menu.param, menu.tick, false);
        self.with_prop_rows_mut(|s| {
            s.keyframe_menu = Some(menu);
        });
    }

    pub fn close_keyframe_context_menu(&mut self) {
        self.with_prop_rows_mut(|s| {
            s.keyframe_menu = None;
        });
    }

    pub fn keyframe_context_menu(&self) -> Option<KeyframeContextMenu> {
        self.with_prop_rows(|s| s.keyframe_menu)
    }

    /// Start dragging selected keyframe(s) horizontally in time on a property row.
    pub fn begin_timeline_keyframe_drag(
        &mut self,
        clip: ClipId,
        param: ClipParam,
        tick: i64,
        additive: bool,
    ) {
        self.select_timeline_keyframe(clip, param, tick, additive);
        let selected = self.selected_timeline_keyframes();
        let mut origins = Vec::new();
        for sel in selected {
            let Some((model, audio_lane)) = self.clip_and_lane_kind(sel.clip) else {
                continue;
            };
            let rows = key_params_of(&model, audio_lane);
            let Some(row) = rows.iter().find(|r| r.param() == sel.param) else {
                continue;
            };
            let local_tick = model.param_tick(sel.tick);
            let Some(value) = row.spec.sample(&model, local_tick) else {
                continue;
            };
            let easing = row
                .spec
                .easing_at(&model, local_tick)
                .unwrap_or(Easing::Linear);
            let tangents = if sel.param == ClipParam::Position {
                model.transform.position.tangents_at(local_tick)
            } else {
                None
            };
            origins.push((sel.clip, sel.param, sel.tick, value, easing, tangents));
        }
        self.with_prop_rows_mut(|s| {
            s.time_drag = Some(KeyframeTimeDrag {
                anchor_clip: clip,
                anchor_param: param,
                anchor_origin_tick: tick,
                delta_ticks: 0,
                origins,
            });
        });
    }

    /// Update an in-flight keyframe time drag, snapping to the playhead and to
    /// other keyframes on the clip unless `alt` is held.
    pub fn update_timeline_keyframe_drag(
        &mut self,
        raw_delta_ticks: i64,
        threshold_ticks: i64,
        alt_disable_snap: bool,
    ) -> bool {
        let Some(drag) = self.with_prop_rows(|s| s.time_drag.clone()) else {
            return false;
        };
        let Some((clip_model, audio_lane)) = self.clip_and_lane_kind(drag.anchor_clip) else {
            return false;
        };
        let clip_start = clip_model.timeline.start.value;
        let clip_end = clip_model.timeline.end_tick();
        let mut target_tick =
            (drag.anchor_origin_tick + raw_delta_ticks).clamp(clip_start, clip_end);
        let mut guide = None;

        if !alt_disable_snap && threshold_ticks > 0 {
            let mut candidates = vec![(self.playhead.value, crate::snap::SnapKind::Playhead)];
            let rows = key_params_of(&clip_model, audio_lane);
            for row in rows.iter() {
                for mark in row.marks.iter() {
                    let is_dragged = drag.origins.iter().any(|(c, p, t, _, _, _)| {
                        *c == drag.anchor_clip && *p == row.param() && *t == mark.tick
                    });
                    if !is_dragged {
                        candidates.push((mark.tick, crate::snap::SnapKind::Keyframe));
                    }
                }
            }
            if let Some((best_tick, kind)) = candidates
                .into_iter()
                .filter(|(t, _)| (*t - target_tick).abs() <= threshold_ticks)
                .min_by_key(|(t, _)| (*t - target_tick).abs())
            {
                target_tick = best_tick.clamp(clip_start, clip_end);
                guide = Some(crate::snap::SnapCandidate::new(target_tick, kind));
            }
        }
        self.snap_guide = guide;
        let next_delta = target_tick - drag.anchor_origin_tick;
        self.with_prop_rows_mut(|s| {
            if let Some(d) = &mut s.time_drag {
                let changed = d.delta_ticks != next_delta;
                d.delta_ticks = next_delta;
                changed
            } else {
                false
            }
        })
    }

    /// Commit an in-flight keyframe time drag as a single undo entry.
    pub fn commit_timeline_keyframe_drag(&mut self) -> Result<(), EngineError> {
        self.snap_guide = None;
        let Some(drag) = self.with_prop_rows_mut(|s| s.time_drag.take()) else {
            return Ok(());
        };
        if drag.delta_ticks == 0 || drag.origins.is_empty() {
            return Ok(());
        }
        let rate = self.store.sequence.frame_rate;
        let mut new_selection = Vec::with_capacity(drag.origins.len());
        self.one_step(|this| {
            for &(clip, param, origin_tick, _, _, _) in &drag.origins {
                this.apply_command(Command::Edit(EditCommand::RemoveParamKeyframe {
                    clip,
                    param,
                    at: RationalTime::new(origin_tick, rate),
                }))?;
            }
            for (clip, param, origin_tick, value, easing, tangents) in drag.origins {
                let (lo, hi) = this
                    .ui_clip_ref(clip)
                    .map(|c| (c.timeline_start_ticks, c.timeline_end_ticks))
                    .unwrap_or((0, i64::MAX));
                let next_tick = (origin_tick + drag.delta_ticks).clamp(lo, hi);
                this.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                    clip,
                    param,
                    at: RationalTime::new(next_tick, rate),
                    value,
                    easing,
                    tangents,
                }))?;
                new_selection.push(SelectedKeyframe {
                    clip,
                    param,
                    tick: next_tick,
                });
            }
            Ok(())
        })?;
        self.with_prop_rows_mut(|s| {
            s.selected_keyframes.clear();
            for item in new_selection {
                s.selected_keyframes.insert(item);
            }
        });
        Ok(())
    }

    /// Copy the selected timeline keyframes to the keyframe clipboard.
    pub fn copy_selected_keyframes(&mut self) -> usize {
        let selected = self.selected_timeline_keyframes();
        if selected.is_empty() {
            return 0;
        }
        let min_tick = selected.iter().map(|k| k.tick).min().unwrap_or(0);
        let mut copied = Vec::new();
        for sel in selected {
            let Some((model, audio_lane)) = self.clip_and_lane_kind(sel.clip) else {
                continue;
            };
            let rows = key_params_of(&model, audio_lane);
            let Some(row) = rows.iter().find(|r| r.param() == sel.param) else {
                continue;
            };
            let local_tick = model.param_tick(sel.tick);
            let Some(value) = row.spec.sample(&model, local_tick) else {
                continue;
            };
            let easing = row
                .spec
                .easing_at(&model, local_tick)
                .unwrap_or(Easing::Linear);
            let tangents = if sel.param == ClipParam::Position {
                model.transform.position.tangents_at(local_tick)
            } else {
                None
            };
            copied.push(CopiedKeyframe {
                param: sel.param,
                offset_ticks: sel.tick - min_tick,
                value,
                easing,
                tangents,
            });
        }
        let count = copied.len();
        self.with_prop_rows_mut(|s| {
            s.copied_keyframes = copied;
        });
        if count > 0 {
            self.status_message = Some(format!("Copied {count} keyframe(s)"));
        }
        count
    }

    /// Paste copied keyframes onto the selected clip starting at the playhead,
    /// as a single undo entry.
    pub fn paste_keyframes_at_playhead(&mut self) -> Result<usize, EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(0);
        };
        let copied = self.with_prop_rows(|s| s.copied_keyframes.clone());
        if copied.is_empty() {
            return Ok(0);
        }
        let Some(ui) = self.ui_clip(clip) else {
            return Ok(0);
        };
        let base_tick = self.playhead.value;
        let rate = self.store.sequence.frame_rate;
        let mut count = 0usize;
        self.one_step(|this| {
            for item in copied {
                let tick = (base_tick + item.offset_ticks)
                    .clamp(ui.timeline_start_ticks, ui.timeline_end_ticks);
                this.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                    clip,
                    param: item.param,
                    at: RationalTime::new(tick, rate),
                    value: item.value,
                    easing: item.easing,
                    tangents: item.tangents,
                }))?;
                count += 1;
            }
            Ok(())
        })?;
        Ok(count)
    }

    /// Delete all currently selected timeline keyframes in a single undo entry.
    pub fn delete_selected_keyframes(&mut self) -> Result<usize, EngineError> {
        let selected = self.selected_timeline_keyframes();
        if selected.is_empty() {
            return Ok(0);
        }
        let rate = self.store.sequence.frame_rate;
        let count = selected.len();
        self.one_step(|this| {
            for sel in &selected {
                this.apply_command(Command::Edit(EditCommand::RemoveParamKeyframe {
                    clip: sel.clip,
                    param: sel.param,
                    at: RationalTime::new(sel.tick, rate),
                }))?;
            }
            Ok(())
        })?;
        self.with_prop_rows_mut(|s| {
            s.selected_keyframes.clear();
        });
        Ok(count)
    }

    /// Target keyframes for an ease or hold shortcut (`F9`, `Shift+F9`,
    /// `Ctrl+Shift+F9`, `Ctrl+Alt+H`): the selected timeline keyframes when any
    /// are selected, otherwise any keyframe sitting at the playhead on the
    /// selected clip.
    pub(crate) fn ease_target_keyframes(&self) -> Vec<SelectedKeyframe> {
        let selected = self.selected_timeline_keyframes();
        if !selected.is_empty() {
            return selected;
        }
        let Some(clip) = self.selected_clip() else {
            return Vec::new();
        };
        let at = self.playhead.value;
        let visible = self.visible_params_for_clip(clip);
        let rows = self.key_params();
        let mut out = Vec::new();
        for row in rows.iter() {
            if (!visible.is_empty() && !visible.contains(&row.param())) || !row.keyed_at(at) {
                continue;
            }
            out.push(SelectedKeyframe {
                clip,
                param: row.param(),
                tick: at,
            });
        }
        if out.is_empty() {
            for row in rows.iter() {
                if row.keyed_at(at) {
                    out.push(SelectedKeyframe {
                        clip,
                        param: row.param(),
                        tick: at,
                    });
                }
            }
        }
        out
    }

    /// `F9`: Easy Ease (33.33% influence, 0 speed on both sides) on selected
    /// keyframes or keyframes at the playhead.
    pub fn easy_ease_selected_or_playhead(&mut self) -> Result<(), EngineError> {
        self.apply_ease_to_targets(
            Some(TemporalEase::EASY_EASE),
            Some(TemporalEase::EASY_EASE),
            Some(false),
        )
    }

    /// `Shift+F9`: Easy Ease In (33.33% incoming influence, 0 incoming speed)
    /// on selected keyframes or keyframes at the playhead.
    pub fn easy_ease_in_selected_or_playhead(&mut self) -> Result<(), EngineError> {
        self.apply_ease_to_targets(Some(TemporalEase::EASY_EASE), None, Some(false))
    }

    /// `Ctrl+Shift+F9`: Easy Ease Out (33.33% outgoing influence, 0 outgoing
    /// speed) on selected keyframes or keyframes at the playhead.
    pub fn easy_ease_out_selected_or_playhead(&mut self) -> Result<(), EngineError> {
        self.apply_ease_to_targets(None, Some(TemporalEase::EASY_EASE), Some(false))
    }

    /// `Ctrl+Alt+H`: Toggle Hold interpolation on selected keyframes or
    /// keyframes at the playhead.
    pub fn toggle_hold_selected_or_playhead(&mut self) -> Result<(), EngineError> {
        self.apply_ease_to_targets(None, None, None)
    }

    /// Animation > Keyframe Interpolation > Hold: Hold interpolation on the
    /// selected keyframes or the keyframes at the playhead, whatever they held.
    pub fn hold_selected_or_playhead(&mut self) -> Result<(), EngineError> {
        self.apply_ease_to_targets(None, None, Some(true))
    }

    fn apply_ease_to_targets(
        &mut self,
        in_ease: Option<TemporalEase>,
        out_ease: Option<TemporalEase>,
        explicit_hold: Option<bool>,
    ) -> Result<(), EngineError> {
        let targets = self.ease_target_keyframes();
        if targets.is_empty() {
            self.status_message = Some("Select a keyframe first".into());
            return Ok(());
        }
        let rate = self.store.sequence.frame_rate;
        self.one_step(|this| {
            for target in targets {
                // `SetKeyframeEase` replaces both sides, so a side this shortcut
                // leaves alone is written back with the handle it already has.
                let current = this
                    .clip_and_lane_kind(target.clip)
                    .and_then(|(c, a)| {
                        let local = c.param_tick(target.tick);
                        key_params_of(&c, a)
                            .iter()
                            .find(|r| r.param() == target.param)
                            .and_then(|r| r.spec.easing_at(&c, local))
                    })
                    .unwrap_or(Easing::Linear);
                let hold = explicit_hold.unwrap_or_else(|| !current.is_hold());
                this.apply_command(Command::Edit(EditCommand::SetKeyframeEase {
                    clip: target.clip,
                    param: target.param,
                    at: RationalTime::new(target.tick, rate),
                    in_ease: in_ease.or(current.in_ease()),
                    out_ease: out_ease.or(current.out_ease()),
                    hold,
                }))?;
            }
            Ok(())
        })
    }

    /// Set a keyframe at `(clip, param, tick)` back to Linear interpolation.
    pub fn set_keyframe_linear_at(
        &mut self,
        clip: ClipId,
        param: ClipParam,
        tick: i64,
    ) -> Result<(), EngineError> {
        let Some((model, audio_lane)) = self.clip_and_lane_kind(clip) else {
            return Ok(());
        };
        let rows = key_params_of(&model, audio_lane);
        let Some(row) = rows.iter().find(|r| r.param() == param) else {
            return Ok(());
        };
        let local = model.param_tick(tick);
        let Some(value) = row.spec.sample(&model, local) else {
            return Ok(());
        };
        let tangents = if param == ClipParam::Position {
            model.transform.position.tangents_at(local)
        } else {
            None
        };
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param,
            at: RationalTime::new(tick, self.store.sequence.frame_rate),
            value,
            easing: Easing::Linear,
            tangents,
        }))?;
        Ok(())
    }

    /// Toggle roving across time on a specific interior keyframe.
    pub fn toggle_roving_keyframe(
        &mut self,
        clip: ClipId,
        param: ClipParam,
        tick: i64,
    ) -> Result<(), EngineError> {
        let Some((model, audio_lane)) = self.clip_and_lane_kind(clip) else {
            return Ok(());
        };
        let rows = key_params_of(&model, audio_lane);
        let Some(row) = rows.iter().find(|r| r.param() == param) else {
            return Ok(());
        };
        let local = model.param_tick(tick);
        let currently_roving = row
            .spec
            .easing_at(&model, local)
            .is_some_and(|e| e.is_roving());
        self.apply_command(Command::Edit(EditCommand::SetKeyframeRoving {
            clip,
            param,
            at: RationalTime::new(tick, self.store.sequence.frame_rate),
            roving: !currently_roving,
        }))?;
        Ok(())
    }

    /// Write a scalar value (or first component of a `Vec2`) to `(clip, param)`
    /// at the playhead, writing a keyframe when the property is already
    /// keyframed and updating the constant value otherwise.
    pub fn write_property_scalar_at_playhead(
        &mut self,
        clip: ClipId,
        param: ClipParam,
        scalar: f32,
    ) -> Result<(), EngineError> {
        let Some((model, audio_lane)) = self.clip_and_lane_kind(clip) else {
            return Ok(());
        };
        let rows = key_params_of(&model, audio_lane);
        let Some(row) = rows.iter().find(|r| r.param() == param) else {
            return Ok(());
        };
        let clamped = row.spec.range.clamp(scalar);
        let local_tick = model.param_tick(self.playhead.value);
        let payload = match (row.spec.kind, row.spec.sample(&model, local_tick)) {
            (ParamKind::Vec2, Some(ParamValue::Vec2([_, y]))) => ParamValue::Vec2([clamped, y]),
            (ParamKind::Vec2, _) => ParamValue::Vec2([clamped, clamped]),
            (ParamKind::Vec3, Some(ParamValue::Vec3([_, y, z]))) => {
                ParamValue::Vec3([clamped, y, z])
            }
            (ParamKind::Vec3, _) => ParamValue::Vec3([clamped, clamped, clamped]),
            _ => ParamValue::Scalar(clamped),
        };
        if row.marks.is_empty() {
            self.apply_command(Command::Edit(EditCommand::SetParamConstant {
                clip,
                param,
                value: payload,
            }))?;
        } else {
            let easing = row
                .spec
                .easing_at(&model, local_tick)
                .unwrap_or(Easing::Linear);
            self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                clip,
                param,
                at: self.playhead,
                value: payload,
                easing,
                tangents: None,
            }))?;
        }
        Ok(())
    }

    /// Build the timeline graph editor model for the current frame, reusing
    /// [`build_curve_polyline`] and [`sample_param_scalar`].
    pub fn build_timeline_graph_model(
        &self,
        fps: f32,
        pps: f32,
        scroll_x: f32,
        height: f32,
    ) -> TimelineGraphModel {
        let (mode, mut targets, selected_keys, active_drag) = self.with_prop_rows(|s| {
            (
                s.graph_mode,
                s.graph_params.clone(),
                s.selected_keyframes.clone(),
                s.graph_drag.clone(),
            )
        });
        if targets.is_empty()
            && let Some(clip) = self.selected_clip()
        {
            let vis = self.visible_params_for_clip(clip);
            let param = vis
                .first()
                .copied()
                .or_else(|| self.curve_param())
                .unwrap_or(ClipParam::Position);
            targets.push((clip, param));
        }

        let pad_y = 18.0;
        let plot_h = (height - pad_y * 2.0).max(24.0);
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        let mut resolved: Vec<(Clip, ParamSpec, SharedString)> = Vec::new();
        for (clip_id, param) in &targets {
            let Some((clip, audio_lane)) = self.clip_and_lane_kind(*clip_id) else {
                continue;
            };
            let rows = key_params_of(&clip, audio_lane);
            let Some(row) = rows.iter().find(|r| r.param() == *param) else {
                continue;
            };
            let component =
                matches!(row.spec.kind, ParamKind::Vec2 | ParamKind::Vec3).then_some(0u8);
            let start = clip.timeline.start.value;
            let end = clip.timeline.end_tick().max(start + 1);
            match mode {
                GraphMode::Value => {
                    for i in 0..=32 {
                        let t = start as f64 + (end - start) as f64 * (i as f64 / 32.0);
                        if let Some(v) =
                            sample_param_scalar(&row.spec, &clip, component, clip.param_tick_f(t))
                        {
                            min_y = min_y.min(v);
                            max_y = max_y.max(v);
                        }
                    }
                }
                GraphMode::Speed => {
                    min_y = 0.0;
                    for i in 0..=48 {
                        let t = start as f64 + (end - start) as f64 * (i as f64 / 48.0);
                        let s = sample_speed_at(&row.spec, &clip, component, t, fps).abs();
                        max_y = max_y.max(s);
                    }
                }
            }
            resolved.push((clip, row.spec.clone(), row.label.clone()));
        }

        if !min_y.is_finite() || !max_y.is_finite() {
            min_y = 0.0;
            max_y = 100.0;
        } else if (max_y - min_y).abs() < 1e-3 {
            min_y -= 1.0;
            max_y += 1.0;
        } else {
            let margin = (max_y - min_y) * 0.12;
            if mode == GraphMode::Value {
                min_y -= margin;
            }
            max_y += margin;
        }

        let y_of = |v: f32| -> f32 {
            let frac = ((v - min_y) / (max_y - min_y).max(1e-5)).clamp(0.0, 1.0);
            pad_y + (1.0 - frac) * plot_h
        };
        let x_of = |tick: f64| -> f32 { ((tick as f32 / fps.max(1.0)) * pps) - scroll_x };

        let mut curves = Vec::with_capacity(resolved.len());
        for (clip, spec, label) in resolved {
            let clip_id = clip.id;
            let param = spec.param;
            let component = matches!(spec.kind, ParamKind::Vec2 | ParamKind::Vec3).then_some(0u8);
            let start = clip.timeline.start.value;
            let end = clip.timeline.end_tick().max(start + 1);
            let clip_px_w = ((end - start) as f32 / fps.max(1.0) * pps).max(16.0);
            let bounds = CurveBounds::new(start, end, min_y, max_y, clip_px_w, plot_h);
            let poly = build_curve_polyline(&spec, &clip, component, bounds, 72);

            let mut samples = Vec::new();
            match mode {
                GraphMode::Value => {
                    if let Some(p) = &poly {
                        for (_, _, abs_tick, val) in &p.samples {
                            samples.push((x_of(*abs_tick), y_of(*val)));
                        }
                    } else {
                        let v = sample_param_scalar(&spec, &clip, component, 0.0).unwrap_or(0.0);
                        samples.push((x_of(start as f64), y_of(v)));
                        samples.push((x_of(end as f64), y_of(v)));
                    }
                }
                GraphMode::Speed => {
                    for i in 0..=72 {
                        let frac = i as f64 / 72.0;
                        let abs_t = start as f64 + frac * (end - start) as f64;
                        let spd = sample_speed_at(&spec, &clip, component, abs_t, fps).abs();
                        samples.push((x_of(abs_t), y_of(spd)));
                    }
                }
            }

            let raw_keys = spec.keyframes(&clip);
            let total = raw_keys.len();
            let mut keys = Vec::with_capacity(total);
            let mut influence_handles = Vec::new();
            let mut spatial_handles = Vec::new();

            for (idx, (local_t, easing)) in raw_keys.iter().enumerate() {
                let mut abs_tick = clip.param_timeline_tick(*local_t);
                let mut val =
                    sample_param_scalar(&spec, &clip, component, *local_t as f64).unwrap_or(0.0);
                if let Some(GraphDragTarget::Keyframe {
                    clip: dc,
                    param: dp,
                    origin_tick,
                    current_tick,
                    current_value,
                    ..
                }) = &active_drag
                    && *dc == clip_id
                    && *dp == param
                    && *origin_tick == abs_tick
                {
                    abs_tick = *current_tick;
                    val = *current_value;
                }
                let spd = sample_speed_at(&spec, &clip, component, abs_tick as f64, fps).abs();
                let kx = x_of(abs_tick as f64);
                let ky = match mode {
                    GraphMode::Value => y_of(val),
                    GraphMode::Speed => y_of(spd),
                };
                let selected = selected_keys.contains(&SelectedKeyframe {
                    clip: clip_id,
                    param,
                    tick: abs_tick,
                });
                let interior = idx > 0 && idx + 1 < total;
                keys.push(GraphKeyNode {
                    clip: clip_id,
                    param,
                    tick: abs_tick,
                    value: val,
                    speed: spd,
                    x: kx,
                    y: ky,
                    shape: KeyframeIconShape::classify(idx, total, *easing),
                    selected,
                    roving: interior && easing.is_roving(),
                    hold: easing.is_hold(),
                    interior,
                });

                let (mut in_ease, mut out_ease) = spec
                    .temporal_ease_at(&clip, *local_t)
                    .unwrap_or((TemporalEase::EASY_EASE, TemporalEase::EASY_EASE));
                if let Some(GraphDragTarget::Influence {
                    clip: dc,
                    param: dp,
                    tick: dt,
                    in_ease: di,
                    out_ease: do_e,
                    ..
                }) = &active_drag
                    && *dc == clip_id
                    && *dp == param
                    && *dt == abs_tick
                {
                    in_ease = *di;
                    out_ease = *do_e;
                }

                if idx > 0 {
                    let prev_abs = clip.param_timeline_tick(raw_keys[idx - 1].0);
                    let seg_w = (kx - x_of(prev_abs as f64)).abs().max(24.0);
                    let hx = kx - in_ease.influence.clamp(MIN_TEMPORAL_INFLUENCE, 1.0) * seg_w;
                    let hy = match mode {
                        GraphMode::Value => ky + in_ease.speed * 0.25,
                        GraphMode::Speed => y_of(in_ease.speed.abs().max(spd)),
                    };
                    influence_handles.push(GraphInfluenceHandle {
                        clip: clip_id,
                        param,
                        tick: abs_tick,
                        side: GraphHandleSide::Incoming,
                        anchor_x: kx,
                        anchor_y: ky,
                        handle_x: hx,
                        handle_y: hy,
                        segment_px_w: seg_w,
                        influence: in_ease.influence,
                        speed: in_ease.speed,
                    });
                }
                if idx + 1 < total {
                    let next_abs = clip.param_timeline_tick(raw_keys[idx + 1].0);
                    let seg_w = (x_of(next_abs as f64) - kx).abs().max(24.0);
                    let hx = kx + out_ease.influence.clamp(MIN_TEMPORAL_INFLUENCE, 1.0) * seg_w;
                    let hy = match mode {
                        GraphMode::Value => ky - out_ease.speed * 0.25,
                        GraphMode::Speed => y_of(out_ease.speed.abs().max(spd)),
                    };
                    influence_handles.push(GraphInfluenceHandle {
                        clip: clip_id,
                        param,
                        tick: abs_tick,
                        side: GraphHandleSide::Outgoing,
                        anchor_x: kx,
                        anchor_y: ky,
                        handle_x: hx,
                        handle_y: hy,
                        segment_px_w: seg_w,
                        influence: out_ease.influence,
                        speed: out_ease.speed,
                    });
                }

                if mode == GraphMode::Value
                    && param == ClipParam::Position
                    && let Some(tan) = clip.transform.position.tangents_at(*local_t)
                {
                    let tan = match &active_drag {
                        Some(GraphDragTarget::SpatialTangent {
                            clip: dc,
                            tick: dt,
                            current_tangents,
                            ..
                        }) if *dc == clip_id && *dt == abs_tick => *current_tangents,
                        _ => tan,
                    };
                    spatial_handles.push(GraphSpatialHandle {
                        clip: clip_id,
                        tick: abs_tick,
                        side: GraphHandleSide::Incoming,
                        anchor_x: kx,
                        anchor_y: ky,
                        handle_x: kx + tan.in_t[0],
                        handle_y: ky - tan.in_t[1],
                        tangent: tan.in_t,
                    });
                    spatial_handles.push(GraphSpatialHandle {
                        clip: clip_id,
                        tick: abs_tick,
                        side: GraphHandleSide::Outgoing,
                        anchor_x: kx,
                        anchor_y: ky,
                        handle_x: kx + tan.out_t[0],
                        handle_y: ky - tan.out_t[1],
                        tangent: tan.out_t,
                    });
                }
            }

            curves.push(TimelineGraphCurve {
                clip: clip_id,
                param,
                label,
                samples,
                keys,
                influence_handles,
                spatial_handles,
            });
        }

        let unit = match mode {
            GraphMode::Value => "",
            GraphMode::Speed => "/s",
        };
        let mid_y = (min_y + max_y) * 0.5;
        TimelineGraphModel {
            mode,
            min_y,
            max_y,
            top_label: format!("{max_y:.1}{unit}").into(),
            mid_label: format!("{mid_y:.1}{unit}").into(),
            bottom_label: format!("{min_y:.1}{unit}").into(),
            curves,
        }
    }

    /// Start dragging an influence handle, spatial tangent handle, or keyframe
    /// node inside the timeline graph editor.
    pub fn begin_timeline_graph_drag(
        &mut self,
        local_x: f32,
        local_y: f32,
        fps: f32,
        pps: f32,
        scroll_x: f32,
        height: f32,
    ) -> bool {
        let model = self.build_timeline_graph_model(fps, pps, scroll_x, height);
        let hit_r2 = 64.0f32;

        for curve in &model.curves {
            for h in &curve.influence_handles {
                let dx = local_x - h.handle_x;
                let dy = local_y - h.handle_y;
                if dx * dx + dy * dy <= hit_r2 {
                    let Some((clip_model, audio_lane)) = self.clip_and_lane_kind(h.clip) else {
                        continue;
                    };
                    let local_t = clip_model.param_tick(h.tick);
                    let rows = key_params_of(&clip_model, audio_lane);
                    let row = rows.iter().find(|r| r.param() == h.param);
                    let easing = row
                        .and_then(|r| r.spec.easing_at(&clip_model, local_t))
                        .unwrap_or(Easing::Linear);
                    let (in_ease, out_ease) = row
                        .and_then(|r| r.spec.temporal_ease_at(&clip_model, local_t))
                        .unwrap_or((TemporalEase::EASY_EASE, TemporalEase::EASY_EASE));
                    self.with_prop_rows_mut(|s| {
                        s.graph_drag = Some(GraphDragTarget::Influence {
                            clip: h.clip,
                            param: h.param,
                            tick: h.tick,
                            side: h.side,
                            anchor_x: h.anchor_x,
                            anchor_y: h.anchor_y,
                            segment_px_w: h.segment_px_w,
                            in_ease,
                            out_ease,
                            hold: easing.is_hold(),
                            min_y: model.min_y,
                            max_y: model.max_y,
                            height,
                        });
                    });
                    return true;
                }
            }
            for sh in &curve.spatial_handles {
                let dx = local_x - sh.handle_x;
                let dy = local_y - sh.handle_y;
                if dx * dx + dy * dy <= hit_r2 {
                    let Some((clip_model, _)) = self.clip_and_lane_kind(sh.clip) else {
                        continue;
                    };
                    let local_t = clip_model.param_tick(sh.tick);
                    let pt = clip_model.transform.position.sample(local_t);
                    let easing = clip_model
                        .transform
                        .position
                        .easing_at(local_t)
                        .unwrap_or(Easing::Linear);
                    let origin_tangents = clip_model
                        .transform
                        .position
                        .tangents_at(local_t)
                        .unwrap_or(SpatialTangents::ZERO);
                    self.with_prop_rows_mut(|s| {
                        s.graph_drag = Some(GraphDragTarget::SpatialTangent {
                            clip: sh.clip,
                            tick: sh.tick,
                            side: sh.side,
                            origin_x: local_x,
                            origin_y: local_y,
                            origin_tangents,
                            current_tangents: origin_tangents,
                            value: ParamValue::Vec2(pt),
                            easing,
                        });
                    });
                    return true;
                }
            }
            for node in &curve.keys {
                let dx = local_x - node.x;
                let dy = local_y - node.y;
                if dx * dx + dy * dy <= hit_r2 {
                    self.select_timeline_keyframe(node.clip, node.param, node.tick, false);
                    let easing = self
                        .clip_and_lane_kind(node.clip)
                        .and_then(|(c, a)| {
                            let local = c.param_tick(node.tick);
                            key_params_of(&c, a)
                                .iter()
                                .find(|r| r.param() == node.param)
                                .and_then(|r| r.spec.easing_at(&c, local))
                        })
                        .unwrap_or(Easing::Linear);
                    self.with_prop_rows_mut(|s| {
                        s.graph_drag = Some(GraphDragTarget::Keyframe {
                            clip: node.clip,
                            param: node.param,
                            origin_tick: node.tick,
                            origin_value: node.value,
                            current_tick: node.tick,
                            current_value: node.value,
                            easing,
                        });
                    });
                    return true;
                }
            }
        }
        false
    }

    /// Update an in-flight timeline graph editor drag.
    pub fn update_timeline_graph_drag(
        &mut self,
        local_x: f32,
        local_y: f32,
        fps: f32,
        pps: f32,
        scroll_x: f32,
        height: f32,
    ) -> bool {
        let model = self.build_timeline_graph_model(fps, pps, scroll_x, height);
        let pad_y = 18.0;
        let plot_h = (height - pad_y * 2.0).max(24.0);
        let val_at_y = |y: f32| -> f32 {
            let frac = (1.0 - ((y - pad_y) / plot_h)).clamp(0.0, 1.0);
            model.min_y + frac * (model.max_y - model.min_y)
        };
        let tick_at_x = |x: f32| -> i64 {
            (((x + scroll_x).max(0.0) / pps.max(1.0)) * fps.max(1.0)).round() as i64
        };

        self.with_prop_rows_mut(|s| {
            let Some(drag) = &mut s.graph_drag else {
                return false;
            };
            match drag {
                GraphDragTarget::Keyframe {
                    current_tick,
                    current_value,
                    ..
                } => {
                    let next_t = tick_at_x(local_x);
                    let next_v = if s.graph_mode == GraphMode::Value {
                        val_at_y(local_y)
                    } else {
                        *current_value
                    };
                    let moved = *current_tick != next_t || (*current_value - next_v).abs() > 1e-4;
                    *current_tick = next_t;
                    *current_value = next_v;
                    moved
                }
                GraphDragTarget::Influence {
                    side,
                    anchor_x,
                    anchor_y,
                    segment_px_w,
                    in_ease,
                    out_ease,
                    ..
                } => {
                    let dist_x = match side {
                        GraphHandleSide::Incoming => (*anchor_x - local_x).max(0.0),
                        GraphHandleSide::Outgoing => (local_x - *anchor_x).max(0.0),
                    };
                    let inf = (dist_x / segment_px_w.max(1.0)).clamp(MIN_TEMPORAL_INFLUENCE, 1.0);
                    let spd = (*anchor_y - local_y) * 2.0;
                    let next_ease = TemporalEase::new(spd, inf);
                    match side {
                        GraphHandleSide::Incoming => *in_ease = next_ease,
                        GraphHandleSide::Outgoing => *out_ease = next_ease,
                    }
                    true
                }
                GraphDragTarget::SpatialTangent {
                    side,
                    origin_x,
                    origin_y,
                    origin_tangents,
                    current_tangents,
                    ..
                } => {
                    let dx = local_x - *origin_x;
                    let dy = *origin_y - local_y;
                    match side {
                        GraphHandleSide::Incoming => {
                            current_tangents.in_t =
                                [origin_tangents.in_t[0] + dx, origin_tangents.in_t[1] + dy];
                        }
                        GraphHandleSide::Outgoing => {
                            current_tangents.out_t =
                                [origin_tangents.out_t[0] + dx, origin_tangents.out_t[1] + dy];
                        }
                    }
                    true
                }
            }
        })
    }

    /// Commit an in-flight timeline graph editor drag as a single undo entry.
    pub fn commit_timeline_graph_drag(&mut self) -> Result<(), EngineError> {
        let Some(drag) = self.with_prop_rows_mut(|s| s.graph_drag.take()) else {
            return Ok(());
        };
        let rate = self.store.sequence.frame_rate;
        match drag {
            GraphDragTarget::Keyframe {
                clip,
                param,
                origin_tick,
                origin_value,
                current_tick,
                current_value,
                easing,
            } => {
                if origin_tick == current_tick && (origin_value - current_value).abs() < 1e-4 {
                    return Ok(());
                }
                let (lo, hi) = self
                    .ui_clip_ref(clip)
                    .map(|c| (c.timeline_start_ticks, c.timeline_end_ticks))
                    .unwrap_or((0, i64::MAX));
                let target_tick = current_tick.clamp(lo, hi);
                let Some((model, audio_lane)) = self.clip_and_lane_kind(clip) else {
                    return Ok(());
                };
                let rows = key_params_of(&model, audio_lane);
                let Some(row) = rows.iter().find(|r| r.param() == param) else {
                    return Ok(());
                };
                let clamped = row.spec.range.clamp(current_value);
                let local_orig = model.param_tick(origin_tick);
                let payload = match (row.spec.kind, row.spec.sample(&model, local_orig)) {
                    (ParamKind::Vec2, Some(ParamValue::Vec2([_, y]))) => {
                        ParamValue::Vec2([clamped, y])
                    }
                    (ParamKind::Vec3, Some(ParamValue::Vec3([_, y, z]))) => {
                        ParamValue::Vec3([clamped, y, z])
                    }
                    _ => ParamValue::Scalar(clamped),
                };
                let tangents = if param == ClipParam::Position {
                    model.transform.position.tangents_at(local_orig)
                } else {
                    None
                };
                self.one_step(|this| {
                    if origin_tick != target_tick {
                        this.apply_command(Command::Edit(EditCommand::RemoveParamKeyframe {
                            clip,
                            param,
                            at: RationalTime::new(origin_tick, rate),
                        }))?;
                    }
                    this.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                        clip,
                        param,
                        at: RationalTime::new(target_tick, rate),
                        value: payload,
                        easing,
                        tangents,
                    }))?;
                    Ok(())
                })?;
                self.select_timeline_keyframe(clip, param, target_tick, false);
            }
            GraphDragTarget::Influence {
                clip,
                param,
                tick,
                in_ease,
                out_ease,
                hold,
                ..
            } => {
                self.apply_command(Command::Edit(EditCommand::SetKeyframeEase {
                    clip,
                    param,
                    at: RationalTime::new(tick, rate),
                    in_ease: Some(in_ease),
                    out_ease: Some(out_ease),
                    hold,
                }))?;
            }
            GraphDragTarget::SpatialTangent {
                clip,
                tick,
                current_tangents,
                value,
                easing,
                ..
            } => {
                self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                    clip,
                    param: ClipParam::Position,
                    at: RationalTime::new(tick, rate),
                    value,
                    easing,
                    tangents: Some(current_tangents),
                }))?;
            }
        }
        Ok(())
    }
}

fn sample_speed_at(
    spec: &ParamSpec,
    clip: &Clip,
    component: Option<u8>,
    abs_tick: f64,
    fps: f32,
) -> f32 {
    let local_a = clip.param_tick_f((abs_tick - 0.5).max(clip.timeline.start.value as f64));
    let local_b = clip.param_tick_f((abs_tick + 0.5).min(clip.timeline.end_tick() as f64));
    let dt = ((local_b - local_a).abs() / fps.max(1.0) as f64).max(1.0 / fps.max(1.0) as f64);
    let va = sample_param_scalar(spec, clip, component, local_a).unwrap_or(0.0);
    let vb = sample_param_scalar(spec, clip, component, local_b).unwrap_or(0.0);
    ((vb - va) as f64 / dt) as f32
}
