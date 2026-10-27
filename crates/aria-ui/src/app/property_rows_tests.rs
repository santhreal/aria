//! Contract tests for timeline property rows, reveal chords, keyframe gestures,
//! temporal ease / roving commands, and the timeline graph editor.

use aria_commands::{Command, EditCommand};
use aria_models::{
    ClipParam, EASY_EASE_INFLUENCE, Easing, MIN_TEMPORAL_INFLUENCE, ParamValue, RationalTime,
    SpatialTangents, TemporalEase,
};

use super::{GraphMode, KeyframeIconShape, PropertyGroup, RevealAction, TimelineOutlineRow};
use crate::app::AppState;
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

/// The properties whose timeline rows on track `track` are lit as graphed.
fn lit(state: &AppState, track: u64) -> Vec<ClipParam> {
    state
        .track_outline_rows(track)
        .iter()
        .filter_map(|row| match row {
            TimelineOutlineRow::Property(p) if p.selected_for_graph => Some(p.param),
            _ => None,
        })
        .collect()
}

/// The properties the timeline graph editor draws.
fn drawn(state: &AppState) -> Vec<ClipParam> {
    state
        .build_timeline_graph_model(24.0, 120.0, 0.0, 160.0)
        .curves
        .iter()
        .map(|curve| curve.param)
        .collect()
}

