//! Contract tests for timeline property rows, reveal chords, keyframe gestures,
//! temporal ease / roving commands, and the timeline graph editor.

use aria_models::{
    ClipParam, EASY_EASE_INFLUENCE, Easing, MIN_TEMPORAL_INFLUENCE, ParamValue, RationalTime,
    SpatialTangents,
};

use super::{GraphMode, KeyframeIconShape, PropertyGroup, RevealAction, TimelineOutlineRow};
use crate::app::premiere_tests::with_clip;

#[test]
fn reveal_chords_open_replace_extend_with_shift_and_close_with_backtick() {
    let (mut state, clip) = with_clip();
    assert!(state.visible_params_for_clip(clip).is_empty());

    // Alt+P reveals Position alone.
    state.reveal_properties(RevealAction::Position, false);
    assert_eq!(
        state.visible_params_for_clip(clip),
        vec![ClipParam::Position]
    );

    // Shift+Alt+S adds Scale to the visible set.
    state.reveal_properties(RevealAction::Scale, true);
    assert_eq!(
        state.visible_params_for_clip(clip),
        vec![ClipParam::Position, ClipParam::Scale]
    );

    // Shift+Alt+P removes Position while keeping Scale.
    state.reveal_properties(RevealAction::Position, true);
    assert_eq!(state.visible_params_for_clip(clip), vec![ClipParam::Scale]);

    // Alt+R without Shift replaces the set with Rotation.
    state.reveal_properties(RevealAction::Rotation, false);
    assert_eq!(
        state.visible_params_for_clip(clip),
        vec![ClipParam::Rotation]
    );

    // Pressing Alt+R again when only Rotation is open toggles it closed.
    state.reveal_properties(RevealAction::Rotation, false);
    assert!(state.visible_params_for_clip(clip).is_empty());

    // Reopen Opacity and Anchor, then close all with Alt+`. The rows keep
    // the clip's property order, not the order they were revealed in.
    state.reveal_properties(RevealAction::Opacity, false);
    state.reveal_properties(RevealAction::Anchor, true);
    assert_eq!(
        state.visible_params_for_clip(clip),
        vec![ClipParam::AnchorPoint, ClipParam::Opacity]
    );
    state.reveal_properties(RevealAction::CloseAll, false);
    assert!(state.visible_params_for_clip(clip).is_empty());
}

#[test]
fn alt_u_reveals_animated_first_and_modified_on_second_press() {
    let (mut state, clip) = with_clip();

    // Keyframe Position at the playhead and modify Rotation as a non-default constant.
    state
        .add_keyframe_at_playhead(ClipParam::Position)
        .expect("keyframe position");
    state
        .write_property_scalar_at_playhead(clip, ClipParam::Rotation, 25.0)
        .expect("modify rotation constant");

    // First Alt+U reveals only animated properties (Position).
    state.reveal_properties(RevealAction::AnimatedOrModified, false);
    assert_eq!(
        state.visible_params_for_clip(clip),
        vec![ClipParam::Position]
    );

    // Second consecutive Alt+U reveals all modified properties (Position and Rotation).
    state.reveal_properties(RevealAction::AnimatedOrModified, false);
    let shown = state.visible_params_for_clip(clip);
    assert!(shown.contains(&ClipParam::Position));
    assert!(shown.contains(&ClipParam::Rotation));
    assert!(!shown.contains(&ClipParam::Opacity));
}

#[test]
fn outline_twirl_opens_groups_and_projects_property_rows() {
    let (mut state, clip) = with_clip();
    let track_id = state.store.sequence.tracks[0].id;

    state.toggle_clip_outline(clip.raw());
    let rows = state.track_outline_rows(track_id);
    assert!(rows.iter().any(|r| matches!(
        r,
        TimelineOutlineRow::GroupHeader {
            group: PropertyGroup::Transform,
            expanded: true,
            ..
        }
    )));
    assert!(rows.iter().any(|r| matches!(
        r,
        TimelineOutlineRow::Property(p) if p.param == ClipParam::Position
    )));

    // Twirling Transform closed hides its property rows while keeping group headers.
    state.toggle_property_group(clip, PropertyGroup::Transform);
    let rows_after = state.track_outline_rows(track_id);
    assert!(!rows_after.iter().any(|r| matches!(
        r,
        TimelineOutlineRow::Property(p) if p.param == ClipParam::Position
    )));
}

