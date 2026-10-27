//! How far a trim takes each edge of a clip.
//!
//! # Why
//!
//! Closes the class where a drag on a clip edge passes the end of its take,
//! the clip beside it, or the sequence start, and the trim is refused at
//! release. Every case holds both ends of both edges of
//! [`Project::trim_reach`] against the trim itself: each end lands, one tick
//! past a bounded end does not. The cases cover a take between neighbors,
//! retimed and reversed takes, a take at another rate, a still, generated
//! clips, and a linked pair whose members have different handles.
//!
//! Not covered: multicam angles and nested sequences, whose tail no source
//! bounds; speed ramps; two linked members on one lane.

use super::*;

/// Whether moving the head of `clip` by `head` ticks and its tail by `tail`
/// lands, with `linked` on every clip linked to it as the linked trim does.
/// With `lane` it is the trim itself, which the neighbors and the sequence
/// start refuse. Without, only the source refuses it.
fn lands(project: &Project, clip: ClipId, lane: bool, linked: bool, head: i64, tail: i64) -> bool {
    let members = match project.clip(clip).unwrap().link {
        Some(link) if linked => project.linked_clips(link),
        Some(_) | None => vec![clip],
    };
    let mut trial = project.clone();
    members.into_iter().all(|member| {
        let at = trial.clip(member).unwrap().timeline;
        let (start, end) = (at.start.value + head, at.end_tick() + tail);
        let range = TimeRange::at_rate(start, end - start, at.start.rate);
        if lane {
            trial.trim_clip(member, range).is_ok()
        } else {
            trial.plan_trim(member, range, false).is_ok()
        }
    })
}

/// The reach of `clip`, after asserting that each end of each edge lands
/// and one tick past each bounded end does not.
fn held(project: &Project, clip: ClipId, lane: bool, linked: bool) -> TrimReach {
    let reach = project.trim_reach(clip, lane, linked).unwrap();
    let at = project.clip(clip).unwrap().timeline;
    let (start, end) = (at.start.value, at.end_tick());
    let lands = |head: i64, tail: i64| lands(project, clip, lane, linked, head, tail);
    let (earliest, latest_start) = reach.start;
    match earliest {
        Some(earliest) => {
            assert!(lands(earliest - start, 0), "head to {earliest}: {reach:?}");
            assert!(
                !lands(earliest - start - 1, 0),
                "head past {earliest}: {reach:?}"
            );
        }
        None => assert!(
            lands(-(1 << 40), 0),
            "an unbounded head lands far: {reach:?}"
        ),
    }
    assert!(
        lands(latest_start - start, 0),
        "head to {latest_start}: {reach:?}"
    );
    assert!(
        !lands(latest_start - start + 1, 0),
        "head past {latest_start}: {reach:?}"
    );
    let (earliest_end, latest) = reach.end;
    assert!(
        lands(0, earliest_end - end),
        "tail to {earliest_end}: {reach:?}"
    );
    assert!(
        !lands(0, earliest_end - end - 1),
        "tail past {earliest_end}: {reach:?}"
    );
    match latest {
        Some(latest) => {
            assert!(lands(0, latest - end), "tail to {latest}: {reach:?}");
            assert!(!lands(0, latest - end + 1), "tail past {latest}: {reach:?}");
        }
        None => assert!(lands(0, 1 << 40), "an unbounded tail lands far: {reach:?}"),
    }
    reach
}

fn reach(start: (Option<i64>, i64), end: (i64, Option<i64>)) -> TrimReach {
    TrimReach { start, end }
}

#[test]
fn a_take_between_neighbors_stops_at_them_and_at_its_source() {
    let (mut project, media, track) = project_with_media(200);
    project.add_clip(track, media, tr(0, 24), rt(0)).unwrap();
    let take = project.add_clip(track, media, tr(30, 48), rt(40)).unwrap();
    project.add_clip(track, media, tr(0, 24), rt(100)).unwrap();

    // The left neighbor ends at 24, the right one starts at 100.
    assert_eq!(
        held(&project, take, true, false),
        reach((Some(24), 87), (41, Some(100)))
    );
    // 30 source ticks before the take, 122 after it.
    assert_eq!(
        held(&project, take, false, false),
        reach((Some(10), 87), (41, Some(210)))
    );
}

#[test]
fn a_take_at_the_sequence_start_stops_there() {
    let (mut project, media, track) = project_with_media(200);
    let take = project.add_clip(track, media, tr(30, 48), rt(0)).unwrap();
    assert_eq!(
        held(&project, take, true, false),
        reach((Some(0), 47), (1, Some(170)))
    );
    // A trim that moves the lane out of its way reaches all 30 source ticks.
    assert_eq!(
        held(&project, take, false, false),
        reach((Some(-30), 47), (1, Some(170)))
    );
}

