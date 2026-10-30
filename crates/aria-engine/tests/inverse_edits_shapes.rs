//! `inverse_edits` tests covering shapes. A `#[path]` child of the
//! `inverse_edits` test binary; the tests reach their fixtures through
//! `super::*`.

use super::*;

#[test]
fn shape_param_keyframe_undo_redo_roundtrip() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = shape_clip(&mut engine);
    let width_param = ClipParam::Shape {
        param: ShapeParam::Width,
    };

    for (tick, value) in [(0, 100.0), (40, 500.0)] {
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: clip_id,
                param: width_param,
                at: rt(tick),
                value: ParamValue::Scalar(value),
                easing: Easing::Linear,
                tangents: None,
            }))
            .expect("keyframe");
    }

    let width = |engine: &aria_engine::Engine| match shape_generator(engine, clip_id) {
        Generator::Shape { width, .. } => width,
        other => panic!("expected shape, got {other:?}"),
    };
    assert_eq!(width(&engine).keyframes().len(), 2);
    assert_eq!(width(&engine).sample(20), 300.0);

    // Undo peels one keyframe at a time, then restores the drop constant.
    assert!(engine.undo());
    assert_eq!(width(&engine).keyframes().len(), 1);
    assert!(engine.undo());
    assert!(!width(&engine).is_animated());

    assert!(engine.redo());
    assert!(engine.redo());
    assert_eq!(width(&engine).sample(20), 300.0);
}

#[test]
fn shape_fill_color_keyframes_animate_and_flatten() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = shape_clip(&mut engine);
    let fill_param = ClipParam::Shape {
        param: ShapeParam::Fill,
    };

    for (tick, rgba) in [(0, [0, 0, 0, 255]), (40, [200, 100, 0, 255])] {
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: clip_id,
                param: fill_param,
                at: rt(tick),
                value: ParamValue::Color(rgba),
                easing: Easing::Linear,
                tangents: None,
            }))
            .expect("keyframe");
    }

    let fill = |engine: &aria_engine::Engine| match shape_generator(engine, clip_id) {
        Generator::Shape { rgba, .. } => rgba,
        other => panic!("expected shape, got {other:?}"),
    };
    assert_eq!(fill(&engine).sample(20), [100, 50, 0, 255]);

    // Flattening to a constant drops the curve; undo restores it.
    engine
        .apply(Command::Edit(EditCommand::SetParamConstant {
            clip: clip_id,
            param: fill_param,
            value: ParamValue::Color([1, 2, 3, 255]),
        }))
        .expect("flatten");
    assert_eq!(fill(&engine).constant(), Some([1, 2, 3, 255]));

    assert!(engine.undo());
    assert_eq!(fill(&engine).keyframes().len(), 2);
    assert!(engine.redo());
    assert_eq!(fill(&engine).constant(), Some([1, 2, 3, 255]));
}

#[test]
fn shape_param_commands_reject_bad_targets_without_history() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = shape_clip(&mut engine);
    let before = engine.project().clip(clip_id).unwrap().clone();

    // Stroke params need a stroke; the fixture has none.
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: clip_id,
                param: ClipParam::Shape {
                    param: ShapeParam::StrokeWidth,
                },
                at: rt(0),
                value: ParamValue::Scalar(4.0),
                easing: Easing::Linear,
                tangents: None,
            }))
            .is_err()
    );
    // Color params reject scalar values (and vice versa).
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: clip_id,
                param: ClipParam::Shape {
                    param: ShapeParam::Fill,
                },
                at: rt(0),
                value: ParamValue::Scalar(0.5),
                easing: Easing::Linear,
                tangents: None,
            }))
            .is_err()
    );
    // Shape params are meaningless on non-shape clips.
    let text = text_clip(&mut engine);
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip: text,
                param: ClipParam::Shape {
                    param: ShapeParam::Width,
                },
                at: rt(0),
                value: ParamValue::Scalar(100.0),
                easing: Easing::Linear,
                tangents: None,
            }))
            .is_err()
    );

    assert_eq!(engine.project().clip(clip_id).unwrap(), &before);
    // Failed edits push no history: only the three setup steps (two tracks +
    // two generated clips = four) are undoable.
    assert!(engine.undo()); // add text clip
    assert!(engine.undo()); // add text track
    assert!(engine.undo()); // add shape clip
    assert!(engine.undo()); // add sticker track
    assert!(!engine.can_undo());
}

#[test]
fn set_generator_recolors_shape_with_undo() {
    let (_dir, mut engine) = temp_engine();
    let clip_id = shape_clip(&mut engine);

    // Recolor + re-shape in one SetGenerator, as the properties panel does.
    let mut generator = shape_generator(&engine, clip_id);
    let Generator::Shape { shape, rgba, .. } = &mut generator else {
        panic!("expected shape");
    };
    *shape = Shape::Star {
        points: 5,
        inner_ratio: aria_models::Param::Constant(0.5),
    };
    *rgba = aria_models::Param::Constant([0, 255, 0, 255]);
    engine
        .apply(Command::Edit(EditCommand::SetGenerator {
            clip: clip_id,
            generator,
        }))
        .expect("set generator");

    match shape_generator(&engine, clip_id) {
        Generator::Shape { shape, rgba, .. } => {
            assert!(matches!(shape, Shape::Star { points: 5, .. }));
            assert_eq!(rgba.constant(), Some([0, 255, 0, 255]));
        }
        other => panic!("expected shape, got {other:?}"),
    }

    assert!(engine.undo());
    match shape_generator(&engine, clip_id) {
        Generator::Shape { shape, rgba, .. } => {
            assert_eq!(shape, Shape::Ellipse);
            assert_eq!(rgba.constant(), Some([255, 0, 0, 255]));
        }
        other => panic!("expected shape, got {other:?}"),
    }

    // Invalid generators are rejected at the engine boundary too.
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetGenerator {
                clip: clip_id,
                generator: Generator::shape(Shape::Polygon { sides: 2 }, [255, 255, 255, 255]),
            }))
            .is_err()
    );
}
