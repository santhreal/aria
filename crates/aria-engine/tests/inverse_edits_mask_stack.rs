//! `inverse_edits` tests for the mask stack commands and for the inverse of a
//! property write; a `#[path]` child of the `inverse_edits` test binary.

use aria_models::{
    ChromaKey, Clip, ClipId, ColorAdjustments, DriverCode, Filter, LayerShadow, LayerStyles, Lut,
    MAX_CLIP_MASKS, Mask, MaskKind, MaskMode, MaskParam, MatteSource, ParamRange, TrackMatte,
    clip_params_for,
};

use super::*;

fn clip_of(engine: &aria_engine::Engine, clip: ClipId) -> Clip {
    engine.project().clip(clip).expect("clip").clone()
}

fn kinds(clip: &Clip) -> Vec<(MaskKind, MaskMode)> {
    clip.masks
        .iter()
        .map(|mask| (mask.kind, mask.mode))
        .collect()
}

fn edit(engine: &mut aria_engine::Engine, command: EditCommand) {
    engine
        .apply(Command::Edit(command.clone()))
        .unwrap_or_else(|e| panic!("{command:?}: {e}"));
}

/// Each stack edit is one history entry, and undo walks the stack back through
/// every state it passed while redo walks it forward again.
#[test]
fn mask_stack_edits_undo_and_redo_one_at_a_time() {
    let (_dir, mut engine) = temp_engine();
    let clip = shape_clip(&mut engine);
    let mut states = vec![clip_of(&engine, clip)];
    let subtract = Mask {
        mode: MaskMode::Subtract,
        opacity: 0.5.into(),
        ..Mask::new(MaskKind::Rectangle)
    };
    let steps = [
        EditCommand::AddClipMask {
            clip,
            mask: Mask::new(MaskKind::Circle),
        },
        EditCommand::AddClipMask {
            clip,
            mask: subtract.clone(),
        },
        EditCommand::SetClipMask {
            clip,
            index: 1,
            mask: Mask {
                expansion: 24.0.into(),
                ..subtract
            },
        },
        EditCommand::MoveClipMask {
            clip,
            from_index: 0,
            to_index: 1,
        },
        EditCommand::RemoveClipMask { clip, index: 0 },
    ];
    for step in steps {
        edit(&mut engine, step);
        states.push(clip_of(&engine, clip));
    }
    assert_eq!(
        kinds(&states[4]),
        [
            (MaskKind::Rectangle, MaskMode::Subtract),
            (MaskKind::Circle, MaskMode::Add)
        ],
        "the move put the rectangle on top"
    );
    assert_eq!(states[3].masks[1].expansion.sample(0), 24.0);
    assert_eq!(kinds(&states[5]), [(MaskKind::Circle, MaskMode::Add)]);
    for want in states.iter().rev().skip(1) {
        assert!(engine.undo());
        assert_eq!(&clip_of(&engine, clip), want);
    }
    for want in &states[1..] {
        assert!(engine.redo());
        assert_eq!(&clip_of(&engine, clip), want);
    }
}

/// Removing a mask drops the drivers on its properties and moves the drivers on
/// the masks below it up one index; moving a mask carries its drivers. Undo
/// puts every driver back at the index it had.
#[test]
fn mask_drivers_follow_their_masks_through_edits_and_undo() {
    let (_dir, mut engine) = temp_engine();
    let clip = shape_clip(&mut engine);
    for kind in [MaskKind::Circle, MaskKind::Rectangle, MaskKind::Star] {
        edit(
            &mut engine,
            EditCommand::AddClipMask {
                clip,
                mask: Mask::new(kind),
            },
        );
    }
    let param = |mask: u32| ClipParam::Mask {
        mask,
        param: MaskParam::Rotation,
    };
    let code = |n: u32| DriverCode::new("aria.expressions", format!("time * {n}"));
    for mask in 0..3 {
        edit(
            &mut engine,
            EditCommand::SetParamDriver {
                clip,
                param: param(mask),
                code: Some(code(mask)),
            },
        );
    }
    let drivers = |engine: &aria_engine::Engine| -> Vec<Option<DriverCode>> {
        let clip = clip_of(engine, clip);
        (0..3)
            .map(|mask| clip.driver(param(mask)).cloned())
            .collect()
    };
    let attached = drivers(&engine);
    assert_eq!(attached, [Some(code(0)), Some(code(1)), Some(code(2))]);

    edit(&mut engine, EditCommand::RemoveClipMask { clip, index: 1 });
    assert_eq!(drivers(&engine), [Some(code(0)), Some(code(2)), None]);
    edit(
        &mut engine,
        EditCommand::MoveClipMask {
            clip,
            from_index: 0,
            to_index: 1,
        },
    );
    assert_eq!(drivers(&engine), [Some(code(2)), Some(code(0)), None]);
    assert_eq!(
        kinds(&clip_of(&engine, clip)),
        [
            (MaskKind::Star, MaskMode::Add),
            (MaskKind::Circle, MaskMode::Add)
        ]
    );

    assert!(engine.undo());
    assert!(engine.undo());
    assert_eq!(drivers(&engine), attached);
}

