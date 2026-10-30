//! Docked panel bodies and chrome parts drawn as cached views.
//!
//! Each frame's body is a view of its own, mounted through
//! [`gpui::AnyView::cached`]: the frame lays the body out as one box, and the
//! body is built, laid out and painted again only when its view is notified.
//! Otherwise the window replays the body's last drawing at the same bounds.
//! The editor bar and each frame's tab row are drawn the same way, as
//! [`ChromePart`]s.
//!
//! Every notification of the shell notifies every part and every body
//! ([`Shell::redraw_bodies`], run by the shell's own observer), so a change
//! made anywhere is drawn everywhere. A body that keeps a `PanelStamp` of
//! its inputs, such as the Audio Meters and the Effects panel, is skipped
//! while the stamp is unchanged and the editor has arrived. Two paths are
//! narrow: [`Shell::redraw`], for a change that moves only the timeline view,
//! the playhead, the pool tile under the pointer, what the Viewer shows or
//! reads under the pointer, the cursor the timeline's lanes set, or a held
//! drag on the timeline, redraws the chrome the shell draws itself and the
//! bodies whose drawing reads what moved, and
//! [`Shell::redraw_chrome`], for a change to chrome alone, redraws the chrome.
//! The rest replay their last drawing. The timeline's playhead is painted by
//! the shell over the timeline body (`crate::timeline_playhead`), so
//! a playhead move replays the timeline body while that layer holds.

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StyleRefinement, Styled as _, WeakEntity, Window,
    WindowControls, div, px,
};

use crate::chrome::window_frame::Frame as WindowFrame;
use crate::chrome::{BarDrop, EDITOR_BAR_H};
use crate::dock::{
    Frame, PANEL_AUDIO_METERS, PANEL_EFFECTS, PANEL_INSPECTOR, PANEL_PROJECT, PANEL_TIMELINE,
    PANEL_VIEWER, PanelId,
};
use crate::motion;
use crate::theme::Theme;

use super::Shell;
use super::dock_view::frame_menu::FrameMenu;

#[cfg(test)]
#[path = "panel_view_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "panel_view_idle_tests.rs"]
mod idle_tests;

#[cfg(test)]
#[path = "panel_view_hover_tests.rs"]
mod hover_tests;

#[cfg(test)]
#[path = "panel_view_pointer_tests.rs"]
mod pointer_tests;

#[cfg(test)]
#[path = "panel_view_chrome_tests.rs"]
mod chrome_tests;

#[cfg(test)]
#[path = "panel_view_playback_tests.rs"]
mod playback_tests;

#[cfg(test)]
#[path = "panel_view_playhead_tests.rs"]
mod playhead_tests;

#[cfg(test)]
#[path = "panel_view_session_tests.rs"]
mod session_tests;

/// What a narrow change moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    /// The timeline's horizontal scroll, zoom, or lane scroll.
    View,
    /// The playhead, scrubbed or moved by a playback vsync
    /// ([`crate::app::PlayheadMoved`]).
    Playhead,
    /// A media tile's scrub position, from the tile or from the mini ruler of
    /// its Source tab, or a tile picture arriving
    /// ([`crate::app::PoolTileChanged`]).
    PoolHover,
    /// A timeline filmstrip cell arriving ([`crate::app::FilmstripChanged`]).
    Filmstrip,
    /// What the Viewer shows: a frame, or a scope or probe reading of the one
    /// on screen, arriving from the preview worker
    /// ([`crate::app::FrameReady`]); the take of a Source tab scrubbed on its
    /// mini ruler; the pixel the Info panel reads under the pointer; the
    /// rubber band of a mask path the Pen is drawing; or the pan of a zoomed
    /// frame or a guide, held under the pointer.
    Frame,
    /// The pointer over the timeline's lanes, onto a target that sets another
    /// cursor than the lanes last painted.
    Pointer,
    /// A drag held on the timeline: what it draws while the button is down,
    /// a clip drag's cards, readout and magnet (`AppState::hold_drag`), or the
    /// drop row of a lane dragged by its header. It lands nothing in the
    /// project before its release.
    Drag,
}

impl Moved {
    pub const ALL: [Moved; 7] = [
        Moved::View,
        Moved::Playhead,
        Moved::PoolHover,
        Moved::Filmstrip,
        Moved::Frame,
        Moved::Pointer,
        Moved::Drag,
    ];
}

