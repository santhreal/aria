use std::collections::BTreeSet;

use aria_engine::EngineError;
use aria_models::{
    BinId, ClipId, MarkerId, MediaId, MediaKind, ModelError, MulticamGroupId, Rational, SequenceId,
    SlotMedia, TrackId, TrackKind,
};

use super::{describe_engine_error, describe_export_refusal};

#[test]
fn missing_media_names_the_path_and_the_fix() {
    let msg = describe_engine_error(&EngineError::MissingMedia(vec!["/tmp/gone.mp4".into()]));
    assert_eq!(
        msg,
        "Couldn't find media at /tmp/gone.mp4. Move the file back or click Relink."
    );
}

/// WHY: an Open with several missing files named one of them as if it were
/// the only one. The copy counts them and names the first.
#[test]
fn several_missing_files_are_counted() {
    let paths = vec!["/a.mp4".into(), "/b.mp4".into(), "/c.mp4".into()];
    let msg = describe_engine_error(&EngineError::MissingMedia(paths));
    assert_eq!(
        msg,
        "Couldn't find 3 media files, starting with /a.mp4. Move them back or click Relink All."
    );
}

#[test]
fn export_names_disk_as_the_fix() {
    let msg = describe_engine_error(&EngineError::Export("disk full".into()));
    assert!(msg.contains("disk"));
    assert!(msg.contains("export"));
}

#[test]
fn decode_points_at_supported_formats() {
    let msg = describe_engine_error(&EngineError::Decode(aria_core::DecodeError::Unsupported(
        "xyz".into(),
    )));
    assert!(msg.contains("supported"));
}

/// Each reason an export can be empty gets copy of its own, naming the control
/// that fixes it: a sequence with nothing in it needs a clip, a marked range
/// that covers no frame needs the in/out points moved, and a silent sound
/// export needs a fader. Derived from
/// [`aria_render::EmptyExport::ALL`], so a new reason without its own sentence
/// fails here rather than pointing half the reasons at the wrong control.
#[test]
fn empty_export_names_what_is_missing() {
    let mut seen: Vec<String> = Vec::new();
    for reason in aria_render::EmptyExport::ALL {
        let msg = describe_engine_error(&EngineError::Render(aria_render::RenderError::Empty(
            reason,
        )));
        assert!(msg.contains("Nothing to export"), "{reason:?}: {msg}");
        assert!(
            !seen.contains(&msg),
            "{reason:?} reuses another reason's copy: {msg}"
        );
        seen.push(msg);
    }
    assert_eq!(seen.len(), aria_render::EmptyExport::ALL.len());

    let empty = describe_engine_error(&EngineError::Render(aria_render::RenderError::Empty(
        aria_render::EmptyExport::Sequence,
    )));
    assert!(empty.contains("empty"), "{empty}");
    assert!(empty.contains("place a clip"), "{empty}");

    let ranged = describe_engine_error(&EngineError::Render(aria_render::RenderError::Empty(
        aria_render::EmptyExport::Range,
    )));
    assert!(ranged.contains("range"), "{ranged}");
    assert!(ranged.contains("in and out"), "{ranged}");
}

/// An export refused before its job exists reads as the same export refused
/// by the engine: the sheet and the engine are two doors onto one refusal, and
/// two wordings for it read as two problems. Derived from
/// [`aria_render::EmptyExport::ALL`], so a reason the sheet words on its own
/// fails here. A refusal that is not about emptiness keeps the render path's
/// words, which name the format or encoder that was turned down.
///
/// Not covered: whether a caller posts this copy rather than the error's own
/// text; `jobs_tests::export_refuses_an_empty_timeline_before_spawning` reads
/// that at the sheet.
#[test]
fn an_export_refused_early_reads_as_the_engine_words_it() {
    for reason in aria_render::EmptyExport::ALL {
        let refusal = aria_render::RenderError::Empty(reason);
        let engine = describe_engine_error(&EngineError::Render(aria_render::RenderError::Empty(
            reason,
        )));
        assert_eq!(describe_export_refusal(&refusal), engine, "{reason:?}");
        assert_ne!(
            describe_export_refusal(&refusal),
            refusal.to_string(),
            "{reason:?} posts the render path's words, not the shell's copy"
        );
    }

    let unwritable = aria_render::RenderError::from(aria_render::ExportFormatError::NoEncoder);
    assert_eq!(describe_export_refusal(&unwritable), unwritable.to_string());
}

