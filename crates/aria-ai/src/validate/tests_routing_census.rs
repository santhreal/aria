//! Every lane the mixer holds is routable through the tools, and every
//! refusal the model can hit says what to do instead.
//!
//! # Why
//!
//! The mixer shipped as an operator surface with no tool behind it: five
//! commands existed, the assistant could not reach any of them, and nothing
//! was red. The class is "a lane kind, a level, or a route the model can
//! name and the tools answer differently from the model". So the lane cases
//! are derived from [`aria_models::track_kind_specs`] at run time — a lane
//! kind added to the catalog is covered here by existing, and one whose
//! `in_mixer` column disagrees with the tool's guard fails — and the level
//! bound is read from [`AudioSend::MAX_LEVEL`] rather than typed out, so
//! raising it cannot leave a test asserting the old ceiling.
//!
//! What this does not catch: how the engine folds the graph (that is
//! `aria-models`' own submix suite) and whether a bus is audible (that is the
//! mixer in `aria-render`).

use super::*;
use crate::wire;
use aria_models::{AudioSend, track_kind_specs};

/// A project holding one lane of `kind` plus a bus to route into.
///
/// The bus is a second lane, so a routing destination always exists — even
/// when the lane under test is itself the bus kind.
fn lane_and_bus(kind: TrackKind) -> (Project, u64, u64) {
    let mut project = Project::new("routing census", R24);
    let lane = project.add_track(kind, "L1");
    let bus = project.add_track(TrackKind::Submix, "Bus 1");
    (project, lane.raw(), bus.raw())
}

/// The refusal `cmd` earns, or `None` when it validates.
fn verdict(project: &Project, cmd: WireCommand) -> Option<String> {
    validate(&cmd, project).err().map(|r| r.message)
}

/// An audio lane sending to a bus at unity: what `set_send` and `remove_send`
/// address.
fn lane_with_send() -> (Project, u64, u64) {
    let (mut project, lane, bus) = lane_and_bus(TrackKind::Audio);
    project
        .timeline_mut()
        .add_send(TrackId::from_raw(lane), TrackId::from_raw(bus), 1.0, false)
        .expect("an audio lane may send to a bus");
    (project, lane, bus)
}

#[test]
fn every_lane_in_the_mixer_is_routable_and_every_other_is_refused() {
    for spec in track_kind_specs() {
        let (project, lane, bus) = lane_and_bus(spec.kind);
        let calls = [
            WireCommand::SetTrackGain(wire::SetTrackGain {
                track: lane,
                gain: 0.5,
            }),
            WireCommand::SetTrackOutput(wire::SetTrackOutput {
                track: lane,
                to: Some(bus),
            }),
            WireCommand::AddSend(wire::AddSend {
                track: lane,
                to: bus,
                level: None,
                pre_fader: None,
            }),
        ];
        for call in calls {
            let name = call.tool_name();
            let refusal = verdict(&project, call);
            assert_eq!(
                refusal.is_none(),
                spec.in_mixer(),
                "lane '{}': {name} disagrees with its in_mixer column ({refusal:?})",
                spec.id
            );
            if let Some(message) = refusal {
                assert!(
                    message.contains("carries no sound")
                        && message.contains(spec.id)
                        && message.contains("audio"),
                    "lane '{}': {name} refused without naming the lane and the \
                     ones that do route: {message}",
                    spec.id
                );
            }
        }
    }
}

#[test]
fn every_lane_kind_the_catalog_offers_an_agent_is_reachable() {
    for spec in track_kind_specs() {
        let wire_kind: Result<wire::WireTrackKind, _> =
            serde_json::from_value(serde_json::json!(spec.id));
        assert_eq!(
            wire_kind.is_ok(),
            spec.agent_creates,
            "lane '{}': its agent_creates column and the tool's lane list disagree",
            spec.id
        );
        let Ok(wire_kind) = wire_kind else {
            continue;
        };
        let project = Project::new("routing census", R24);
        let lowered = lower(
            &project,
            WireCommand::AddTrack(wire::AddTrack {
                kind: wire_kind,
                name: "L1".to_string(),
                index: None,
            }),
        );
        assert_eq!(
            lowered,
            EditCommand::AddTrack {
                kind: spec.kind,
                name: "L1".to_string(),
                index: None,
                pinned: false,
            },
            "lane '{}' lowers to another kind",
            spec.id
        );
    }
}

