//! Placing a cut in a real engine session: the timeline adopts the cut's
//! rate and frame size only while empty, each file is imported once and found
//! by path, by reel, or beside the timeline file, overlapping events take
//! numbered tracks, and every event, dissolve and mark the timeline cannot
//! hold as written is placed the nearest way it can and reported with its
//! exact line.
//!
//! Not caught here: what a format reads into a cut; each format's suite
//! covers that.

use std::path::PathBuf;

use aria_commands::EditCommand;
use aria_models::{ClipSource, MarkerColor, MediaId, Param, Project, Rational};
use aria_plugin::{Command, CommandProposed};

use crate::model::{Event, LaneKind, Source};
use crate::project::DISSOLVE;
use crate::test_support::{
    PICTURE, PICTURE_30, R24, Session, TALKING, TONE, UNIT, at, cut, event, lane, mark, session,
    track,
};

/// The pool name of 24 fps `media`, and the frames it holds.
fn pooled(session: &Session, media: MediaId) -> (String, i64) {
    let project = session.project();
    let media = project.media(media).expect("the file is in the pool");
    assert_eq!(
        media.frame_rate,
        R24,
        "{} is not 24 fps",
        media.display_name()
    );
    (media.display_name().to_owned(), media.duration.value)
}

/// One placed clip: timeline start and length, source start and length,
/// speed, and its own name.
#[derive(Debug, PartialEq)]
struct Placed {
    at: (i64, i64),
    source: (i64, i64),
    speed: Rational,
    name: Option<String>,
}

fn clip(at: (i64, i64), source: (i64, i64), name: Option<&str>) -> Placed {
    Placed {
        at,
        source,
        speed: UNIT,
        name: name.map(str::to_owned),
    }
}

fn placed(project: &Project, name: &str) -> Vec<Placed> {
    track(project, name)
        .clips_ordered()
        .map(|clip| {
            let ClipSource::Media { source, .. } = &clip.content else {
                panic!("{name} holds a clip that reads no file");
            };
            Placed {
                at: (clip.timeline.start.value, clip.timeline.duration.value),
                source: (source.start.value, source.duration.value),
                speed: clip.speed,
                name: clip.name.clone(),
            }
        })
        .collect()
}

fn has_track(project: &Project, name: &str) -> bool {
    project
        .timeline()
        .tracks_ordered()
        .any(|track| track.name == name)
}

/// An event that names only a reel.
fn reel_event(name: &str, start: i64, duration: i64, reel: &str) -> Event {
    Event {
        source: Source {
            reel: reel.to_owned(),
            path: None,
            start: at(0, R24),
        },
        ..event(name, start, duration, "", at(0, R24))
    }
}

fn no_report() -> Vec<String> {
    Vec::new()
}

#[test]
fn an_empty_timeline_takes_the_rate_and_frame_size_of_the_cut() {
    let session = session();
    session.copy(PICTURE_30, "shot.mp4");
    let mut cut = cut(
        "Reel",
        Rational::FPS_30,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![event("shot", 0, 30, "shot.mp4", at(15, Rational::FPS_30))],
        )],
        vec![],
    );
    cut.size = Some((1280, 720));

    assert_eq!(session.place(&cut), no_report());
    let project = session.project();
    assert_eq!(project.timeline().frame_rate, Rational::FPS_30);
    assert_eq!(aria_render::canvas_size(&project), (1280, 720));
    assert_eq!(
        placed(&project, "Picture"),
        vec![clip((0, 30), (15, 30), None)]
    );
}

