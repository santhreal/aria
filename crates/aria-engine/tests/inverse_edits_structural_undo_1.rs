//! `inverse_edits` tests covering structural undo (part 1). A `#[path]` child
//! of the `inverse_edits` test binary; the tests reach their fixtures through
//! `super::*`.

use super::*;

#[test]
fn undo_split_restores_single_clip() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::SplitClip {
            clip: clip_id,
            at: rt(24),
        }))
        .expect("split");
    assert_eq!(engine.project().timeline().clip_count(), 2);

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().clip_count(), 1);
    assert_eq!(
        engine
            .project()
            .clip(clip_id)
            .unwrap()
            .timeline
            .duration
            .value,
        48
    );
}

#[test]
fn redo_split_after_undo() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::SplitClip {
            clip: clip_id,
            at: rt(24),
        }))
        .expect("split");

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().clip_count(), 1);

    assert!(engine.redo());
    assert_eq!(engine.project().timeline().clip_count(), 2);
    assert_eq!(
        engine
            .project()
            .clip(clip_id)
            .unwrap()
            .timeline
            .duration
            .value,
        24
    );
}

#[test]
fn undo_ripple_delete_restores_gap() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Sticker, "T1");

    let first_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::SolidColor {
                    rgba: [0, 0, 0, 255],
                },
                timeline: tr(0, 10),
            }))
            .expect("first"),
    );

    let second_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::SolidColor {
                    rgba: [255, 0, 0, 255],
                },
                timeline: tr(20, 10),
            }))
            .expect("second"),
    );

    engine
        .apply(Command::Edit(EditCommand::RippleDelete { clip: first_id }))
        .expect("ripple");
    assert_eq!(engine.project().clip(second_id).unwrap().start().value, 10);

    assert!(engine.undo());
    assert!(engine.project().clip(first_id).is_some());
    assert_eq!(engine.project().clip(second_id).unwrap().start().value, 20);
}

#[test]
fn undo_ripple_delete_middle_of_three_adjacent() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let a = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(0, 10),
            }))
            .expect("a"),
    );
    let b = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(10, 10),
            }))
            .expect("b"),
    );
    let c = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(20, 10),
            }))
            .expect("c"),
    );

    engine
        .apply(Command::Edit(EditCommand::RippleDelete { clip: b }))
        .expect("ripple middle");

    assert!(engine.project().clip(b).is_none());
    assert_eq!(engine.project().clip(a).unwrap().start().value, 0);
    assert_eq!(engine.project().clip(c).unwrap().start().value, 10);

    assert!(engine.undo());
    assert!(engine.project().clip(b).is_some());
    assert_eq!(engine.project().clip(a).unwrap().start().value, 0);
    assert_eq!(engine.project().clip(b).unwrap().start().value, 10);
    assert_eq!(engine.project().clip(c).unwrap().start().value, 20);
}

#[test]
fn trim_extends_image_clip_past_default_placement() {
    let Some(png) = image_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &png);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");
    let source = engine.project().media(media_id).unwrap().full_range();

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source,
                start: rt(0),
            }))
            .expect("add"),
    );

    // Default 5s placement at 24fps timeline.
    assert_eq!(engine.project().clip(clip_id).unwrap().timeline, tr(0, 120));

    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: clip_id,
            timeline: tr(0, 240),
        }))
        .expect("extend still to 10s");

    let clip = engine.project().clip(clip_id).unwrap();
    assert_eq!(clip.timeline, tr(0, 240));
    let source = clip.source_range().unwrap();
    assert!(source.duration.value > 5_000);
}

#[test]
fn trim_video_past_source_bounds_fails() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );

    let err = engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: clip_id,
            timeline: tr(0, 10_000),
        }))
        .unwrap_err();
    assert!(
        format!("{err}").contains("bounds") || format!("{err}").contains("Source"),
        "expected source bounds error, got {err}"
    );
    assert_eq!(engine.project().clip(clip_id).unwrap().timeline, tr(0, 48));
}

