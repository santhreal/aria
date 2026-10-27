//! A gesture in flight on the state: carried by the window's moves, ended by
//! the release of the button or abandoned by Escape.
//!
//! The main window and every floating panel window call these, so a gesture
//! begun in either ends the same way: a drag follows the pointer past the
//! element it started on, a release anywhere writes it, and Escape puts back
//! what the press found.

use std::rc::Rc;

use super::*;
use crate::surfaces::mixer_strip;
use crate::surfaces::overlay_gesture::{self, Step};
use crate::surfaces::ramp_gesture::{self, Gesture};

type Carry = dyn Fn(&mut Shell, &MouseMoveEvent, &mut Context<Shell>);
type Release = dyn Fn(&mut Shell, &MouseUpEvent, &mut Context<Shell>);

/// A press on an element whose drag the shell carries: every move goes to
/// `carry` wherever the pointer is, and the release goes to `release`
/// wherever the button comes up. The element's own handlers see the pointer
/// only while it is over the element.
#[derive(Clone)]
pub(crate) struct Hold {
    carry: Rc<Carry>,
    release: Rc<Release>,
}

impl Hold {
    pub(crate) fn new(
        carry: impl Fn(&mut Shell, &MouseMoveEvent, &mut Context<Shell>) + 'static,
        release: impl Fn(&mut Shell, &MouseUpEvent, &mut Context<Shell>) + 'static,
    ) -> Self {
        Self {
            carry: Rc::new(carry),
            release: Rc::new(release),
        }
    }
}

impl Shell {
    /// Carry the drag of a press on an element until the button comes up.
    pub(crate) fn hold(&mut self, hold: Hold) {
        self.hold = Some(hold);
    }

