//! One case per property gesture, and the refusal every importing gesture owes.
//!
//! The fixtures and readers are defined in `intents_gesture_tests.rs` and
//! imported through `use super::gesture_tests::*`, so both modules build the
//! same two-second project.
//!
//! The census that makes a new [`Intent`] variant fail by default reads this
//! file too — see `every_intent_variant_has_a_case` in the sibling.

use super::gesture_tests::*;
use super::*;

// --- properties -------------------------------------------------------------

/// Speed is stored as the exact milli-rational the slider implies, the
/// direction flag is separate from the rate, and the timeline length re-derives
/// from it (faster occupies less timeline).
#[test]
fn set_speed_stores_an_exact_rational_and_a_separate_direction() {
    let (mut engine, main, clips) = main_with(1);
    run(
        &mut engine,
        Intent::SetSpeed {
            clip: clips[0],
            speed: 2.0,
            reversed: true,
        },
    )
    .expect("retime");
    let clip = clip_at(&engine, clips[0]);
    assert_eq!(clip.speed, Rational::new(2000, 1000));
    assert!(clip.reversed);
    assert_eq!(
        spans(&engine, main),
        vec![(0, 30)],
        "double speed halves the timeline span"
    );

    // Below the model's floor the rate clamps rather than reaching zero, which
    // would be a clip that never advances.
    run(
        &mut engine,
        Intent::SetSpeed {
            clip: clips[0],
            speed: 0.0,
            reversed: false,
        },
    )
    .expect("clamped retime");
    let clip = clip_at(&engine, clips[0]);
    assert_eq!(clip.speed, Rational::new(50, 1000));
    assert!(!clip.reversed);
    assert_eq!(
        spans(&engine, main),
        vec![(0, 1200)],
        "a twentieth of speed stretches the span twenty times"
    );
}

/// UI canvas coordinates are recentred on the way in: 0.5 is the middle of the
/// canvas, and the model stores an offset from that middle.
#[test]
fn set_transform_recentres_ui_coordinates() {
    let (mut engine, _, clips) = main_with(1);
    run(
        &mut engine,
        Intent::SetTransform {
            clip: clips[0],
            pos_x: 0.75,
            pos_y: 0.25,
            scale: 0.5,
            rotation_degrees: 15.0,
            opacity: 0.8,
            at_seconds: None,
        },
    )
    .expect("transform");
    let pose = clip_at(&engine, clips[0]).transform.sample(0);
    assert_eq!(pose.position, [0.25, -0.25]);
    assert_eq!(pose.rotation, 15.0);
    assert_eq!(pose.opacity, 0.8);
    assert_eq!(
        pose.anchor_point,
        [0.5, 0.5],
        "the gesture rotates and scales about the clip's center"
    );

    // The centre of the canvas is the origin, not (0.5, 0.5).
    run(
        &mut engine,
        Intent::SetTransform {
            clip: clips[0],
            pos_x: 0.5,
            pos_y: 0.5,
            scale: 1.0,
            rotation_degrees: 0.0,
            opacity: 1.0,
            at_seconds: None,
        },
    )
    .expect("transform");
    let pose = clip_at(&engine, clips[0]).transform.sample(0);
    assert_eq!(pose.position, [0.0, 0.0]);
}

/// The diamond toggles: the first press stamps the pose the clip is showing,
/// the second press takes it back off, and a position outside the clip is
/// refused instead of stamping at a clamped tick.
#[test]
fn the_transform_diamond_stamps_then_clears() {
    let (mut engine, _, clips) = main_with(1);
    let clip = clips[0];

    let stamped = run(
        &mut engine,
        Intent::ToggleTransformKeyframe { clip, seconds: 1.0 },
    )
    .expect("stamp");
    assert_eq!(stamped["keyframed"], serde_json::json!(true));
    assert!(
        clip_at(&engine, clip).transform.is_animated(),
        "a stamped diamond must leave the transform animated"
    );

    let cleared = run(
        &mut engine,
        Intent::ToggleTransformKeyframe { clip, seconds: 1.0 },
    )
    .expect("clear");
    assert_eq!(cleared["keyframed"], serde_json::json!(false));
    assert!(
        !clip_at(&engine, clip).transform.is_animated(),
        "clearing the only diamond must flatten the transform again"
    );

    let outside = run(
        &mut engine,
        Intent::ToggleTransformKeyframe { clip, seconds: 9.0 },
    )
    .expect_err("a diamond outside the clip is refused");
    assert!(
        outside.to_string().contains("outside the clip"),
        "the refusal should name the reason, got: {outside}"
    );
}