/// With no row chosen for it, the timeline graph draws the Inspector graph's
/// property while that row is open, the first open row while it is not, and
/// lights the row it draws and no other. Closes "the graph draws one property
/// while another row is lit" and "a default graph of a constant property
/// while another holds keyframes".
#[test]
fn the_graph_draws_and_lights_one_row_until_a_row_is_chosen() {
    let (mut state, clip) = with_clip();
    let track = state.store.sequence.tracks[0].id;
    let tick = state.playhead.value;
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("keyframe opacity");
    state.toggle_clip_outline(clip.raw());
    assert_eq!(drawn(&state), [ClipParam::Opacity], "the keyframed opacity");
    assert_eq!(lit(&state, track), [ClipParam::Opacity]);

    // A property chosen in the Inspector's graph is drawn while its row is open.
    state.set_curve_param(ClipParam::Position);
    assert_eq!(drawn(&state), [ClipParam::Position], "the chosen position");
    assert_eq!(lit(&state, track), [ClipParam::Position]);

    // With the graphed property's row closed, the first open row is drawn.
    state.set_curve_param(ClipParam::Opacity);
    state.reveal_properties(RevealAction::Position, false);
    assert_eq!(
        drawn(&state),
        [ClipParam::Position],
        "the open position row"
    );
    assert_eq!(lit(&state, track), [ClipParam::Position]);

    // A chosen row replaces the default, which is no longer lit.
    state.select_timeline_keyframe(clip, ClipParam::Opacity, tick, false);
    assert_eq!(drawn(&state), [ClipParam::Opacity], "the chosen opacity");
    assert_eq!(lit(&state, track), Vec::<ClipParam>::new());
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

/// A band on a property row selects what the press left and the keys it
/// covers: drawn back past a key, it deselects that key, Shift held or not.
/// Escape puts back what the press left, and the release keeps what the band
/// covers.
#[test]
fn a_key_band_drawn_back_deselects_and_escape_puts_the_selection_back() {
    let (mut state, clip) = with_clip();
    for (tick, value) in [(2, 0.25), (6, 0.5), (12, 0.75)] {
        state.set_playhead(tick);
        state
            .set_keyframe_scalar(ClipParam::Opacity, tick, value)
            .expect("a key");
    }
    let selected = |state: &AppState| {
        state
            .selected_timeline_keyframes()
            .iter()
            .map(|key| key.tick)
            .collect::<Vec<_>>()
    };
    // A Shift press beside the key at 12 keeps it.
    state.select_timeline_keyframe(clip, ClipParam::Opacity, 12, false);
    state.begin_key_band(clip, ClipParam::Opacity, 0);
    state.drag_key_band(8);
    assert_eq!(selected(&state), [2, 6, 12], "the band over 2 and 6");
    state.drag_key_band(4);
    assert_eq!(selected(&state), [2, 12], "the band drawn back past 6");
    state.cancel_key_band();
    assert_eq!(selected(&state), [12], "Escape");
    state.drag_key_band(8);
    assert_eq!(selected(&state), [12], "a move after Escape");

    state.begin_key_band(clip, ClipParam::Opacity, 0);
    state.drag_key_band(4);
    state.release_key_band();
    state.drag_key_band(8);
    assert_eq!(selected(&state), [2, 12], "the release");
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

    // Pull key 0's outgoing handle out of the key, then drag it back to
    // minimum influence and commit.
    state
        .apply_command(Command::Edit(EditCommand::SetKeyframeEase {
            clip,
            param: ClipParam::Position,
            at: RationalTime::new(0, rate),
            in_ease: None,
            out_ease: Some(TemporalEase::EASY_EASE),
            hold: false,
        }))
        .expect("ease k0");
    let h0 = state
        .build_timeline_graph_model(24.0, 120.0, 0.0, 160.0)
        .curves[0]
        .influence_handles[0];
    assert!(
        (h0.influence - EASY_EASE_INFLUENCE).abs() < 1e-4,
        "the pulled handle: {h0:?}"
    );
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

/// A press in the graph editor takes what is drawn on top under it. A key
/// paints over its influence handles, so a press on a key's icon moves the
/// key even where its handles collapse under it, as a linear key's do. A
/// press past the icon takes the nearest key or handle in reach, which pulls a
/// collapsed handle out of its key. The class closed: a press on a linear key
/// taking a handle drawn under it, so the key could not be moved on the graph.
/// Not covered: spatial tangent handles, which sit within a pixel of their key.
#[test]
fn a_graph_press_takes_the_key_on_its_icon_and_the_nearest_handle_past_it() {
    let (mut state, clip) = with_clip();
    let rate = state.store.sequence.frame_rate;
    for (tick, value) in [(0, 0.2), (20, 0.8)] {
        state
            .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                clip,
                param: ClipParam::Opacity,
                at: RationalTime::new(tick, rate),
                value: ParamValue::Scalar(value),
                easing: Easing::Linear,
                tangents: None,
            }))
            .expect("the key is set");
    }
    state.toggle_timeline_graph_editor();
    state.select_timeline_property(clip, ClipParam::Opacity, false);
    let graph = |state: &AppState| state.build_timeline_graph_model(24.0, 120.0, 0.0, 160.0);
    let keys = |state: &AppState| {
        state
            .engine()
            .lock()
            .project()
            .clip(clip)
            .expect("the clip is in the project")
            .transform
            .opacity
            .keyframes()
            .iter()
            .map(|key| (key.tick, key.value))
            .collect::<Vec<_>>()
    };
    let out_influence = |state: &AppState| {
        state
            .keyframe_easing_at(ClipParam::Opacity, 0)
            .and_then(Easing::out_ease)
            .map(|ease| ease.influence)
    };
    let drag = |state: &mut AppState, from: (f32, f32), to: (f32, f32)| {
        assert!(
            state.begin_timeline_graph_drag(from.0, from.1, 24.0, 120.0, 0.0, 160.0),
            "a press at {from:?} lands on the graph"
        );
        state.update_timeline_graph_drag(to.0, to.1, 24.0, 120.0, 0.0, 160.0);
        state
            .commit_timeline_graph_drag()
            .expect("the drag commits");
    };

    let model = graph(&state);
    let key = model.curves[0].keys[0];
    let handle = model.curves[0].influence_handles[0];
    let key_d = 1.0f32;
    let handle_d = (key.x + key_d - handle.handle_x).hypot(key.y - handle.handle_y);
    assert!(
        handle_d < key_d,
        "the outgoing handle collapses under the linear key, nearer the press than the key: {handle:?}"
    );

    // On the key's icon: the key moves up, its ease stays.
    let before = keys(&state);
    let ease = out_influence(&state);
    drag(
        &mut state,
        (key.x + key_d, key.y),
        (key.x + key_d, key.y - 30.0),
    );
    let moved = keys(&state);
    assert_eq!(moved[0].0, before[0].0, "a press on the key keeps its tick");
    assert!(
        moved[0].1 > before[0].1 + 0.05,
        "a press on the key's icon moves the key: {before:?} -> {moved:?}"
    );
    assert_eq!(moved[1], before[1], "the other key");
    assert_eq!(out_influence(&state), ease, "the key's ease");

    // Past the icon, nearer the collapsed handle than the key: the handle
    // pulls out to half the segment, and the keys stay.
    let model = graph(&state);
    let key = model.curves[0].keys[0];
    let handle = model.curves[0].influence_handles[0];
    let past = 7.0f32;
    assert!(
        (key.x + past - handle.handle_x).hypot(key.y - handle.handle_y) < past,
        "the handle is nearer the press than the key: {handle:?}"
    );
    drag(
        &mut state,
        (key.x + past, key.y),
        (key.x + handle.segment_px_w * 0.5, key.y),
    );
    let influence = out_influence(&state).expect("the pulled handle sets an outgoing ease");
    assert!(
        (influence - 0.5).abs() < 1e-3,
        "the handle pulled to half the segment: {influence}"
    );
    assert_eq!(keys(&state), moved, "pulling a handle moves no key");
}