#[test]
fn a_timeline_that_holds_clips_keeps_its_rate_and_converts_the_cut() {
    let session = session();
    session.copy(PICTURE, "shot.mp4");
    let first = cut(
        "First",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![event("shot", 0, 24, "shot.mp4", at(0, R24))],
        )],
        vec![],
    );
    assert_eq!(session.place(&first), no_report());
    let canvas = session.project().timeline().canvas().resolution;

    let r48 = Rational::new(48, 1);
    let mut second = cut(
        "Second",
        r48,
        vec![lane(
            LaneKind::Video,
            "Overlay",
            vec![event("shot", 48, 48, "shot.mp4", at(24, R24))],
        )],
        vec![mark(96, "Out", "", Some(MarkerColor::Green))],
    );
    second.size = Some((1280, 720));

    assert_eq!(
        session.place(&second),
        vec!["the file runs at 48 fps; its cut is placed at the timeline's 24 fps"]
    );
    let project = session.project();
    assert_eq!(project.timeline().frame_rate, R24);
    assert_eq!(
        project.timeline().canvas().resolution,
        canvas,
        "a timeline that holds clips keeps its canvas"
    );
    assert_eq!(
        placed(&project, "Overlay"),
        vec![clip((24, 24), (24, 24), None)]
    );
    let marks: Vec<(i64, &str)> = project
        .timeline()
        .markers()
        .iter()
        .map(|marker| (marker.tick.value, marker.name.as_str()))
        .collect();
    assert_eq!(marks, vec![(48, "Out")]);
    assert_eq!(
        project.media_iter().count(),
        1,
        "the second cut reads the file the first imported"
    );
}

#[test]
fn a_frame_size_that_is_no_canvas_leaves_the_canvas() {
    let session = session();
    session.copy(PICTURE, "shot.mp4");
    let before = session.project().timeline().canvas().resolution;
    let mut cut = cut(
        "Odd",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![event("shot", 0, 24, "shot.mp4", at(0, R24))],
        )],
        vec![],
    );
    cut.size = Some((641, 360));

    assert_eq!(
        session.place(&cut),
        vec!["the file's 641x360 frame is not a canvas size; the canvas is unchanged"]
    );
    let project = session.project();
    assert_eq!(project.timeline().canvas().resolution, before);
    assert_eq!(placed(&project, "Picture").len(), 1);
}

#[test]
fn each_file_is_imported_once_and_events_place_in_record_order() {
    let session = session();
    let path = session.copy(PICTURE, "shot.mp4");
    let absolute = path.to_str().expect("UTF-8 path");
    let cut = cut(
        "Cut",
        R24,
        vec![
            lane(
                LaneKind::Video,
                "Picture",
                vec![
                    event("b", 24, 24, "shot.mp4", at(24, R24)),
                    event("a", 0, 24, "shot.mp4", at(0, R24)),
                ],
            ),
            lane(
                LaneKind::Video,
                "Overlay",
                vec![event("c", 0, 12, absolute, at(48, R24))],
            ),
        ],
        vec![],
    );

    assert_eq!(session.place(&cut), no_report());
    let project = session.project();
    assert_eq!(project.media_iter().count(), 1);
    assert_eq!(
        placed(&project, "Picture"),
        vec![
            clip((0, 24), (0, 24), Some("a")),
            clip((24, 24), (24, 24), Some("b")),
        ]
    );
    assert_eq!(
        placed(&project, "Overlay"),
        vec![clip((0, 12), (48, 12), Some("c"))]
    );
}

#[test]
fn a_missing_file_is_reported_once_with_the_events_it_leaves_out() {
    let session = session();
    let cut = cut(
        "Cut",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![
                event("one", 0, 24, "gone.mp4", at(0, R24)),
                event("two", 24, 24, "gone.mp4", at(0, R24)),
                event("three", 48, 24, "lost.mov", at(0, R24)),
            ],
        )],
        vec![],
    );

    let dir = session.dir.path();
    assert_eq!(
        session.place(&cut),
        vec![
            format!(
                "gone: no file at {}; 2 events not placed",
                dir.join("gone.mp4").display()
            ),
            format!(
                "lost: no file at {}; 1 event not placed",
                dir.join("lost.mov").display()
            ),
        ]
    );
    let project = session.project();
    assert_eq!(project.media_iter().count(), 0);
    assert!(
        !has_track(&project, "Picture"),
        "a lane with nothing placed adds no track"
    );
}

#[test]
fn a_file_that_moved_is_found_beside_the_timeline_file() {
    let session = session();
    let copy = session.copy(PICTURE, "shot.mp4");
    let cut = cut(
        "Moved",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![event(
                "shot",
                0,
                24,
                "/nowhere/aria-moved/shot.mp4",
                at(0, R24),
            )],
        )],
        vec![],
    );

    assert_eq!(session.place(&cut), no_report());
    let project = session.project();
    let pool: Vec<PathBuf> = project
        .media_iter()
        .map(|media| media.path().canonicalize().expect("the pool file exists"))
        .collect();
    assert_eq!(pool, vec![copy.canonicalize().expect("the copy exists")]);
    assert_eq!(
        placed(&project, "Picture"),
        vec![clip((0, 24), (0, 24), None)]
    );
}