/// The panels whose drawing reads nothing a change of each kind moves.
///
/// A panel not listed is redrawn by every narrow change, so a panel added to
/// the registry is redrawn until it is listed here. The Timeline is listed
/// for a playhead move by [`unmoved`] while its playhead layer holds.
/// `panel_view_tests` draws every registered panel after each kind of change
/// both ways, and fails when a listed panel's drawing differs from a full
/// redraw.
const UNMOVED: &[(PanelId, &[Moved])] = &[
    (
        PANEL_PROJECT,
        &[
            Moved::View,
            Moved::Playhead,
            Moved::Filmstrip,
            Moved::Frame,
            Moved::Pointer,
            Moved::Drag,
        ],
    ),
    (PANEL_EFFECTS, &Moved::ALL),
    (PANEL_AUDIO_METERS, &Moved::ALL),
    (
        PANEL_VIEWER,
        &[
            Moved::View,
            Moved::PoolHover,
            Moved::Filmstrip,
            Moved::Pointer,
            Moved::Drag,
        ],
    ),
    (
        PANEL_INSPECTOR,
        &[
            Moved::View,
            Moved::PoolHover,
            Moved::Filmstrip,
            Moved::Pointer,
            Moved::Drag,
        ],
    ),
    (PANEL_TIMELINE, &[Moved::PoolHover, Moved::Frame]),
];

/// Whether `panel` draws the same after a change that moved `moved`, with
/// `state` as it stands after the change.
pub(crate) fn unmoved(panel: PanelId, moved: Moved, state: &crate::app::AppState) -> bool {
    UNMOVED
        .iter()
        .any(|(listed, kinds)| *listed == panel && kinds.contains(&moved))
        || (panel == PANEL_TIMELINE
            && moved == Moved::Playhead
            && state.timeline_playhead.holds(state))
}

/// Notifies `entity` once the draw in progress ends.
///
/// A notification made while the window draws, such as from a canvas that
/// records the geometry it was given, marks no view dirty and reaches no
/// observer: the value it recorded would be drawn only when something else
/// redraws the window. Deferred, the notification redraws every view that
/// reads `entity` on the next frame.
pub fn notify_after_draw<T: 'static>(entity: &Entity<T>, cx: &mut App) {
    let entity = entity.downgrade();
    cx.defer(move |cx| {
        // A view dropped before the draw ended has nothing left to redraw.
        let _ = entity.update(cx, |_, cx| cx.notify());
    });
}

/// Redraws `shell`'s window as [`Shell::redraw`] does for a change that moved
/// only `moved`, once the draw in progress ends: [`notify_after_draw`] for a
/// value recorded during the draw that only the bodies `moved` redraws read.
pub fn redraw_after_draw(shell: WeakEntity<Shell>, moved: Moved, cx: &mut App) {
    cx.defer(move |cx| {
        // A window closed before the draw ended has nothing left to redraw.
        let _ = shell.update(cx, |shell, cx| shell.redraw(moved, cx));
    });
}

/// What a stamped panel body drew from: the theme, and the panel's inputs read
/// through the one function its drawing reads them through.
#[derive(Clone, PartialEq)]
enum PanelStamp {
    AudioMeters {
        theme: u32,
        /// `MeterReading::drawn`.
        reading: ([f32; 2], [f32; 2], [f32; 2], bool),
    },
    Effects {
        theme: u32,
        reduced: bool,
        inputs: crate::app::effects_browser::PanelInputs,
    },
}

fn panel_stamp(panel: PanelId, state: &crate::app::AppState, reduced: bool) -> Option<PanelStamp> {
    let theme = crate::theme::theme_generation();
    match panel {
        PANEL_AUDIO_METERS => Some(PanelStamp::AudioMeters {
            theme,
            reading: state.meter().drawn(),
        }),
        PANEL_EFFECTS => Some(PanelStamp::Effects {
            theme,
            reduced,
            inputs: state.effects_panel_inputs(),
        }),
        _ => None,
    }
}

/// The body of one docked panel.
pub(crate) struct PanelBody {
    shell: WeakEntity<Shell>,
    panel: PanelId,
    stamp: Option<PanelStamp>,
}

impl Render for PanelBody {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        builds::record(self.panel);
        let panel = self.panel;
        let shell = self.shell.clone();
        let reduced = cx.reduce_motion();
        self.stamp = shell
            .upgrade()
            .and_then(|s| panel_stamp(panel, s.read(cx).state.read(cx), reduced));
        let children = motion::build_view(cx.entity_id(), || {
            shell
                .update(cx, |shell, cx| shell.render_panel_body(panel, cx))
                .unwrap_or_default()
        });
        // Painted in a layer of its own. The window ranks what is painted in a
        // spatial index per layer, and an insert costs more the more that index
        // holds, so a body replayed on a frame where it did not change pays for
        // its own drawing rather than for every primitive painted before it.
        crate::frost::layered(
            div()
                .debug_selector(move || format!("panel-body-{panel}"))
                .flex()
                .flex_col()
                .size_full()
                .relative()
                .children(children),
        )
    }
}

