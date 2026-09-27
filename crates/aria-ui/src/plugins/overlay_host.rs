//! Where a dialog or an overlay comes from.
//!
//! Panels resolve from [`crate::plugins::panel_host`], but the shell used to name its
//! nine dialog and overlay modules directly, so a plugin could not add a dialog
//! at all. An overlay is an entry in this registry instead: the shipped ones
//! register through the same call a plugin makes, and the shell asks by what
//! makes the overlay visible.
//!
//! A plugin's own dialog is [`crate::app::DialogKind::Plugin`], keyed by the id
//! it registered, so opening one is the same `open_dialog` call a shipped dialog
//! takes and only one dialog is ever on screen.

use std::time::Duration;

use aria_plugin::{PluginContext, Service};
use gpui::motion::{FrameInstant, MotionPolicy, Timestamp as _, presets};
use gpui::{AnyElement, Entity};
use parking_lot::RwLock;

use crate::app::{AppState, DialogKind};
use crate::shell::Shell;

/// How an overlay draws itself. A plain function pointer for the same reason a
/// panel's is: the overlay's state lives in `AppState`, not in the registry.
pub type OverlayRender = fn(&Entity<AppState>, &mut gpui::Context<Shell>) -> AnyElement;

/// What puts an overlay on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayWhen {
    /// `AppState::dialog` names this kind. One dialog is visible at a time.
    Dialog(DialogKind),
    /// A context menu is open.
    ContextMenu,
    /// A rename is in progress.
    Rename,
}

/// One contributed overlay.
pub struct OverlayEntry {
    pub id: &'static str,
    pub when: OverlayWhen,
    pub render: OverlayRender,
}

/// Every overlay the shell can draw, in registration order.
#[derive(Default)]
pub struct OverlayHost {
    entries: RwLock<Vec<OverlayEntry>>,
    presence: RwLock<Vec<Presence>>,
}

/// One condition the shell is currently drawing, and whether it has gone false.
///
/// An overlay that vanished on the frame its condition cleared had no exit to
/// play. The shell asks about every condition on every frame and says whether it
/// holds, so the host can keep handing back a leaving overlay until
/// [`presets::MENU_OUT`] is spent. The shell has no animation state, and the
/// overlay's own render draws what leaving looks like.
///
/// Time is the open motion frame's instant, so the exit agrees with every other
/// value the frame draws.
struct Presence {
    when: OverlayWhen,
    /// `None` while the condition holds; the frame it went false otherwise.
    leaving_since: Option<FrameInstant>,
}

/// How long an overlay keeps drawing after its condition went false, under
/// `policy`: [`presets::MENU_OUT`] at the policy's duration scale, and nothing
/// under reduced motion, where the overlay leaves on the frame it closed.
fn exit_window(policy: MotionPolicy) -> Duration {
    if policy.reduced() {
        return Duration::ZERO;
    }
    policy.scale(presets::MENU_OUT.total())
}

/// Whether two conditions occupy the same slot. Every `Dialog` shares one, so
/// the host can find the dialog that is leaving without being told its kind —
/// by then `AppState::dialog` is [`DialogKind::None`] and the kind is gone.
fn same_slot(a: OverlayWhen, b: OverlayWhen) -> bool {
    match (a, b) {
        (OverlayWhen::Dialog(_), OverlayWhen::Dialog(_)) => true,
        _ => a == b,
    }
}

impl Service for OverlayHost {}

impl OverlayHost {
    /// Add an overlay. A second registration of the same id replaces the first,
    /// so a plugin can override a shipped dialog rather than draw a second one
    /// on top of it.
    pub fn add(&self, entry: OverlayEntry) {
        let mut entries = self.entries.write();
        if let Some(existing) = entries.iter_mut().find(|held| held.id == entry.id) {
            *existing = entry;
            return;
        }
        entries.push(entry);
    }

    /// The dialog to draw this frame, or `None`. First registration wins: a
    /// dialog is exclusive, so a second one for the same kind would have to be
    /// drawn on top of the first, which is not a dialog any more.
    ///
    /// `active` is whether a dialog is open at all. `false` is not the same as
    /// nothing to draw: for one `exit_window` afterwards the dialog that just
    /// closed is still the answer, so it can play its exit. [`Self::exit_t`]
    /// says how far through that it is.
    pub fn dialog(&self, kind: DialogKind, active: bool) -> Option<OverlayRender> {
        let drawn = self.mark(OverlayWhen::Dialog(kind), active)?;
        self.entries
            .read()
            .iter()
            .find(|entry| entry.when == drawn)
            .map(|entry| entry.render)
    }

