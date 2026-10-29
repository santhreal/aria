//! Four-panel editor chrome: the window frame, the splitters, and the slots the
//! panel and overlay registries fill.

use crate::app::{AppState, DialogKind, spawn_preview, start_playback};
use crate::chrome::window_frame::{self, Frame, WindowTitle};
use crate::keys::{After, KeyRoute};
use crate::motion;
use crate::motion::glide;
use crate::plugins::overlay_host as overlays;
use crate::plugins::panel_host::host_of;
use crate::settings::TIMELINE_MIN_HEIGHT;
use crate::theme::Theme;
use crate::types::{EditorTool, UiRoute};
use aria_plugin::PanelLocation;
use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext as _, Context, Entity, ExternalPaths, InteractiveElement, IntoElement, KeyDownEvent,
    KeyUpEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render,
    Styled, Window, div, px,
};
use std::time::{Duration, Instant};
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
// Docked panel bodies as cached views.
mod panel_view;
pub use panel_view::{Moved, notify_after_draw};
/// A press held on the Viewer's picture with the left or middle button.
#[derive(Clone, Copy)]
pub(crate) struct PreviewPan {
    /// Where the pointer last sat, in window coordinates.
    pub(crate) at: [f32; 2],
    /// The button holding it, whose release ends the pan.
    pub(crate) button: MouseButton,
}

pub struct Shell {
    pub state: Entity<AppState>,
    drag: Option<Drag>,
    /// A press on an element whose drag this shell carries past the element.
    hold: Option<gesture::Hold>,
    /// The preview audio stream, which the main window's shell pumps. A
    /// floating window's shell has none: a second system on the one handle
    /// would never start a stream, and dropping it would clear the clock the
    /// first one drives.
    audio: Option<crate::audio::AudioSystem>,
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
    /// A press held on the Viewer's picture, or `None` with no such press: a
    /// zoomed picture pans by each move's travel from where it last sat.
    pub(crate) preview_pan: Option<PreviewPan>,
    /// Wheel travel over the Viewer's picture, in notches, not yet taken as a
    /// zoom level: a trackpad's pixels add up to whole levels.
    pub(crate) viewer_wheel: f32,
    /// The window focus the platform input handler is attached to. Key routing
    /// and an IME commit have to arrive at the same place, and a platform hands
    /// text to whatever holds focus.
    focus: gpui::FocusHandle,
    /// How many characters of the focused box are an IME preedit run, so a
    /// commit replaces them instead of doubling them.
    ime_marked: Option<usize>,
    /// Whether the Alt held now was pressed alone, which opens the menu bar on
    /// its release. See [`crate::keys::lone_alt`].
    lone_alt: crate::keys::lone_alt::LoneAlt,
    /// Hovered frame and drop zone while dragging a panel tab.
    pub(super) tab_hover: Option<(crate::dock::FrameId, crate::dock::Zone)>,
    /// The frame menu or tab overflow menu open in this window.
    pub(super) frame_menu: Option<dock_view::frame_menu::FrameMenu>,
    /// When `Some`, this `Shell` instance renders a single floating frame window.
    pub(super) floating_frame: Option<crate::dock::FrameId>,
    /// Open floating GPUI window handles owned by the main `Shell`.
    pub(super) floating_windows: Vec<(crate::dock::FrameId, gpui::AnyWindowHandle)>,
    /// The window's frame as of this draw.
    pub(super) frame: Frame,
    /// A press on an empty stretch of the bar that becomes a window move once
    /// the pointer travels.
    pub(crate) window_move: bool,
    /// The title the platform window was last given.
    title: Option<WindowTitle>,
    /// The cached body view of each docked panel drawn in this window.
    bodies: std::collections::HashMap<crate::dock::PanelId, Entity<panel_view::PanelBody>>,
    /// The cached view of each chrome part drawn in this window's last frame.
    parts: Vec<panel_view::PartSlot>,
    /// Whether this draw mounts the chrome parts as cached views. False while
    /// the editor arrives, when every frame draws the parts at a new opacity.
    chrome_cached: bool,
    /// An empty view whose notification redraws the chrome alone.
    beacon: Entity<panel_view::Beacon>,
    /// The timeline projection count the panel bodies were last redrawn for.
    drawn_projections: u64,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("state", &self.state.entity_id())
            .field("drawn_projections", &self.drawn_projections)
            .finish_non_exhaustive()
    }
}