#[test]
fn keyframe_drag_snaps_and_commits_in_one_undo_step() {
    let (mut state, clip) = with_clip();
    state.set_playhead(0);
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("first keyframe");
    state.set_playhead(20);
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("second keyframe");

    // Place playhead at tick 12 and drag the keyframe at 0 by +10 with threshold 4:
    // it should snap to playhead tick 12.
    state.set_playhead(12);
    state.begin_timeline_keyframe_drag(clip, ClipParam::Opacity, 0, false);
    assert!(state.update_timeline_keyframe_drag(10, 4, false));
    assert_eq!(state.snap_guide.map(|g| g.tick), Some(12));
    state
        .commit_timeline_keyframe_drag()
        .expect("commit dragged keyframe");

    let marks: Vec<i64> = state
        .keyframes_of(ClipParam::Opacity)
        .iter()
        .map(|m| m.tick)
        .collect();
    assert_eq!(marks, vec![12, 20]);

    // A single undo restores the keyframe to tick 0.
    assert!(state.undo(), "undo single step");
    let marks_after_undo: Vec<i64> = state
        .keyframes_of(ClipParam::Opacity)
        .iter()
        .map(|m| m.tick)
        .collect();
    assert_eq!(marks_after_undo, vec![0, 20]);
}

#[test]
fn copy_paste_and_delete_keyframes_each_take_one_undo_step() {
    let (mut state, clip) = with_clip();
    state.set_playhead(2);
    state
        .set_keyframe_scalar(ClipParam::Opacity, 2, 0.25)
        .expect("k0");
    state.set_playhead(6);
    state
        .set_keyframe_scalar(ClipParam::Opacity, 6, 0.75)
        .expect("k1");

    state.marquee_select_keyframes(clip, ClipParam::Opacity, 0, 10, false);
    assert_eq!(state.selected_timeline_keyframes().len(), 2);
    assert_eq!(state.copy_selected_keyframes(), 2);

    // Paste at playhead 10 -> writes keyframes at 10 and 14 in one undo step.
    state.set_playhead(10);
    assert_eq!(
        state
            .paste_keyframes_at_playhead()
            .expect("paste keyframes"),
        2
    );
    let ticks: Vec<i64> = state
        .keyframes_of(ClipParam::Opacity)
        .iter()
        .map(|m| m.tick)
        .collect();
    assert_eq!(ticks, vec![2, 6, 10, 14]);

    assert!(state.undo(), "undo paste");
    let ticks_undo: Vec<i64> = state
        .keyframes_of(ClipParam::Opacity)
        .iter()
        .map(|m| m.tick)
        .collect();
    assert_eq!(ticks_undo, vec![2, 6]);

    // Delete selected keyframes in one undo step.
    state
        .delete_selected_keyframes()
        .expect("delete selected keyframes");
    assert!(state.keyframes_of(ClipParam::Opacity).is_empty());
    assert!(state.undo(), "undo delete");
    assert_eq!(state.keyframes_of(ClipParam::Opacity).len(), 2);
}