#[test]
fn a_reel_with_no_file_reads_the_pool_file_of_that_name() {
    let session = session();
    let copy = session.copy(PICTURE, "Interview.mp4");
    session.import(&copy);
    let cut = cut(
        "Reels",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![
                reel_event("first", 0, 24, "Interview"),
                reel_event("second", 24, 24, "Absent"),
            ],
        )],
        vec![],
    );

    assert_eq!(
        session.place(&cut),
        vec![
            "Absent: the file names no media for this reel, and no file in the pool is named \
             Absent; 1 event not placed"
        ]
    );
    let project = session.project();
    assert_eq!(project.media_iter().count(), 1);
    assert_eq!(
        placed(&project, "Picture"),
        vec![clip((0, 24), (0, 24), Some("first"))]
    );
}

#[test]
fn overlapping_events_take_numbered_tracks_of_their_lane() {
    let session = session();
    session.copy(PICTURE, "shot.mp4");
    let cut = cut(
        "Stack",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![
                event("a", 0, 24, "shot.mp4", at(0, R24)),
                event("b", 12, 24, "shot.mp4", at(24, R24)),
                event("c", 24, 24, "shot.mp4", at(48, R24)),
            ],
        )],
        vec![],
    );

    assert_eq!(session.place(&cut), no_report());
    let project = session.project();
    assert_eq!(
        placed(&project, "Picture"),
        vec![
            clip((0, 24), (0, 24), Some("a")),
            clip((24, 24), (48, 24), Some("c")),
        ]
    );
    assert_eq!(
        placed(&project, "Picture (2)"),
        vec![clip((12, 24), (24, 24), Some("b"))]
    );
    assert!(!has_track(&project, "Picture (3)"));
}

#[test]
fn events_the_timeline_cannot_hold_are_reported_and_the_rest_placed() {
    let session = session();
    session.copy(PICTURE, "shot.mp4");
    let cut = cut(
        "Edges",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![
                event("kept", 48, 24, "shot.mp4", at(0, R24)),
                event("empty", 24, 0, "shot.mp4", at(0, R24)),
                event("early", -5, 24, "shot.mp4", at(0, R24)),
            ],
        )],
        vec![],
    );

    assert_eq!(
        session.place(&cut),
        vec![
            "Picture early at 00:00:00:00: before the start of the timeline",
            "Picture empty at 00:00:01:00: shorter than a frame at the timeline rate",
        ]
    );
    assert_eq!(
        placed(&session.project(), "Picture"),
        vec![clip((48, 24), (0, 24), Some("kept"))]
    );
}

/// Also the regression for a track taken before its event was fitted: an
/// event its file cannot play left an empty track, and the lane's next
/// event took a second track of the same name.
#[test]
fn a_source_window_past_the_file_is_reported_or_shortened() {
    let session = session();
    let copy = session.copy(PICTURE, "shot.mp4");
    let (name, length) = pooled(&session, session.import(&copy));
    let sliver = Event {
        speed: Rational::new(4, 1),
        ..event("sliver", 96, 1, "shot.mp4", at(length - 1, R24))
    };
    let cut = cut(
        "Ends",
        R24,
        vec![
            lane(
                LaneKind::Video,
                "Picture",
                vec![
                    event("before", 0, 24, "shot.mp4", at(-1, R24)),
                    event("after", 24, 24, "shot.mp4", at(length, R24)),
                    event("short", 48, 24, "shot.mp4", at(length - 8, R24)),
                    sliver,
                ],
            ),
            lane(
                LaneKind::Video,
                "Spare",
                vec![event("past", 0, 24, "shot.mp4", at(length + 5, R24))],
            ),
        ],
        vec![],
    );

    assert_eq!(
        session.place(&cut),
        vec![
            format!("Picture before at 00:00:00:00: its source starts before the start of {name}"),
            format!("Picture after at 00:00:01:00: its source starts after the end of {name}"),
            format!(
                "Picture short at 00:00:02:00: {name} ends 16 frames early; the clip is shortened"
            ),
            format!("Picture sliver at 00:00:04:00: {name} holds less than a frame of it"),
            format!("Spare past at 00:00:00:00: its source starts after the end of {name}"),
        ]
    );
    let project = session.project();
    assert_eq!(
        placed(&project, "Picture"),
        vec![clip((48, 8), (length - 8, 8), Some("short"))]
    );
    assert!(!has_track(&project, "Picture (2)"));
    assert!(
        !has_track(&project, "Spare"),
        "a lane its file plays none of adds no track"
    );
}

