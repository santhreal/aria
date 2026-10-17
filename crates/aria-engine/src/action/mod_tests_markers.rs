//! Regression tests for sequence and clip marker actions: single-step edit of
//! `(at, duration, name, color, comment, kind)`, clip-marker add/remove/edit
//! undo/redo, and duration validation refusals.

use super::*;
use crate::action::edit::marker;
use aria_models::{MarkerColor, MarkerKind};

#[test]
fn edit_marker_action_updates_all_fields_and_undoes_in_one_step() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Video, "V1");
    let media = project.add_media(MediaSource::new(
        "clip.mp4",
        1920,
        1080,
        Rational::FPS_24,
        200,
        true,
    ));
    let _clip = project.add_clip(track, media, tr(0, 120), rt(0)).unwrap();

    let mut path = None;
    let mut history = History::new(32);

    let (id, add_inv) = {
        let mut ctx = test_ctx(&mut project, &mut path, &mut history);
        marker::add(&mut ctx, rt(24), "Marker 1".into(), Some(MarkerColor::Teal)).unwrap()
    };
    history.record_do("Add marker", add_inv);
    assert_eq!(history.undo_labels().len(), 1);

    let edit_inv = {
        let mut ctx = test_ctx(&mut project, &mut path, &mut history);
        marker::edit(
            &mut ctx,
            id,
            rt(24),
            rt(48),
            "Scene 1".into(),
            MarkerColor::Orange,
            "Opening beat".into(),
            MarkerKind::Chapter,
        )
        .unwrap()
    };
    history.record_do("Edit marker", edit_inv);
    assert_eq!(
        history.undo_labels().len(),
        2,
        "editing all marker fields must record a single undo step"
    );

    let edited = project.timeline().marker(id).unwrap();
    assert_eq!(edited.tick, rt(24));
    assert_eq!(edited.duration, rt(48));
    assert_eq!(edited.name, "Scene 1");
    assert_eq!(edited.color, MarkerColor::Orange);
    assert_eq!(edited.comment, "Opening beat");
    assert_eq!(edited.kind, MarkerKind::Chapter);

    // Undo the single step -> all fields revert to initial values.
    let undo_step = history.pop_undo().unwrap();
    let redo_action = {
        let mut ctx = test_ctx(&mut project, &mut path, &mut history);
        undo_step.action.apply(&mut ctx).unwrap()
    };
    let reverted = project.timeline().marker(id).unwrap();
    assert_eq!(reverted.tick, rt(24));
    assert_eq!(reverted.duration, rt(0));
    assert_eq!(reverted.name, "Marker 1");
    assert_eq!(reverted.color, MarkerColor::Teal);
    assert_eq!(reverted.comment, "");
    assert_eq!(reverted.kind, MarkerKind::Comment);

    // Redo -> all fields restored.
    {
        let mut ctx = test_ctx(&mut project, &mut path, &mut history);
        redo_action.apply(&mut ctx).unwrap();
    }
    let redone = project.timeline().marker(id).unwrap();
    assert_eq!(redone.duration, rt(48));
    assert_eq!(redone.name, "Scene 1");
    assert_eq!(redone.color, MarkerColor::Orange);
    assert_eq!(redone.comment, "Opening beat");
    assert_eq!(redone.kind, MarkerKind::Chapter);
}

#[test]
fn clip_marker_add_edit_remove_oscillate_and_refuse_invalid_durations() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Video, "V1");
    let media = project.add_media(MediaSource::new(
        "clip.mp4",
        1920,
        1080,
        Rational::FPS_24,
        200,
        true,
    ));
    let clip_id = project.add_clip(track, media, tr(10, 60), rt(20)).unwrap();

    let mut path = None;
    let mut history = History::new(32);

    let (marker_id, add_inv) = {
        let mut ctx = test_ctx(&mut project, &mut path, &mut history);
        marker::add_on_clip(
            &mut ctx,
            clip_id,
            rt(25),
            "Clip Cue".into(),
            Some(MarkerColor::Purple),
        )
        .unwrap()
    };
    assert_eq!(project.timeline().marker_clip(marker_id), Some(clip_id));

    // Negative duration is refused with an actionable error.
    let neg_err = {
        let mut ctx = test_ctx(&mut project, &mut path, &mut history);
        match marker::edit(
            &mut ctx,
            marker_id,
            rt(25),
            rt(-5),
            "Clip Cue".into(),
            MarkerColor::Purple,
            String::new(),
            MarkerKind::Comment,
        ) {
            Ok(_) => panic!("negative duration must be refused"),
            Err(err) => err,
        }
    };
    assert!(neg_err.to_string().contains("non-negative"));

    // Duration running past clip source end (10 + 60 = 70; 25 + 50 = 75 > 70) is refused.
    let past_clip_err = {
        let mut ctx = test_ctx(&mut project, &mut path, &mut history);
        match marker::edit(
            &mut ctx,
            marker_id,
            rt(25),
            rt(50),
            "Clip Cue".into(),
            MarkerColor::Purple,
            String::new(),
            MarkerKind::Segmentation,
        ) {
            Ok(_) => panic!("duration past clip source end must be refused"),
            Err(err) => err,
        }
    };
    assert!(
        past_clip_err
            .to_string()
            .contains("exceeds clip source end")
    );

    // Undo the add -> removes the clip marker; redo -> restores it on the clip.
    let redo_add = {
        let mut ctx = test_ctx(&mut project, &mut path, &mut history);
        add_inv.apply(&mut ctx).unwrap()
    };
    assert!(project.timeline().marker(marker_id).is_none());
    {
        let mut ctx = test_ctx(&mut project, &mut path, &mut history);
        redo_add.apply(&mut ctx).unwrap();
    }
    assert_eq!(project.timeline().marker_clip(marker_id), Some(clip_id));
}