#[test]
fn easy_ease_hold_and_roving_update_keyframe_shapes_and_undo() {
    let (mut state, clip) = with_clip();
    let track_id = state.store.sequence.tracks[0].id;
    state.reveal_properties(RevealAction::Opacity, false);

    state.set_playhead(0);
    state
        .set_keyframe_scalar(ClipParam::Opacity, 0, 0.0)
        .expect("k0");
    state.set_playhead(4);
    state
        .set_keyframe_scalar(ClipParam::Opacity, 4, 0.5)
        .expect("k1");
    state.set_playhead(20);
    state
        .set_keyframe_scalar(ClipParam::Opacity, 20, 1.0)
        .expect("k2");

    // Select k1 at tick 4 and apply F9 Easy Ease.
    state.select_timeline_keyframe(clip, ClipParam::Opacity, 4, false);
    state
        .easy_ease_selected_or_playhead()
        .expect("F9 easy ease");
    let easing = state
        .keyframe_easing_at(ClipParam::Opacity, 4)
        .expect("easing at 4");
    assert!((easing.in_ease().expect("in ease").influence - EASY_EASE_INFLUENCE).abs() < 1e-4);
    assert!((easing.out_ease().expect("out ease").influence - EASY_EASE_INFLUENCE).abs() < 1e-4);

    // Toggle Hold (Ctrl+Alt+H).
    state
        .toggle_hold_selected_or_playhead()
        .expect("toggle hold");
    assert!(
        state
            .keyframe_easing_at(ClipParam::Opacity, 4)
            .expect("easing")
            .is_hold()
    );
    state
        .toggle_hold_selected_or_playhead()
        .expect("toggle hold off");
    // Hold toggles over the eases rather than replacing them: toggling it off
    // returns the Easy Ease handles F9 wrote.
    let restored = state
        .keyframe_easing_at(ClipParam::Opacity, 4)
        .expect("easing");
    assert!(!restored.is_hold());
    assert_eq!(
        restored.in_ease(),
        Some(aria_models::TemporalEase::EASY_EASE)
    );
    assert_eq!(
        restored.out_ease(),
        Some(aria_models::TemporalEase::EASY_EASE)
    );

    // Toggle roving on interior keyframe k1 (originally at tick 4; midpoint of 0..20 is 10).
    state
        .toggle_roving_keyframe(clip, ClipParam::Opacity, 4)
        .expect("toggle roving on");
    let rows = state.track_outline_rows(track_id);
    let TimelineOutlineRow::Property(op_row) = &rows[0] else {
        panic!("expected property row");
    };
    assert_eq!(op_row.keys[1].tick, 10);
    assert_eq!(op_row.keys[1].shape, KeyframeIconShape::Roving);
}

#[test]
fn timeline_graph_editor_builds_value_and_speed_graphs_and_drags_handles() {
    let (mut state, clip) = with_clip();
    let rate = state.store.sequence.frame_rate;
    state
        .apply_command(aria_commands::Command::Edit(
            aria_commands::EditCommand::SetParamKeyframe {
                clip,
                param: ClipParam::Position,
                at: RationalTime::new(0, rate),
                value: ParamValue::Vec2([0.0, 0.0]),
                easing: Easing::Linear,
                // Positions and their handles are canvas fractions.
                tangents: Some(SpatialTangents {
                    in_t: [-0.1, 0.0],
                    out_t: [0.2, 0.15],
                }),
            },
        ))
        .expect("pos k0");
    state
        .apply_command(aria_commands::Command::Edit(
            aria_commands::EditCommand::SetParamKeyframe {
                clip,
                param: ClipParam::Position,
                at: RationalTime::new(20, rate),
                value: ParamValue::Vec2([0.5, 0.25]),
                easing: Easing::Linear,
                tangents: Some(SpatialTangents {
                    in_t: [-0.2, -0.15],
                    out_t: [0.1, 0.0],
                }),
            },
        ))
        .expect("pos k1");

    state.toggle_timeline_graph_editor();
    assert!(state.timeline_graph_open());
    state.select_timeline_property(clip, ClipParam::Position, false);

    let value_model = state.build_timeline_graph_model(24.0, 120.0, 0.0, 160.0);
    assert_eq!(value_model.mode, GraphMode::Value);
    assert_eq!(value_model.curves.len(), 1);
    let curve = &value_model.curves[0];
    assert_eq!(curve.keys.len(), 2);
    assert_eq!(curve.influence_handles.len(), 2);
    assert_eq!(curve.spatial_handles.len(), 4);

    // Drag the outgoing influence handle of key 0 to minimum influence and commit.
    let h0 = curve.influence_handles[0];
    assert!(state.begin_timeline_graph_drag(h0.handle_x, h0.handle_y, 24.0, 120.0, 0.0, 160.0,));
    assert!(state.update_timeline_graph_drag(
        h0.anchor_x + 0.01,
        h0.anchor_y,
        24.0,
        120.0,
        0.0,
        160.0,
    ));
    state
        .commit_timeline_graph_drag()
        .expect("commit influence drag");
    let easing = state
        .keyframe_easing_at(ClipParam::Position, 0)
        .expect("easing at 0");
    assert!((easing.out_ease().expect("out ease").influence - MIN_TEMPORAL_INFLUENCE).abs() < 1e-4);

    // Switch to Speed graph and verify speed samples are non-empty.
    state.set_timeline_graph_mode(GraphMode::Speed);
    let speed_model = state.build_timeline_graph_model(24.0, 120.0, 0.0, 160.0);
    assert_eq!(speed_model.mode, GraphMode::Speed);
    assert!(!speed_model.curves[0].samples.is_empty());
}
