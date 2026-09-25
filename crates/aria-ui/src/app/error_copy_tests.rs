use super::describe_engine_error;
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
/// fails here rather than sending half the operators to the wrong control.
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

#[test]
fn corrupt_project_names_the_file_and_the_fix() {
    let msg = describe_engine_error(&EngineError::Model(
        aria_models::ModelError::InvalidProjectFile("bad json".into()),
    ));
    assert!(msg.contains("corrupted"));
    assert!(msg.contains(".aria"));
}