    /// Every overlay for a non-dialog condition, in registration order. These do
    /// stack: a plugin adding to the rename bar's condition draws beside it.
    /// `active` carries the same meaning it does for [`Self::dialog`].
    pub fn matching(&self, when: OverlayWhen, active: bool) -> Vec<OverlayRender> {
        let Some(drawn) = self.mark(when, active) else {
            return Vec::new();
        };
        self.entries
            .read()
            .iter()
            .filter(|entry| entry.when == drawn)
            .map(|entry| entry.render)
            .collect()
    }

    /// Fold this frame's condition into the presence record and name the
    /// condition that should be drawn, which is `when` itself while the
    /// condition holds and the condition that is still leaving otherwise.
    fn mark(&self, when: OverlayWhen, active: bool) -> Option<OverlayWhen> {
        let mut presence = self.presence.write();
        if active {
            // A dialog arriving over one that is leaving replaces it outright:
            // two cards crossfading over one scrim reads as a glitch, not depth.
            presence.retain(|held| !same_slot(held.when, when));
            presence.push(Presence {
                when,
                leaving_since: None,
            });
            return Some(when);
        }
        let index = presence
            .iter()
            .position(|held| same_slot(held.when, when))?;
        let frame = crate::motion::frame();
        let since = *presence[index].leaving_since.get_or_insert(frame.now());
        if frame.now().seconds_since(since) >= exit_window(frame.policy()).as_secs_f32() {
            presence.remove(index);
            return None;
        }
        Some(presence[index].when)
    }

    /// How far the overlay in `when`'s slot is through leaving, 0 at the start
    /// and 1 at the end, or `None` when it is arriving or settled.
    ///
    /// An overlay's own render asks this to choose between an entry and an exit.
    pub fn exit_t(&self, when: OverlayWhen) -> Option<f32> {
        let presence = self.presence.read();
        let since = presence
            .iter()
            .find(|held| same_slot(held.when, when))?
            .leaving_since?;
        let frame = crate::motion::frame();
        let window = exit_window(frame.policy()).as_secs_f32();
        if window <= 0.0 {
            return Some(1.0);
        }
        Some((frame.now().seconds_since(since) / window).clamp(0.0, 1.0))
    }

    /// Every registered id, in order. For the Plugins list and the tests.
    pub fn ids(&self) -> Vec<&'static str> {
        self.entries.read().iter().map(|entry| entry.id).collect()
    }

    /// The kinds a dialog is registered for, in registration order.
    pub fn dialog_kinds(&self) -> Vec<DialogKind> {
        self.entries
            .read()
            .iter()
            .filter_map(|entry| match entry.when {
                OverlayWhen::Dialog(kind) => Some(kind),
                _ => None,
            })
            .collect()
    }
}

/// The registry the shell reads, or `None` when no host mounted it.
///
/// A missing registry is not an error: a headless `AppState` draws nothing.
pub fn host_of(ctx: &PluginContext) -> Option<std::sync::Arc<OverlayHost>> {
    ctx.try_get::<OverlayHost>()
}