#[test]
fn corrupt_project_names_the_file_and_the_fix() {
    let msg = describe_engine_error(&EngineError::Model(
        aria_models::ModelError::InvalidProjectFile("bad json".into()),
    ));
    assert!(msg.contains("corrupted"));
    assert!(msg.contains(".aria"));
}

/// Variants of [`ModelError`]; [`variant`] numbers them `0..VARIANTS`.
const VARIANTS: usize = 46;

/// Where `err` sits in [`ModelError`]. The match has no catch-all, so a new
/// variant stops compiling here until it is numbered, and
/// [`every_model_refusal_names_its_obstacle_and_the_fix`] then fails until
/// [`every_refusal`] holds one of it.
fn variant(err: &ModelError) -> usize {
    match err {
        ModelError::TemplateLoad(_) => 0,
        ModelError::InvalidProjectFile(_) => 1,
        ModelError::UnknownTrack(_) => 2,
        ModelError::UnknownMedia(_) => 3,
        ModelError::UnknownClip(_) => 4,
        ModelError::DuplicateClip(_) => 5,
        ModelError::UnknownMarker(_) => 6,
        ModelError::UnknownBin(_) => 7,
        ModelError::BinCycle(_) => 8,
        ModelError::UnknownSequence(_) => 9,
        ModelError::SequenceCycle(_) => 10,
        ModelError::ParentCycle(_) => 11,
        ModelError::ParentElsewhere { .. } => 12,
        ModelError::ParentApart { .. } => 13,
        ModelError::ParentOffPicture(_) => 14,
        ModelError::ParentFlattened(_) => 15,
        ModelError::ParentTooDeep(_) => 16,
        ModelError::Overlap(_) => 17,
        ModelError::IncompatibleTrackKind { .. } => 18,
        ModelError::MediaReferenced(_) => 19,
        ModelError::SourceOutOfBounds => 20,
        ModelError::InvalidRange => 21,
        ModelError::InvalidTransform(_) => 22,
        ModelError::InvalidParam(_) => 23,
        ModelError::RateMismatch { .. } => 24,
        ModelError::TimeOverflow => 25,
        ModelError::SlotMediaMismatch { .. } => 26,
        ModelError::SlotDurationUnmet { .. } => 27,
        ModelError::SlotRetimeUnsupported { .. } => 28,
        ModelError::TooManyPicks { .. } => 29,
        ModelError::UnknownMulticamGroup(_) => 30,
        ModelError::EmptyMulticamGroup => 31,
        ModelError::DuplicateMulticamMedia(_) => 32,
        ModelError::MulticamAngleOutOfBounds { .. } => 33,
        ModelError::InvalidMulticamCuts => 34,
        ModelError::InvalidMulticamOffset(_) => 35,
        ModelError::NotABus(_) => 36,
        ModelError::LaneCarriesNoSound(_) => 37,
        ModelError::RoutingCycle(_) => 38,
        ModelError::DuplicateSend { .. } => 39,
        ModelError::UnknownSend { .. } => 40,
        ModelError::UnknownAudioEffect(_) => 41,
        ModelError::ClipCarriesNoSound(_) => 42,
        ModelError::UnknownInsert { .. } => 43,
        ModelError::InsertsFull(_) => 44,
        ModelError::ProjectLoad(_) => 45,
    }
}