#[test]
fn undo_trim_restores_timeline_and_source() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media: media_id,
                source: tr(0, 48),
                start: rt(0),
            }))
            .expect("add"),
    );

    let before_tl = engine.project().clip(clip_id).unwrap().timeline;
    let before_src = engine
        .project()
        .clip(clip_id)
        .unwrap()
        .source_range()
        .unwrap();

    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: clip_id,
            timeline: tr(10, 28),
        }))
        .expect("trim");

    assert_ne!(engine.project().clip(clip_id).unwrap().timeline, before_tl);

    assert!(engine.undo());
    let clip = engine.project().clip(clip_id).unwrap();
    assert_eq!(clip.timeline, before_tl);
    assert_eq!(clip.source_range().unwrap(), before_src);
}

#[test]
fn undo_move_across_tracks() {
    let (_dir, mut engine) = temp_engine();
    let v1 = common::add_track(&mut engine, TrackKind::Text, "T1");
    let v2 = common::add_track(&mut engine, TrackKind::Text, "T2");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track: v1,
                generator: Generator::text("title"),
                timeline: tr(5, 20),
            }))
            .expect("add"),
    );

    engine
        .apply(Command::Edit(EditCommand::MoveClip {
            clip: clip_id,
            to_track: v2,
            start: rt(40),
        }))
        .expect("move");

    assert_eq!(engine.project().timeline().track_of(clip_id), Some(v2));
    assert_eq!(engine.project().clip(clip_id).unwrap().start().value, 40);

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().track_of(clip_id), Some(v1));
    assert_eq!(engine.project().clip(clip_id).unwrap().start().value, 5);
}

#[test]
fn undo_remove_clip_restores_with_gap() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let a = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(0, 10),
            }))
            .expect("a"),
    );
    let b = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(20, 10),
            }))
            .expect("b"),
    );

    engine
        .apply(Command::Edit(EditCommand::RemoveClip { clip: a }))
        .expect("remove");

    assert!(engine.project().clip(a).is_none());
    assert_eq!(engine.project().clip(b).unwrap().start().value, 20);

    assert!(engine.undo());
    assert!(engine.project().clip(a).is_some());
    assert_eq!(engine.project().clip(a).unwrap().start().value, 0);
    assert_eq!(engine.project().clip(b).unwrap().start().value, 20);
}

#[test]
fn undo_add_generated() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Sticker, "S1");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::SolidColor { rgba: [1, 2, 3, 4] },
                timeline: tr(0, 24),
            }))
            .expect("add"),
    );

    assert!(engine.undo());
    assert!(engine.project().clip(clip_id).is_none());

    assert!(engine.redo());
    assert!(engine.project().clip(clip_id).is_some());
}

#[test]
fn failed_split_does_not_push_undo() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(0, 48),
            }))
            .expect("add"),
    );

    let err = engine
        .apply(Command::Edit(EditCommand::SplitClip {
            clip: clip_id,
            at: rt(0),
        }))
        .unwrap_err();
    assert!(format!("{err}").contains("range") || format!("{err}").contains("Invalid"));
    assert_eq!(engine.project().timeline().clip_count(), 1);
    // Failed split must not add an undo step; only add-track + add-generated remain.
    assert!(engine.can_undo());
    assert!(engine.undo());
    assert_eq!(engine.project().timeline().clip_count(), 0);
    assert!(engine.can_undo());
    assert!(engine.undo());
    assert!(!engine.can_undo());
}

#[test]
fn failed_trim_does_not_push_undo() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Adjustment, "FX");

    let clip_id = created(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::Adjustment,
                timeline: tr(0, 48),
            }))
            .expect("add"),
    );

    let err = engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: clip_id,
            timeline: tr(0, 0),
        }))
        .unwrap_err();
    assert!(format!("{err}").contains("range") || format!("{err}").contains("Invalid"));
    assert!(engine.can_undo());
    assert!(engine.undo());
    assert_eq!(engine.project().timeline().clip_count(), 0);
}