/// How far the overlay in `when`'s slot is through leaving, for a render that
/// holds the state rather than the host.
pub fn exit_t(when: OverlayWhen, state: &Entity<AppState>, cx: &gpui::App) -> Option<f32> {
    host_of(&state.read(cx).ctx)?.exit_t(when)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing(_: &Entity<AppState>, _: &mut gpui::Context<Shell>) -> AnyElement {
        unreachable!("a test never draws")
    }

    #[test]
    fn a_dialog_resolves_by_kind_and_an_unregistered_kind_does_not() {
        let _frames = crate::motion::TestFrames::new();
        let host = OverlayHost::default();
        host.add(OverlayEntry {
            id: "export",
            when: OverlayWhen::Dialog(DialogKind::Export),
            render: nothing,
        });
        assert!(host.dialog(DialogKind::Export, true).is_some());
        assert!(host.dialog(DialogKind::Settings, true).is_none());
        // No dialog open, and none leaving either.
        assert!(host.dialog(DialogKind::None, false).is_none());
        assert_eq!(host.dialog_kinds(), vec![DialogKind::Export]);
    }

    #[test]
    fn a_second_registration_of_an_id_replaces_it() {
        let host = OverlayHost::default();
        host.add(OverlayEntry {
            id: "rename",
            when: OverlayWhen::Rename,
            render: nothing,
        });
        host.add(OverlayEntry {
            id: "rename",
            when: OverlayWhen::ContextMenu,
            render: nothing,
        });
        assert_eq!(host.ids(), vec!["rename"]);
        assert_eq!(host.matching(OverlayWhen::Rename, true).len(), 0);
        assert_eq!(host.matching(OverlayWhen::ContextMenu, true).len(), 1);
    }

    #[test]
    fn a_plugin_dialog_is_keyed_by_its_own_id() {
        let host = OverlayHost::default();
        host.add(OverlayEntry {
            id: "notes",
            when: OverlayWhen::Dialog(DialogKind::Plugin("notes")),
            render: nothing,
        });
        assert!(host.dialog(DialogKind::Plugin("notes"), true).is_some());
        assert!(host.dialog(DialogKind::Plugin("other"), true).is_none());
    }

    /// The exit window is what gives a dialog somewhere to animate: the frame
    /// after it closed still resolves to it, and the progress it reports is what
    /// the render turns into a fade.
    #[test]
    fn a_closed_dialog_keeps_drawing_until_its_exit_is_spent() {
        let frames = crate::motion::TestFrames::new();
        let host = OverlayHost::default();
        host.add(OverlayEntry {
            id: "export",
            when: OverlayWhen::Dialog(DialogKind::Export),
            render: nothing,
        });
        assert!(host.dialog(DialogKind::Export, true).is_some());
        assert!(
            host.exit_t(OverlayWhen::Dialog(DialogKind::Export))
                .is_none()
        );

        // Closed: the kind is gone from the state, and the slot still answers.
        assert!(host.dialog(DialogKind::None, false).is_some());
        let progress = host
            .exit_t(OverlayWhen::Dialog(DialogKind::None))
            .expect("a leaving dialog reports progress");
        assert!(
            (0.0..1.0).contains(&progress),
            "{progress} should be a fade"
        );

        // Halfway through the exit the slot still answers, further along.
        frames.advance(presets::MENU_OUT.total() / 2);
        assert!(host.dialog(DialogKind::None, false).is_some());
        let halfway = host
            .exit_t(OverlayWhen::Dialog(DialogKind::None))
            .expect("a leaving dialog reports progress");
        assert!(
            progress < halfway && halfway < 1.0,
            "{halfway} should be further through the fade than {progress}"
        );

        // A dialog arriving over the leaving one takes the slot outright.
        assert!(host.dialog(DialogKind::Export, true).is_some());
        assert!(
            host.exit_t(OverlayWhen::Dialog(DialogKind::Export))
                .is_none()
        );
    }

    /// The exit ends: once `MENU_OUT` is spent, the closed dialog is dropped
    /// and nothing is drawn for its slot.
    #[test]
    fn a_closed_dialog_stops_drawing_once_its_exit_is_spent() {
        let frames = crate::motion::TestFrames::new();
        let host = OverlayHost::default();
        host.add(OverlayEntry {
            id: "export",
            when: OverlayWhen::Dialog(DialogKind::Export),
            render: nothing,
        });
        assert!(host.dialog(DialogKind::Export, true).is_some());
        assert!(host.dialog(DialogKind::None, false).is_some());
        frames.advance(presets::MENU_OUT.total());
        assert!(host.dialog(DialogKind::None, false).is_none());
        assert!(host.exit_t(OverlayWhen::Dialog(DialogKind::None)).is_none());
    }

    /// Reduced motion draws the end state on the frame the change happened: a
    /// closed dialog is gone on that frame, with no exit to play.
    #[test]
    fn under_reduced_motion_a_closed_dialog_is_gone_on_the_frame_it_closed() {
        let frames = crate::motion::TestFrames::new();
        frames.update(|cx| cx.set_reduce_motion(true));
        let host = OverlayHost::default();
        host.add(OverlayEntry {
            id: "export",
            when: OverlayWhen::Dialog(DialogKind::Export),
            render: nothing,
        });
        assert!(host.dialog(DialogKind::Export, true).is_some());
        assert!(host.dialog(DialogKind::None, false).is_none());
    }

    /// A condition that was never on screen has nothing to animate out, so an
    /// inactive frame must not conjure one.
    #[test]
    fn a_condition_that_never_held_draws_nothing_when_it_goes_false() {
        let _frames = crate::motion::TestFrames::new();
        let host = OverlayHost::default();
        host.add(OverlayEntry {
            id: "context",
            when: OverlayWhen::ContextMenu,
            render: nothing,
        });
        assert!(host.matching(OverlayWhen::ContextMenu, false).is_empty());
        assert_eq!(host.matching(OverlayWhen::ContextMenu, true).len(), 1);
        assert_eq!(host.matching(OverlayWhen::ContextMenu, false).len(), 1);
    }
}
