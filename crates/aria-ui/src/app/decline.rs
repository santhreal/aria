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

use aria_models::{BakeRefusal, ClipId};

use super::AppState;
use super::duck::DuckRefusal;

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
    /// Extract Audio sent to a clip with sound that shares its link with
    /// another clip, which the extraction would leave out of sync.
    LinkedSound,
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
    /// A label write to clips or a pool item that already have that label.
    SameLabel,
    /// An Audio Channels choice every selected clip with sound already plays.
    SameChannels,
    /// A Matte from Above row sent to a clip whose matte already reads that
    /// source.
    SameMatte,
    /// Move Nearest Marker to Playhead with that marker on the playhead.
    MarkerAtPlayhead,
    /// A transition applied to a cut that already holds it.
    SameTransition,
    /// An ease row whose easing every target keyframe already has.
    SameEase,
    /// Add Keyframe where every selected property already has a keyframe at
    /// the playhead holding its value.
    KeyedHere,
    /// Add Keyframe with no selected property row of a clip under the playhead.
    NoPropertyAtPlayhead,
    /// Move Track Up on a lane at the top of its part of the stack: text lanes
    /// stay above picture lanes, the main track below the other picture lanes,
    /// and sound lanes below the picture.
    TrackAtTop,
    /// Move Track Down on a lane at the bottom of its part of the stack.
    TrackAtBottom,
    /// A lane shift from the playhead on a lane with no clip starting at or
    /// after it.
    NothingAfterPlayhead,
    /// Reset Transform on a clip whose transform is already the default and
    /// not animated.
    DefaultTransform,
    /// Open or Place Nested Sequence sent to a selected clip that plays no
    /// nested sequence.
    NotANest,
    /// Leave Nested Sequence sent with the root sequence open.
    AtRootSequence,
    /// Render and Replace in a project that has never been saved: the render
    /// is kept beside the project file.
    UnsavedRender,
    /// Close Gap at Playhead where the free span before the first clip at or
    /// after the playhead is shorter than the close.
    GapTooShort,
    /// A mask row sent to a clip with no active mask.
    NoMask,
    /// A grade row sent to a clip whose color is not adjusted.
    NoGrade,
    /// A chroma key row sent to a clip with no chroma key.
    NoChromaKey,
    /// A sound row sent with no selected clip that carries sound.
    SelectSound,
    /// An Import Recent row whose file is no longer at its path.
    ImportMissing,
    /// A row that clears Import Recent, sent with the list empty.
    NoImports,
    /// A row that acts on several clips, sent with fewer than two selected.
    FewerThanTwo,
    /// Unlink sent to a selection that is not linked.
    NotLinked,
    /// Ungroup sent to a selection that is not a clip group.
    NotGrouped,
    /// Undo sent with no step behind.
    NothingToUndo,
    /// Redo sent with no step ahead.
    NothingToRedo,
    /// A row that clears In and Out, sent with neither set.
    NoMarks,
    /// A row that reads the In point, sent with none set.
    NoInPoint,
    /// A row that reads the Out point, sent with none set.
    NoOutPoint,
    /// A row that acts on the sequence's clips, sent to an empty sequence.
    NoClips,
    /// A property row command sent with no row of the selected clip selected.
    NoPropertyRow,
    /// A revert row sent to a project that has no file.
    NeverSaved,
    /// A revert row sent with no edit since the last save.
    NoUnsavedChanges,
    /// A row that places pool media, sent with none selected online.
    NoPoolSelection,
    /// A comparison row sent while the Viewer compares nothing.
    NotComparing,
    /// A row that applies the last effect, sent before any is applied.
    NoEffectApplied,
    /// A row that pastes effects, sent with none copied.
    NoEffectCopied,
    /// An effects row sent to clips that hold no effect.
    NoEffects,
    /// A recent project row whose file is gone.
    ProjectMissing,
    /// A timeline export row with no exporter mounted.
    NoExporter,
    /// A timeline import row with no importer mounted.
    NoImporter,
    /// A row that deletes empty tracks, sent when every track holds a clip.
    NoEmptyTrack,
    /// A row that leaves the open folder, sent at the top of the media pool.
    AtPoolTop,
    /// A moment row sent before analysis detects any moment.
    NoMoments,
    /// A previous moment row with no detected moment before the playhead.
    NoMomentBefore,
    /// A next moment row with no detected moment after the playhead.
    NoMomentAfter,
    /// A template slot row sent to a clip that plays no media.
    NotSlotMedia,
    /// A title row sent to a clip that is not a title.
    NotATitle,
    /// A keyframe row sent with the playhead outside the selected clip.
    PlayheadOutsideClip,
    /// A keyframe row for a property the selected clip does not have.
    NoSuchProperty,
    /// Remove Opacity Keyframe with no opacity keyframe at the playhead.
    NoOpacityKeyAtPlayhead,
    /// A duck source row sent to a clip on a picture track.
    NotOnAudioTrack,
    /// A row that selects a clip under the playhead, sent with none there.
    NoClipAtPlayhead,
    /// A row that duplicates the clip to another track, with none of its kind.
    NoOtherTrack,
    /// A history row sent with no step to undo or redo.
    NoHistory,
    /// A row that measures drawn content, sent to a clip that draws none.
    NothingToMeasure,
    /// A row that clears the pool filter, sent with none typed.
    FilterEmpty,
    /// A row that clears proxies, sent with no pool item on one.
    NoProxies,
    /// A pool row sent to an empty media pool.
    PoolEmpty,
    /// A row that reloads the focused pool item, sent with none focused.
    NoPoolFocus,
    /// A row that reports the last frame's cost, sent before any frame is rendered.
    NoFrameRendered,
    /// A source row sent to a clip that plays no pool entry.
    NoPoolEntry,
    /// A measurement row sent with no frame in the Viewer.
    NoFrameOnScreen,
    /// A normalize row sent before loudness is measured.
    NoLoudness,
    /// An analysis row sent to a generated clip.
    GeneratedAnalysis,
    /// An analysis row sent to a clip whose media left the pool.
    MediaGone,
    /// An analysis row sent to a clip whose media is offline.
    MediaOffline,
    /// A transcribe row sent with no transcription model installed.
    NoTranscriptionModel,
    /// A mask tracking row sent with no clip selected.
    NoMaskClip,
    /// A mask tracking row sent to a clip with no active mask.
    NoMaskToTrack,
    /// A mask tracking row sent with the playhead on the clip's last frame.
    NothingAheadToTrack,
    /// An AI endpoint check sent while one runs.
    AiCheckRunning,
    /// Freeze Frame sent to a clip on a locked track.
    TrackLocked,
    /// Freeze Frame sent with the playhead off the selected clip.
    PlayheadOffClip,
    /// Freeze Frame sent to a clip linked to another clip.
    FreezeLinked,
    /// Freeze Frame where a clip after the playhead shares a link the shift would
    /// split.
    FreezeLinkedAfter,
    /// Freeze Frame on the first frame of a clip with an entrance animation.
    FreezeEntrance,
    /// Freeze Frame on the last frame of a clip with an exit animation.
    FreezeExit,
    /// Render and Replace sent to a clip on a track a render cannot stand in for.
    NotBakeable,
    /// Restore Unrendered sent to a clip that is not a rendered stand-in.
    NotRendered,
    /// Ripple Trim In with no edit before the playhead on a targeted track.
    NoEarlierEdit,
    /// Ripple Trim Out with no edit after the playhead on a targeted track.
    NoLaterEdit,
    /// A duck that cannot run, in the words the run states.
    Duck(DuckRefusal),
    /// Render and Replace sent to a clip a render cannot stand in for.
    Bake(BakeRefusal),
}

