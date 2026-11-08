//! Human-readable engine errors. Every message names the next action.

use aria_core::count::counted;
use aria_engine::EngineError;
use aria_models::{
    BakeRefusal, ClipSetRefusal, FreezeRefusal, GeneratedRefusal, MAX_CLIP_MASKS, MAX_PARENT_DEPTH,
    ModelError, SlipRefusal, SplitRefusal,
};

pub(crate) fn describe_engine_error(err: &EngineError) -> String {
    match err {
        EngineError::MissingMedia(paths) => missing_media_copy(paths),
        EngineError::Import(msg) => {
            format!("Couldn't import that file: {msg}. Try another format or check the path.")
        }
        EngineError::Render(aria_render::RenderError::Empty(reason)) => {
            empty_export_copy(*reason).into()
        }
        EngineError::Export(msg) => {
            format!("Couldn't write the export: {msg}. Check the destination and free disk space.")
        }
        EngineError::Decode(err) => decode_copy(err),
        EngineError::Io(err) => format!(
            "Couldn't read or write that file ({}). Check the path, its permissions and the free \
             disk space.",
            aria_models::io_reason(err)
        ),
        EngineError::Render(_) => {
            "Couldn't render this frame. Scrub the playhead or reopen the project.".into()
        }
        EngineError::Model(model) => model_copy(model),
        EngineError::Time(_) => {
            "That time is outside the sequence. Move the playhead and try again.".into()
        }
        EngineError::Unsupported(msg) => format!("This build can't do that yet: {msg}."),
        // The reason states the obstacle and the next action and comes from the
        // host, plugin or edit that refused, so it is shown as written.
        EngineError::Refused(reason) => reason.clone(),
    }
}

/// Names the one missing file, or counts them and names the first.
fn missing_media_copy(paths: &[String]) -> String {
    match paths {
        [one] => format!("Couldn't find media at {one}. Move the file back or click Relink."),
        [first, ..] => format!(
            "Couldn't find {} media files, starting with {first}. Move them back or click Relink \
             All.",
            paths.len()
        ),
        [] => "Couldn't find this project's media. Move it back or click Relink All.".into(),
    }
}

