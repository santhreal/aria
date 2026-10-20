//! Why a command did nothing.
//!
//! # Why
//!
//! A command that finds nothing to act on returned `Ok(())` and said nothing.
//! The dogfood pass found it on the palette's "Add cross fade at cut": after a
//! split the selection holds the right half of the cut, whose own right junction
//! abuts nothing, so the row ran, refused itself and left no trace. On screen
//! the editor had ignored a key, which is the one answer
//! a surface must never give.
//!
//! Declining is not an error: nothing failed, and `last_error` is for a failure
//! that needs a fix. It is a fact about the state on screen, so it goes in the
//! status line. The reasons are a small
//! registry rather than strings at the call sites: a command declines for one of
//! a handful of reasons, and the words for each have one owner.

use aria_models::ClipId;

use super::AppState;

/// A reason a command found nothing to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decline {
    /// The command acts on the selection and there is none.
    NoSelection,
    /// The clip's right junction has no clip on the other side, so there is no
    /// cut for a blend to happen at.
    NoAbuttingCut,
    /// An id no transition catalog holds, which means a stale surface.
    UnknownTransition,
    /// The junction carries no transition, so there is none to change or remove.
    NoTransitionAtCut,
    /// An audio crossfade sent to a cut between pictures.
    PictureCut,
    /// A picture transition sent to a cut between sounds.
    SoundCut,
    /// A group with one camera has nothing to sync against.
    OneAngleOnly,
    /// The angles' peaks are still being decoded, so there is nothing to
    /// correlate yet.
    AnglesStillDecoding,
    /// A camera's file carries no sound, or none that varies: silence says
    /// nothing about where in time it was recorded.
    NoAngleSound,
    /// The cameras' sound correlates too weakly to place one against the other.
    AnglesDoNotMatch,
    /// The cameras' sound matches equally well in more than one place, so the
    /// best match is a guess: a repeating sound cannot say where it was.
    AnglesRepeat,
}

impl Decline {
    /// Every reason, for a census over the wording.
    pub const ALL: [Decline; 11] = [
        Decline::NoSelection,
        Decline::NoAbuttingCut,
        Decline::UnknownTransition,
        Decline::NoTransitionAtCut,
        Decline::PictureCut,
        Decline::SoundCut,
        Decline::OneAngleOnly,
        Decline::AnglesStillDecoding,
        Decline::NoAngleSound,
        Decline::AnglesDoNotMatch,
        Decline::AnglesRepeat,
    ];

    /// The status line text. Present tense, about the state on screen rather
    /// than about the command that stopped.
    pub fn reason(self) -> &'static str {
        match self {
            Decline::NoSelection => "Nothing is selected",
            Decline::NoAbuttingCut => "No clip on the other side of that cut",
            Decline::UnknownTransition => "No such transition",
            Decline::NoTransitionAtCut => "No transition at that cut",
            Decline::PictureCut => "That cut is between pictures: Ctrl+T puts a transition on it",
            Decline::SoundCut => "That cut is between sounds: Ctrl+Shift+T crossfades it",
            Decline::OneAngleOnly => "A group needs a second camera to sync against",
            Decline::AnglesStillDecoding => "The angles' sound is still being read",
            Decline::NoAngleSound => "An angle has no sound to sync by",
            Decline::AnglesDoNotMatch => "The angles' sound does not match",
            Decline::AnglesRepeat => "The angles' sound repeats, so it cannot say where it is",
        }
    }
}

impl AppState {
    /// Say why a command did nothing.
    pub(crate) fn decline(&mut self, why: Decline) {
        self.status_message = Some(why.reason().to_string());
    }

    /// The clip a command that acts on the selection works with, or nothing
    /// plus the line that says so.
    ///
    /// Every such command reads the selection through here, so "the editor
    /// ignored me" cannot come back one command at a time.
    pub(crate) fn selected_clip_or_decline(&mut self) -> Option<ClipId> {
        match self.selected_clip() {
            Some(clip) => Some(clip),
            None => {
                self.decline(Decline::NoSelection);
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "decline_tests.rs"]
mod tests;
