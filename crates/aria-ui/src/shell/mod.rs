//! Four-panel editor chrome: the window frame, the splitters, and the slots the
//! panel and overlay registries fill.

use crate::app::{AppState, DialogKind, spawn_preview, start_playback};
use crate::fluent::FluentBuilder;
use crate::keys::{After, KeyRoute};
use crate::motion;
use crate::motion::glide;
use crate::plugins::overlay_host as overlays;
use crate::plugins::panel_host::host_of;
use crate::settings::{
    INSPECTOR_MAX, INSPECTOR_MIN, SIDEBAR_MAX, SIDEBAR_MIN, TIMELINE_MIN_HEIGHT,
};
use crate::theme::Theme;
use crate::types::{EditorTool, UiRoute};
use aria_plugin::PanelLocation;
use gpui::{
    AnimationExt, Context, CursorStyle, Entity, ExternalPaths, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use std::time::{Duration, Instant};
const SPLIT: f32 = 4.0;
mod text_input;
// The boot cover, moved out whole: the shell holds the field, that file owns
// every decision about it.
mod splash;
/// The startup budget the boot cover enforces, for a caller measuring a start.
pub use splash::SPLASH_MAX;
use splash::Splash;
// The route transition: one chased value for both ends.
mod route;
// What a drag is, and what letting go of one is worth.
mod drag;
use drag::{Drag, DragKind};
pub struct Shell {
    pub state: Entity<AppState>,
    drag: Option<Drag>,
    audio: crate::audio::AudioSystem,
    splash: Splash,
    /// Where a catalog drag last hovered, as (track id, tick).
    ///
    /// A drop listener is handed the payload but not the pointer, so the position
    /// has to be captured while the drag moves. Cleared on the drop it serves.
    catalog_drop: Option<(u64, i64)>,
    /// Where a pool-media drag last hovered over a lane, as (track id, tick).
    /// Separate from the catalog hover because a lane and an edge strip can both
    /// see the same drag pass over them, and only one of them is the target.
    media_drop: Option<(u64, i64)>,
    /// Where a pool-media drag last hovered over an edge strip, as a tick.
    media_edge_drop: Option<i64>,
    /// Where a media drag last hovered over the freeform pool canvas, in canvas coordinates.
    freeform_drop: Option<(f32, f32)>,
    /// Where a preview pan drag last sat, in window coordinates: deltas
    /// are what the pan wants, and GPUI hands out absolute positions.
    pub(crate) preview_pan_anchor: (f32, f32),
    /// The window focus the platform input handler is attached to. Key routing
    /// and an IME commit have to arrive at the same place, and a platform hands
    /// text to whatever holds focus.
    focus: gpui::FocusHandle,
    /// How many characters of the focused box are an IME preedit run, so a
    /// commit replaces them instead of doubling them.
    ime_marked: Option<usize>,
}
impl Shell {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let audio = crate::audio::AudioSystem::new(state.read(cx).audio.clone());
        cx.observe(&state, |this, _, cx| {
            this.audio.pump();
            cx.notify();
        })
        .detach();
        spawn_preview(state.clone(), cx);
        // One subscriber for the session's lifetime: job rows update on transitions,
        // never on a timer.
        state.update(cx, |state, cx| state.watch_jobs(cx));
        spawn_status_ageing(cx);
        spawn_meter(cx);
        // A tooltip is handed an `&mut App` and an element id, so the registry
        // has to be reachable from one: without this a hover would keep naming
        // the shipped chord for a key the operator has rebound.
        let host = crate::keys::host_of(&state.read(cx).ctx);
        if let Some(host) = host {
            crate::keys::set_mounted(host, cx);
        }
        Self {
            state,
            drag: None,
            audio,
            splash: Splash::Covering(Instant::now()),
            catalog_drop: None,
            media_drop: None,
            media_edge_drop: None,
            freeform_drop: None,
            preview_pan_anchor: (0.0, 0.0),
            focus: cx.focus_handle(),
            ime_marked: None,
        }
    }

    pub(crate) fn persist(&self, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.persist_settings_later(cx));
    }

    /// Record where a catalog drag is hovering, so the drop that follows knows the
    /// lane and the tick the pointer was over.
    pub(crate) fn set_catalog_drop(&mut self, track: u64, tick: i64) {
        self.catalog_drop = Some((track, tick.max(0)));
    }

    /// Take the recorded hover. `None` when a drop arrives with no move before it,
    /// which is the case a caller must treat as "no target" rather than tick 0.
    pub(crate) fn take_catalog_drop(&mut self) -> Option<(u64, i64)> {
        self.catalog_drop.take()
    }

    /// Record where a media drag is hovering over a lane.
    pub(crate) fn set_media_drop(&mut self, track: u64, tick: i64) {
        self.media_drop = Some((track, tick.max(0)));
    }

    pub(crate) fn take_media_drop(&mut self) -> Option<(u64, i64)> {
        self.media_drop.take()
    }

    /// Record where a media drag is hovering over an edge strip.
    pub(crate) fn set_media_edge_drop(&mut self, tick: i64) {
        self.media_edge_drop = Some(tick.max(0));
    }

    pub(crate) fn take_media_edge_drop(&mut self) -> Option<i64> {
        self.media_edge_drop.take()
    }

    /// Record where a media drag is hovering over the freeform canvas.
    pub(crate) fn set_freeform_drop(&mut self, x: f32, y: f32) {
        self.freeform_drop = Some((x.max(0.0), y.max(0.0)));
    }

    /// Take the recorded freeform canvas hover position.
    pub(crate) fn take_freeform_drop(&mut self) -> Option<(f32, f32)> {
        self.freeform_drop.take()
    }
    pub(crate) fn start_pan(&mut self, x: f32, cx: &mut Context<Self>) {
        let start = self.state.read(cx).scroll_x;
        self.drag = Some(Drag::new(DragKind::Pan, x, start));
    }

    /// Begin resizing `track` from its lower border. `y` is where the pointer
    /// went down and `height` the lane's height at that moment, so the drag is
    /// relative and the lane does not jump under the cursor.
    pub(crate) fn start_lane_resize(&mut self, track: u64, y: f32, height: f32) {
        self.drag = Some(Drag::new(DragKind::Lane(track), y, height));
    }

    /// Zoom by `factor`, keeping the second under the pointer where it is. `x`
    /// is the pointer in window coordinates, so it is measured against where the
    /// timeline's content starts rather than against the header width alone —
    /// the media rail is to the left of both, and it resizes.
    pub(crate) fn zoom_at(&mut self, x: f32, factor: f32, cx: &mut Context<Self>) {
        let Some(left) = self.state.read(cx).timeline_content_left() else {
            return;
        };
        self.state.update(cx, |state, _| {
            let pointer = (x - left).max(0.0);
            let (zoom, scroll) =
                crate::transport::zoom_keep_pointer(state.zoom, state.scroll_x, pointer, factor);
            state.set_zoom(zoom, false);
            state.scroll_timeline_to(scroll, false);
        });
    }
}
/// One stage of the editor arriving.
///
/// The window is on screen before any of this: what the operator watches is the
/// layout resolving, so the order is the reading order — the bar, then the rails
/// and the picture, then the timeline. Each stage waits one
/// [`motion::OPEN_STAGE`] longer than the one before it, and the whole
/// sequence fits inside the splash's own exit, so nothing is waited on.
///
/// Reduced motion draws the stage plainly: not a faster open, no open.
fn opening<E>(
    key: impl Into<motion::ControlId>,
    stage: usize,
    reduced: bool,
    el: E,
) -> gpui::AnyElement
where
    E: Styled + IntoElement + 'static,
{
    if reduced {
        return el.into_any_element();
    }
    let spec = motion::OPEN.with_delay(motion::OPEN_STAGE.times(stage as u64));
    el.with_animation(key.into().element("opening"), spec.animation(), |el, t| {
        el.opacity(t).relative().top(px(6.0 * (1.0 - t)))
    })
    .into_any_element()
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        // Hover fades advance on the frame clock, so while one is in flight the
        // window has to keep drawing or the blend freezes where the last mouse
        // event left it. This is also the tick that retires entries nothing drew
        // this frame, which is why it belongs here, once, and not in a child.
        if motion::hover_fades_active() {
            window.request_animation_frame();
        }
        // The window's keyboard lives on one handle: `on_key_down` below and the
        // platform input handler both hang off it, so a keystroke and an IME
        // commit arrive at the same place. Nothing else in the app takes focus,
        // so claiming it once is enough.
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let window_w = f32::from(window.viewport_size().width);
        let window_h = f32::from(window.viewport_size().height);
        if window_w > 0.0 && (self.state.read(cx).last_window_w - window_w).abs() > 0.5 {
            self.state
                .update(cx, |state, _| state.last_window_w = window_w);
        }
        if window_h > 0.0 && (self.state.read(cx).last_window_h - window_h).abs() > 0.5 {
            self.state
                .update(cx, |state, _| state.last_window_h = window_h);
        }
        // Five numbers, not the whole settings: the settings carry the key map,
        // and copying that to read a width is a copy per frame for nothing.
        let panes = self.state.read(cx).ui_settings.panes();
        let reduced = motion::reduced_motion(cx);
        let dragging = self.drag.as_ref().map(|drag| drag.kind);
        // A pane's drawn size is a glide toward the committed one, so collapsing
        // a rail and releasing a splitter both settle instead of jumping. While
        // a splitter is under the pointer its pane is exempt: an eased edge would
        // lag the hand dragging it.
        let sidebar_w = glide::glide(
            motion::MotionKey::new("pane", "sidebar"),
            if panes.sidebar_collapsed {
                0.0
            } else {
                panes.sidebar_width
            },
            glide::PANE,
            reduced || matches!(dragging, Some(DragKind::Sidebar)),
        );
        let inspector_w = glide::glide(
            motion::MotionKey::new("pane", "inspector"),
            if panes.inspector_collapsed {
                0.0
            } else {
                panes.inspector_width
            },
            glide::PANE,
            reduced || matches!(dragging, Some(DragKind::Inspector)),
        );
        let timeline_h = glide::glide(
            motion::MotionKey::new("pane", "timeline"),
            panes.timeline_height.max(TIMELINE_MIN_HEIGHT),
            glide::PANE,
            reduced || matches!(dragging, Some(DragKind::Timeline)),
        );

        let route = self.state.read(cx).route;
        let dialog = self.state.read(cx).dialog;
        let has_context = self.state.read(cx).context_menu.is_some();
        let renaming = self.state.read(cx).rename.is_some();
        let status = self.state.read(cx).status_message.clone();
        let err = self.state.read(cx).last_error.clone();

        // Every frame, whichever route is drawn: the value has to be asked for
        // to stay alive in the store, and the one being left is asked for too.
        let arrived = route::fade(route, reduced);
        let body = if route == UiRoute::Home {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .opacity(arrived)
                .child(crate::chrome::home_bar(&theme, window, cx))
                .children(self.slot(PanelLocation::Home, cx))
                .into_any_element()
        } else if self.state.read(cx).preview_view.fullscreen {
            // Fullscreen is the viewer with nothing beside it. The overlay slot
            // stays, so a dialog opened from a shortcut is still reachable, and
            // the status strip stays because a refusal must remain readable.
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .opacity(arrived)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .children(self.slot(PanelLocation::Viewer, cx))
                        .children(self.slot(PanelLocation::Overlay, cx)),
                )
                .child(status_strip(&theme, &self.state, status, err, cx))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .opacity(arrived)
                .child(opening(
                    "open-bar",
                    0,
                    reduced,
                    crate::chrome::editor_bar(&theme, &self.state, window, cx),
                ))
                .child(opening(
                    "open-body",
                    1,
                    reduced,
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .child(if panes.sidebar_collapsed && sidebar_w <= 0.5 {
                            collapsed_strip("Media", true, cx).into_any_element()
                        } else {
                            pane(
                                sidebar_w,
                                div()
                                    .flex()
                                    .flex_col()
                                    .size_full()
                                    .children(self.slot(PanelLocation::Sidebar, cx)),
                            )
                            .into_any_element()
                        })
                        .child(split_col(cx, DragKind::Sidebar))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .relative()
                                        .children(self.slot(PanelLocation::Viewer, cx))
                                        .children(self.slot(PanelLocation::Overlay, cx)),
                                )
                                .child(split_row(cx, DragKind::Timeline))
                                .child(opening(
                                    "open-timeline",
                                    2,
                                    reduced,
                                    pane_h(
                                        timeline_h.max(TIMELINE_MIN_HEIGHT),
                                        div()
                                            .flex()
                                            .flex_col()
                                            .size_full()
                                            .children(self.slot(PanelLocation::Timeline, cx)),
                                    ),
                                )),
                        )
                        .child(split_col(cx, DragKind::Inspector))
                        .child(if panes.inspector_collapsed && inspector_w <= 0.5 {
                            collapsed_strip("Inspector", false, cx).into_any_element()
                        } else {
                            pane(
                                inspector_w,
                                div()
                                    .flex()
                                    .flex_col()
                                    .size_full()
                                    .children(self.slot(PanelLocation::Inspector, cx)),
                            )
                            .into_any_element()
                        }),
                ))
                .child(status_strip(&theme, &self.state, status, err, cx))
                .into_any_element()
        };

        // Built before the chain: each overlay draws with `&mut Context`, and one
        // closure per condition inside the builder would hold that borrow twice.
        // Every condition is asked on every frame, including the frames after it
        // goes false: that is what lets the host animate an overlay out instead
        // of dropping it mid-fade.
        let dialog_overlay = self.overlay(
            overlays::OverlayWhen::Dialog(dialog),
            dialog != crate::app::DialogKind::None,
            cx,
        );
        let context_overlay = self.overlay(overlays::OverlayWhen::ContextMenu, has_context, cx);
        let rename_overlay = self.overlay(overlays::OverlayWhen::Rename, renaming, cx);

        // Registered during paint, which is the only time a window takes an
        // input handler. A zero-size canvas is the smallest element that gets a
        // paint callback; it draws nothing and takes no pointer.
        let ime = {
            let entity = cx.entity();
            let focus = self.focus.clone();
            gpui::canvas(
                |_, _, _| (),
                move |bounds, _, window, app| {
                    window.handle_input(
                        &focus,
                        gpui::ElementInputHandler::new(bounds, entity.clone()),
                        app,
                    );
                },
            )
            .absolute()
            .size(px(0.))
        };
        // Asked before the tree is built, because drawing the cover is what
        // decides whether the cover is over.
        let splash = self.splash_frame(&theme, reduced, window, cx);
        div()
            .id("shell")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .bg(theme.bg)
            .text_color(theme.text)
            .font_family(theme.font_sans.clone())
            .text_size(px(13.))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _window, cx| {
                    let Some(drag) = this.drag.take() else {
                        return;
                    };
                    if let DragKind::Lane(track) = drag.kind {
                        this.state.update(cx, |state, _| {
                            if let Some(height) = state.take_lane_preview() {
                                if let Err(err) = state.set_lane_height(track, height) {
                                    state.report_error(&err);
                                }
                            }
                        });
                    }
                    // The frame the hand leaves is the only one that knows what
                    // the gesture was worth. A throw of nothing leaves the view
                    // exactly where the pointer left it, which is what a slow
                    // drag, a pause before the release, and reduced motion all
                    // ask for.
                    let throw = drag.throw(motion::reduced_motion(cx));
                    if throw != 0.0 {
                        this.state.update(cx, |state, _| {
                            let from = state.scroll_x;
                            state.fling_timeline_to(from - throw);
                        });
                    }
                    this.persist(cx);
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(Self::on_key))
            // Where a platform menu row lands: macOS dispatches the row's action
            // into the focused element chain, and this is the root of it. The row
            // then runs the registry entry the CSD bar would have run.
            .on_action(cx.listener(
                |this, action: &crate::chrome::native_menus::RunMenuRow, _window, cx| {
                    this.state.update(cx, |state, cx| {
                        crate::chrome::native_menus::dispatch(action, state, cx);
                    });
                    cx.notify();
                },
            ))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                let files = paths.paths().to_vec();
                this.state.update(cx, |state, _| {
                    if state.route == UiRoute::Home {
                        // A drop on Home replaces the session, so it answers to
                        // the unsaved prompt like New and Open do.
                        state.request_drop_on_home(files);
                    } else if let Err(err) = state.handle_dropped_paths(files) {
                        state.report_error(&err);
                    }
                });
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }))
            .child(body)
            .child(ime)
            // Nothing to paint: the frame that drew a value still travelling asks
            // for the next one, so a collapse, a ripple or a follow runs to its
            // end. Mounted here because the shell is the one element drawn in
            // every route.
            .child(motion_driver())
            .children(splash)
            .children(dialog_overlay)
            .children(context_overlay)
            .children(rename_overlay)
    }
}