/// The copy for a model refusal. The match has no catch-all, so a new
/// refusal stops compiling here until it has a sentence of its own, rather
/// than borrowing one that points at the wrong fix.
fn model_copy(err: &ModelError) -> String {
    let copy = match err {
        ModelError::ProjectLoad(_)
        | ModelError::TemplateLoad(_)
        | ModelError::TemplateBundle(_)
        | ModelError::InvalidProjectFile(_) => {
            return err.to_string();
        }
        ModelError::Overlap(_) => {
            "That would overlap another clip on the same track. Make room for it, then try again."
        }
        ModelError::IncompatibleTrackKind { .. } => {
            "That track can't hold this kind of clip. Drop it on a matching video or audio track."
        }
        ModelError::NotOnPicture { .. } => {
            "That clip is on an audio track, which shows no picture. Select a clip on a video track, then try again."
        }
        ModelError::NotOnSound { .. } => {
            "Only a clip on an audio track takes an audio role. Select a clip on an audio track, then try again."
        }
        ModelError::SourceOutOfBounds => {
            "That reaches past the ends of the source media. Trim within the media's length."
        }
        ModelError::InvalidRange => {
            "That would leave the clip with no length. Keep the out point after the in point."
        }
        ModelError::InvalidTransform(_) | ModelError::InvalidParam(_) => {
            "That value isn't accepted here. Enter another value, then try again."
        }
        ModelError::RateMismatch { .. } => {
            "Those times use different frame rates. Conform the media to the sequence rate, then try again."
        }
        ModelError::TimeOverflow => {
            "That time is too far from the start to represent. Use a time closer to the start."
        }
        ModelError::MediaReferenced(_) => {
            "That media is still used on the timeline. Delete its clips first."
        }
        ModelError::UnknownTrack(_) => "That track no longer exists. Pick another track.",
        ModelError::UnknownMedia(_) => {
            "That media is no longer in the project. Import it again, then try again."
        }
        ModelError::UnknownClip(_) => {
            "That clip is no longer on the timeline. Select a clip, then try again."
        }
        ModelError::DuplicateClip(_) => {
            "That clip is already on the timeline. Select it there instead."
        }
        ModelError::UnknownMarker(_) => "That marker no longer exists. Pick another marker.",
        ModelError::UnknownBin(_) => "That bin no longer exists. Pick another bin.",
        ModelError::BinCycle(_) => {
            "A bin can't go inside itself or one of its own bins. Drop it on another bin."
        }
        ModelError::UnknownSequence(_) => "That sequence no longer exists. Open another sequence.",
        ModelError::SequenceCycle(_) => {
            "A sequence can't contain itself. Nest a different sequence."
        }
        ModelError::ParentCycle(_) => {
            "A clip can't be parented to itself or to its own child. Pick another parent."
        }
        ModelError::ParentElsewhere { .. } => {
            "The parent has to be in the same sequence. Pick a clip in this sequence."
        }
        ModelError::ParentApart { .. } => {
            "The parent has to overlap the clip in time. Pick a clip that overlaps it."
        }
        ModelError::ParentOffPicture(_) => {
            "Only a picture clip can follow a parent. Select a video, image, title, or shape clip."
        }
        ModelError::ParentFlattened(_) => {
            "That parent is scaled to zero. Give it a size, then parent to it."
        }
        ModelError::ParentTooDeep(_) => {
            return format!(
                "A clip can sit under at most {MAX_PARENT_DEPTH} parents. Pick a parent higher in the chain."
            );
        }
        ModelError::SlotMediaMismatch { .. } => {
            "That media doesn't fit this template slot. Drop media of the kind the slot takes."
        }
        ModelError::SlotDurationUnmet { .. } => {
            "That media is shorter than the slot. Pick a longer clip."
        }
        ModelError::SlotRetimeUnsupported { .. } => {
            "That slot is speed-ramped or reversed, so it can't be filled. Fill another slot."
        }
        ModelError::TooManyPicks { given, slots } => {
            return format!(
                "{} for a template with {}. Pick at most {slots}.",
                counted(*given, "pick", "picks"),
                counted(*slots, "slot", "slots")
            );
        }
        ModelError::UnknownMulticamGroup(_) => {
            "That multicam group no longer exists. Pick another multicam clip."
        }
        ModelError::EmptyMulticamGroup => {
            "A multicam group needs at least one angle. Select clips to use as angles."
        }
        ModelError::DuplicateMulticamMedia(_) => {
            "That media is already an angle in this group. Pick other media."
        }
        ModelError::MulticamAngleOutOfBounds { total, .. } => {
            return format!(
                "This multicam group has {}. Pick an angle from 1 to {total}.",
                counted(*total, "angle", "angles")
            );
        }
        ModelError::InvalidMulticamCuts => {
            "Multicam cuts have to be in time order, one per frame. Move the cut, then try again."
        }
        ModelError::InvalidMulticamOffset(_) => {
            "That angle offset isn't a time. Enter a time such as 00:00:01:00."
        }
        ModelError::NotABus(_) => "Only a bus can take a send. Route to a submix bus.",
        ModelError::LaneCarriesNoSound(_) => {
            "That track has no sound to route. Pick an audio track."
        }
        ModelError::RoutingCycle(_) => {
            "That would route the track back into itself. Pick another bus."
        }
        ModelError::DuplicateSend { .. } => {
            "That track already sends to this bus. Adjust the existing send."
        }
        ModelError::UnknownSend { .. } => "That send no longer exists. Pick another send.",
        ModelError::UnknownAudioEffect(_) => {
            "That audio effect isn't installed. Pick one from Audio Effects."
        }
        ModelError::ClipCarriesNoSound(_) => {
            "That clip has no sound to process. Select a clip with audio."
        }
        ModelError::UnknownInsert { .. } => {
            "That effect is no longer in the chain. Pick another effect."
        }
        ModelError::InsertsFull(max) => {
            return format!(
                "A track or the master holds at most {max} effects. Remove one, then add another."
            );
        }
        ModelError::MasksFull(_) => {
            return format!(
                "A clip holds at most {MAX_CLIP_MASKS} masks. Remove one, then add another."
            );
        }
        ModelError::ClipSet(refusal) => match refusal {
            ClipSetRefusal::NoClips => "No clip is selected. Select a clip, then try again.",
            ClipSetRefusal::LinkOfOne => {
                "Linking needs two different clips. Select another clip, then try again."
            }
            ClipSetRefusal::GroupOfOne => {
                "Grouping needs two different clips. Select another clip, then try again."
            }
            ClipSetRefusal::NoneLinked => {
                "None of the selected clips is linked. Select a linked clip to unlink."
            }
            ClipSetRefusal::NoneGrouped => {
                "None of the selected clips is in a group. Select a grouped clip to ungroup."
            }
        },
        ModelError::Split(refusal) => split_copy(*refusal),
        ModelError::Slip(refusal) => match refusal {
            SlipRefusal::FreezeFrame => {
                "A freeze frame holds one frame, so it has nothing to slip. Trim or move the clip \
                 instead."
            }
            SlipRefusal::Still => {
                "A still shows one frame throughout, so it has nothing to slip. Trim or move the \
                 clip instead."
            }
            SlipRefusal::Generated => {
                "A generated clip plays no footage, so it has nothing to slip. Trim or move the \
                 clip instead."
            }
            SlipRefusal::WholeTake => {
                "That clip already plays its whole take, so there is nothing to slip. Trim the \
                 clip shorter, then slip it."
            }
        },
        ModelError::FrozenMedia(_) => {
            "That clip is a freeze frame, which holds one frame of its media. Delete it, then \
             place the new media."
        }
        ModelError::Generated(refusal) => match refusal {
            GeneratedRefusal::Speed => {
                "A generated clip plays no footage, so it has no speed to change. Trim the clip \
                 to change its length."
            }
            GeneratedRefusal::Sound => {
                "A generated clip plays no sound, so it has no volume or fades to set. Select a \
                 clip with sound instead."
            }
        },
        ModelError::Bake(refusal) => bake_copy(*refusal),
        ModelError::Freeze(refusal) => freeze_copy(*refusal),
    };
    copy.into()
}

