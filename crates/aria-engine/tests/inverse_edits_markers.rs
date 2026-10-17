//! `inverse_edits` tests covering markers. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `inverse_edits` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn undo_add_marker_removes_it_and_redo_restores_same_id() {
    let (_dir, mut engine) = temp_engine();

    let id = created_marker(
        engine
            .apply(Command::Edit(EditCommand::AddMarker {
                at: rt(48),
                name: "drop".into(),
                color: Some(aria_models::MarkerColor::Red),
            }))
            .expect("add marker"),
    );
    assert_eq!(engine.project().timeline().marker_count(), 1);

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().marker_count(), 0);

    assert!(engine.redo());
    let marker = engine
        .project()
        .timeline()
        .marker(id)
        .expect("same id restored");
    assert_eq!(marker.tick, rt(48));
    assert_eq!(marker.name, "drop");
    assert_eq!(marker.color, aria_models::MarkerColor::Red);
}

#[test]
fn add_marker_without_color_cycles_the_palette() {
    let (_dir, mut engine) = temp_engine();
    for i in 0..3 {
        engine
            .apply(Command::Edit(EditCommand::AddMarker {
                at: rt(i * 10),
                name: String::new(),
                color: None,
            }))
            .expect("add marker");
    }
    let colors: Vec<_> = engine
        .project()
        .timeline()
        .markers()
        .iter()
        .map(|m| m.color)
        .collect();
    assert_eq!(
        colors,
        [
            aria_models::MarkerColor::cycle(0),
            aria_models::MarkerColor::cycle(1),
            aria_models::MarkerColor::cycle(2),
        ]
    );
}

#[test]
fn undo_remove_marker_restores_snapshot() {
    let (_dir, mut engine) = temp_engine();
    let id = created_marker(
        engine
            .apply(Command::Edit(EditCommand::AddMarker {
                at: rt(24),
                name: "beat".into(),
                color: Some(aria_models::MarkerColor::Blue),
            }))
            .expect("add marker"),
    );

    engine
        .apply(Command::Edit(EditCommand::RemoveMarker { marker: id }))
        .expect("remove marker");
    assert_eq!(engine.project().timeline().marker_count(), 0);

    assert!(engine.undo());
    let marker = engine.project().timeline().marker(id).expect("restored");
    assert_eq!((marker.tick, marker.name.as_str()), (rt(24), "beat"));

    assert!(engine.redo());
    assert_eq!(engine.project().timeline().marker_count(), 0);
}

#[test]
fn undo_set_marker_restores_previous_state() {
    let (_dir, mut engine) = temp_engine();
    let id = created_marker(
        engine
            .apply(Command::Edit(EditCommand::AddMarker {
                at: rt(10),
                name: "intro".into(),
                color: Some(aria_models::MarkerColor::Teal),
            }))
            .expect("add marker"),
    );

    engine
        .apply(Command::Edit(EditCommand::SetMarker {
            marker: id,
            at: rt(10),
            name: "intro".into(),
            color: aria_models::MarkerColor::Teal,
            comment: "first note".into(),
        }))
        .expect("annotate marker");

    engine
        .apply(Command::Edit(EditCommand::SetMarker {
            marker: id,
            at: rt(99),
            name: "outro".into(),
            color: aria_models::MarkerColor::Green,
            comment: "second note".into(),
        }))
        .expect("set marker");
    let moved = engine.project().timeline().marker(id).unwrap();
    assert_eq!((moved.tick, moved.name.as_str()), (rt(99), "outro"));
    assert_eq!(moved.comment, "second note");

    assert!(engine.undo());
    let restored = engine.project().timeline().marker(id).unwrap();
    assert_eq!((restored.tick, restored.name.as_str()), (rt(10), "intro"));
    assert_eq!(restored.color, aria_models::MarkerColor::Teal);
    assert_eq!(
        restored.comment, "first note",
        "the comment is part of the state the inverse restores"
    );

    assert!(engine.redo());
    let again = engine.project().timeline().marker(id).unwrap();
    assert_eq!((again.tick, again.name.as_str()), (rt(99), "outro"));
    assert_eq!(again.comment, "second note");
}

#[test]
fn marker_commands_reject_unknown_ids_without_history() {
    let (_dir, mut engine) = temp_engine();
    let missing = aria_models::MarkerId::from_raw(404);

    assert!(
        engine
            .apply(Command::Edit(EditCommand::RemoveMarker { marker: missing }))
            .is_err()
    );
    assert!(
        engine
            .apply(Command::Edit(EditCommand::SetMarker {
                marker: missing,
                at: rt(0),
                name: String::new(),
                color: aria_models::MarkerColor::Teal,
                comment: String::new(),
            }))
            .is_err()
    );
    assert!(!engine.can_undo(), "failed marker edits push no history");
}

#[test]
fn edit_marker_updates_all_fields_in_one_undo_step_and_refuses_invalid_durations() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Text, "T1");
    engine
        .apply(Command::Edit(EditCommand::AddGenerated {
            track,
            generator: Generator::text("title"),
            timeline: tr(0, 120),
        }))
        .unwrap();

    let outcome = engine
        .apply(Command::Edit(EditCommand::AddMarker {
            at: rt(24),
            name: "Marker 1".into(),
            color: Some(aria_models::MarkerColor::Teal),
        }))
        .unwrap();
    let aria_engine::ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedMarker(id)) = outcome
    else {
        panic!("expected CreatedMarker");
    };

    let depth_before = engine.history_depth();
    engine
        .edit_marker(
            id,
            rt(24),
            rt(48),
            "Chapter 1".into(),
            aria_models::MarkerColor::Orange,
            "Opening sequence".into(),
            aria_models::MarkerKind::Chapter,
        )
        .expect("edit marker");
    assert_eq!(
        engine.history_depth(),
        depth_before + 1,
        "one dialog commit must record one undo step"
    );
    let m = engine.project().timeline().marker(id).unwrap();
    assert_eq!(m.name, "Chapter 1");
    assert_eq!(m.comment, "Opening sequence");
    assert_eq!(m.color, aria_models::MarkerColor::Orange);
    assert_eq!(m.duration, rt(48));
    assert_eq!(m.kind, aria_models::MarkerKind::Chapter);

    assert!(engine.undo());
    let reverted = engine.project().timeline().marker(id).unwrap();
    assert_eq!(reverted.name, "Marker 1");
    assert_eq!(reverted.comment, "");
    assert_eq!(reverted.color, aria_models::MarkerColor::Teal);
    assert_eq!(reverted.duration, rt(0));
    assert_eq!(reverted.kind, aria_models::MarkerKind::Comment);

    // Refuses duration running past sequence end (120 ticks).
    let err = engine
        .edit_marker(
            id,
            rt(24),
            rt(110),
            "Too long".into(),
            aria_models::MarkerColor::Orange,
            String::new(),
            aria_models::MarkerKind::Chapter,
        )
        .unwrap_err();
    assert!(err.to_string().contains("exceeds sequence end"));
}