/// An index outside the stack, and a mask past the stack's limit, are refused
/// with no history entry and no change to the clip.
#[test]
fn mask_edits_outside_the_stack_are_refused_without_history() {
    let (_dir, mut engine) = temp_engine();
    let clip = shape_clip(&mut engine);
    for _ in 0..MAX_CLIP_MASKS {
        edit(
            &mut engine,
            EditCommand::AddClipMask {
                clip,
                mask: Mask::new(MaskKind::Circle),
            },
        );
    }
    let before = clip_of(&engine, clip);
    let depth = engine.history_depth();
    let last = MAX_CLIP_MASKS as u32;
    for refused in [
        EditCommand::AddClipMask {
            clip,
            mask: Mask::new(MaskKind::Star),
        },
        EditCommand::SetClipMask {
            clip,
            index: last,
            mask: Mask::new(MaskKind::Star),
        },
        EditCommand::RemoveClipMask { clip, index: last },
        EditCommand::MoveClipMask {
            clip,
            from_index: 0,
            to_index: last,
        },
        EditCommand::MoveClipMask {
            clip,
            from_index: last,
            to_index: 0,
        },
        EditCommand::SetClipMask {
            clip,
            index: 0,
            mask: Mask {
                opacity: 1.5.into(),
                ..Mask::new(MaskKind::Star)
            },
        },
    ] {
        let what = format!("{refused:?}");
        assert!(
            engine.apply(Command::Edit(refused)).is_err(),
            "{what} was accepted"
        );
        assert_eq!(clip_of(&engine, clip), before, "{what} changed the clip");
        assert_eq!(engine.history_depth(), depth, "{what} left history");
    }
}

/// `value` moved a tenth of `range` (or a quarter where the range is open),
/// held inside the range, the other way when the first way is at a bound.
fn moved(value: ParamValue, range: ParamRange) -> ParamValue {
    let step = |v: f32| {
        let d = range.span().map_or(0.25, |span| span * 0.1);
        let up = range.clamp(v + d);
        if up == v { range.clamp(v - d) } else { up }
    };
    match value {
        ParamValue::Scalar(v) => ParamValue::Scalar(step(v)),
        ParamValue::Vec2([x, y]) => ParamValue::Vec2([step(x), step(y)]),
        ParamValue::Vec3([x, y, z]) => ParamValue::Vec3([step(x), step(y), step(z)]),
        ParamValue::Color([r, g, b, a]) => ParamValue::Color([255 - r, g, b, a]),
        ParamValue::Rect(_) => ParamValue::Rect([0.1, 0.1, 0.8, 0.8]),
    }
}

/// Undoing a constant written to any property restores the whole clip, for
/// every row the property catalog offers on a clip with every look block, a
/// layer style, an effect and a two-mask stack.
///
/// WHY: the inverse of a look-property write captured the grade alone, so undo
/// left a filter, LUT, chroma-key or matte strength write standing. The rows
/// come from `clip_params_for`, so a new property, block or mask property is
/// covered once the fixture offers it. Not caught: a block this fixture does
/// not carry, such as text or shape properties, which `inverse_edits_shapes`
/// covers.
#[test]
fn undoing_a_constant_write_restores_every_property() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");
    let clip = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );
    for dress in [
        EditCommand::SetClipFilter {
            clip,
            filter: Some(Filter::new("noir")),
        },
        EditCommand::SetClipLut {
            clip,
            lut: Some(Lut::new("look.cube")),
        },
        EditCommand::SetClipAdjustments {
            clip,
            adjust: ColorAdjustments {
                exposure: 0.5.into(),
                ..Default::default()
            },
        },
        EditCommand::SetClipChroma {
            clip,
            chroma: Some(ChromaKey {
                rgb: [0, 255, 0],
                strength: 0.7.into(),
                shadow: 0.1.into(),
            }),
        },
        EditCommand::SetClipMatte {
            clip,
            matte: Some(TrackMatte {
                source: MatteSource::Luma,
                invert: false,
                softness: 0.2.into(),
            }),
        },
        EditCommand::SetClipLayerStyles {
            clip,
            styles: LayerStyles {
                shadow: Some(LayerShadow {
                    rgba: Param::Constant([0, 0, 0, 128]),
                    offset: Param::Constant([4.0, 4.0]),
                    blur: Param::Constant(8.0),
                }),
                ..Default::default()
            },
        },
        EditCommand::AddEffect {
            clip,
            effect_id: "gaussian_blur".into(),
        },
        EditCommand::AddClipMask {
            clip,
            mask: Mask::new(MaskKind::Rectangle),
        },
        EditCommand::AddClipMask {
            clip,
            mask: Mask {
                mode: MaskMode::Subtract,
                ..Mask::new(MaskKind::Circle)
            },
        },
    ] {
        edit(&mut engine, dress);
    }
    let dressed = clip_of(&engine, clip);
    let rows: Vec<_> = clip_params_for(&dressed)
        .into_iter()
        .filter(|spec| spec.param != ClipParam::Speed)
        .collect();
    for (group, needs) in [
        (
            "look",
            ClipParam::Look {
                param: aria_models::LookParam::ChromaStrength,
            },
        ),
        (
            "mask",
            ClipParam::Mask {
                mask: 1,
                param: MaskParam::Expansion,
            },
        ),
    ] {
        assert!(
            rows.iter().any(|spec| spec.param == needs),
            "the fixture offers no {group} row {needs:?}"
        );
    }
    for spec in rows {
        let before = clip_of(&engine, clip);
        let current = spec.sample(&before, 0).expect("an offered row samples");
        let value = moved(current, spec.range);
        assert_ne!(value, current, "{}: no value to move to", spec.id);
        edit(
            &mut engine,
            EditCommand::SetParamConstant {
                clip,
                param: spec.param,
                value,
            },
        );
        assert_ne!(
            clip_of(&engine, clip),
            before,
            "{}: the write changed nothing",
            spec.id
        );
        assert!(engine.undo(), "{}: nothing to undo", spec.id);
        assert_eq!(
            clip_of(&engine, clip),
            before,
            "{}: undo left the clip changed",
            spec.id
        );
    }
}