/// Whether `err` refuses an edit of the selected clips, so selecting other
/// clips leaves it stale. Neither match has a catch-all: a new error stops
/// compiling here until it is placed on one side.
pub(crate) fn refuses_the_selection(err: &EngineError) -> bool {
    match err {
        EngineError::Model(model) => model_refuses_the_selection(model),
        EngineError::MissingMedia(_)
        | EngineError::Import(_)
        | EngineError::Export(_)
        | EngineError::Render(_)
        | EngineError::Decode(_)
        | EngineError::Io(_)
        | EngineError::Time(_)
        | EngineError::Unsupported(_)
        | EngineError::Refused(_) => false,
    }
}

fn model_refuses_the_selection(err: &ModelError) -> bool {
    match err {
        ModelError::UnknownClip { .. }
        | ModelError::DuplicateClip { .. }
        | ModelError::ParentCycle { .. }
        | ModelError::ParentElsewhere { .. }
        | ModelError::ParentApart { .. }
        | ModelError::ParentOffPicture { .. }
        | ModelError::ParentFlattened { .. }
        | ModelError::ParentTooDeep { .. }
        | ModelError::Overlap { .. }
        | ModelError::IncompatibleTrackKind { .. }
        | ModelError::NotOnPicture { .. }
        | ModelError::NotOnSound { .. }
        | ModelError::SourceOutOfBounds
        | ModelError::InvalidRange
        | ModelError::InvalidTransform { .. }
        | ModelError::InvalidParam { .. }
        | ModelError::UnknownMulticamGroup { .. }
        | ModelError::EmptyMulticamGroup
        | ModelError::DuplicateMulticamMedia { .. }
        | ModelError::MulticamAngleOutOfBounds { .. }
        | ModelError::InvalidMulticamCuts
        | ModelError::InvalidMulticamOffset { .. }
        | ModelError::ClipCarriesNoSound { .. }
        | ModelError::ClipSet { .. }
        | ModelError::Split { .. }
        | ModelError::MasksFull { .. }
        | ModelError::Slip { .. }
        | ModelError::FrozenMedia { .. }
        | ModelError::Generated { .. }
        | ModelError::Bake { .. }
        | ModelError::Freeze { .. } => true,
        ModelError::ProjectLoad { .. }
        | ModelError::TemplateLoad { .. }
        | ModelError::TemplateBundle { .. }
        | ModelError::InvalidProjectFile { .. }
        | ModelError::UnknownTrack { .. }
        | ModelError::UnknownMedia { .. }
        | ModelError::UnknownMarker { .. }
        | ModelError::UnknownBin { .. }
        | ModelError::BinCycle { .. }
        | ModelError::UnknownSequence { .. }
        | ModelError::SequenceCycle { .. }
        | ModelError::MediaReferenced { .. }
        | ModelError::RateMismatch { .. }
        | ModelError::TimeOverflow
        | ModelError::SlotMediaMismatch { .. }
        | ModelError::SlotDurationUnmet { .. }
        | ModelError::SlotRetimeUnsupported { .. }
        | ModelError::TooManyPicks { .. }
        | ModelError::NotABus { .. }
        | ModelError::LaneCarriesNoSound { .. }
        | ModelError::RoutingCycle { .. }
        | ModelError::DuplicateSend { .. }
        | ModelError::UnknownSend { .. }
        | ModelError::UnknownAudioEffect { .. }
        | ModelError::UnknownInsert { .. }
        | ModelError::InsertsFull { .. } => false,
    }
}