    /// Carry the gesture the held button makes to the pointer of `event`.
    /// A move with the button up is a release the window did not receive,
    /// and ends the gesture. `true` when a gesture took the move.
    pub(super) fn carry_gesture(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let held = event.pressed_button == Some(MouseButton::Left);
        // A press held for an element: its drag, or its release when the
        // button came up where no window saw it.
        if let Some(hold) = self.hold.clone() {
            if held {
                (hold.carry)(self, event, cx);
            } else {
                self.hold = None;
                let up = MouseUpEvent {
                    button: MouseButton::Left,
                    position: event.position,
                    modifiers: event.modifiers,
                    click_count: 1,
                };
                (hold.release)(self, &up, cx);
            }
            return true;
        }
        // A press on the Viewer's playhead field or mini ruler is held on the
        // state, so the drag keeps going off the field or the strip.
        if self.state.read(cx).viewer_scrub.press().is_some() {
            let state = &self.state;
            if held {
                let dx = f32::from(event.delta.x);
                let x = f32::from(event.position.x);
                let mods = event.modifiers;
                let moved = state.update(cx, |state, _| {
                    state.drag_viewer(dx, x, mods.shift, mods.control || mods.platform)
                });
                // The playhead field reads relative travel, so past the slop
                // the cursor hides and holds its place, as on a value readout.
                if state.read(cx).viewer_scrub.drag_held() {
                    window.lock_pointer();
                }
                if moved {
                    spawn_preview(self.state.clone(), cx);
                    cx.notify();
                }
            } else if state.update(cx, |state, _| state.release_viewer()) {
                window.unlock_pointer();
                cx.notify();
            }
            return true;
        }
        // A press on the work area bar, likewise: a drag keeps moving In and
        // Out after the pointer leaves the 8 px bar.
        if self.state.read(cx).work_area_held() {
            let x = f32::from(event.position.x);
            let moved = if held {
                self.state.update(cx, |state, _| state.drag_work_area(x))
            } else {
                self.state.update(cx, |state, _| state.release_work_area())
            };
            if moved {
                cx.notify();
            }
            return true;
        }
        // A press on the zoom scroll bar, likewise: a scroll or a zoom keeps
        // going after the pointer leaves the bar.
        if self.state.read(cx).zoom_bar_held() {
            let x = f32::from(event.position.x);
            let moved = if held {
                self.state.update(cx, |state, _| state.drag_zoom_bar(x))
            } else {
                self.state.update(cx, |state, _| state.release_zoom_bar())
            };
            if moved {
                self.redraw(Moved::View, cx);
            }
            return true;
        }
        // A pressed value readout, likewise: the drag keeps going when the
        // pointer leaves the readout.
        if self.state.read(cx).value_scrubs().pressed() {
            let dx = f32::from(event.delta.x);
            let mods = event.modifiers;
            let state = &self.state;
            if held {
                let changed = state.update(cx, |state, _| {
                    state.drag_value(dx, mods.shift, mods.control || mods.platform)
                });
                // Past the slop the cursor hides and holds its place, so the
                // drag reads relative travel and never stops at a screen edge.
                if state.read(cx).value_scrubs().drag_held() {
                    window.lock_pointer();
                }
                if changed {
                    spawn_preview(self.state.clone(), cx);
                    cx.notify();
                }
            } else if state.update(cx, |state, _| state.release_value()) {
                window.unlock_pointer();
                cx.notify();
            }
            return true;
        }
        // A held color wheel, likewise: the drag keeps going when the pointer
        // leaves the wheel.
        if self.state.read(cx).wheel_held().is_some() {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            let state = &self.state;
            let changed = if held {
                state.update(cx, |state, _| state.drag_wheel(at))
            } else {
                state.update(cx, |state, _| state.release_wheel())
            };
            if changed {
                spawn_preview(self.state.clone(), cx);
                cx.notify();
            }
            return true;
        }
        // A press on an Effect Controls time area, likewise: a keyframe drag or
        // a scrub keeps going when the pointer leaves the strip.
        if self.state.read(cx).key_strip_held() {
            let x = f32::from(event.position.x);
            let alt = event.modifiers.alt;
            let state = &self.state;
            let changed = if held {
                state.update(cx, |state, _| state.drag_key_strip(x, alt))
            } else {
                state.update(cx, |state, _| state.release_key_strip())
            };
            if changed {
                spawn_preview(self.state.clone(), cx);
                cx.notify();
            }
            return true;
        }
        // A grabbed keyframe or handle on the Inspector's graph, likewise: the
        // drag follows the pointer past the graph's edge.
        if self.state.read(cx).curve_dragging().is_some() {
            let [x, y] = [f32::from(event.position.x), f32::from(event.position.y)];
            let state = &self.state;
            let changed = if held {
                state.update(cx, |state, _| state.update_curve_drag(x, y))
            } else {
                state.update(cx, |state, _| state.release_curve_drag())
            };
            if changed {
                cx.notify();
            }
            return true;
        }
        // A grabbed point on the Speed tab's ramp, likewise.
        if self.state.read(cx).ramp_dragging().is_some() {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            let gesture = if held {
                Gesture::Drag { at }
            } else {
                Gesture::Release
            };
            if self
                .state
                .update(cx, |state, _| ramp_gesture::run(state, gesture))
            {
                cx.notify();
            }
            return true;
        }
        // A plugin panel's slider, likewise: its value is the panel's, and the
        // shell only routes the pointer.
        if self.state.read(cx).plugin_panels.slide.is_some() {
            let at = f32::from(event.position.x);
            let state = &self.state;
            let changed = if held {
                state.update(cx, |state, _| state.ride_panel_slider(at))
            } else {
                state.update(cx, |state, _| state.release_panel_slider())
            };
            if changed {
                cx.notify();
            }
            return true;
        }
        // A viewer overlay's handle, likewise: the plugin moves it.
        if self.state.read(cx).plugin_overlays.grab.is_some() {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            let state = &self.state;
            if held {
                state.update(cx, |state, _| state.carry_overlay_handle(at));
            } else {
                state.update(cx, |state, _| state.release_overlay_handle(at));
            }
            return true;
        }
        // A grabbed handle, motion path point or mask vertex on the Viewer's
        // picture, likewise: the drag keeps going off the picture.
        if self.state.read(cx).overlay_dragging() {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            let step = if held { Step::Carry(at) } else { Step::Release };
            self.state
                .update(cx, |state, _| overlay_gesture::run(state, step));
            cx.notify();
            return true;
        }
        // The Type tool's held caret, likewise: the selection follows the
        // pointer off the title.
        if self.state.read(cx).text_dragging() {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            let changed = self.state.update(cx, |state, _| {
                if held {
                    state.drag_text_caret(at)
                } else {
                    state.end_text_drag();
                    true
                }
            });
            if changed {
                cx.notify();
            }
            return true;
        }
        // A guide pulled from a Viewer ruler, likewise.
        if self.state.read(cx).viewer_guides.dragging.is_some() {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            self.state.update(cx, |state, _| {
                state.drag_guide(at);
                if !held {
                    state.release_guide(at);
                }
            });
            cx.notify();
            return true;
        }
        // A press held on the Viewer's picture pans a zoomed frame by the
        // pointer's travel, on the picture or off it.
        if let Some(from) = self.preview_pan {
            if !held {
                self.preview_pan = None;
                return false;
            }
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            self.preview_pan = Some(at);
            if self.state.read(cx).preview_view.pannable() {
                self.state.update(cx, |state, _| {
                    state.pan_preview(at[0] - from[0], at[1] - from[1])
                });
                cx.notify();
            }
            return true;
        }
        // A grabbed fader or pan knob, likewise: the value rides the pointer
        // off the strip.
        if let Some(drag) = self.state.read(cx).fader_dragging() {
            let gesture = if held {
                let at = if drag.target.is_pan() {
                    event.position.x
                } else {
                    event.position.y
                };
                mixer_strip::Gesture::RideFader { at: f32::from(at) }
            } else {
                mixer_strip::Gesture::ReleaseFader
            };
            let changed = self.state.update(cx, |state, _| {
                mixer_strip::apply(state, gesture).unwrap_or_else(|err| {
                    state.report_error(&err);
                    false
                })
            });
            if changed {
                cx.notify();
            }
            return true;
        }
        // A track header held to reorder the lanes: the header under the
        // pointer marks the drop row, and a move with the button up is the
        // release.
        if !held && self.state.read(cx).track_dragging() {
            self.release_track_drag(cx);
        }
        false
    }

