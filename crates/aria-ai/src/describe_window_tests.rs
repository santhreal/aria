//! Tests for the description window: what a long project hands the model.
//!
//! The contract is three claims. A project inside the budget is described whole,
//! so nothing about the common case changed. A project over the budget is
//! described around the playhead, not from the front of the id list. And a
//! windowed description states its own bounds and the size of what it dropped,
//! because a silent window is worse than a truncated one: the model cannot ask
//! for what it does not know is missing.

use aria_models::{
    Marker, MarkerColor, MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind,
};

use super::{CLIP_BUDGET, SummaryRequest, WindowReason, plan, requested};
use crate::describe::{summarize, summarize_for};

const R24: Rational = Rational::FPS_24;

/// A project with `clips` back-to-back 1-second clips on one video track.
fn packed(clips: usize) -> Project {
    let mut project = Project::new("long", R24);
    let media = project.add_media(MediaSource::new(
        "/footage/take.mp4",
        1920,
        1080,
        R24,
        24 * 60,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    for index in 0..clips {
        project
            .add_clip(
                track,
                media,
                TimeRange::at_rate(0, 24, R24),
                RationalTime::new(index as i64 * 24, R24),
            )
            .expect("a fresh second of an empty track is free");
    }
    project
}

#[test]
fn a_project_inside_the_budget_is_described_whole() {
    let project = packed(CLIP_BUDGET);
    let windowed = summarize_for(
        &project,
        SummaryRequest::Session {
            playhead_seconds: 4.0,
        },
    );

    assert!(
        windowed.window.is_none(),
        "a project that fits one description carries no window"
    );
    assert_eq!(windowed.tracks[0].clips.len(), CLIP_BUDGET);
    assert_eq!(
        windowed,
        summarize(&project),
        "under the budget the session description is the whole-project description"
    );
}

#[test]
fn over_the_budget_the_window_holds_the_clips_around_the_playhead() {
    let project = packed(CLIP_BUDGET + 200);
    // Frame 6000 is clip 250, deep past the ids a front-of-list truncation
    // would have kept.
    let summary = summarize_for(
        &project,
        SummaryRequest::Session {
            playhead_seconds: 250.0,
        },
    );
    let window = summary.window.expect("over the budget, so windowed");

    assert_eq!(window.described_clips, CLIP_BUDGET);
    assert_eq!(window.omitted_clips, 200);
    assert_eq!(summary.tracks[0].clips.len(), CLIP_BUDGET);
    assert_eq!(window.reason, WindowReason::ClipBudget.text());

    // The playhead sits inside the span, and the span is the clips kept.
    assert!(window.start_frames <= 6000 && window.end_frames >= 6000);
    assert_eq!(window.start_frames, 6000 - 24 * (CLIP_BUDGET as i64 / 2));
    assert_eq!(window.end_frames, 6000 + 24 * (CLIP_BUDGET as i64 / 2));
    let first = summary.tracks[0].clips.first().expect("clips kept");
    let last = summary.tracks[0].clips.last().expect("clips kept");
    assert_eq!(first.start_frames, window.start_frames);
    assert_eq!(
        last.start_frames + last.duration_frames,
        window.end_frames,
        "the stated span is exactly the clips described"
    );

    // The whole project's duration is still reported: the model has to know the
    // timeline runs past what it can see.
    assert_eq!(summary.duration_seconds, (CLIP_BUDGET + 200) as f64);
}

#[test]
fn the_window_is_centred_on_the_playhead_not_on_the_lowest_ids() {
    let project = packed(CLIP_BUDGET + 100);
    let front = summarize_for(
        &project,
        SummaryRequest::Session {
            playhead_seconds: 0.0,
        },
    );
    let back = summarize_for(
        &project,
        SummaryRequest::Session {
            playhead_seconds: CLIP_BUDGET as f64,
        },
    );

    let front_ids: Vec<u64> = front.tracks[0].clips.iter().map(|clip| clip.id).collect();
    let back_ids: Vec<u64> = back.tracks[0].clips.iter().map(|clip| clip.id).collect();
    assert_ne!(
        front_ids, back_ids,
        "moving the playhead has to move the window, or this is truncation with extra steps"
    );
    assert_eq!(front.tracks[0].clips[0].start_frames, 0);
    assert!(
        back.tracks[0].clips[0].start_frames > 0,
        "a playhead at the end describes the end"
    );
}

#[test]
fn a_named_range_is_described_whatever_the_project_size() {
    let project = packed(10);
    let summary = summarize_for(
        &project,
        SummaryRequest::Range {
            from_seconds: 3.0,
            to_seconds: 5.0,
        },
    );
    let window = summary.window.expect("a range was asked for");

    assert_eq!(window.reason, WindowReason::RangeAsked.text());
    assert_eq!(window.start_frames, 72);
    assert_eq!(window.end_frames, 120);
    // The clips playing during [3s, 5s): the one ending at 3s and the one
    // starting at 5s are outside the span the caller named.
    let starts: Vec<i64> = summary.tracks[0]
        .clips
        .iter()
        .map(|clip| clip.start_frames)
        .collect();
    assert_eq!(starts, vec![72, 96]);
    assert_eq!(window.described_clips, 2);
    assert_eq!(window.omitted_clips, 8);
}

#[test]
fn a_reversed_or_unreadable_range_is_read_as_a_span_of_the_timeline() {
    let project = packed(10);
    let forward = summarize_for(
        &project,
        SummaryRequest::Range {
            from_seconds: 2.0,
            to_seconds: 4.0,
        },
    );
    let reversed = summarize_for(
        &project,
        SummaryRequest::Range {
            from_seconds: 4.0,
            to_seconds: 2.0,
        },
    );
    assert_eq!(
        forward, reversed,
        "a range read backwards is the same span of timeline"
    );

    let open = summarize_for(
        &project,
        SummaryRequest::Range {
            from_seconds: f64::NAN,
            to_seconds: f64::INFINITY,
        },
    );
    let window = open.window.expect("a range was asked for");
    assert_eq!(window.start_frames, 0);
    assert_eq!(
        window.end_frames, 240,
        "an unreadable end is the last frame"
    );
    assert_eq!(window.omitted_clips, 0);
    assert_eq!(open.tracks[0].clips.len(), 10);
}

#[test]
fn markers_outside_the_window_are_counted_not_dropped_in_silence() {
    let mut project = packed(10);
    for (at, name) in [(24, "inside"), (240, "past the end")] {
        project
            .timeline_mut()
            .add_marker(Marker::new(
                RationalTime::new(at, R24),
                name,
                MarkerColor::Red,
            ))
            .expect("a valid marker position");
    }

    let summary = summarize_for(
        &project,
        SummaryRequest::Range {
            from_seconds: 0.0,
            to_seconds: 2.0,
        },
    );
    let window = summary.window.expect("a range was asked for");
    assert_eq!(summary.markers.len(), 1);
    assert_eq!(summary.markers[0].name, "inside");
    assert_eq!(window.omitted_markers, 1);
}

#[test]
fn a_range_wider_than_the_budget_still_reports_what_it_left_out() {
    let project = packed(CLIP_BUDGET + 50);
    let summary = summarize_for(
        &project,
        SummaryRequest::Range {
            from_seconds: 0.0,
            to_seconds: CLIP_BUDGET as f64 + 50.0,
        },
    );
    let window = summary.window.expect("a range was asked for");
    assert_eq!(window.described_clips, CLIP_BUDGET);
    assert_eq!(window.omitted_clips, 50);
    assert_eq!(window.reason, WindowReason::RangeAsked.text());
}

#[test]
fn the_plan_is_the_same_plan_every_time_it_is_asked() {
    let project = packed(CLIP_BUDGET + 40);
    let first = plan(
        &project,
        SummaryRequest::Session {
            playhead_seconds: 100.0,
        },
    )
    .expect("over the budget");
    let again = plan(
        &project,
        SummaryRequest::Session {
            playhead_seconds: 100.0,
        },
    )
    .expect("over the budget");
    assert_eq!(first.kept, again.kept);
    assert_eq!(first.start_frames, again.start_frames);
    assert_eq!(first.end_frames, again.end_frames);
    assert!(
        first.kept.windows(2).all(|pair| pair[0] < pair[1]),
        "kept ids are ascending, which is what makes the lookup a binary search"
    );
}

#[test]
fn every_reason_is_produced_by_a_fixture_and_reads_as_its_own_sentence() {
    let over_budget = packed(CLIP_BUDGET + 1);
    let produced = [
        plan(
            &over_budget,
            SummaryRequest::Session {
                playhead_seconds: 0.0,
            },
        )
        .expect("over the budget")
        .reason,
        plan(
            &over_budget,
            SummaryRequest::Range {
                from_seconds: 0.0,
                to_seconds: 1.0,
            },
        )
        .expect("a range was asked for")
        .reason,
    ];
    for reason in WindowReason::ALL {
        assert!(
            produced.contains(&reason),
            "{reason:?} is a reason no fixture can produce, so the model never reads it"
        );
        assert!(
            !reason.text().is_empty(),
            "{reason:?} has no wording to read"
        );
    }
    for (index, reason) in WindowReason::ALL.iter().enumerate() {
        for other in &WindowReason::ALL[index + 1..] {
            assert_ne!(
                reason.text(),
                other.text(),
                "{reason:?} and {other:?} share a wording, so the model cannot tell them apart"
            );
        }
    }
}

#[test]
fn every_argument_the_tool_offers_is_an_argument_the_reader_honours() {
    // The schema and the reader are one contract in two files: the spec says
    // what the model may send, this says what happens when it does. A property
    // added to one and not the other is a documented argument that does
    // nothing, which is the failure this catches on the run that adds it.
    let spec = crate::wire::describe_project_spec();
    let offered = spec.parameters["properties"]
        .as_object()
        .expect("the tool declares an object schema");
    assert!(
        !offered.is_empty(),
        "the tool takes a stretch of timeline, so it has to declare one"
    );
    for name in offered.keys() {
        let arguments = serde_json::json!({ name.as_str(): 1.0 });
        assert!(
            matches!(requested(&arguments, 0.0), SummaryRequest::Range { .. }),
            "{name} is offered to the model, but the reader ignores it"
        );
    }

    // No arguments is what the tool did before it took any: the session window.
    assert_eq!(
        requested(&serde_json::json!({}), 7.0),
        SummaryRequest::Session {
            playhead_seconds: 7.0
        }
    );
    // A bound that is not a number is not a bound.
    assert_eq!(
        requested(&serde_json::json!({ "from_seconds": "the start" }), 7.0),
        SummaryRequest::Session {
            playhead_seconds: 7.0
        }
    );
    // One bound alone still names a stretch.
    assert_eq!(
        requested(&serde_json::json!({ "to_seconds": 4.0 }), 7.0),
        SummaryRequest::Range {
            from_seconds: 0.0,
            to_seconds: 4.0
        }
    );
}
