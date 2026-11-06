//! Keyframe selection, time dragging, clipboard, ease shortcuts, and scalar
//! writes on timeline property rows.

use aria_commands::{Command, EditCommand};
use aria_core::count::counted;
use aria_engine::EngineError;
use aria_models::{ClipId, ClipParam, Easing, ParamKind, ParamValue, RationalTime, TemporalEase};

use super::{CopiedKeyframe, KeyBand, KeyframeTimeDrag, SelectedKeyframe};
use crate::app::AppState;
use crate::app::clip_keys::{key_magnets, key_params_of, key_span};
use crate::app::decline::Decline;

/// What an ease shortcut writes: the handles it sets, where a side left `None`
/// keeps the handle the keyframe has, and Hold set, cleared, or toggled
/// (`None`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct EaseWrite {
    in_ease: Option<TemporalEase>,
    out_ease: Option<TemporalEase>,
    hold: Option<bool>,
}

impl EaseWrite {
    /// Easy Ease on both sides, which Bezier also writes.
    pub(crate) const EASY: Self = Self {
        in_ease: Some(TemporalEase::EASY_EASE),
        out_ease: Some(TemporalEase::EASY_EASE),
        hold: Some(false),
    };
    /// Easy Ease on the incoming side.
    pub(crate) const EASY_IN: Self = Self {
        in_ease: Some(TemporalEase::EASY_EASE),
        out_ease: None,
        hold: Some(false),
    };
    /// Easy Ease on the outgoing side.
    pub(crate) const EASY_OUT: Self = Self {
        in_ease: None,
        out_ease: Some(TemporalEase::EASY_EASE),
        hold: Some(false),
    };
    /// Hold, whatever the keyframe held.
    pub(crate) const HOLD: Self = Self {
        in_ease: None,
        out_ease: None,
        hold: Some(true),
    };
    /// Hold toggled.
    const TOGGLE_HOLD: Self = Self {
        in_ease: None,
        out_ease: None,
        hold: None,
    };
}

impl AppState {
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

    /// Press a band on the row of `(clip, param)` at `tick`. The selection the
    /// press left is the band's base.
    pub fn begin_key_band(&mut self, clip: ClipId, param: ClipParam, tick: i64) {
        self.with_prop_rows_mut(|s| {
            s.key_band = Some(KeyBand {
                clip,
                param,
                origin: tick,
                base: s.selected_keyframes.clone(),
            });
        });
    }

    /// Carry the held band to `tick`: the selection is its base and the
    /// keyframes the band covers, so a band drawn back deselects what it
    /// passed.
    pub fn drag_key_band(&mut self, tick: i64) {
        let Some((clip, param, origin)) = self.with_prop_rows_mut(|s| {
            let band = s.key_band.as_ref()?;
            s.selected_keyframes.clone_from(&band.base);
            Some((band.clip, band.param, band.origin))
        }) else {
            return;
        };
        self.marquee_select_keyframes(clip, param, origin, tick, true);
    }

    /// Let go of the held band; the selection stays.
    pub fn release_key_band(&mut self) {
        self.with_prop_rows_mut(|s| s.key_band = None);
    }

    /// Escape on the held band: the selection goes back to its base.
    pub fn cancel_key_band(&mut self) {
        self.with_prop_rows_mut(|s| {
            if let Some(band) = s.key_band.take() {
                s.selected_keyframes = band.base;
            }
        });
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
        let span = key_span(
            clip_model.timeline.start.value,
            clip_model.timeline.end_tick(),
        );
        let magnets = if alt_disable_snap {
            Vec::new()
        } else {
            key_magnets(
                &clip_model,
                audio_lane,
                self.playhead.value,
                |param, tick| {
                    drag.origins.iter().any(|(c, p, t, _, _, _)| {
                        *c == drag.anchor_clip && *p == param && *t == tick
                    })
                },
            )
        };
        let (target_tick, guide) = crate::snap::pull_within(
            drag.anchor_origin_tick + raw_delta_ticks,
            threshold_ticks,
            &magnets,
            span,
        );
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
                    .map(|c| key_span(c.timeline_start_ticks, c.timeline_end_ticks))
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

    /// Drop an in-flight keyframe time drag with nothing written.
    pub fn cancel_timeline_keyframe_drag(&mut self) {
        self.with_prop_rows_mut(|s| s.time_drag = None);
        self.snap_guide = None;
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
            self.status_message = Some(format!(
                "Copied {}",
                counted(count, "keyframe", "keyframes")
            ));
        }
        count
    }

