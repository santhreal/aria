//! Docked panel bodies drawn as cached views.
//!
//! Each frame's body is a view of its own, mounted through
//! [`gpui::AnyView::cached`]: the frame lays the body out as one box, and the
//! body is built, laid out and painted again only when its view is notified.
//! Otherwise the window replays the body's last drawing at the same bounds.
//!
//! Every notification of the shell notifies every body
//! ([`Shell::redraw_bodies`], run by the shell's own observer), so a change
//! made anywhere is drawn everywhere. Two paths are narrow: [`Shell::redraw`],
//! for a change that moves only the timeline view, the playhead, or the pool
//! tile under the pointer, redraws the window chrome and the bodies whose
//! drawing reads what moved, and [`Shell::redraw_chrome`], for a change to
//! chrome alone, redraws the chrome. The rest replay their last drawing.

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _,
    Render, StyleRefinement, Styled as _, WeakEntity, Window, div,
};

use crate::dock::{
    PANEL_AUDIO_METERS, PANEL_EFFECTS, PANEL_INSPECTOR, PANEL_PROJECT, PANEL_TIMELINE,
    PANEL_VIEWER, PanelId,
};
use crate::motion;

use super::Shell;

#[cfg(test)]
#[path = "panel_view_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "panel_view_idle_tests.rs"]
mod idle_tests;

#[cfg(test)]
#[path = "panel_view_hover_tests.rs"]
mod hover_tests;

/// What a narrow change moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    /// The timeline's horizontal scroll, zoom, or lane scroll.
    View,
    /// The playhead.
    Playhead,
    /// A media tile's scrub position, or a tile picture arriving
    /// ([`crate::app::PoolTileChanged`]).
    PoolHover,
    /// A timeline filmstrip cell arriving ([`crate::app::FilmstripChanged`]).
    Filmstrip,
}

impl Moved {
    pub const ALL: [Moved; 4] = [
        Moved::View,
        Moved::Playhead,
        Moved::PoolHover,
        Moved::Filmstrip,
    ];
}

/// The panels whose drawing reads nothing a change of each kind moves.
///
/// A panel not listed is redrawn by every narrow change, so a panel added to
/// the registry is redrawn until it is listed here.
/// `panel_view_tests` draws every registered panel after each kind of change
/// both ways, and fails when a listed panel's drawing differs from a full
/// redraw.
const UNMOVED: &[(PanelId, &[Moved])] = &[
    (
        PANEL_PROJECT,
        &[Moved::View, Moved::Playhead, Moved::Filmstrip],
    ),
    (PANEL_EFFECTS, &Moved::ALL),
    (PANEL_AUDIO_METERS, &Moved::ALL),
    (
        PANEL_VIEWER,
        &[Moved::View, Moved::PoolHover, Moved::Filmstrip],
    ),
    (
        PANEL_INSPECTOR,
        &[Moved::View, Moved::PoolHover, Moved::Filmstrip],
    ),
    (PANEL_TIMELINE, &[Moved::PoolHover]),
];

/// Whether `panel` draws the same after a change that moved `moved`.
pub(crate) fn unmoved(panel: PanelId, moved: Moved) -> bool {
    UNMOVED
        .iter()
        .any(|(listed, kinds)| *listed == panel && kinds.contains(&moved))
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

#[derive(Clone, PartialEq)]
enum PanelStamp {
    AudioMeters {
        theme: u32,
        rms: [u32; 2],
        peaks: [u32; 2],
        hold: [u32; 2],
        clipped: bool,
    },
    Effects {
        theme: u32,
        reduced: bool,
        entries: usize,
        presets: Vec<crate::app::effects_browser::Entry>,
        failures: Vec<crate::app::effects_browser::Failed>,
        browser: crate::app::effects_browser::EffectsBrowser,
        focused: bool,
        favourites: Vec<String>,
    },
}

fn panel_stamp(panel: PanelId, state: &crate::app::AppState, reduced: bool) -> Option<PanelStamp> {
    let theme = crate::theme::theme_generation();
    match panel {
        PANEL_AUDIO_METERS => {
            let reading = state.meter();
            Some(PanelStamp::AudioMeters {
                theme,
                rms: reading.rms.map(f32::to_bits),
                peaks: reading.peaks.map(f32::to_bits),
                hold: reading.hold.map(f32::to_bits),
                clipped: reading.clipped,
            })
        }
        PANEL_EFFECTS => Some(PanelStamp::Effects {
            theme,
            reduced,
            entries: std::sync::Arc::as_ptr(&crate::catalog_text::effect_entries()) as *const ()
                as usize,
            presets: state.effects_preset_entries(),
            failures: state.effects_failures(),
            browser: state.effects_browser.clone(),
            focused: state.effects_search_focused(),
            favourites: state.effect_favourites().to_vec(),
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
                .flex()
                .flex_col()
                .size_full()
                .relative()
                .children(children),
        )
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

    /// Notifies every panel body whose inputs may have changed since its last draw.
    pub(crate) fn redraw_bodies(&mut self, cx: &mut Context<Self>) {
        self.drawn_projections = self.state.read(cx).projections;
        let reduced = cx.reduce_motion();
        for (&panel, body) in &self.bodies {
            let unchanged = {
                let state = self.state.read(cx);
                panel_stamp(panel, state, reduced)
                    .is_some_and(|now| body.read(cx).stamp.as_ref() == Some(&now))
            };
            if !unchanged {
                body.update(cx, |_, cx| cx.notify());
            }
        }
    }

    /// Redraws the window after a change that moved only `moved`: the chrome
    /// and the panel bodies that read it. The rest replay their last drawing.
    ///
    /// A change that also re-projected the timeline redraws every body, so a
    /// gesture that wrote to the project while it moved the view is drawn in
    /// full.
    pub fn redraw(&mut self, moved: Moved, cx: &mut Context<Self>) {
        if self.state.read(cx).projections != self.drawn_projections {
            self.redraw_bodies(cx);
        } else {
            for (panel, body) in &self.bodies {
                if !unmoved(panel, moved) {
                    body.update(cx, |_, cx| cx.notify());
                }
            }
        }
        self.beacon.update(cx, |_, cx| cx.notify());
    }

    /// Redraws the window chrome after a change to chrome alone, such as a
    /// menu flyout opening. Every panel body replays its last drawing.
    pub fn redraw_chrome(&self, cx: &mut Context<Self>) {
        self.beacon.update(cx, |_, cx| cx.notify());
    }

    /// The beacon, for the shell to mount.
    pub(super) fn beacon(&self) -> Entity<Beacon> {
        self.beacon.clone()
    }
}

/// Which panel bodies were built, for a test that holds a replayed body to
/// zero work.
#[cfg(test)]
pub(crate) mod builds {
    use std::cell::RefCell;

    use crate::dock::PanelId;

    thread_local! {
        static BUILT: RefCell<Vec<PanelId>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn record(panel: PanelId) {
        BUILT.with_borrow_mut(|built| built.push(panel));
    }

    /// The panels built since the last call, in build order.
    pub(crate) fn take() -> Vec<PanelId> {
        BUILT.with_borrow_mut(std::mem::take)
    }
}