/// A part of the window chrome drawn as a cached view, keyed by everything it
/// is drawn from besides the app state. A part whose key changes is drawn by a
/// new view, so it never replays a drawing made from other inputs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ChromePart {
    /// The editor bar: the menu titles and the tool strip, at rest.
    EditorBar {
        frame: WindowFrame,
        controls: WindowControls,
    },
    /// The tab row of one dock frame.
    TabRow(TabRow),
}

/// What a dock frame's tab row is drawn from besides the app state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TabRow {
    /// The frame as drawn: its registered tabs and the active one.
    pub(super) frame: Frame,
    pub(super) floating: bool,
    pub(super) maximized: bool,
    /// Whether the row's overflow menu is open.
    pub(super) overflow: bool,
    /// The window's frame, which a floating window's row moves the window by.
    pub(super) window: WindowFrame,
}

/// A chrome part's view, and whether the frame being built mounted it.
pub(crate) struct PartSlot {
    part: ChromePart,
    view: Entity<ChromeView>,
    drawn: bool,
}

/// The view of one [`ChromePart`].
pub(crate) struct ChromeView {
    shell: WeakEntity<Shell>,
    part: ChromePart,
}

impl Render for ChromeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        builds::record_part(&self.part);
        let part = &self.part;
        let shell = self.shell.clone();
        let built = motion::build_view(cx.entity_id(), || {
            shell
                .update(cx, |shell, cx| shell.render_part(part, window, cx))
                .ok()
        });
        // Painted in a layer of its own, as a panel body is.
        crate::frost::layered(div().size_full().children(built))
    }
}

/// A view with nothing in it, mounted in the shell so that notifying it redraws
/// the window chrome without notifying the shell.
pub(crate) struct Beacon;

impl Render for Beacon {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

impl Shell {
    /// The body of `panel` as a cached view, created on its first draw.
    pub(super) fn panel_body(&mut self, panel: PanelId, cx: &mut Context<Self>) -> AnyElement {
        let shell = cx.entity().downgrade();
        let body = self
            .bodies
            .entry(panel)
            .or_insert_with(|| {
                cx.new(|_| PanelBody {
                    shell,
                    panel,
                    stamp: None,
                })
            })
            .clone();
        motion::mount_view(body.entity_id());
        AnyView::from(body)
            .cached(StyleRefinement::default().size_full())
            .into_any_element()
    }

    /// Notifies every chrome part and every panel body whose inputs may have
    /// changed since its last draw. While the editor arrives, the shell draws
    /// every body under an opacity that moves each frame and that no stamp
    /// holds, so every body is notified.
    pub(crate) fn redraw_bodies(&mut self, cx: &mut Context<Self>) {
        self.drawn_projections = self.state.read(cx).projections;
        let reduced = cx.reduce_motion();
        let arrived = self.chrome_cached;
        for (&panel, body) in &self.bodies {
            let unchanged = arrived && {
                let state = self.state.read(cx);
                panel_stamp(panel, state, reduced)
                    .is_some_and(|now| body.read(cx).stamp.as_ref() == Some(&now))
            };
            if !unchanged {
                body.update(cx, |_, cx| cx.notify());
            }
        }
        self.redraw_parts(cx);
    }

    /// Notifies every chrome part.
    fn redraw_parts(&self, cx: &mut Context<Self>) {
        for slot in &self.parts {
            slot.view.update(cx, |_, cx| cx.notify());
        }
    }

    /// Redraws the window after a change that moved only `moved`: the chrome
    /// the shell draws itself and the panel bodies that read it. The chrome
    /// parts read nothing a narrow change moves, and they replay their last
    /// drawing with the rest.
    ///
    /// A change that also re-projected the timeline redraws every body, so a
    /// gesture that wrote to the project while it moved the view is drawn in
    /// full.
    pub fn redraw(&mut self, moved: Moved, cx: &mut Context<Self>) {
        if self.state.read(cx).projections != self.drawn_projections {
            self.redraw_bodies(cx);
        } else {
            for (panel, body) in &self.bodies {
                if !unmoved(panel, moved, self.state.read(cx)) {
                    body.update(cx, |_, cx| cx.notify());
                }
            }
        }
        self.beacon.update(cx, |_, cx| cx.notify());
    }

    /// Redraws the window chrome after a change to chrome alone, such as a
    /// menu flyout opening. Every panel body replays its last drawing.
    pub fn redraw_chrome(&self, cx: &mut Context<Self>) {
        self.redraw_parts(cx);
        self.beacon.update(cx, |_, cx| cx.notify());
    }