impl Shell {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let audio = Some(crate::audio::AudioSystem::new(state.read(cx).audio.clone()));
        cx.observe(&state, |this, state, cx| {
            // A stream that did not start is posted by the pump that tried
            // it. The post notifies `state` again, and that pump starts
            // nothing, so the observer settles.
            if let Some(err) = this.audio.as_mut().and_then(|audio| audio.pump()) {
                state.update(cx, |state, cx| {
                    state.report_error(err);
                    cx.notify();
                });
            }
            cx.notify();
        })
        .detach();
        Self::redraw_on_narrow_changes(&state, cx);
        spawn_preview(state.clone(), cx);
        // One subscriber each for the session's lifetime: job rows update on
        // transitions, edits from plugins and the assistant re-project when
        // they commit, main-lane plugin handlers run when they are queued, and
        // plugin panels and viewer overlays are read again when one rebuilds,
        // never on a timer.
        state.update(cx, |state, cx| {
            state.watch_jobs(cx);
            state.watch_edits(cx);
            state.watch_main_lane(cx);
            state.watch_plugin_panels(cx);
            state.watch_plugin_overlays(cx);
            state.watch_preview_worker(cx);
        });
        spawn_status_ageing(cx);
        spawn_meter(cx);
        // A tooltip is handed an `&mut App` and an element id, so the registry
        // has to be reachable from one: without this a hover would keep naming
        // the shipped chord for a rebound key.
        let host = crate::keys::host_of(&state.read(cx).ctx);
        if let Some(host) = host {
            crate::keys::set_mounted(host, cx);
        }
        cx.observe_self(|this, cx| this.redraw_bodies(cx)).detach();
        Self {
            state,
            drag: None,
            hold: None,
            audio,
            splash: Splash::Covering(Instant::now()),
            catalog_drop: None,
            media_drop: None,
            media_edge_drop: None,
            freeform_drop: None,
            preview_pan: None,
            viewer_wheel: 0.0,
            focus: cx.focus_handle(),
            ime_marked: None,
            lone_alt: Default::default(),
            tab_hover: None,
            frame_menu: None,
            floating_frame: None,
            floating_windows: Vec::new(),
            frame: Frame::Platform,
            window_move: false,
            title: None,
            bodies: Default::default(),
            parts: Vec::new(),
            chrome_cached: true,
            beacon: cx.new(|_| panel_view::Beacon),
            drawn_projections: 0,
        }
    }

    /// Construct a `Shell` view that renders a single floating [`crate::dock::Frame`]
    /// sharing the main window's `AppState` entity.
    pub fn new_floating(
        state: Entity<AppState>,
        frame_id: crate::dock::FrameId,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self::redraw_on_narrow_changes(&state, cx);
        cx.observe_self(|this, cx| this.redraw_bodies(cx)).detach();
        Self {
            state,
            drag: None,
            hold: None,
            audio: None,
            splash: Splash::Done,
            catalog_drop: None,
            media_drop: None,
            media_edge_drop: None,
            freeform_drop: None,
            preview_pan: None,
            viewer_wheel: 0.0,
            focus: cx.focus_handle(),
            ime_marked: None,
            lone_alt: Default::default(),
            tab_hover: None,
            frame_menu: None,
            floating_frame: Some(frame_id),
            floating_windows: Vec::new(),
            frame: Frame::Platform,
            window_move: false,
            title: None,
            bodies: Default::default(),
            parts: Vec::new(),
            chrome_cached: true,
            beacon: cx.new(|_| panel_view::Beacon),
            drawn_projections: 0,
        }
    }

    /// Redraws the panel bodies that draw a tile, a filmstrip cell, the
    /// playhead or a Viewer frame when `state` reports one changed and
    /// nothing else did.
    fn redraw_on_narrow_changes(state: &Entity<AppState>, cx: &mut Context<Self>) {
        cx.subscribe(state, |this, _, _: &crate::app::PoolTileChanged, cx| {
            this.redraw(Moved::PoolHover, cx)
        })
        .detach();
        cx.subscribe(state, |this, _, _: &crate::app::FilmstripChanged, cx| {
            this.redraw(Moved::Filmstrip, cx)
        })
        .detach();
        cx.subscribe(state, |this, _, _: &crate::app::FrameReady, cx| {
            this.redraw(Moved::Frame, cx)
        })
        .detach();
        cx.subscribe(state, |this, _, moved: &crate::app::PlayheadMoved, cx| {
            // A playhead that ran past the view scrolled it, and the timeline
            // body draws the scroll.
            if moved.followed {
                this.redraw(Moved::View, cx);
            }
            this.redraw(Moved::Playhead, cx)
        })
        .detach();
    }

    /// Give the platform window `title` when it differs from the one it has,
    /// so a draw that changes nothing sends the platform nothing.
    fn set_title(&mut self, title: WindowTitle, window: &mut Window) {
        if self.title.as_ref() == Some(&title) {
            return;
        }
        window.set_window_title(&title.text());
        window.set_window_edited(title.edited());
        self.title = Some(title);
    }

    /// The buttons whose press can begin a drag. Each one's release ends the
    /// drag it pressed.
    const DRAG_BUTTONS: [MouseButton; 2] = [MouseButton::Left, MouseButton::Middle];

    /// `root`, ending a press's gesture on the release of its button over this
    /// window or outside it: the press began in this window, so its release is
    /// this window's, and a tab let go past the edge floats.
    fn ends_drags<E: InteractiveElement>(root: E, cx: &mut Context<Self>) -> E {
        Self::DRAG_BUTTONS.into_iter().fold(root, |root, button| {
            root.on_mouse_up(button, cx.listener(Self::on_release))
                .on_mouse_up_out(button, cx.listener(Self::on_release))
        })
    }

    /// A button came up, over this window or outside it. The main window and
    /// every floating panel window end their gestures here: the left button's
    /// holds, and the drag of whichever button came up.
    fn on_release(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.button == MouseButton::Left {
            // A press on the bar that never travelled moves nothing.
            self.window_move = false;
            self.release_gestures(event, window, cx);
        }
        if self
            .preview_pan
            .is_some_and(|pan| pan.button == event.button)
        {
            self.preview_pan = None;
        }
        let Some(drag) = self.drag.take_if(|drag| drag.kind.button() == event.button) else {
            return;
        };
        let hover = self.tab_hover.take();
        // A tab dragged out of a floating window is dropped on the frame the
        // pointer is over in whichever window drew its drop zones.
        let shared = dock_view::active_tab_hover();
        // The hover is shared with every floating panel window, and each one
        // draws its drop zones while it is set.
        dock_view::set_active_tab_hover(None);
        match drag.kind {
            DragKind::Lane(track) => {
                self.state.update(cx, |state, _| {
                    if let Some(height) = state.take_lane_preview()
                        && let Err(err) = state.set_lane_height(track, height)
                    {
                        state.report_error(&err);
                    }
                });
            }
            DragKind::DockSplit { .. } => {
                self.state.update(cx, |state, cx| {
                    state.record_dock_edit(cx);
                });
            }
            DragKind::Outline | DragKind::Timeline { .. } => {
                self.state
                    .update(cx, |state, cx| state.persist_settings_later(cx));
            }
            DragKind::DockTab {
                panel,
                source_frame,
                moved,
                ..
            } => {
                if moved && let Some(frame_id) = self.floating_frame {
                    if let Some((target_frame, zone)) = hover.or(shared)
                        && !(target_frame == source_frame && zone == crate::dock::Zone::Center)
                    {
                        let still_floating = self.state.update(cx, |state, cx| {
                            state.dock_tree.dock(panel, target_frame, zone);
                            state.record_dock_edit(cx);
                            state
                                .dock_tree
                                .floating
                                .iter()
                                .any(|fl| fl.frame.id == frame_id)
                        });
                        // The frame's last panel left it: so does its window.
                        if !still_floating {
                            window.remove_window();
                            return;
                        }
                    }
                } else if moved {
                    let x = f32::from(event.position.x);
                    let y = f32::from(event.position.y);
                    let (win_w, win_h) = {
                        let read = self.state.read(cx);
                        (read.last_window_w, read.last_window_h)
                    };
                    let outside = x < 0.0
                        || y < 0.0
                        || (win_w > 0.0 && x > win_w)
                        || (win_h > 0.0 && y > win_h);
                    if outside {
                        // A floating window is placed on the screen, and the
                        // release is a position in this window.
                        let origin = window.bounds().origin;
                        self.state.update(cx, |state, cx| {
                            state.float_dock_panel(
                                panel,
                                crate::dock::FloatRect {
                                    x: (x + f32::from(origin.x)).max(40.0),
                                    y: (y + f32::from(origin.y)).max(40.0),
                                    w: 480.0,
                                    h: 320.0,
                                },
                                cx,
                            );
                        });
                        self.sync_floating_windows(cx);
                    } else if let Some((target_frame, zone)) = hover {
                        self.state.update(cx, |state, cx| {
                            if !(target_frame == source_frame && zone == crate::dock::Zone::Center)
                            {
                                state.dock_tree.dock(panel, target_frame, zone);
                            }
                            state.record_dock_edit(cx);
                        });
                    }
                }
            }
            DragKind::Pan { .. } | DragKind::AvSplit { .. } => {}
        }
        // The frame the hand leaves is the only one that knows what
        // the gesture was worth. A throw of nothing leaves the view
        // exactly where the pointer left it, which is what a slow
        // drag, a pause before the release, and reduced motion all
        // ask for.
        let throw = drag.throw(cx.reduce_motion());
        if throw != 0.0 {
            self.state.update(cx, |state, _| {
                let from = state.scroll_x;
                state.fling_timeline_to(from - throw);
            });
        }
        self.persist(cx);
        cx.notify();
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

    /// Begin scrolling the timeline by the pointer's travel from window x `x`,
    /// until `button`, which pressed it, comes up.
    pub(crate) fn start_pan(&mut self, x: f32, button: MouseButton, cx: &mut Context<Self>) {
        let start = self.state.read(cx).scroll_x;
        self.drag = Some(Drag::new(DragKind::Pan { button }, x, start));
    }

    /// Begin resizing `track` from its lower border. `y` is where the pointer
    /// went down and `height` the lane's height at that moment, so the drag is
    /// relative and the lane does not jump under the cursor.
    pub(crate) fn start_lane_resize(&mut self, track: u64, y: f32, height: f32) {
        self.drag = Some(Drag::new(DragKind::Lane(track), y, height));
    }

    /// Begin resizing the timeline's outline column from its right border. `x`
    /// is where the pointer went down and `width` the column's width then.
    pub(crate) fn start_outline_resize(&mut self, x: f32, width: f32) {
        self.drag = Some(Drag::new(DragKind::Outline, x, width));
    }

    /// Begin moving the divider between the timeline's video and audio sides.
    /// `y` is where the pointer went down, `split` the split then, and
    /// `stack_h` the height the two sides share.
    pub(crate) fn start_av_split(&mut self, y: f32, split: f32, stack_h: f32) {
        self.drag = Some(Drag::new(DragKind::AvSplit { stack_h }, y, split));
    }

    /// Zoom by `factor`, keeping the second under the pointer where it is. `x`
    /// is the pointer in window coordinates (see `AppState::zoom_at_pointer`).
    pub fn zoom_at(&mut self, x: f32, factor: f32, cx: &mut Context<Self>) {
        if self
            .state
            .update(cx, |state, _| state.zoom_at_pointer(x, factor))
        {
            self.redraw(Moved::View, cx);
        }
    }
}
/// One stage of the editor arriving, drawn at the route value `arrived`.
///
/// The window is on screen before any of this: what is visible is the layout
/// resolving, so the order is the reading order: the bar, then the
/// docked panels, each [`motion::OPEN_STAGE_LAG`] of the route behind the one
/// before it. The stages are read off the route value rather than played as
/// their own animations, so the editor is drawn by one clock: it lands when the
/// route lands, a route change during it continues from the screen, and
/// reduced motion, which lands the route on its first frame, draws every stage
/// at rest.
fn opening<E>(stage: usize, arrived: f32, el: E) -> gpui::AnyElement
where
    E: Styled + IntoElement + 'static,
{
    let t = motion::open_stage(arrived, stage);
    if t >= 1.0 {
        return el.into_any_element();
    }
    el.opacity(t)
        .relative()
        .top(px(6.0 * (1.0 - t)))
        .into_any_element()
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        // Ctrl+Q, File > Quit and the close control set this once the
        // unsaved prompt is answered, or immediately when nothing is unsaved.
        if self.state.read(cx).quit_requested() {
            crate::session_end::quit(window, &self.state, cx);
        }
        // Every hover blend, glide and entrance this draw builds samples the
        // frame opened here, so it comes before any child is built.
        motion::begin_frame(cx);
        motion::hold_for_playback(self.state.read(cx).playing);
        self.begin_parts();
        // An edit or a playhead move made while paused leaves the old frame on
        // screen. The first draw after it asks for the frame again, whichever
        // control, key or job made it; a control that asked cleared the mark.
        if self.state.read(cx).still_frame_stale() {
            let state = self.state.clone();
            cx.defer(move |app| spawn_preview(state, app));
        }
        let frame = Frame::of(window);
        self.frame = frame;
        if let Some(frame_id) = self.floating_frame {
            let floating = self.render_floating_window(frame_id, &theme, window, cx);
            return window_frame::framed(floating, frame, &theme, window);
        }
        motion::mount_chrome(self.beacon.entity_id());
        let title = {
            let read = self.state.read(cx);
            if read.route == UiRoute::Home {
                WindowTitle::Home
            } else {
                WindowTitle::Project {
                    name: read.store.project_name.clone(),
                    dirty: read.store.is_dirty,
                }
            }
        };
        self.set_title(title, window);
        // The window's keyboard lives on one handle: `on_key_down` below and the
        // platform input handler both hang off it, so a keystroke and an IME
        // commit arrive at the same place. Nothing else in the app takes focus,
        // so claiming it once is enough.
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        // Alt+Tab releases Alt in another window, and the release that arrives
        // when focus returns is not a lone press. Losing focus redraws, so the
        // press is forgotten here.
        if !window.is_window_active() {
            self.lone_alt.reset();
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
        // Plugins read the view a frame was drawn from, and are told what
        // changed since the last one.
        self.state.read(cx).publish_view();
        let reduced = cx.reduce_motion();
        let route = self.state.read(cx).route;
        let dialog = self.state.read(cx).dialog;
        let has_context = self.state.read(cx).context_menu.is_some();
        let renaming = self.state.read(cx).rename.is_some();
        let status = self.state.read(cx).status_message.clone();
        let err = self.state.read(cx).last_error.clone();

        // Every frame, whichever route is drawn: the value has to be asked for
        // to stay alive in the store, and the one being left is asked for too.
        let arrived = route::fade(route, reduced);
        // Every stage of the opening lands with the route value.
        self.chrome_cached = arrived >= 1.0;
        let body = if route == UiRoute::Home {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .opacity(arrived)
                .when(frame.has_title_bar(), |el| {
                    el.child(crate::chrome::home_bar(&theme, window, cx))
                })
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
                .child(self.editor_bar(&theme, arrived, window, cx))
                .child(opening(
                    1,
                    arrived,
                    // The frames stand a gutter in from the bars and the
                    // window's edges, as far as they stand from each other.
                    div()
                        .debug_selector(|| "dock-area".into())
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .size_full()
                        .p(px(crate::dock::DOCK_GUTTER))
                        .child(self.render_editor_dock(&theme, window_w, window_h, reduced, cx)),
                ))
                .child(opening(
                    1,
                    arrived,
                    div().flex().flex_col().child(status_strip(
                        &theme,
                        &self.state,
                        status,
                        err,
                        cx,
                    )),
                ))
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
        // A click or a wheel while Alt is down is a gesture, not a menu press.
        // Watched in the capture phase, so a panel that stops a wheel still
        // spoils the press. Draws nothing and takes no pointer, like `ime`.
        //
        // The pointer's travel is watched here too, on the window rather than on
        // the shell's hitbox: a held gesture follows the pointer past the
        // window's edge, where the platform still reports it and nothing is
        // hovered.
        let alt_watch = {
            let shell = cx.entity().downgrade();
            gpui::canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    let moved = shell.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, app| {
                        if phase == gpui::DispatchPhase::Bubble {
                            let _ =
                                moved.update(app, |this, cx| this.on_mouse_move(event, window, cx));
                        }
                    });
                    let down = shell.clone();
                    window.on_mouse_event(move |_: &MouseDownEvent, phase, _, app| {
                        if phase == gpui::DispatchPhase::Capture {
                            let _ = down.update(app, |this, _| this.lone_alt.spoil());
                        }
                    });
                    window.on_mouse_event(move |_: &gpui::ScrollWheelEvent, phase, _, app| {
                        if phase == gpui::DispatchPhase::Capture {
                            let _ = shell.update(app, |this, _| this.lone_alt.spoil());
                        }
                    });
                },
            )
            .absolute()
            .size(px(0.))
        };
        // Asked before the tree is built, because drawing the cover is what
        // decides whether the cover is over.
        let splash = self.splash_frame(&theme, reduced, window, cx);
        let shell = div()
            .id("shell")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .bg(theme.bg)
            .text_color(theme.text)
            .font_family(theme.font_sans.clone())
            .text_size(Theme::TEXT_UI)
            .map(|root| Self::ends_drags(root, cx))
            // Any key while Alt is down makes it a chord. Captured, so a key a
            // child consumes still counts.
            .capture_key_down(cx.listener(|this, _: &KeyDownEvent, _, _| this.lone_alt.spoil()))
            .on_key_down(cx.listener(Self::on_key))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_modifiers_changed(cx.listener(Self::on_modifiers))
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
                let urls: Vec<String> = paths.urls().iter().map(|u| u.to_string()).collect();
                this.state.update(cx, |state, _| {
                    if !files.is_empty() {
                        if state.route == UiRoute::Home {
                            // A drop on Home replaces the session, so it answers to
                            // the unsaved prompt like New and Open do.
                            state.request_drop_on_home(files);
                        } else if let Err(err) = state.handle_dropped_paths(files) {
                            state.report_error(&err);
                        }
                    }
                    if !urls.is_empty() {
                        state.handle_dropped_urls(urls);
                    }
                });
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }))
            .child(body)
            .child(ime)
            .child(alt_watch)
            // Nothing to paint: ends the motion frame after every element that
            // read the stores, and requests the next frame while a value is
            // still travelling, so a hover, a collapse, a ripple or a follow
            // runs to its end. Mounted here because the shell is the one element
            // drawn in every route.
            .child(motion_driver())
            // Notified by a narrow redraw, so the chrome is drawn again while
            // the panel bodies that did not move replay their last drawing.
            .child(self.beacon())
            .children(splash)
            .children(dialog_overlay)
            .children(context_overlay)
            .children(rename_overlay)
            .into_any_element();
        window_frame::framed(shell, frame, &theme, window)
    }
}

/// The element that keeps the chrome's own motion running.
///
/// A GPUI view repaints when something notifies it, and an eased value has
/// nothing to notify it: the value is a function of the clock. So the frame that
/// drew an unsettled value requests the next one through [`motion::end_frame`].
/// Nothing is painted, and once every value has landed the requests stop.
///
/// It is mounted by the shell rather than by the stores because exactly one
/// element ends each frame: a second would advance the stores' frame counter
/// twice and drop the values the first had just drawn.
fn motion_driver() -> impl IntoElement {
    gpui::canvas(
        |_, window, cx| {
            // A drag released off a panel sends no event to the row it left, so
            // the hint is dropped on the first frame with no drag in flight.
            if !gpui::App::has_active_drag(cx) {
                crate::drag_hint::clear();
            }
            motion::end_frame(window);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .w(px(0.))
    .h(px(0.))
}

// Further methods live in sibling files; inherent methods keep their paths.
pub mod dock_view;
mod gesture;
pub(crate) use gesture::Hold;
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
use strip::{spawn_meter, spawn_status_ageing, status_strip};