impl Decline {
    /// Every reason, for a census over the wording.
    pub const ALL: [Decline; 137] = [
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
        Decline::LinkedSound,
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
        Decline::SameLabel,
        Decline::SameChannels,
        Decline::SameMatte,
        Decline::MarkerAtPlayhead,
        Decline::SameTransition,
        Decline::SameEase,
        Decline::KeyedHere,
        Decline::NoPropertyAtPlayhead,
        Decline::TrackAtTop,
        Decline::TrackAtBottom,
        Decline::NothingAfterPlayhead,
        Decline::DefaultTransform,
        Decline::NotANest,
        Decline::AtRootSequence,
        Decline::UnsavedRender,
        Decline::GapTooShort,
        Decline::NoMask,
        Decline::NoGrade,
        Decline::NoChromaKey,
        Decline::SelectSound,
        Decline::ImportMissing,
        Decline::NoImports,
        Decline::FewerThanTwo,
        Decline::NotLinked,
        Decline::NotGrouped,
        Decline::NothingToUndo,
        Decline::NothingToRedo,
        Decline::NoMarks,
        Decline::NoInPoint,
        Decline::NoOutPoint,
        Decline::NoClips,
        Decline::NoPropertyRow,
        Decline::NeverSaved,
        Decline::NoUnsavedChanges,
        Decline::NoPoolSelection,
        Decline::NotComparing,
        Decline::NoEffectApplied,
        Decline::NoEffectCopied,
        Decline::NoEffects,
        Decline::ProjectMissing,
        Decline::NoExporter,
        Decline::NoImporter,
        Decline::NoEmptyTrack,
        Decline::AtPoolTop,
        Decline::NoMoments,
        Decline::NoMomentBefore,
        Decline::NoMomentAfter,
        Decline::NotSlotMedia,
        Decline::NotATitle,
        Decline::PlayheadOutsideClip,
        Decline::NoSuchProperty,
        Decline::NoOpacityKeyAtPlayhead,
        Decline::NotOnAudioTrack,
        Decline::NoClipAtPlayhead,
        Decline::NoOtherTrack,
        Decline::NoHistory,
        Decline::NothingToMeasure,
        Decline::FilterEmpty,
        Decline::NoProxies,
        Decline::PoolEmpty,
        Decline::NoPoolFocus,
        Decline::NoFrameRendered,
        Decline::NoPoolEntry,
        Decline::NoFrameOnScreen,
        Decline::NoLoudness,
        Decline::GeneratedAnalysis,
        Decline::MediaGone,
        Decline::MediaOffline,
        Decline::NoTranscriptionModel,
        Decline::NoMaskClip,
        Decline::NoMaskToTrack,
        Decline::NothingAheadToTrack,
        Decline::AiCheckRunning,
        Decline::TrackLocked,
        Decline::PlayheadOffClip,
        Decline::FreezeLinked,
        Decline::FreezeLinkedAfter,
        Decline::FreezeEntrance,
        Decline::FreezeExit,
        Decline::NotBakeable,
        Decline::NotRendered,
        Decline::NoEarlierEdit,
        Decline::NoLaterEdit,
        Decline::Duck(DuckRefusal::NoSelection),
        Decline::Duck(DuckRefusal::NoSource),
        Decline::Duck(DuckRefusal::SilentSource),
        Decline::Duck(DuckRefusal::OnlyVoice),
        Decline::Duck(DuckRefusal::NoMusic),
        Decline::Bake(BakeRefusal::Rendered),
        Decline::Bake(BakeRefusal::LaneBar),
        Decline::Bake(BakeRefusal::Undressed),
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
            Decline::AnglesRepeat => "The angles' sound matches at more than one offset",
            Decline::AlreadyLinked => "The selected clips are already linked",
            Decline::AlreadyGrouped => "The selected clips are already a group",
            Decline::NoSourceToRemap => "The selected clip plays no source media to remap",
            Decline::NoFrameToHold => "The selected clip has no video frame to hold",
            Decline::NoSoundToExtract => "The selected clip has no sound to extract",
            Decline::LinkedSound => {
                "The selected clip is linked to another clip: unlink it to extract its sound"
            }
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
            Decline::SameLabel => "That label is already set",
            Decline::SameChannels => "The selected clips already play those channels",
            Decline::SameMatte => "The selected clip's matte already reads that source",
            Decline::MarkerAtPlayhead => "The nearest marker is already at the playhead",
            Decline::SameTransition => "That cut already has that transition",
            Decline::SameEase => "The keyframes already have that easing",
            Decline::KeyedHere => "The selected properties are already keyed at the playhead",
            Decline::NoPropertyAtPlayhead => {
                "Select a property row of a clip under the playhead first"
            }
            Decline::TrackAtTop => "The track cannot move higher in the stack",
            Decline::TrackAtBottom => "The track cannot move lower in the stack",
            Decline::NothingAfterPlayhead => "No clip on the track starts at or after the playhead",
            Decline::DefaultTransform => "The selected clip already has the default transform",
            Decline::NotANest => "The selected clip is not a nested sequence",
            Decline::AtRootSequence => "The root sequence is already open",
            Decline::UnsavedRender => {
                "Save the project first: a rendered clip is kept beside the project file"
            }
            Decline::GapTooShort => {
                "The gap before the next clip on the track is shorter than one second"
            }
            Decline::NoMask => "The selected clip has no mask",
            Decline::NoGrade => "The selected clip has no grade",
            Decline::NoChromaKey => "The selected clip has no chroma key",
            Decline::SelectSound => "Select a clip with sound",
            Decline::ImportMissing => "The file is missing",
            Decline::NoImports => "No file has been imported yet",
            Decline::FewerThanTwo => "Select two or more clips",
            Decline::NotLinked => "The selected clips are not linked",
            Decline::NotGrouped => "The selected clips are not a group",
            Decline::NothingToUndo => "Nothing to undo",
            Decline::NothingToRedo => "Nothing to redo",
            Decline::NoMarks => "Neither an In nor an Out point is set",
            Decline::NoInPoint => "No In point is set",
            Decline::NoOutPoint => "No Out point is set",
            Decline::NoClips => "The sequence has no clips",
            Decline::NoPropertyRow => "Select a property row of the selected clip",
            Decline::NeverSaved => "The project has never been saved",
            Decline::NoUnsavedChanges => "There are no changes since the last save",
            Decline::NoPoolSelection => "No media is selected in the pool",
            Decline::NotComparing => "Nothing is being compared",
            Decline::NoEffectApplied => "No effect has been applied yet",
            Decline::NoEffectCopied => "No effect has been copied",
            Decline::NoEffects => "The selected clips have no effects",
            Decline::ProjectMissing => "The project file is missing",
            Decline::NoExporter => "No timeline exporter is mounted",
            Decline::NoImporter => "No timeline importer is mounted",
            Decline::NoEmptyTrack => "Every track holds a clip",
            Decline::AtPoolTop => "Already at the top of the media pool",
            Decline::NoMoments => "No detected moments",
            Decline::NoMomentBefore => "No detected moment before the playhead",
            Decline::NoMomentAfter => "No detected moment after the playhead",
            Decline::NotSlotMedia => "Only media clips can be template slots",
            Decline::NotATitle => "Select a title clip first",
            Decline::PlayheadOutsideClip => "Move the playhead inside the selected clip",
            Decline::NoSuchProperty => "The selected clip does not have that property",
            Decline::NoOpacityKeyAtPlayhead => {
                "The selected clip has no opacity keyframe at the playhead"
            }
            Decline::NotOnAudioTrack => "Select a clip on an audio track",
            Decline::NoClipAtPlayhead => "No clip under the playhead",
            Decline::NoOtherTrack => "No other track accepts this clip",
            Decline::NoHistory => "No steps to undo or redo",
            Decline::NothingToMeasure => "The selection draws no measurable content",
            Decline::FilterEmpty => "The pool filter is already empty",
            Decline::NoProxies => "No proxies in use",
            Decline::PoolEmpty => "The pool is empty",
            Decline::NoPoolFocus => "Pick a pool item to reload",
            Decline::NoFrameRendered => "No frame has been rendered yet",
            Decline::NoPoolEntry => "Select a clip that plays a pool entry",
            Decline::NoFrameOnScreen => "No frame on screen to measure",
            Decline::NoLoudness => "Measure loudness in the Analysis panel first, then normalize",
            Decline::GeneratedAnalysis => "A generated clip has no media file to analyze",
            Decline::MediaGone => "The clip's media is no longer in the pool",
            Decline::MediaOffline => "The clip's media is offline",
            Decline::NoTranscriptionModel => "Install a transcription model in Settings first",
            Decline::NoMaskClip => "Select a clip with a mask to track",
            Decline::NoMaskToTrack => "Draw a mask on the clip before tracking it",
            Decline::NothingAheadToTrack => {
                "The playhead is at the end of the clip, so there is nothing ahead to track"
            }
            Decline::AiCheckRunning => "An AI endpoint check is already running",
            Decline::TrackLocked => "Unlock the track first",
            Decline::PlayheadOffClip => "Move the playhead onto the selected clip",
            Decline::FreezeLinked => "Unlink the clip before freezing a frame",
            Decline::FreezeLinkedAfter => {
                "Unlink the linked clip after the playhead before freezing a frame"
            }
            Decline::FreezeEntrance => {
                "Move the playhead inside the clip or remove its entrance animation before freezing"
            }
            Decline::FreezeExit => {
                "Move the playhead inside the clip or remove its exit animation before freezing"
            }
            Decline::NotBakeable => "The selected clip is on a track a render cannot stand in for",
            Decline::NotRendered => "Select a rendered clip to restore",
            Decline::NoEarlierEdit => "No earlier edit on a targeted track",
            Decline::NoLaterEdit => "No later edit on a targeted track",
            Decline::Duck(why) => why.reason(),
            Decline::Bake(why) => super::bake_copy(why),
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