    /// Drops the views of the chrome parts the last frame did not mount, and
    /// opens a frame that has mounted none.
    pub(super) fn begin_parts(&mut self) {
        self.parts.retain(|slot| slot.drawn);
        for slot in &mut self.parts {
            slot.drawn = false;
        }
    }

    /// The cached view of `part` at the size `style` gives it, created on its
    /// first draw.
    fn part_view(
        &mut self,
        part: ChromePart,
        style: StyleRefinement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = match self.parts.iter_mut().find(|slot| slot.part == part) {
            Some(slot) => {
                slot.drawn = true;
                slot.view.clone()
            }
            None => {
                let shell = cx.entity().downgrade();
                let view = cx.new(|_| ChromeView {
                    shell,
                    part: part.clone(),
                });
                self.parts.push(PartSlot {
                    part,
                    view: view.clone(),
                    drawn: true,
                });
                view
            }
        };
        motion::mount_view(view.entity_id());
        AnyView::from(view).cached(style).into_any_element()
    }

    /// The editor bar: a cached part at rest, and built in place while the
    /// editor arrives, while a menu drop is drawn under a title, and while a
    /// tool's variants are open under the strip.
    pub(super) fn editor_bar(
        &mut self,
        theme: &Theme,
        arrived: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let drop = BarDrop::of(&self.state, cx);
        let over = drop.is_drawn() || self.state.read(cx).tool_variants_open().is_some();
        if over || !self.chrome_cached {
            let bar = crate::chrome::editor_bar(theme, &self.state, drop, window, cx);
            return super::opening(0, arrived, bar);
        }
        let part = ChromePart::EditorBar {
            frame: WindowFrame::of(window),
            controls: window.window_controls(),
        };
        let style = StyleRefinement::default()
            .w_full()
            .h(px(EDITOR_BAR_H))
            .flex_none();
        self.part_view(part, style, cx)
    }

    /// The tab row of the frame `row` draws.
    pub(super) fn tab_row(
        &mut self,
        row: TabRow,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.chrome_cached {
            return self.render_tab_bar(&row, theme, cx);
        }
        let style = StyleRefinement::default()
            .w_full()
            .h(px(Theme::TAB_ROW_HEIGHT))
            .flex_none();
        self.part_view(ChromePart::TabRow(row), style, cx)
    }

    /// Builds `part`, for its cached view.
    fn render_part(
        &mut self,
        part: &ChromePart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx);
        match part {
            // Cached only while no drop is drawn, so the bar draws none.
            ChromePart::EditorBar { .. } => {
                let state = self.state.clone();
                crate::chrome::editor_bar(&theme, &state, BarDrop::default(), window, cx)
                    .into_any_element()
            }
            ChromePart::TabRow(row) => self.render_tab_bar(row, &theme, cx),
        }
    }

    /// Whether the overflow menu of `frame` is open.
    pub(super) fn overflow_open(&self, frame: crate::dock::FrameId) -> bool {
        self.frame_menu == Some(FrameMenu::Overflow(frame))
    }

    /// The beacon, for the shell to mount.
    pub(super) fn beacon(&self) -> Entity<Beacon> {
        self.beacon.clone()
    }
}

/// Which panel bodies and chrome parts were built, for a test that holds a
/// replayed one to zero work.
#[cfg(test)]
pub(crate) mod builds {
    use std::cell::RefCell;

    use super::ChromePart;
    use crate::dock::{FrameId, PanelId};

    /// A chrome part as a test names it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub(crate) enum Part {
        EditorBar,
        TabRow(FrameId),
    }

    thread_local! {
        static BUILT: RefCell<Vec<PanelId>> = const { RefCell::new(Vec::new()) };
        static PARTS: RefCell<Vec<Part>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn record(panel: PanelId) {
        BUILT.with_borrow_mut(|built| built.push(panel));
    }

    pub(super) fn record_part(part: &ChromePart) {
        let part = match part {
            ChromePart::EditorBar { .. } => Part::EditorBar,
            ChromePart::TabRow(row) => Part::TabRow(row.frame.id),
        };
        PARTS.with_borrow_mut(|built| built.push(part));
    }

    /// The panels built since the last call, in build order.
    pub(crate) fn take() -> Vec<PanelId> {
        BUILT.with_borrow_mut(std::mem::take)
    }

    /// The chrome parts built since the last call, in build order.
    pub(crate) fn take_parts() -> Vec<Part> {
        PARTS.with_borrow_mut(std::mem::take)
    }
}
