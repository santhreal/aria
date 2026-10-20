//! The boot cover: what it is doing, when it stops, and what draws it.
//!
//! Split out of `shell.rs` when that file reached its ceiling. The shell holds
//! the field and draws what this decides; every decision about the cover is
//! here, and the decision itself is a pure function so it can be asserted
//! without a window.

use super::*;

/// What the boot splash is doing.
///
/// A wall clock used to hold it for a fixed 1100ms whether the editor was ready
/// in forty milliseconds or nine hundred, so a fast start was slowed by the wait
/// and a slow one was uncovered when the wall ran out. Readiness is what ends it
/// now: the session is built before the window opens, so Home is ready on its
/// first frame, and a project opened from the command line is ready when its
/// first picture has been decoded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Splash {
    /// Covering the window since this instant, waiting on readiness. Bounded by
    /// [`SPLASH_MAX`], so a decode that never lands cannot hold the cover.
    Covering(Instant),
    /// Ready; the cover is fading from this instant.
    Fading(Instant),
    /// Gone, and not drawn again for the life of the window.
    Done,
}

/// The longest the cover is held while waiting for a first picture.
///
/// A ceiling rather than a target: a missing file, a codec the host cannot
/// decode, or a device that has gone away all end with no picture arriving, and
/// the window should show the editor and its error, not a splash.
///
/// Public because it is the startup budget: the desktop crate's startup test
/// measures a cold and a warm start against it rather than restating a number
/// that would then drift from the one the cover uses.
pub const SPLASH_MAX: Duration = Duration::from_millis(2_000);

/// Where the cover goes this frame, and whether to draw it fading.
///
/// The whole rule, with no window and no clock of its own: `Some(false)` is the
/// cover held over an editor that has nothing to show yet, `Some(true)` is the
/// cover on its way out, and `None` is nothing to draw. Under reduced motion the
/// cover does not fade, so it goes straight from covering to gone.
pub(super) fn step(
    splash: Splash,
    ready: bool,
    reduced: bool,
    now: Instant,
) -> (Splash, Option<bool>) {
    match splash {
        Splash::Done => (Splash::Done, None),
        Splash::Covering(since) => {
            if !ready && now.saturating_duration_since(since) < SPLASH_MAX {
                (Splash::Covering(since), Some(false))
            } else if reduced {
                (Splash::Done, None)
            } else {
                (Splash::Fading(now), Some(true))
            }
        }
        Splash::Fading(since) => {
            if now.saturating_duration_since(since) >= gpui::motion::presets::SPLASH_OUT.total() {
                (Splash::Done, None)
            } else {
                (Splash::Fading(since), Some(true))
            }
        }
    }
}

impl Shell {
    /// The cover to draw over this frame, and the state change that goes with
    /// drawing it.
    ///
    /// Asks for the next frame whenever there is still a cover, because an
    /// opacity animation nobody repaints stops where the last frame left it, and
    /// a cover waiting on a decode has to notice the decode landing.
    pub(super) fn splash_frame(
        &mut self,
        theme: &Theme,
        reduced: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let (next, draw) = step(self.splash, self.ready(cx), reduced, Instant::now());
        self.splash = next;
        let fading = draw?;
        window.request_animation_frame();
        Some(crate::loaders::splash_overlay(theme, fading))
    }

    /// Whether the editor has something true behind the cover.
    ///
    /// Home is ready as soon as it draws: its list is held on the state and the
    /// session was built before the window opened. The editor is ready when the
    /// first picture has been decoded, because a project opened from the command
    /// line draws an empty monitor until then.
    fn ready(&self, cx: &Context<Self>) -> bool {
        let state = self.state.read(cx);
        state.route != UiRoute::Editor
            || state.last_render.is_some()
            || state.preview_worker.shown_texture().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cover ends because the editor is ready, not because a clock ran out.
    /// Asserted as the transition it is: held while unready, fading on the frame
    /// readiness arrives, and gone once the fade's own duration has passed.
    #[test]
    fn the_cover_waits_for_readiness_and_then_fades_once() {
        let start = Instant::now();
        let (held, draw) = step(Splash::Covering(start), false, false, start);
        assert_eq!(
            held,
            Splash::Covering(start),
            "an unready editor stays covered"
        );
        assert_eq!(draw, Some(false), "and the cover is not fading yet");

        let ready_at = start + Duration::from_millis(40);
        let (fading, draw) = step(held, true, false, ready_at);
        assert_eq!(
            fading,
            Splash::Fading(ready_at),
            "readiness starts the fade"
        );
        assert_eq!(draw, Some(true));

        let mid = ready_at + gpui::motion::presets::SPLASH_OUT.total() / 2;
        assert_eq!(
            step(fading, true, false, mid),
            (Splash::Fading(ready_at), Some(true)),
            "a fade in flight keeps its own start"
        );

        let after = ready_at + gpui::motion::presets::SPLASH_OUT.total();
        assert_eq!(
            step(fading, true, false, after),
            (Splash::Done, None),
            "the fade ends when its duration has run"
        );
        assert_eq!(
            step(Splash::Done, true, false, after),
            (Splash::Done, None),
            "and the cover never comes back"
        );
    }

    /// A picture that never arrives must not hold the window: the ceiling ends
    /// the cover even though the editor is still unready.
    #[test]
    fn a_decode_that_never_lands_cannot_hold_the_cover() {
        let start = Instant::now();
        let before = start + SPLASH_MAX - Duration::from_millis(1);
        assert_eq!(
            step(Splash::Covering(start), false, false, before),
            (Splash::Covering(start), Some(false))
        );
        let at = start + SPLASH_MAX;
        let (next, draw) = step(Splash::Covering(start), false, false, at);
        assert_eq!(next, Splash::Fading(at), "the ceiling starts the fade");
        assert_eq!(draw, Some(true));
    }

    /// Reduced motion asks for no animation, so there is no fade to draw: the
    /// cover is gone on the frame the editor is ready, not a fade later.
    #[test]
    fn reduced_motion_removes_the_cover_instead_of_fading_it() {
        let start = Instant::now();
        assert_eq!(
            step(Splash::Covering(start), true, true, start),
            (Splash::Done, None)
        );
        assert_eq!(
            step(Splash::Covering(start), false, true, start),
            (Splash::Covering(start), Some(false)),
            "reduced motion is not a reason to uncover an unready editor"
        );
    }
}