    /// Whether keyframes are copied, so a paste has something to add.
    pub fn has_copied_keyframes(&self) -> bool {
        self.with_prop_rows(|s| !s.copied_keyframes.is_empty())
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
        self.apply_ease_to_targets(EaseWrite::EASY)
    }

    /// `Shift+F9`: Easy Ease In (33.33% incoming influence, 0 incoming speed)
    /// on selected keyframes or keyframes at the playhead.
    pub fn easy_ease_in_selected_or_playhead(&mut self) -> Result<(), EngineError> {
        self.apply_ease_to_targets(EaseWrite::EASY_IN)
    }

    /// `Ctrl+Shift+F9`: Easy Ease Out (33.33% outgoing influence, 0 outgoing
    /// speed) on selected keyframes or keyframes at the playhead.
    pub fn easy_ease_out_selected_or_playhead(&mut self) -> Result<(), EngineError> {
        self.apply_ease_to_targets(EaseWrite::EASY_OUT)
    }

    /// `Ctrl+Alt+H`: Toggle Hold interpolation on selected keyframes or
    /// keyframes at the playhead.
    pub fn toggle_hold_selected_or_playhead(&mut self) -> Result<(), EngineError> {
        self.apply_ease_to_targets(EaseWrite::TOGGLE_HOLD)
    }

    /// Animation > Keyframe Interpolation > Hold: Hold interpolation on the
    /// selected keyframes or the keyframes at the playhead, whatever they held.
    pub fn hold_selected_or_playhead(&mut self) -> Result<(), EngineError> {
        self.apply_ease_to_targets(EaseWrite::HOLD)
    }

    /// The keyframes `ease` rewrites: the ease targets whose easing it
    /// changes. The rows' guards and the shortcuts both read this.
    pub(crate) fn ease_targets(&self, ease: EaseWrite) -> Result<Vec<SelectedKeyframe>, Decline> {
        let targets = self.ease_target_keyframes();
        if targets.is_empty() {
            return Err(Decline::NoKeyframeTarget);
        }
        let changed: Vec<SelectedKeyframe> = targets
            .into_iter()
            .filter(|&target| self.ease_command(target, ease).1)
            .collect();
        if changed.is_empty() {
            return Err(Decline::SameEase);
        }
        Ok(changed)
    }

    /// The write `ease` sends for `target`, and whether it changes the
    /// property's curve, found by running the model's own write on a copy of
    /// the clip. `SetKeyframeEase` replaces both sides, so a side `ease`
    /// leaves alone is written back with the handle it already has.
    fn ease_command(&self, target: SelectedKeyframe, ease: EaseWrite) -> (EditCommand, bool) {
        let found = self
            .clip_and_lane_kind(target.clip)
            .and_then(|(clip, audio_lane)| {
                let row = key_params_of(&clip, audio_lane)
                    .iter()
                    .find(|row| row.param() == target.param)
                    .cloned()?;
                Some((clip, row))
            });
        let current = found
            .as_ref()
            .and_then(|(clip, row)| row.spec.easing_at(clip, clip.param_tick(target.tick)))
            .unwrap_or(Easing::Linear);
        let hold = ease.hold.unwrap_or_else(|| !current.is_hold());
        let in_ease = ease.in_ease.or(current.in_ease());
        let out_ease = ease.out_ease.or(current.out_ease());
        // A write the model rejects is sent anyway, so its error is reported.
        let changes = found.is_none_or(|(mut clip, row)| {
            let before = row.spec.curve(&clip);
            let local = clip.param_tick(target.tick);
            target
                .param
                .set_keyframe_temporal_ease(&mut clip, local, in_ease, out_ease, hold)
                .is_err()
                || row.spec.curve(&clip) != before
        });
        let write = EditCommand::SetKeyframeEase {
            clip: target.clip,
            param: target.param,
            at: RationalTime::new(target.tick, self.store.sequence.frame_rate),
            in_ease,
            out_ease,
            hold,
        };
        (write, changes)
    }

    fn apply_ease_to_targets(&mut self, ease: EaseWrite) -> Result<(), EngineError> {
        let targets = match self.ease_targets(ease) {
            Ok(targets) => targets,
            Err(why) => {
                self.decline(why);
                return Ok(());
            }
        };
        self.one_step(|this| {
            for target in targets {
                // Read at the write: an earlier write in the step can move what
                // this keyframe holds.
                let (write, _) = this.ease_command(target, ease);
                this.apply_command(Command::Edit(write))?;
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
}
