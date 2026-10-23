use super::{describe_engine_error, describe_export_refusal};
use aria_engine::EngineError;

#[test]
fn missing_media_names_the_path_and_the_fix() {
    let msg = describe_engine_error(&EngineError::MissingMedia("/tmp/gone.mp4".into()));
    assert!(msg.contains("/tmp/gone.mp4"));
    assert!(msg.contains("Relink"));
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
