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
    /// The command acts on a selected clip and none is selected.
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
    /// Link sent to a selection that is already one link group.
    AlreadyLinked,
    /// Group sent to a selection that is already one clip group.
    AlreadyGrouped,
    /// Time remapping sent to a clip that plays no source media.
    NoSourceToRemap,
    /// Freeze Frame sent to a clip with no video frame to hold.
    NoFrameToHold,
    /// Extract Audio sent to a clip whose sound cannot move to its own lane.
    NoSoundToExtract,
    /// Audio Gain sent to a selection that carries no sound.
    NoSound,
    /// A marker command sent to a sequence that holds no marker.
    NoMarkers,
    /// A marker command that acts on the marker under the playhead, sent with
    /// none within a second of it.
    NoMarkerAtPlayhead,
    /// A paste with nothing on the clipboard.
    NothingCopied,
    /// A paste whose copy no lane of the open sequence takes.
    NoLaneForCopy,
    /// Paste to Fit retimes one clip, sent with several on the clipboard.
    SeveralCopied,
    /// A command that acts on the span from In to Out, sent with no span marked.
    NoMarkedRange,
    /// A command that acts on the selected clips or the selected gap, sent with
    /// neither selected.
    NoClipOrGap,
    /// Loop or work-area playback sent with the In point at the end of the
    /// sequence, so the range to play holds no frame.
    InAtEnd,
    /// A command that acts on the clips under the playhead on the targeted
    /// lanes, sent with no unlocked clip there.
    NoTargetedClipAtPlayhead,
    /// Match Frame sent with no media clip under the playhead.
    NoMediaClipAtPlayhead,
    /// Add Edit sent with no clip it cuts strictly inside under the playhead.
    NoClipToCut,
    /// Ctrl+T or Ctrl+Shift+T sent with no edge selected and no cut of its
    /// kind between two clips on the targeted lanes.
    NoCutForTransition,
    /// Trim Edit sent with no unlocked clip on an unlocked lane.
    NoEditPoint,
    /// Extend Edit sent with no edit point selected.
    NoSelectedEdge,
    /// Extend Edit sent with the selected edit point on the playhead.
    EdgeAtPlayhead,
    /// A lane or selection shift that would take a clip before tick 0.
    ShiftBeforeStart,
    /// Reset Framing sent to a clip that shows its whole frame unmirrored.
    FullFrame,
    /// Remove Track Matte sent to a clip with no matte.
    NoMatte,
    /// A keyframe row sent with no keyframe selected and none under the playhead.
    NoKeyframeTarget,
    /// Linear sent to keyframes that are all Linear already.
    AlreadyLinear,
    /// A span row sent without two selected keyframes on one property.
    NoKeyframeSpan,
    /// Time-Reverse Keyframes sent to keyframes that play the same reversed.
    ReversedSame,
    /// Paste Attributes onto the clip the copy came from.
    CopiedFromHere,
    /// Paste Attributes onto a clip that already has the copied look.
    SameAttributes,
    /// A row that takes the Source monitor's source, with none open.
    NoMonitorSource,
    /// Replace from Source sent to a generated clip.
    GeneratedClip,
    /// Replace from Source with too little media around the Source playhead.
    SourceTooShort,
    /// Replace from Source that would put back the source frames the clip plays.
    SameSource,
}

impl Decline {
    /// Every reason, for a census over the wording.
    pub const ALL: [Decline; 45] = [
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
        Decline::AlreadyLinked,
        Decline::AlreadyGrouped,
        Decline::NoSourceToRemap,
        Decline::NoFrameToHold,
        Decline::NoSoundToExtract,
        Decline::NoSound,
        Decline::NoMarkers,
        Decline::NoMarkerAtPlayhead,
        Decline::NothingCopied,
        Decline::NoLaneForCopy,
        Decline::SeveralCopied,
        Decline::NoMarkedRange,
        Decline::NoClipOrGap,
        Decline::InAtEnd,
        Decline::NoTargetedClipAtPlayhead,
        Decline::NoMediaClipAtPlayhead,
        Decline::NoClipToCut,
        Decline::NoCutForTransition,
        Decline::NoEditPoint,
        Decline::NoSelectedEdge,
        Decline::EdgeAtPlayhead,
        Decline::ShiftBeforeStart,
        Decline::FullFrame,
        Decline::NoMatte,
        Decline::NoKeyframeTarget,
        Decline::AlreadyLinear,
        Decline::NoKeyframeSpan,
        Decline::ReversedSame,
        Decline::CopiedFromHere,
        Decline::SameAttributes,
        Decline::NoMonitorSource,
        Decline::GeneratedClip,
        Decline::SourceTooShort,
        Decline::SameSource,
    ];

    /// The status line text. Present tense, about the state on screen rather
    /// than about the command that stopped.
    pub fn reason(self) -> &'static str {
        match self {
            Decline::NoSelection => "No clip is selected",
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
            Decline::AlreadyLinked => "The selected clips are already linked",
            Decline::AlreadyGrouped => "The selected clips are already a group",
            Decline::NoSourceToRemap => "The selected clip plays no source media to remap",
            Decline::NoFrameToHold => "The selected clip has no video frame to hold",
            Decline::NoSoundToExtract => "The selected clip has no sound to extract",
            Decline::NoSound => "No selected clip carries sound",
            Decline::NoMarkers => "The sequence has no markers",
            Decline::NoMarkerAtPlayhead => "No marker is within a second of the playhead",
            Decline::NothingCopied => "Nothing has been copied or cut",
            Decline::NoLaneForCopy => "No lane in this sequence takes the copied clip",
            Decline::SeveralCopied => "The copy holds more than one clip",
            Decline::NoMarkedRange => "Mark an In and an Out point first",
            Decline::NoClipOrGap => "No clip or gap is selected",
            Decline::InAtEnd => "The In point is at the end of the sequence",
            Decline::NoTargetedClipAtPlayhead => "No clip under the playhead on a targeted track",
            Decline::NoMediaClipAtPlayhead => "No media clip under the playhead",
            Decline::NoClipToCut => "No clip under the playhead to cut",
            Decline::NoCutForTransition => "No cut between two clips to put a transition on",
            Decline::NoEditPoint => "The sequence has no edit points",
            Decline::NoSelectedEdge => "Select an edit point first",
            Decline::EdgeAtPlayhead => "The edit point is already at the playhead",
            Decline::ShiftBeforeStart => {
                "That offset would move a clip before the start of the sequence"
            }
            Decline::FullFrame => "The selected clip shows its whole frame",
            Decline::NoMatte => "The selected clip has no track matte",
            Decline::NoKeyframeTarget => "Select a keyframe, or put the playhead on one",
            Decline::AlreadyLinear => "The keyframes are already Linear",
            Decline::NoKeyframeSpan => "Select two or more keyframes on one property",
            Decline::ReversedSame => "The selected keyframes play the same reversed",
            Decline::CopiedFromHere => "The copy came from the selected clip",
            Decline::SameAttributes => "The selected clip already has the copied attributes",
            Decline::NoMonitorSource => "Open a source in the Source monitor first",
            Decline::GeneratedClip => "A generated clip has no media to replace",
            Decline::SourceTooShort => {
                "The source does not hold enough media around its playhead to fill the clip"
            }
            Decline::SameSource => "The clip already plays those source frames",
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