/// The copy for a render that cannot stand in for a clip: the run's error and
/// the reason Render and Replace's row is disabled, so the two read the same.
pub(crate) fn bake_copy(refusal: BakeRefusal) -> &'static str {
    match refusal {
        BakeRefusal::Rendered => {
            "That clip is already rendered. Choose Restore Unrendered, then render it again."
        }
        BakeRefusal::LaneBar => {
            "A lane bar reprocesses the tracks beneath it and has no picture of its own. Render \
             a clip with a picture instead."
        }
        BakeRefusal::Undressed => {
            "Nothing is applied to that clip, so a render would only re-encode its file. Apply \
             an effect, a look or a speed change first, then render it."
        }
    }
}

/// The copy for a cut that would change what a clip renders: the run's error
/// of a split and of the freeze that cuts there, and the reason Freeze Frame's
/// row is disabled for that cut.
pub(crate) fn split_copy(refusal: SplitRefusal) -> &'static str {
    match refusal {
        SplitRefusal::InsideFadeIn => {
            "That cut lands inside the clip's fade-in. Cut after the fade-in ends, or shorten \
             the fade-in first."
        }
        SplitRefusal::InsideFadeOut => {
            "That cut lands inside the clip's fade-out. Cut before the fade-out starts, or \
             shorten the fade-out first."
        }
        SplitRefusal::OffComboCycle => {
            "That cut splits a cycle of the clip's combo animation. Cut a whole cycle (about \
             one second) into the clip, or remove the combo animation first."
        }
        SplitRefusal::ShortensEntrance => {
            "That cut leaves the first part too short for the clip's entrance animation. Cut \
             later, or remove the entrance animation first."
        }
        SplitRefusal::ShortensExit => {
            "That cut leaves the second part too short for the clip's exit animation. Cut \
             earlier, or remove the exit animation first."
        }
        SplitRefusal::Lottie => "A Lottie clip can't be cut. Trim its ends instead.",
        SplitRefusal::AnimatedSticker => "An animated sticker can't be cut. Trim its ends instead.",
        SplitRefusal::RampKeyframeAtCut => {
            "That cut lands too close to a speed-ramp keyframe. Move the cut or the keyframe, \
             then try again."
        }
        SplitRefusal::OffClip => {
            "That cut is at or past an end of the clip. Cut between its first and last frame."
        }
    }
}