/// A track whose first clip a `CommandProposed` handler refused holds the
/// lane's next event rather than leaving it a second track of the same name.
#[test]
fn a_clip_refused_on_a_new_track_leaves_the_track_to_the_next_event() {
    let session = session();
    session.copy(PICTURE, "shot.mp4");
    let _policy = session
        .context()
        .on_waterfall::<CommandProposed, _>(|proposed| {
            if let Command::Edit(EditCommand::AddClip { start, .. }) = &proposed.command
                && start.value == 0
            {
                proposed.refuse("no clip at the start");
            }
        });
    let cut = cut(
        "Refused",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![
                event("refused", 0, 24, "shot.mp4", at(0, R24)),
                event("kept", 24, 24, "shot.mp4", at(24, R24)),
            ],
        )],
        vec![],
    );

    assert_eq!(
        session.place(&cut),
        vec![
            "Picture refused at 00:00:00:00: Add clip was refused: no clip at the start".to_owned()
        ]
    );
    let project = session.project();
    assert_eq!(
        placed(&project, "Picture"),
        vec![clip((24, 24), (24, 24), Some("kept"))]
    );
    assert!(!has_track(&project, "Picture (2)"));
}

#[test]
fn a_speed_clips_cannot_play_is_held_to_the_nearest_bound() {
    let session = session();
    let copy = session.copy(PICTURE, "shot.mp4");
    let (name, length) = pooled(&session, session.import(&copy));
    assert!(length < 100, "the fast event needs fewer than 100 frames");
    let slow = Event {
        speed: Rational::new(1, 100),
        ..event("slow", 0, 40, "shot.mp4", at(0, R24))
    };
    let fast = Event {
        speed: Rational::new(200, 1),
        ..event("fast", 48, 1, "shot.mp4", at(0, R24))
    };
    let cut = cut(
        "Speeds",
        R24,
        vec![lane(LaneKind::Video, "Picture", vec![slow, fast])],
        vec![],
    );

    assert_eq!(
        session.place(&cut),
        vec![
            "Picture slow at 00:00:00:00: 0.01x is slower than clips play; placed at 0.05x"
                .to_owned(),
            "Picture fast at 00:00:02:00: 200x is faster than clips play; placed at 100x"
                .to_owned(),
            format!("Picture fast at 00:00:02:00: {name} holds less than a frame of it"),
        ]
    );
    assert_eq!(
        placed(&session.project(), "Picture"),
        vec![Placed {
            speed: Rational::new(1, 20),
            ..clip((0, 40), (0, 2), Some("slow"))
        }]
    );
}

#[test]
fn a_picture_placed_without_its_sound_is_muted() {
    let session = session();
    session.copy(TALKING, "talk.mp4");
    session.copy(PICTURE, "quiet.mp4");
    let sounding = Event {
        sound: true,
        ..event("loud", 24, 24, "talk.mp4", at(0, R24))
    };
    let cut = cut(
        "Sound",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![
                event("mute", 0, 24, "talk.mp4", at(0, R24)),
                sounding,
                event("silent", 48, 24, "quiet.mp4", at(0, R24)),
            ],
        )],
        vec![],
    );

    assert_eq!(session.place(&cut), no_report());
    let project = session.project();
    let volumes: Vec<(Option<&str>, &Param<f32>)> = track(&project, "Picture")
        .clips_ordered()
        .map(|clip| (clip.name.as_deref(), &clip.volume))
        .collect();
    assert_eq!(
        volumes,
        vec![
            (Some("mute"), &Param::Constant(0.0)),
            (Some("loud"), &Param::Constant(1.0)),
            (Some("silent"), &Param::Constant(1.0)),
        ]
    );
}