#[test]
fn a_retimed_take_reaches_its_source_at_its_speed() {
    let (mut project, media, track) = project_with_media(200);
    let fast = project.add_clip(track, media, tr(30, 48), rt(40)).unwrap();
    project
        .set_clip_speed(fast, Rational::new(2, 1), false)
        .unwrap();
    // 40..64 plays 30..78 at twice the rate: 15 ticks of head, 61 of tail.
    assert_eq!(
        held(&project, fast, true, false),
        reach((Some(25), 63), (41, Some(125)))
    );

    let (mut project, media, track) = project_with_media(200);
    let slow = project.add_clip(track, media, tr(30, 48), rt(100)).unwrap();
    project
        .set_clip_speed(slow, Rational::new(1, 2), false)
        .unwrap();
    // 100..196 plays 30..78 at half the rate, truncating source ticks.
    assert_eq!(
        held(&project, slow, true, false),
        reach((Some(39), 195), (101, Some(441)))
    );
}

#[test]
fn a_reversed_take_reaches_its_source_from_the_other_end() {
    let (mut project, media, track) = project_with_media(200);
    let take = project
        .add_clip(track, media, tr(150, 48), rt(100))
        .unwrap();
    // Forward, 150 source ticks lie before the head and 2 after the tail.
    assert_eq!(
        held(&project, take, true, false),
        reach((Some(0), 147), (101, Some(150)))
    );
    project
        .set_clip_speed(take, Rational::new(1, 1), true)
        .unwrap();
    // Reversed, the head shows the source end and the tail its start.
    assert_eq!(
        held(&project, take, true, false),
        reach((Some(98), 147), (101, Some(298)))
    );
}

#[test]
fn a_take_at_another_rate_reaches_its_source_in_sequence_ticks() {
    let mut project = Project::new("test", R24);
    let media = project.add_media(sample_media(R30, 200));
    let track = project.add_track(TrackKind::Video, "V1");
    let take = project
        .add_clip(track, media, tr_at(60, 60, R30), rt(100))
        .unwrap();
    // 60 source ticks at 30 fps are 48 at 24; 140 after the tail are 112.
    assert_eq!(
        held(&project, take, true, false),
        reach((Some(52), 147), (101, Some(212)))
    );
}

#[test]
fn a_still_extends_until_a_neighbor() {
    let mut project = Project::new("test", R24);
    let mut picture = sample_media(R24, 48);
    picture.is_image = true;
    let media = project.add_media(picture);
    let track = project.add_track(TrackKind::Video, "V1");
    let still = project.add_clip(track, media, tr(0, 48), rt(100)).unwrap();
    assert_eq!(
        held(&project, still, true, false),
        reach((Some(0), 147), (101, None))
    );

    project.add_clip(track, media, tr(0, 48), rt(200)).unwrap();
    assert_eq!(
        held(&project, still, true, false),
        reach((Some(0), 147), (101, Some(200)))
    );
    assert_eq!(
        held(&project, still, false, false),
        reach((None, 147), (101, None))
    );
}

#[test]
fn generated_clips_stop_only_at_each_other() {
    let (project, left, right, _) = project_with_abutting_pair();
    assert_eq!(
        held(&project, left, true, false),
        reach((Some(0), 23), (1, Some(24)))
    );
    assert_eq!(
        held(&project, left, false, false),
        reach((None, 23), (1, None))
    );
    assert_eq!(
        held(&project, right, true, false),
        reach((Some(24), 47), (25, None))
    );
    assert_eq!(
        held(&project, right, false, false),
        reach((None, 47), (25, None))
    );
}

#[test]
fn a_linked_trim_stops_at_the_nearest_limit_of_any_member() {
    let (mut project, media, video) = project_with_media(100);
    let audio = project.add_track(TrackKind::Audio, "A1");
    let picture = project.add_clip(video, media, tr(10, 48), rt(100)).unwrap();
    let sound = project.add_clip(audio, media, tr(40, 48), rt(100)).unwrap();
    project.link_clips(&[picture, sound]).unwrap();
    project.add_clip(audio, media, tr(0, 10), rt(150)).unwrap();

    // The picture has 10 source ticks of head and 42 of tail.
    assert_eq!(
        held(&project, picture, true, false),
        reach((Some(90), 147), (101, Some(190)))
    );
    // The sound has 40 of head, 12 of tail, and a clip at 150.
    assert_eq!(
        held(&project, sound, true, false),
        reach((Some(60), 147), (101, Some(150)))
    );
    let both = reach((Some(90), 147), (101, Some(150)));
    assert_eq!(held(&project, picture, true, true), both);
    assert_eq!(held(&project, sound, true, true), both);
    assert_eq!(
        held(&project, picture, false, true),
        reach((Some(90), 147), (101, Some(160)))
    );
}

#[test]
fn a_reach_holds_an_edge_inside_it() {
    let bounded = reach((Some(24), 87), (41, Some(100)));
    assert_eq!(
        (bounded.head(0), bounded.head(50), bounded.head(90)),
        (24, 50, 87)
    );
    assert_eq!(
        (bounded.tail(0), bounded.tail(60), bounded.tail(500)),
        (41, 60, 100)
    );
    let open = reach((None, 47), (25, None));
    assert_eq!(open.head(-(1 << 40)), -(1 << 40));
    assert_eq!(open.head(60), 47);
    assert_eq!(open.tail(1 << 40), 1 << 40);
}