/// The copy for a frame a clip cannot hold: the run's error and the reason
/// Freeze Frame's row is disabled, so the two read the same.
pub(crate) fn freeze_copy(refusal: FreezeRefusal) -> &'static str {
    match refusal {
        FreezeRefusal::NotOnVideoTrack => {
            "A freeze frame holds a picture, and that clip is not on a video track. Select a clip \
             on a video track."
        }
        FreezeRefusal::TrackLocked => {
            "That clip's track is locked. Unlock the track, then freeze the frame."
        }
        FreezeRefusal::NotMedia => {
            "That clip does not play a video file, so it has no frame of its own to hold. Freeze \
             a clip of a video file instead."
        }
        FreezeRefusal::AlreadyFrozen => {
            "That clip is already a freeze frame. Trim it to hold the frame longer."
        }
        FreezeRefusal::NoVideo => {
            "That clip's media has no video frames. Freeze a clip of a video file instead."
        }
        FreezeRefusal::OffClip => {
            "The playhead is off that clip. Move the playhead onto the clip, then freeze the \
             frame."
        }
        FreezeRefusal::Entrance => {
            "The playhead is on the clip's first frame, where its entrance animation plays. Move \
             the playhead inside the clip or remove the animation."
        }
        FreezeRefusal::Exit => {
            "The playhead is on the clip's last frame, where its exit animation plays. Move the \
             playhead inside the clip or remove the animation."
        }
        FreezeRefusal::Linked => {
            "That clip is linked to a clip on a lane that does not ride along, which the \
             freeze would push out of sync. Unlink the clip, or unlock that lane and set its \
             sync lock, then freeze the frame."
        }
        FreezeRefusal::SplitsLink => {
            "A linked clip after the playhead would move apart from its partner on a lane that \
             does not ride along. Unlink it, or unlock that lane and set its sync lock, then \
             freeze the frame."
        }
    }
}

/// The copy for an export refused before it starts: an export with nothing to
/// write reads as the engine's copy for the same reason, and any other refusal
/// as the render path words it.
pub(crate) fn describe_export_refusal(err: &aria_render::RenderError) -> String {
    match err {
        aria_render::RenderError::Empty(reason) => empty_export_copy(*reason).into(),
        other => other.to_string(),
    }
}

/// Why an export has nothing to write, and the control that fixes it. The
/// refusal is typed, so the copy answers for each reason and a new one stops
/// compiling here instead of borrowing the wrong sentence.
fn empty_export_copy(reason: aria_render::EmptyExport) -> &'static str {
    match reason {
        aria_render::EmptyExport::Sequence => {
            "Nothing to export. Your timeline is empty. Import media and place a clip first."
        }
        aria_render::EmptyExport::Range => {
            "Nothing to export. The marked range covers no frames. Move the in and out points \
             apart."
        }
        aria_render::EmptyExport::Silence => {
            "Nothing to export. Nothing in this sequence is audible. Unmute a lane or raise its \
             fader, then try again."
        }
    }
}

/// The copy for a file the decoder could not read: the decoder's reason, then
/// the supported formats as the fix. A reason that ends with a full stop is a
/// sentence that names its own fix, such as GStreamer's "Install missing
/// plugins or pick another file.", and is not followed by the formats, which
/// would point at the wrong fix for a supported container missing a codec.
fn decode_copy(err: &aria_core::DecodeError) -> String {
    use aria_core::DecodeError;
    let (DecodeError::Open(reason)
    | DecodeError::Io(reason)
    | DecodeError::Decode(reason)
    | DecodeError::Seek(reason)
    | DecodeError::Unsupported(reason)) = err;
    match reason.trim() {
        "" => "Couldn't decode this file. Use a supported video, audio, or image format.".into(),
        reason if reason.ends_with('.') => format!("Couldn't decode this file: {reason}"),
        reason => format!(
            "Couldn't decode this file: {reason}. Use a supported video, audio, or image format."
        ),
    }
}