/// One refusal of each [`ModelError`] variant, as the model raises it.
fn every_refusal() -> Vec<ModelError> {
    let track = TrackId::from_raw(2);
    let media = MediaId::from_raw(3);
    let clip = ClipId::from_raw(4);
    let other = ClipId::from_raw(5);
    let bin = BinId::from_raw(6);
    let sequence = SequenceId::from_raw(7);
    vec![
        ModelError::TemplateLoad(aria_models::ProjectLoadError::NotFound {
            path: "missing.ariat".into(),
        }),
        ModelError::InvalidProjectFile("expected value at line 1".into()),
        ModelError::UnknownTrack(track),
        ModelError::UnknownMedia(media),
        ModelError::UnknownClip(clip),
        ModelError::DuplicateClip(clip),
        ModelError::UnknownMarker(MarkerId::from_raw(8)),
        ModelError::UnknownBin(bin),
        ModelError::BinCycle(bin),
        ModelError::UnknownSequence(sequence),
        ModelError::SequenceCycle(sequence),
        ModelError::ParentCycle(clip),
        ModelError::ParentElsewhere {
            child: clip,
            parent: other,
        },
        ModelError::ParentApart {
            child: clip,
            parent: other,
        },
        ModelError::ParentOffPicture(clip),
        ModelError::ParentFlattened(other),
        ModelError::ParentTooDeep(clip),
        ModelError::Overlap(track),
        ModelError::IncompatibleTrackKind {
            track,
            kind: TrackKind::Audio,
        },
        ModelError::MediaReferenced(media),
        ModelError::SourceOutOfBounds,
        ModelError::InvalidRange,
        ModelError::InvalidTransform("scale is not finite".into()),
        ModelError::InvalidParam("mask index 3 out of range".into()),
        ModelError::RateMismatch {
            expected: Rational::new(24, 1),
            got: Rational::new(30, 1),
        },
        ModelError::TimeOverflow,
        ModelError::SlotMediaMismatch {
            slot: clip,
            accepts: SlotMedia::VideoOnly,
            found: MediaKind::Audio,
        },
        ModelError::SlotDurationUnmet { slot: clip },
        ModelError::SlotRetimeUnsupported { slot: clip },
        ModelError::TooManyPicks { given: 4, slots: 3 },
        ModelError::UnknownMulticamGroup(MulticamGroupId::from_raw(9)),
        ModelError::EmptyMulticamGroup,
        ModelError::DuplicateMulticamMedia(media),
        ModelError::MulticamAngleOutOfBounds { index: 5, total: 3 },
        ModelError::InvalidMulticamCuts,
        ModelError::InvalidMulticamOffset("NaN".into()),
        ModelError::NotABus(track),
        ModelError::LaneCarriesNoSound(track),
        ModelError::RoutingCycle(track),
        ModelError::DuplicateSend {
            track,
            to: TrackId::from_raw(10),
        },
        ModelError::UnknownSend { track, index: 2 },
        ModelError::UnknownAudioEffect("reverb".into()),
        ModelError::ClipCarriesNoSound(clip),
        ModelError::UnknownInsert { index: 4, len: 2 },
        ModelError::InsertsFull(8),
        ModelError::ProjectLoad(aria_models::ProjectLoadError::NotFound {
            path: "missing.aria".into(),
        }),
    ]
}

/// Every model refusal reads as its own obstacle and the fix for it: an
/// obstacle sentence, then an action sentence, with no id, debug value or
/// placeholder of the model's own in it.
///
/// Closes "a refusal reads as a shrug": every model error but the two project
/// file ones read "Couldn't apply that edit. Undo, then try again.", so a ramp
/// refused for overlapping the next clip read as a fault to undo rather than
/// a clip to make room for. Derived from [`variant`], so a new refusal fails
/// here until it has copy and a case. Not covered: whether a sentence names
/// the right fix, which is read in the copy itself.
#[test]
fn every_model_refusal_names_its_obstacle_and_the_fix() {
    let refusals = every_refusal();
    let numbered: BTreeSet<usize> = refusals.iter().map(variant).collect();
    assert_eq!(
        numbered,
        (0..VARIANTS).collect(),
        "every_refusal misses a ModelError variant"
    );
    for err in refusals {
        let what = format!("{err:?}");
        let copy = describe_engine_error(&EngineError::Model(err));
        assert!(
            !copy.contains("Couldn't apply that edit"),
            "{what} reads as the generic refusal: {copy}"
        );
        assert!(
            copy.ends_with('.') && copy.contains(". "),
            "{what} is not an obstacle and a fix: {copy}"
        );
        for token in ["#", "{", "}", "Id(", "Rational", "Some("] {
            assert!(
                !copy.contains(token),
                "{what} shows the model's own {token:?}: {copy}"
            );
        }
    }
}