    /// End every gesture the released button held, wherever it came up: each
    /// writes its drag as one undo step.
    pub(super) fn release_gestures(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(hold) = self.hold.take() {
            (hold.release)(self, event, cx);
        }
        // Letting go of a value readout closes its undo step or opens it as
        // text, and gives the pointer back; the picture already shows the
        // value.
        if self.state.update(cx, |state, _| state.release_value()) {
            window.unlock_pointer();
            cx.notify();
        }
        // Letting go of a playhead field after a drag gives the pointer back;
        // after a click it opens the field for typing. Letting go of the mini
        // ruler ends its scrub.
        if self.state.update(cx, |state, _| state.release_viewer()) {
            window.unlock_pointer();
            cx.notify();
        }
        // Letting go of the work area bar ends its move.
        if self.state.update(cx, |state, _| state.release_work_area()) {
            cx.notify();
        }
        // Letting go of the zoom scroll bar ends its scroll or zoom.
        if self.state.update(cx, |state, _| state.release_zoom_bar()) {
            cx.notify();
        }
        if self
            .state
            .update(cx, |state, _| state.release_panel_slider())
        {
            cx.notify();
        }
        // Letting go of a color wheel keeps the drag as one undo step.
        if self.state.update(cx, |state, _| state.release_wheel()) {
            cx.notify();
        }
        // Letting go of a time area writes a keyframe drag as one undo step;
        // a scrub stays where it ended.
        if self.state.update(cx, |state, _| state.release_key_strip()) {
            spawn_preview(self.state.clone(), cx);
            cx.notify();
        }
        // Letting go of a keyframe or handle on the Inspector's graph, or of a
        // point on the Speed tab's ramp, writes the drag as one undo step.
        if self.state.update(cx, |state, _| state.release_curve_drag()) {
            cx.notify();
        }
        if self
            .state
            .update(cx, |state, _| ramp_gesture::run(state, Gesture::Release))
        {
            cx.notify();
        }
        let at = [f32::from(event.position.x), f32::from(event.position.y)];
        if self
            .state
            .update(cx, |state, _| state.release_overlay_handle(at))
        {
            cx.notify();
        }
        // Letting go of a handle, motion path point or mask vertex on the
        // Viewer writes its drag as one undo step.
        if self.state.read(cx).overlay_dragging() {
            self.state
                .update(cx, |state, _| overlay_gesture::run(state, Step::Release));
            cx.notify();
        }
        if self.state.read(cx).text_dragging() {
            self.state.update(cx, |state, _| state.end_text_drag());
            cx.notify();
        }
        // A guide let go over its ruler or off the frame is deleted; anywhere
        // else it stays.
        if self.state.read(cx).viewer_guides.dragging.is_some() {
            self.state.update(cx, |state, _| {
                state.drag_guide(at);
                state.release_guide(at);
            });
            cx.notify();
        }
        self.preview_pan = None;
        // Letting go of a fader or pan knob writes its value as one command.
        if self.state.read(cx).fader_dragging().is_some() {
            self.state.update(cx, |state, _| {
                if let Err(err) = mixer_strip::apply(state, mixer_strip::Gesture::ReleaseFader) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }
        self.release_track_drag(cx);
    }

    /// Let go of a held track header: the lane drops on the row the drag
    /// marks, wherever the button came up.
    fn release_track_drag(&mut self, cx: &mut Context<Self>) {
        let released = self.state.update(cx, |state, _| {
            state.release_track_drag().unwrap_or_else(|err| {
                state.report_error(&err);
                true
            })
        });
        if released {
            cx.notify();
        }
    }

    /// Abandon the gesture in flight on Escape, putting back what its press
    /// found. `true` when there was one.
    pub(super) fn cancel_gesture(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // A value readout's value goes back where the press found it. The
        // pointer stays down, but the gesture is over.
        if self.state.update(cx, |state, _| state.cancel_value_drag()) {
            window.unlock_pointer();
            spawn_preview(self.state.clone(), cx);
            return true;
        }
        // A clip bar's line goes back.
        if self.state.update(cx, |state, _| state.cancel_clip_line()) {
            spawn_preview(self.state.clone(), cx);
            return true;
        }
        // In and Out go back where the press on the work area bar found them.
        if self
            .state
            .update(cx, |state, _| state.cancel_work_area_drag())
        {
            return true;
        }
        // The zoom and the scroll go back where the press on the zoom scroll
        // bar found them.
        if self
            .state
            .update(cx, |state, _| state.cancel_zoom_bar_drag())
        {
            self.redraw(Moved::View, cx);
            return true;
        }
        // The tab in front goes back on the frame a press on a playhead field
        // or the mini ruler found.
        if self.state.update(cx, |state, _| state.cancel_viewer_drag()) {
            window.unlock_pointer();
            spawn_preview(self.state.clone(), cx);
            return true;
        }
        // The grade goes back where a press on a color wheel found it.
        if self.state.update(cx, |state, _| state.cancel_wheel_drag()) {
            spawn_preview(self.state.clone(), cx);
            return true;
        }
        // A time area drops its keyframe drag, or puts the playhead back where
        // the press found it.
        if self.state.update(cx, |state, _| state.cancel_key_strip()) {
            spawn_preview(self.state.clone(), cx);
            return true;
        }
        // A splitter, lane or tab goes back where its press found it.
        if self.cancel_drag(cx) {
            return true;
        }
        // A rubber band leaves the selection as the band found it.
        if self.state.read(cx).marquee.is_some() {
            self.state.update(cx, |state, _| state.cancel_marquee());
            return true;
        }
        // An on-canvas handle drag costs nothing.
        if self.state.read(cx).overlay_dragging() {
            self.state
                .update(cx, |state, _| state.cancel_overlay_drag());
            return true;
        }
        if self.state.read(cx).track_dragging() {
            self.state.update(cx, |state, _| state.cancel_track_drag());
            return true;
        }
        if self.state.read(cx).ramp_dragging().is_some() {
            self.state.update(cx, |state, _| state.cancel_ramp_drag());
            return true;
        }
        if self.state.read(cx).curve_dragging().is_some() {
            self.state.update(cx, |state, _| state.cancel_curve_drag());
            return true;
        }
        false
    }

    /// Put back what a press on a splitter, a lane's border, the outline's
    /// border or a tab found, and drop the drag. `true` when one was in
    /// flight.
    fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.drag.take() else {
            return false;
        };
        self.tab_hover = None;
        super::dock_view::set_active_tab_hover(None);
        self.state.update(cx, |state, _| match drag.kind {
            DragKind::Timeline { .. } => {
                state.ui_settings.timeline_height = drag.start;
            }
            DragKind::Pan => {
                state.scroll_timeline_to(drag.start, false);
            }
            DragKind::Lane(_) => {
                state.take_lane_preview();
            }
            DragKind::Outline => {
                state.set_outline_width(drag.start);
            }
            DragKind::AvSplit { .. } => {
                state.set_timeline_av_split(drag.start);
            }
            DragKind::DockSplit {
                path,
                depth,
                index,
                w0,
                w1,
                ..
            } => {
                let full_path =
                    super::dock_view::active_split_drag_path(&path[..usize::from(depth)]);
                let slice: &[usize] = &full_path;
                if let Some(mut weights) = state.dock_tree.split_weights(slice)
                    && index + 1 < weights.len()
                {
                    weights[index] = w0;
                    weights[index + 1] = w1;
                    state.dock_tree.set_weights(slice, &weights);
                    state.sync_panes_from_dock_tree();
                }
            }
            DragKind::DockTab {
                panel,
                source_frame,
                from_index,
                ..
            } => {
                state
                    .dock_tree
                    .reorder_panel(source_frame, panel, from_index);
            }
        });
        true
    }
}