#[test]
fn a_clip_is_named_only_when_its_event_names_something_besides_the_file() {
    let session = session();
    session.copy(PICTURE, "shot.mp4");
    let cut = cut(
        "Names",
        R24,
        vec![lane(
            LaneKind::Video,
            "Picture",
            vec![
                event("shot", 0, 12, "shot.mp4", at(0, R24)),
                event("shot.mp4", 12, 12, "shot.mp4", at(12, R24)),
                event("", 24, 12, "shot.mp4", at(24, R24)),
                event("Take 2", 36, 12, "shot.mp4", at(36, R24)),
            ],
        )],
        vec![],
    );

    assert_eq!(session.place(&cut), no_report());
    let names: Vec<Option<String>> = placed(&session.project(), "Picture")
        .into_iter()
        .map(|clip| clip.name)
        .collect();
    assert_eq!(names, vec![None, None, None, Some("Take 2".to_owned())]);
}

#[test]
fn a_dissolve_joins_adjacent_pictures_and_is_reported_anywhere_else() {
    let session = session();
    session.copy(PICTURE, "shot.mp4");
    session.copy(TONE, "tone.wav");
    let dissolving = |name: &str, start: i64, duration: i64, file: &str, from: i64, frames| Event {
        dissolve: Some(frames),
        ..event(name, start, duration, file, at(from, R24))
    };
    let sound = |event: Event| Event {
        sound: true,
        ..event
    };
    let cut = cut(
        "Dissolves",
        R24,
        vec![
            lane(
                LaneKind::Video,
                "Picture",
                vec![
                    dissolving("a", 0, 24, "shot.mp4", 0, 6),
                    dissolving("b", 24, 24, "shot.mp4", 24, 8),
                    dissolving("c", 60, 12, "shot.mp4", 48, 4),
                ],
            ),
            lane(
                LaneKind::Audio,
                "Sound",
                vec![
                    sound(event("x", 0, 12, "tone.wav", at(0, R24))),
                    sound(dissolving("y", 12, 12, "tone.wav", 12, 4)),
                ],
            ),
        ],
        vec![],
    );

    assert_eq!(
        session.place(&cut),
        vec![
            "Picture a at 00:00:00:00: a dissolve with no clip before it, placed as a cut",
            "Picture c at 00:00:02:12: a dissolve with no clip before it, placed as a cut",
            "Sound y at 00:00:00:12: an audio crossfade, placed as a cut",
        ]
    );
    let project = session.project();
    let picture = track(&project, "Picture");
    let ids: Vec<_> = picture.clips_ordered().map(|clip| clip.id).collect();
    assert_eq!(ids.len(), 3);
    let joins: Vec<_> = picture
        .transitions()
        .iter()
        .map(|t| (t.left, t.right, t.transition_id.as_str(), t.duration))
        .collect();
    assert_eq!(joins, vec![(ids[0], ids[1], DISSOLVE, 8)]);
    assert!(track(&project, "Sound").transitions().is_empty());
    assert_eq!(placed(&project, "Sound").len(), 2);
}

#[test]
fn marks_land_with_their_names_colors_and_comments() {
    let session = session();
    let cut = cut(
        "Marks",
        R24,
        vec![],
        vec![
            mark(10, "Cue", "check sync", Some(MarkerColor::Red)),
            mark(20, "Plain", "", Some(MarkerColor::Blue)),
            mark(30, "Noted", "a note", None),
            mark(-3, "Early", "", None),
        ],
    );

    assert_eq!(
        session.place(&cut),
        vec!["marker Early at 00:00:00:00: before the start of the timeline"]
    );
    let project = session.project();
    let mut marks: Vec<(i64, &str, Option<MarkerColor>, &str)> = project
        .timeline()
        .markers()
        .iter()
        .map(|marker| {
            let color = (marker.name != "Noted").then_some(marker.color);
            (
                marker.tick.value,
                marker.name.as_str(),
                color,
                marker.comment.as_str(),
            )
        })
        .collect();
    marks.sort_by_key(|mark| mark.0);
    assert_eq!(
        marks,
        vec![
            (10, "Cue", Some(MarkerColor::Red), "check sync"),
            (20, "Plain", Some(MarkerColor::Blue), ""),
            (30, "Noted", None, "a note"),
        ]
    );
}
