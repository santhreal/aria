//! Alt pressed and released with nothing between: the second way to open the
//! menu bar, beside F10.
//!
//! Alt with a letter is a property reveal, and Alt with a click, a drag, or a
//! wheel is a gesture, so only an Alt that did nothing else opens the bar. A
//! platform never sends a key-down for a bare modifier: the press and the
//! release arrive as modifier changes, and every key, click, and wheel the shell
//! sees in between spoils the press.

use gpui::Modifiers;

/// Where one Alt press stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LoneAlt {
    /// Alt is up.
    #[default]
    Idle,
    /// Alt is down alone and nothing else has happened yet.
    Armed,
    /// Alt is down, but with another modifier or after other input.
    Spoiled,
}

impl LoneAlt {
    /// Feed one modifier change. `true` when it completes a lone Alt.
    pub(crate) fn modifiers(&mut self, modifiers: &Modifiers) -> bool {
        let alone = modifiers.alt
            && !modifiers.control
            && !modifiers.shift
            && !modifiers.platform
            && !modifiers.function;
        let (next, fired) = match (*self, modifiers.alt) {
            (Self::Idle | Self::Armed, true) if alone => (Self::Armed, false),
            (_, true) => (Self::Spoiled, false),
            (Self::Armed, false) => (Self::Idle, !modifiers.modified()),
            (_, false) => (Self::Idle, false),
        };
        *self = next;
        fired
    }

    /// A key, click, or wheel arrived while Alt was down: the press was part of
    /// a chord or a gesture.
    pub(crate) fn spoil(&mut self) {
        if *self == Self::Armed {
            *self = Self::Spoiled;
        }
    }

    /// The window lost focus. Alt+Tab leaves Alt down here and releases it in
    /// another window, so the release that arrives on return is not this press.
    pub(crate) fn reset(&mut self) {
        *self = Self::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::LoneAlt;
    use gpui::Modifiers;

    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Modifiers::default()
        }
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }

    /// Feed a run of modifier states and count the lone Alts they complete.
    fn fired(states: &[Modifiers]) -> usize {
        let mut alt = LoneAlt::default();
        states.iter().filter(|m| alt.modifiers(m)).count()
    }

    #[test]
    fn alt_down_and_up_alone_fires_once() {
        assert_eq!(fired(&[alt(), none()]), 1);
        assert_eq!(
            fired(&[alt(), alt(), none(), none()]),
            1,
            "a repeat is still one press"
        );
        assert_eq!(
            fired(&[alt(), none(), alt(), none()]),
            2,
            "two presses open and close"
        );
    }

    #[test]
    fn every_other_modifier_spoils_the_press() {
        let others: [fn(&mut Modifiers); 4] = [
            |m| m.control = true,
            |m| m.shift = true,
            |m| m.platform = true,
            |m| m.function = true,
        ];
        for (at, set) in others.into_iter().enumerate() {
            let mut with = alt();
            set(&mut with);
            let mut only = none();
            set(&mut only);
            // Alt first, then the other key, then both released in either order.
            assert_eq!(
                fired(&[alt(), with, alt(), none()]),
                0,
                "modifier {at} after Alt"
            );
            assert_eq!(
                fired(&[alt(), with, only, none()]),
                0,
                "modifier {at} released last"
            );
            // The other key first.
            assert_eq!(
                fired(&[only, with, only, none()]),
                0,
                "modifier {at} before Alt"
            );
        }
    }

    #[test]
    fn a_key_click_or_wheel_between_spoils_the_press() {
        let mut alt_state = LoneAlt::default();
        assert!(!alt_state.modifiers(&alt()));
        alt_state.spoil();
        assert!(!alt_state.modifiers(&none()), "Alt+key is a chord");
        assert_eq!(alt_state, LoneAlt::Idle, "the next press starts clean");
        assert!(!alt_state.modifiers(&alt()));
        assert!(
            alt_state.modifiers(&none()),
            "a clean press after a spoiled one fires"
        );
    }

    #[test]
    fn spoiling_with_alt_up_does_not_arm_or_poison_the_next_press() {
        let mut alt_state = LoneAlt::default();
        alt_state.spoil();
        assert_eq!(alt_state, LoneAlt::Idle);
        assert!(!alt_state.modifiers(&alt()));
        assert!(alt_state.modifiers(&none()));
    }

    #[test]
    fn focus_loss_forgets_a_press_in_flight() {
        let mut alt_state = LoneAlt::default();
        assert!(!alt_state.modifiers(&alt()));
        alt_state.reset();
        assert!(
            !alt_state.modifiers(&none()),
            "Alt+Tab away and back opens nothing"
        );
    }
}
