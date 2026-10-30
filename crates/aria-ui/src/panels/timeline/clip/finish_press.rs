//! A left press on a lane-group canvas: what it hit, and the gesture it starts.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn on_left_down(
    this: &mut Shell,
    event: &MouseDownEvent,
    bounds: Bounds<Pixels>,
    lanes: &[VisibleLane],
    fps: f32,
    pps: f32,
    scroll_x: f32,
    tool: EditorTool,
    cx: &mut Context<Shell>,
) {
    let (local_x, local_y) = local_coords(event.position, bounds);
    let Some(hit) = hit_test(lanes, local_x, local_y, fps, pps, scroll_x) else {
        return;
    };
    PRESS_CURSOR.set(cursor_for(tool, Some(hit)));
    let origin_x = f32::from(event.position.x);

    // An armed Parent pick whip takes the press on a clip, whatever the tool.
    if let HitTarget::ClipBody { clip_id, .. } = hit
        && land_parent_whip(this, clip_id, event.modifiers.shift, cx)
    {
        cx.stop_propagation();
        return;
    }

    if tool.pans() {
        this.start_pan(origin_x, MouseButton::Left, cx);
        return;
    }
    if tool.zooms() {
        let factor = if event.modifiers.alt {
            1.0 / crate::transport::ZOOM_STEP
        } else {
            crate::transport::ZOOM_STEP
        };
        this.zoom_at(origin_x, factor, cx);
        return;
    }
    if let Some(dir) = tool.track_select() {
        let (track_id, tick) = match hit {
            HitTarget::ClipEdge {
                track_id,
                start_ticks,
                ..
            }
            | HitTarget::ClipBody {
                track_id,
                start_ticks,
                ..
            } => (track_id, start_ticks),
            HitTarget::EmptyLane { track_id, tick, .. } => (track_id, tick),
            _ => {
                let first_track = lanes.first().map(|l| l.track_id).unwrap_or(0);
                (
                    first_track,
                    px_to_ticks((local_x + scroll_x).max(0.0), fps, pps),
                )
            }
        };
        this.state.update(cx, |state, _| {
            state.select_track_clips(
                TrackId::from_raw(track_id),
                tick,
                dir,
                event.modifiers.shift,
            );
        });
        cx.stop_propagation();
        cx.notify();
        return;
    }
    if tool.cuts() {
        let tick = px_to_ticks((local_x + scroll_x).max(0.0), fps, pps);
        let track_id = match hit {
            HitTarget::ClipEdge { track_id, .. }
            | HitTarget::ClipBody { track_id, .. }
            | HitTarget::EmptyLane { track_id, .. } => track_id,
            _ => lanes.first().map(|l| l.track_id).unwrap_or(0),
        };
        this.state.update(cx, |state, _| {
            if let Err(err) =
                state.split_at_tick(TrackId::from_raw(track_id), tick, event.modifiers.shift)
            {
                state.report_error(&err);
            }
        });
        cx.stop_propagation();
        cx.notify();
        return;
    }
    if tool.pens() {
        let tick = px_to_ticks((local_x + scroll_x).max(0.0), fps, pps);
        match hit {
            HitTarget::ClipBody {
                clip_id,
                locked: false,
                ..
            }
            | HitTarget::ClipEdge { clip_id, .. }
            | HitTarget::ClipLine { clip_id, .. } => {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.add_clip_line_keyframe(ClipId::from_raw(clip_id), tick)
                    {
                        state.report_error(&err);
                    }
                });
                cx.stop_propagation();
                cx.notify();
                return;
            }
            HitTarget::PropertyRow { clip, param, tick } => {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.add_param_keyframe_at(clip, param, tick) {
                        state.report_error(&err);
                    }
                });
                cx.stop_propagation();
                cx.notify();
                return;
            }
            _ => {}
        }
    }

    match hit {
        HitTarget::TransitionEdge {
            clip_id,
            right,
            duration_ticks,
        } => {
            cx.stop_propagation();
            ACTIVE_GESTURE.set(Some(CanvasGesture::Clip {
                kind: if right {
                    DragKind::TransitionRight
                } else {
                    DragKind::TransitionLeft
                },
                clip_id,
                origin_track_id: 0,
                target_track_id: 0,
                origin_x,
                start_ticks: duration_ticks,
                end_ticks: 0,
            }));
        }
        HitTarget::TransitionBody { clip_id } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.select_clip(ClipId::from_raw(clip_id));
            });
            cx.notify();
        }
        HitTarget::Fade {
            clip_id,
            left,
            fade_ticks,
            ..
        } => {
            cx.stop_propagation();
            ACTIVE_GESTURE.set(Some(CanvasGesture::Clip {
                kind: if left {
                    DragKind::FadeIn
                } else {
                    DragKind::FadeOut
                },
                clip_id,
                origin_track_id: 0,
                target_track_id: 0,
                origin_x,
                start_ticks: fade_ticks,
                end_ticks: 0,
            }));
        }
        HitTarget::ClipEdge {
            clip_id,
            track_id,
            left,
            start_ticks,
            end_ticks,
        } => {
            cx.stop_propagation();
            let side = if left { TrimSide::In } else { TrimSide::Out };
            let span = Span {
                start: start_ticks,
                end: end_ticks,
                threshold: crate::snap::threshold_ticks(pps, fps),
                magnet: !event.modifiers.alt,
            };
            this.state.update(cx, |state, _| {
                state.select_edge(ClipId::from_raw(clip_id), side);
                let g = resolve_edge_gesture(
                    tool,
                    event.modifiers.control,
                    event.modifiers.alt,
                    left,
                    span,
                );
                let d = Drag {
                    clip: ClipId::from_raw(clip_id),
                    delta: 0,
                };
                state.hold_drag(g, d);
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::Clip {
                kind: if left {
                    DragKind::TrimStart
                } else {
                    DragKind::TrimEnd
                },
                clip_id,
                origin_track_id: track_id,
                target_track_id: track_id,
                origin_x,
                start_ticks,
                end_ticks,
            }));
            cx.notify();
        }
        HitTarget::ClipBody {
            clip_id,
            track_id,
            start_ticks,
            end_ticks,
            locked,
            in_name_band,
        } => {
            cx.stop_propagation();
            if event.click_count >= 2 {
                this.state.update(cx, |state, _| {
                    if in_name_band {
                        state.begin_rename(RenameTarget::Clip(clip_id));
                    } else {
                        state.double_click_clip(clip_id);
                    }
                });
                cx.notify();
                return;
            }
            this.state.update(cx, |state, _| {
                if event.modifiers.shift || event.modifiers.control || event.modifiers.platform {
                    state.toggle_clip_in_selection(ClipId::from_raw(clip_id));
                } else {
                    state.select_clip_linked(ClipId::from_raw(clip_id), !event.modifiers.alt);
                }
            });
            if !locked {
                ACTIVE_GESTURE.set(Some(CanvasGesture::Clip {
                    kind: DragKind::Move,
                    clip_id,
                    origin_track_id: track_id,
                    target_track_id: track_id,
                    origin_x,
                    start_ticks,
                    end_ticks,
                }));
            }
            cx.notify();
        }
        HitTarget::ClipLine {
            clip_id,
            track_id,
            key,
            tick,
            start_ticks,
            end_ticks,
        } => {
            cx.stop_propagation();
            let clip = ClipId::from_raw(clip_id);
            if event.modifiers.control || event.modifiers.platform {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.add_clip_line_keyframe(clip, tick) {
                        state.report_error(&err);
                    }
                });
                crate::app::spawn_preview(this.state.clone(), cx);
                cx.notify();
                return;
            }
            // Off its dots the line is part of the clip's body, so a double
            // click there opens the clip as one anywhere else on it does.
            if key.is_none() && event.click_count >= 2 {
                this.state
                    .update(cx, |state, _| state.double_click_clip(clip_id));
                cx.notify();
                return;
            }
            let band = lanes
                .iter()
                .find(|lane| lane.track_id == track_id)
                .map_or(0.0, |lane| {
                    BarRows::of((lane.clip_y_bottom - lane.y_top).max(1.0)).band()
                });
            let origin_y = f32::from(event.position.y);
            let gesture = this.state.update(cx, |state, _| {
                if !state.selection.contains(&clip) {
                    state.select_clip_linked(clip, !event.modifiers.alt);
                }
                match key {
                    Some(key) => state
                        .press_clip_line(clip, LinePress::Key(key), band)
                        .then_some(CanvasGesture::Line { origin_y }),
                    None => Some(CanvasGesture::LineOrMove {
                        clip_id,
                        track_id,
                        origin_x,
                        origin_y,
                        tick,
                        band,
                        start_ticks,
                        end_ticks,
                    }),
                }
            });
            if gesture.is_some() {
                ACTIVE_GESTURE.set(gesture);
            }
            cx.notify();
        }
        HitTarget::Keyframe { clip, param, tick } => {
            cx.stop_propagation();
            if event.click_count >= 2 {
                this.state.update(cx, |state, _| {
                    state.select_clip(clip);
                    state.select_timeline_keyframe(clip, param, tick, false);
                    state.open_keyframe_velocity();
                });
                cx.notify();
                return;
            }
            this.state.update(cx, |state, _| {
                state.select_clip(clip);
                state.begin_timeline_keyframe_drag(clip, param, tick, event.modifiers.shift);
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::KeyframeDrag { origin_x }));
            cx.notify();
        }
        HitTarget::PropertyRow { clip, param, tick } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.select_clip(clip);
                state.select_timeline_property(clip, param, event.modifiers.shift);
                if !event.modifiers.shift {
                    state.clear_timeline_keyframe_selection();
                }
                state.begin_key_band(clip, param, tick);
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::KeyBand));
            cx.notify();
        }
        HitTarget::MaskPathKey { clip, mask, tick } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.begin_mask_path_key_drag(clip, mask, tick);
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::MaskPathKeyDrag { origin_x }));
            cx.notify();
        }
        HitTarget::MaskPathRow { clip, mask } => {
            cx.stop_propagation();
            this.state
                .update(cx, |state, _| state.focus_mask_path(clip, mask));
            cx.notify();
        }
        HitTarget::EmptyLane { track_id, tick, .. } => {
            this.state.update(cx, |state, _| {
                state.select_gap_at(TrackId::from_raw(track_id), tick);
                state.begin_marquee(track_id, tick);
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::Marquee {
                origin_x,
                moved: false,
            }));
            cx.notify();
        }
    }
}
