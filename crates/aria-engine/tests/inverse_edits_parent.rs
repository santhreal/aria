//! `inverse_edits` tests covering clip parenting: the parent and the rewritten
//! transform leave on undo and come back on redo as one step, and a refused
//! parent leaves the project and the undo stack as they were.

use super::*;
use aria_models::{ParentKeep, Scale2};

/// Two shapes on two lanes: the parent turned a quarter and doubled over
/// `[12, 60)`, the child placed right of centre over `[0, 48)`.
fn parent_and_child(
    engine: &mut aria_engine::Engine,
) -> (aria_models::ClipId, aria_models::ClipId) {
    let child = shape_clip(engine);
    let track = common::add_track(engine, TrackKind::Sticker, "S2");
    let parent = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::shape(Shape::Rectangle, [0, 0, 255, 255]),
                timeline: tr(12, 48),
            }))
            .expect("add the parent"),
    );
    for (clip, transform) in [
        (
            parent,
            ClipTransform {
                position: [0.25, 0.0],
                rotation: 90.0,
                scale: Scale2 { x: 2.0, y: 2.0 },
                ..ClipTransform::IDENTITY
            },
        ),
        (
            child,
            ClipTransform {
                position: [0.1, 0.0],
                ..ClipTransform::IDENTITY
            },
        ),
    ] {
        engine
            .apply(Command::Edit(EditCommand::SetClipTransform {
                clip,
                transform,
                at: None,
            }))
            .expect("place the clip");
    }
    (parent, child)
}

fn clip(engine: &aria_engine::Engine, id: aria_models::ClipId) -> aria_models::Clip {
    engine.project().clip(id).expect("the clip").clone()
}

#[test]
fn undo_takes_the_parent_and_the_rewritten_transform_off_together() {
    for keep in [ParentKeep::Place, ParentKeep::Snap] {
        let (_dir, mut engine) = temp_engine();
        let (parent, child) = parent_and_child(&mut engine);
        let before = clip(&engine, child);
        engine
            .apply(Command::Edit(EditCommand::SetClipParent {
                clip: child,
                parent: Some(parent),
                at: rt(24),
                keep,
            }))
            .expect("parent the clip");
        let after = clip(&engine, child);
        assert_eq!(after.parent, Some(parent), "{keep:?}");
        assert_ne!(
            after.transform, before.transform,
            "{keep:?}: the transform is rewritten into the parent's frame"
        );

        assert!(engine.undo(), "{keep:?}: parenting records an undo step");
        assert_eq!(clip(&engine, child), before, "{keep:?}: undo");
        assert!(engine.redo(), "{keep:?}: the step redoes");
        assert_eq!(clip(&engine, child), after, "{keep:?}: redo");
    }
}

#[test]
fn releasing_restores_the_parent_on_undo() {
    let (_dir, mut engine) = temp_engine();
    let (parent, child) = parent_and_child(&mut engine);
    let parent_once = |engine: &mut aria_engine::Engine, to: Option<aria_models::ClipId>| {
        engine
            .apply(Command::Edit(EditCommand::SetClipParent {
                clip: child,
                parent: to,
                at: rt(24),
                keep: ParentKeep::Place,
            }))
            .expect("set the parent")
    };
    parent_once(&mut engine, Some(parent));
    let parented = clip(&engine, child);
    parent_once(&mut engine, None);
    assert_eq!(clip(&engine, child).parent, None);

    assert!(engine.undo());
    assert_eq!(clip(&engine, child), parented);
}

#[test]
fn a_refused_parent_changes_nothing_and_records_no_step() {
    let (_dir, mut engine) = temp_engine();
    let (parent, child) = parent_and_child(&mut engine);
    engine
        .apply(Command::Edit(EditCommand::SetClipParent {
            clip: child,
            parent: Some(parent),
            at: rt(24),
            keep: ParentKeep::Place,
        }))
        .expect("parent the clip");
    let (parent_before, child_before) = (clip(&engine, parent), clip(&engine, child));

    let refused = engine.apply(Command::Edit(EditCommand::SetClipParent {
        clip: parent,
        parent: Some(child),
        at: rt(24),
        keep: ParentKeep::Snap,
    }));
    assert!(refused.is_err(), "a cycle is refused: {refused:?}");
    assert_eq!(clip(&engine, parent), parent_before);
    assert_eq!(clip(&engine, child), child_before);

    // The step on top is still the parenting, not the refusal.
    assert!(engine.undo());
    assert_eq!(clip(&engine, child).parent, None);
    assert_eq!(clip(&engine, parent), parent_before);
}