/// One junction, three gestures: set, change, clear. Ids come from the
/// catalog, so a host that ships a different transition set still exercises it.
#[test]
fn set_transition_sets_changes_and_clears_one_junction() {
    let catalog = aria_models::transition_catalog();
    let first = catalog.first().expect("a transition").id.as_str();
    let second = catalog.get(1).map_or(first, |spec| spec.id.as_str());
    let (mut engine, main, clips) = main_with(2);
    let left = clips[0];

    run(
        &mut engine,
        Intent::SetTransition {
            clip: left,
            transition_id: Some(first.to_owned()),
            duration_seconds: 0.5,
        },
    )
    .expect("set");
    let placed = junction(&engine, main, left).expect("a transition at the junction");
    assert_eq!(placed.transition_id.as_str(), first);
    assert_eq!(placed.right, clips[1], "it must span the cut, not a gap");
    assert_eq!(
        placed.duration, 15,
        "half a second at 30fps is fifteen ticks"
    );

    run(
        &mut engine,
        Intent::SetTransition {
            clip: left,
            transition_id: Some(second.to_owned()),
            duration_seconds: 0.2,
        },
    )
    .expect("change");
    let changed = junction(&engine, main, left).expect("still one transition");
    assert_eq!(changed.transition_id.as_str(), second);
    assert_eq!(changed.duration, 6);

    run(
        &mut engine,
        Intent::SetTransition {
            clip: left,
            transition_id: Some(second.to_owned()),
            duration_seconds: 0.0,
        },
    )
    .expect("a length the gesture leaves out");
    let kept = junction(&engine, main, left).expect("still one transition");
    assert_eq!(
        (kept.transition_id.as_str(), kept.duration),
        (second, 6),
        "no duration means keep the window already chosen"
    );

    run(
        &mut engine,
        Intent::SetTransition {
            clip: left,
            transition_id: None,
            duration_seconds: 0.0,
        },
    )
    .expect("clear");
    assert!(
        junction(&engine, main, left).is_none(),
        "clearing must remove the transition, not leave a zero-length one"
    );
}

// --- the importing gestures, refused ---------------------------------------

/// A path that is not there is refused, and the refusal leaves the project and
/// the history exactly as they were.
///
/// This is the half of `InsertMain`, `AddPip`, and `ReplaceMedia` that runs
/// without an encoder. Each of them imports before it edits, and each runs
/// inside one history group, so a failed import must roll the group back rather
/// than leave a pool entry behind.
#[test]
fn an_importing_gesture_that_cannot_read_its_file_changes_nothing() {
    let missing = std::env::temp_dir().join("aria-mobile-absent-fixture.mp4");
    assert!(
        !missing.exists(),
        "the fixture for this case must not exist: {}",
        missing.display()
    );
    let (mut engine, main, clips) = main_with(1);
    let before = serde_json::to_vec(engine.project()).expect("serialize");
    let intents = [
        Intent::InsertMain {
            path: missing.to_path_buf(),
            at_seconds: 0.0,
        },
        Intent::AddPip {
            path: missing.to_path_buf(),
            at_seconds: 0.0,
        },
        Intent::ReplaceMedia {
            clip: clips[0],
            path: missing.to_path_buf(),
        },
    ];
    for intent in intents {
        let label = format!("{intent:?}");
        run(&mut engine, intent).expect_err(&format!("{label} must refuse a missing file"));
        assert_eq!(
            serde_json::to_vec(engine.project()).expect("serialize"),
            before,
            "{label} left the project changed after refusing"
        );
        assert!(
            !engine.undo(),
            "{label} recorded history for an edit that did not happen"
        );
    }
    assert_eq!(spans(&engine, main), vec![(0, 60)]);
}
