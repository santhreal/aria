//! Human-readable engine errors. Every message names the next action.

use aria_engine::EngineError;
use aria_models::{MAX_PARENT_DEPTH, ModelError};

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
        EngineError::Decode(_) => {
            "Couldn't decode this file. Use a supported video, audio, or image format.".into()
        }
        EngineError::Io(_) => {
            "Couldn't read or write that file. Check the path and that the disk is not full.".into()
        }
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
        | ModelError::InvalidProjectFile(_) => {
            return err.to_string();
        }
        ModelError::Overlap(_) => {
            "That would overlap another clip on the same track. Make room for it, then try again."
        }
        ModelError::IncompatibleTrackKind { .. } => {
            "That track can't hold this kind of clip. Drop it on a matching video or audio track."
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
                "{given} picks for a template with {slots} slots. Pick at most {slots}."
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
                "This multicam group has {total} angles. Pick an angle from 1 to {total}."
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
    };
    copy.into()
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
