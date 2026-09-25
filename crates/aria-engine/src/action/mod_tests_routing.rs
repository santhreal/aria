//! `set_track_routing` cases: a lane's fader, its output and its sends.
//!
//! The class these close: a routing gesture that leaves the lane in a state its
//! inverse cannot describe. Every gesture snapshots the whole route, so each
//! case here applies the inverse and asserts the lane came back exactly —
//! including a send's index, which a per-field inverse would lose.

use super::*;
use crate::action::edit::set_track_routing::{self, RouteEdit};
use aria_models::{AudioSend, TrackId, TrackOutput};

/// An audio lane and a bus to feed.
fn lane_and_bus(project: &mut Project) -> (TrackId, TrackId) {
    let lane = project.add_track(TrackKind::Audio, "A1");
    let bus = project.add_track(TrackKind::Submix, "Bus 1");
    (lane, bus)
}

#[test]
fn routing_inverse_oscillates() {
    let mut project = setup();
    let (lane, bus) = lane_and_bus(&mut project);
    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inverse =
        set_track_routing::execute(&mut ctx, lane, RouteEdit::Output(TrackOutput::Bus(bus)))
            .unwrap();
    assert_eq!(
        ctx.project.timeline().track(lane).unwrap().output,
        TrackOutput::Bus(bus)
    );

    // Undo: back to the master, and the inverse it hands back re-applies the
    // bus route, so redo is the same swap.
    let redo = inverse.apply(&mut ctx).unwrap();
    assert_eq!(
        ctx.project.timeline().track(lane).unwrap().output,
        TrackOutput::Master
    );
    redo.apply(&mut ctx).unwrap();
    assert_eq!(
        ctx.project.timeline().track(lane).unwrap().output,
        TrackOutput::Bus(bus)
    );
}

#[test]
fn the_fader_comes_back_at_its_previous_value() {
    let mut project = setup();
    let (lane, _bus) = lane_and_bus(&mut project);
    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    set_track_routing::execute(&mut ctx, lane, RouteEdit::Gain(0.5)).unwrap();
    let inverse = set_track_routing::execute(&mut ctx, lane, RouteEdit::Gain(2.0)).unwrap();
    assert_eq!(ctx.project.timeline().track(lane).unwrap().gain, 2.0);
    inverse.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.timeline().track(lane).unwrap().gain, 0.5);
}

#[test]
fn a_dropped_send_returns_at_its_index() {
    let mut project = setup();
    let lane = project.add_track(TrackKind::Audio, "A1");
    let first = project.add_track(TrackKind::Submix, "Bus 1");
    let second = project.add_track(TrackKind::Submix, "Bus 2");
    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    set_track_routing::execute(
        &mut ctx,
        lane,
        RouteEdit::AddSend {
            to: first,
            level: 0.25,
            pre_fader: true,
        },
    )
    .unwrap();
    set_track_routing::execute(
        &mut ctx,
        lane,
        RouteEdit::AddSend {
            to: second,
            level: 0.75,
            pre_fader: false,
        },
    )
    .unwrap();

    // Drop the first of two: the survivor slides down to index 0, so a
    // per-field inverse would restore the pair in the wrong order.
    let inverse =
        set_track_routing::execute(&mut ctx, lane, RouteEdit::RemoveSend { index: 0 }).unwrap();
    assert_eq!(
        ctx.project.timeline().track(lane).unwrap().sends,
        vec![AudioSend {
            to: second,
            level: 0.75,
            pre_fader: false,
        }]
    );

    inverse.apply(&mut ctx).unwrap();
    assert_eq!(
        ctx.project.timeline().track(lane).unwrap().sends,
        vec![
            AudioSend {
                to: first,
                level: 0.25,
                pre_fader: true,
            },
            AudioSend {
                to: second,
                level: 0.75,
                pre_fader: false,
            },
        ]
    );
}

#[test]
fn setting_one_field_of_a_send_leaves_the_other() {
    let mut project = setup();
    let (lane, bus) = lane_and_bus(&mut project);
    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    set_track_routing::execute(
        &mut ctx,
        lane,
        RouteEdit::AddSend {
            to: bus,
            level: 0.5,
            pre_fader: true,
        },
    )
    .unwrap();
    let inverse = set_track_routing::execute(
        &mut ctx,
        lane,
        RouteEdit::SetSend {
            index: 0,
            level: Some(1.0),
            pre_fader: None,
        },
    )
    .unwrap();
    let send = ctx.project.timeline().track(lane).unwrap().sends[0];
    assert_eq!(send.level, 1.0);
    assert!(send.pre_fader, "pre-fader was not part of the gesture");

    inverse.apply(&mut ctx).unwrap();
    assert_eq!(
        ctx.project.timeline().track(lane).unwrap().sends[0].level,
        0.5
    );
}

#[test]
fn a_refused_gesture_records_no_change() {
    let mut project = setup();
    let (lane, bus) = lane_and_bus(&mut project);
    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    // Out of range, a cycle, and an index that names no send: each refuses, and
    // none of them leaves the lane touched.
    assert!(set_track_routing::execute(&mut ctx, lane, RouteEdit::Gain(9.0)).is_err());
    assert!(
        set_track_routing::execute(&mut ctx, bus, RouteEdit::Output(TrackOutput::Bus(bus)))
            .is_err()
    );
    assert!(
        set_track_routing::execute(
            &mut ctx,
            lane,
            RouteEdit::SetSend {
                index: 0,
                level: Some(0.5),
                pre_fader: None,
            },
        )
        .is_err()
    );
    assert!(
        set_track_routing::execute(&mut ctx, lane, RouteEdit::RemoveSend { index: 0 }).is_err()
    );

    let track = ctx.project.timeline().track(lane).unwrap();
    assert_eq!(track.gain, 1.0);
    assert_eq!(track.output, TrackOutput::Master);
    assert!(track.sends.is_empty());
    assert_eq!(
        ctx.project.timeline().track(bus).unwrap().output,
        TrackOutput::Master
    );
}

#[test]
fn routing_an_unknown_lane_errors() {
    let mut project = setup();
    let (lane, _bus) = lane_and_bus(&mut project);
    let missing = aria_models::TrackId::from_raw(999);
    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    assert!(set_track_routing::execute(&mut ctx, missing, RouteEdit::Gain(0.5)).is_err());
    assert!(
        set_track_routing::execute(&mut ctx, lane, RouteEdit::Output(TrackOutput::Bus(missing)))
            .is_err()
    );
}