/// The element that keeps the chrome's own motion running.
///
/// A GPUI view repaints when something notifies it, and an eased value has
/// nothing to notify it: the value is a function of the clock. So the frame that
/// drew an unsettled value asks for the next one. Nothing is painted, and once
/// every value has landed the requests stop.
///
/// It lives with the shell rather than with the store because exactly one
/// window drives the frames: a second driver would advance the store's frame
/// counter twice and evict the values the first had just drawn.
fn motion_driver() -> impl IntoElement {
    gpui::canvas(
        |_, window, cx| {
            // A drag released off a panel sends no event to the row it left, so
            // the hint is dropped on the first frame with no drag in flight.
            if !gpui::App::has_active_drag(cx) {
                crate::drag_hint::clear();
            }
            if glide::tick() {
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .w(px(0.))
    .h(px(0.))
}

// Further methods live in sibling files; inherent methods keep their paths.
mod slot;
/// How often the status line is checked for expiry. Coarse on purpose: the
/// strip is a receipt, so a fraction of a second either side of four is not
/// worth a repaint budget.
const STATUS_TICK: Duration = Duration::from_millis(500);
/// How often the output meter is advanced. Fast enough that a bar falls smoothly
/// rather than in steps, and the wake is cheap: with nothing playing the reading
/// does not change and nothing repaints.
const METER_TICK: Duration = Duration::from_millis(50);

// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
mod strip;
use strip::{
    collapsed_strip, pane, pane_h, spawn_meter, spawn_status_ageing, split_col, split_row,
    status_strip,
};