#[test]
fn a_level_outside_the_bound_is_refused_and_each_bound_is_accepted() {
    let (project, lane, bus) = lane_with_send();
    let top = f64::from(AudioSend::MAX_LEVEL);
    for accepted in [0.0, 1.0, top] {
        for call in [
            WireCommand::SetTrackGain(wire::SetTrackGain {
                track: lane,
                gain: accepted,
            }),
            WireCommand::SetSend(wire::SetSend {
                track: lane,
                index: 0,
                level: Some(accepted),
                pre_fader: None,
            }),
        ] {
            let name = call.tool_name();
            assert_eq!(
                verdict(&project, call),
                None,
                "{name} refused the level {accepted}, which is inside the bound"
            );
        }
    }
    for refused in [-0.5, top + 0.5, 1.0e9] {
        let gain = verdict(
            &project,
            WireCommand::SetTrackGain(wire::SetTrackGain {
                track: lane,
                gain: refused,
            }),
        )
        .unwrap_or_else(|| panic!("a gain of {refused} was accepted"));
        assert!(
            gain.contains("gain") && gain.contains("is outside"),
            "the gain refusal does not name the level: {gain}"
        );
        let level = verdict(
            &project,
            WireCommand::AddSend(wire::AddSend {
                track: lane,
                to: bus,
                level: Some(refused),
                pre_fader: None,
            }),
        )
        .unwrap_or_else(|| panic!("a send level of {refused} was accepted"));
        assert!(
            level.contains("level") && level.contains("is outside"),
            "the send refusal does not name the level: {level}"
        );
    }
}

#[test]
fn a_destination_that_is_not_a_bus_is_refused_with_the_buses_that_are() {
    let (project, lane, bus) = lane_and_bus(TrackKind::Audio);
    let message = verdict(
        &project,
        WireCommand::SetTrackOutput(wire::SetTrackOutput {
            track: lane,
            to: Some(lane),
        }),
    )
    .expect("a lane is not a bus");
    assert!(
        message.contains("would feed it back into itself"),
        "routing a lane into itself is a loop, not a bad destination: {message}"
    );

    let mut project = project;
    let other = project.add_track(TrackKind::Audio, "A2").raw();
    let message = verdict(
        &project,
        WireCommand::SetTrackOutput(wire::SetTrackOutput {
            track: lane,
            to: Some(other),
        }),
    )
    .expect("an audio lane is not a bus");
    assert!(
        message.contains("not a submix bus") && message.contains(&bus.to_string()),
        "the refusal does not offer the bus that would work: {message}"
    );
}

#[test]
fn a_route_that_would_close_a_loop_is_refused() {
    let (mut project, lane, bus) = lane_and_bus(TrackKind::Audio);
    let far = project.add_track(TrackKind::Submix, "Bus 2").raw();
    let lowered = lower(
        &project,
        WireCommand::SetTrackOutput(wire::SetTrackOutput {
            track: bus,
            to: Some(far),
        }),
    );
    project
        .timeline_mut()
        .set_track_output(
            TrackId::from_raw(bus),
            aria_models::TrackOutput::Bus(TrackId::from_raw(far)),
        )
        .expect("a bus may feed another bus");
    assert_eq!(
        lowered,
        EditCommand::SetTrackOutput {
            track: TrackId::from_raw(bus),
            to: Some(TrackId::from_raw(far)),
        }
    );

    // `far` is downstream of `bus`, so feeding it back closes the loop —
    // through the output, and through a send.
    for call in [
        WireCommand::SetTrackOutput(wire::SetTrackOutput {
            track: far,
            to: Some(bus),
        }),
        WireCommand::AddSend(wire::AddSend {
            track: far,
            to: bus,
            level: None,
            pre_fader: None,
        }),
    ] {
        let name = call.tool_name();
        let message = verdict(&project, call).unwrap_or_else(|| panic!("{name} closed a loop"));
        assert!(
            message.contains("would feed it back into itself"),
            "{name} refused a loop without saying so: {message}"
        );
    }

    // The lane feeding the first bus is not downstream of the second, so it
    // may still be routed there.
    assert_eq!(
        verdict(
            &project,
            WireCommand::SetTrackOutput(wire::SetTrackOutput {
                track: lane,
                to: Some(far),
            })
        ),
        None,
        "a route that closes nothing was refused"
    );
}

#[test]
fn a_second_send_to_the_same_bus_is_refused_and_names_the_send_to_change() {
    let (project, lane, bus) = lane_with_send();
    let message = verdict(
        &project,
        WireCommand::AddSend(wire::AddSend {
            track: lane,
            to: bus,
            level: Some(0.5),
            pre_fader: None,
        }),
    )
    .expect("one send per lane per bus");
    assert!(
        message.contains("already sends to bus") && message.contains("index 0"),
        "the refusal does not name the send that already exists: {message}"
    );
}

