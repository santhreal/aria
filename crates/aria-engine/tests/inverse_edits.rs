//! Inverse undo for all edit commands.

mod common;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::ApplyOutcome;
use aria_models::{
    CanvasAspect, ClipParam, ClipTransform, CropRect, Easing, Generator, Param, ParamValue, Shape,
    ShapeParam, TrackKind,
};
use common::{image_asset, import_asset, rt, small_video_asset, temp_engine, tr};

fn created(outcome: ApplyOutcome) -> aria_models::ClipId {
    match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    }
}

/// Add a text clip at [0, 48) and return its id — fixture for param tests.
fn text_clip(engine: &mut aria_engine::Engine) -> aria_models::ClipId {
    let track = common::add_track(engine, TrackKind::Text, "T1");
    created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::text("title"),
                timeline: tr(0, 48),
            }))
            .expect("add"),
    )
}

// --- markers (M1) -----------------------------------------------------------

fn created_marker(outcome: ApplyOutcome) -> aria_models::MarkerId {
    match outcome {
        ApplyOutcome::Edited(EditOutcome::CreatedMarker(id)) => id,
        other => panic!("expected CreatedMarker, got {other:?}"),
    }
}

/// Add an ellipse sticker at [0, 48) and return its id — fixture for shape
/// param tests.
fn shape_clip(engine: &mut aria_engine::Engine) -> aria_models::ClipId {
    let track = common::add_track(engine, TrackKind::Sticker, "S1");
    created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::shape(Shape::Ellipse, [255, 0, 0, 255]),
                timeline: tr(0, 48),
            }))
            .expect("add shape"),
    )
}

/// Borrow the shape generator fields of `clip_id` for assertions.
fn shape_generator(
    engine: &aria_engine::Engine,
    clip_id: aria_models::ClipId,
) -> aria_models::Generator {
    match &engine.project().clip(clip_id).unwrap().content {
        aria_models::ClipSource::Generated(generator) => generator.clone(),
        other => panic!("expected generated clip, got {other:?}"),
    }
}

#[path = "inverse_edits_markers.rs"]
mod markers;

#[path = "inverse_edits_shapes.rs"]
mod shapes;

#[path = "inverse_edits_effects.rs"]
mod effects;

#[path = "inverse_edits_looks_and_audio_role.rs"]
mod looks_and_audio_role;

#[path = "inverse_edits_mask_stack.rs"]
mod mask_stack;

#[path = "inverse_edits_params_and_keyframes_1.rs"]
mod params_and_keyframes_1;

#[path = "inverse_edits_params_and_keyframes_2.rs"]
mod params_and_keyframes_2;

#[path = "inverse_edits_generators.rs"]
mod generators;

#[path = "inverse_edits_parent.rs"]
mod parent;

#[path = "inverse_edits_structural_undo_1.rs"]
mod structural_undo_1;

#[path = "inverse_edits_structural_undo_2.rs"]
mod structural_undo_2;