#[test]
fn a_send_index_the_lane_does_not_hold_is_refused_with_what_it_holds() {
    let (project, lane, _) = lane_and_bus(TrackKind::Audio);
    let empty = verdict(
        &project,
        WireCommand::RemoveSend(wire::RemoveSend {
            track: lane,
            index: 0,
        }),
    )
    .expect("a lane with no sends holds no index");
    assert!(
        empty.contains("has no sends"),
        "the refusal does not say the lane holds none: {empty}"
    );

    let (mut project, lane, _) = lane_with_send();
    let one = verdict(
        &project,
        WireCommand::SetSend(wire::SetSend {
            track: lane,
            index: 1,
            level: Some(0.5),
            pre_fader: None,
        }),
    )
    .expect("index 1 is past the only send");
    assert!(
        one.contains("one send, at index 0"),
        "the refusal does not name the index that works: {one}"
    );

    let second = project.add_track(TrackKind::Submix, "Bus 2").raw();
    project
        .timeline_mut()
        .add_send(
            TrackId::from_raw(lane),
            TrackId::from_raw(second),
            1.0,
            false,
        )
        .expect("a second bus takes its own send");
    let two = verdict(
        &project,
        WireCommand::RemoveSend(wire::RemoveSend {
            track: lane,
            index: 4,
        }),
    )
    .expect("index 4 is past both sends");
    assert!(
        two.contains("2 sends, at indices 0 to 1"),
        "the refusal does not name the indices that work: {two}"
    );
}

#[test]
fn a_send_call_that_would_change_nothing_is_refused() {
    let (project, lane, _) = lane_with_send();
    let message = verdict(
        &project,
        WireCommand::SetSend(wire::SetSend {
            track: lane,
            index: 0,
            level: None,
            pre_fader: None,
        }),
    )
    .expect("a call with nothing to write is a wasted turn");
    assert!(
        message.contains("changes nothing") && message.contains("pre_fader"),
        "the refusal does not say which fields to pass: {message}"
    );
}

#[test]
fn a_send_lowers_its_defaults_and_its_flags() {
    let (project, lane, bus) = lane_and_bus(TrackKind::Audio);
    assert_eq!(
        lower(
            &project,
            WireCommand::AddSend(wire::AddSend {
                track: lane,
                to: bus,
                level: None,
                pre_fader: None,
            })
        ),
        EditCommand::AddSend {
            track: TrackId::from_raw(lane),
            to: TrackId::from_raw(bus),
            level: 1.0,
            pre_fader: false,
        },
        "an omitted level is unity and an omitted tap is post-fader"
    );
    assert_eq!(
        lower(
            &project,
            WireCommand::AddSend(wire::AddSend {
                track: lane,
                to: bus,
                level: Some(0.25),
                pre_fader: Some(true),
            })
        ),
        EditCommand::AddSend {
            track: TrackId::from_raw(lane),
            to: TrackId::from_raw(bus),
            level: 0.25,
            pre_fader: true,
        }
    );
}

#[test]
fn the_routing_a_lane_holds_is_in_the_project_description() {
    let (project, lane, bus) = lane_with_send();
    let mut project = project;
    project
        .timeline_mut()
        .set_track_gain(TrackId::from_raw(lane), 0.5)
        .expect("an audio lane has a fader");
    project
        .timeline_mut()
        .set_track_output(
            TrackId::from_raw(lane),
            aria_models::TrackOutput::Bus(TrackId::from_raw(bus)),
        )
        .expect("an audio lane may feed a bus");

    let summary = crate::describe::summarize(&project);
    let described = summary
        .tracks
        .iter()
        .find(|track| track.id == lane)
        .expect("the lane is described");
    assert_eq!(described.gain, Some(0.5), "the fader is not described");
    assert_eq!(
        described.output_bus,
        Some(bus),
        "where the lane goes is not described"
    );
    assert_eq!(
        described.sends.len(),
        1,
        "the send set_send addresses is not described"
    );
    assert_eq!(described.sends[0].to, bus);
    assert_eq!(described.sends[0].level, 1.0);
    assert!(!described.sends[0].pre_fader);

    let bus_lane = summary
        .tracks
        .iter()
        .find(|track| track.id == bus)
        .expect("the bus is described");
    assert_eq!(
        bus_lane.gain, None,
        "a fader at unity is left out rather than written as 1.0"
    );
    assert!(bus_lane.sends.is_empty());
}
