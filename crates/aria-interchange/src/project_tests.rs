//! Reading the open timeline as a cut: a cut placed in a session reads back
//! as the same cut, a subclip reads as its file from the subclip's start,
//! and every lane, clip, transition and setting a cut cannot hold is written
//! the nearest way it can and reported with its exact line.
//!
//! Not caught here: a generated clip on a picture or sound lane, which the
//! model never places there, and a nested sequence; how a format writes a
//! cut, which each format's suite covers.

use std::path::Path;

use aria_models::{
    ClipTransform, Generator, MarkerColor, MediaId, MulticamAngle, Param, Rational, RationalTime,
    TextStyle, TimeRange, TrackKind, resample, shipped_effect_catalog, shipped_transition_catalog,
};
use aria_plugin::{ApplyOutcome, Command, EditCommand};

use super::{DISSOLVE, cut as read};
use crate::model::{Cut, Event, Lane, LaneKind};
use crate::test_support::{
    PICTURE, R24, Session, TALKING, TONE, at, cut, event, lane, mark, session, track,
};

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

/// The clips on the track named `name`, in order.
fn clips(session: &Session, name: &str) -> Vec<aria_models::ClipId> {
    track(&session.project(), name)
        .clips_ordered()
        .map(|clip| clip.id)
        .collect()
}

fn edit(session: &Session, command: EditCommand) -> ApplyOutcome {
    session.apply(Command::Edit(command))
}

/// The pool file named `file`.
fn project_media(session: &Session, file: &str) -> MediaId {
    session
        .project()
        .media_iter()
        .find(|media| media.display_name() == file)
        .expect("the file is in the pool")
        .id
}

fn lane_named<'c>(cut: &'c Cut, name: &str) -> &'c Lane {
    cut.lanes
        .iter()
        .find(|lane| lane.name == name)
        .unwrap_or_else(|| panic!("no lane is named {name}"))
}

#[test]
fn a_placed_cut_reads_back_as_the_same_cut() {
    let session = session();
    let shot = session.copy(PICTURE, "shot.mp4");
    let talk = session.copy(TALKING, "talk.mp4");
    let tone = session.copy(TONE, "tone.wav");
    let picture = lane(
        LaneKind::Video,
        "Picture",
        vec![
            Event {
                sound: true,
                ..event("talk.mp4", 0, 24, text(&talk), at(0, R24))
            },
            Event {
                dissolve: Some(8),
                ..event("Take 2", 24, 24, text(&shot), at(24, R24))
            },
            Event {
                speed: Rational::new(2, 1),
                reversed: true,
                ..event("shot.mp4", 48, 12, text(&shot), at(0, R24))
            },
            event("quiet", 72, 12, text(&talk), at(0, R24)),
        ],
    );
    let sound = lane(
        LaneKind::Audio,
        "Sound",
        vec![Event {
            sound: true,
            ..event("tone.wav", 0, 24, text(&tone), at(0, R24))
        }],
    );
    let marks = vec![
        mark(12, "Cue", "check sync", Some(MarkerColor::Red)),
        mark(40, "Plain", "", Some(MarkerColor::Blue)),
    ];
    let mut placed = cut(
        "Round trip",
        R24,
        vec![picture.clone(), sound.clone()],
        marks.clone(),
    );
    placed.size = Some((1280, 720));
    assert_eq!(session.place(&placed), Vec::<String>::new());

    let project = session.project();
    let read = read(&project);
    assert_eq!(read.skipped, Vec::<String>::new());
    assert_eq!(read.cut.name, project.name);
    assert_eq!(read.cut.rate, R24);
    assert_eq!(read.cut.size, Some((1280, 720)));
    assert_eq!(read.cut.marks, marks);

    // The file states each window at its file's own rate.
    let at_pool_rate = |lane: &Lane| Lane {
        events: lane
            .events
            .iter()
            .map(|event| {
                let path = event.source.path.as_deref().expect("a file");
                let media = project
                    .media_iter()
                    .find(|media| media.path() == path)
                    .expect("the file is in the pool");
                let mut event = event.clone();
                event.source.start = resample(event.source.start, media.frame_rate);
                event
            })
            .collect(),
        ..lane.clone()
    };
    assert_eq!(lane_named(&read.cut, "Picture"), &at_pool_rate(&picture));
    assert_eq!(lane_named(&read.cut, "Sound"), &at_pool_rate(&sound));
    let others: Vec<&str> = read
        .cut
        .lanes
        .iter()
        .filter(|lane| lane.name != "Picture" && lane.name != "Sound")
        .filter(|lane| !lane.events.is_empty())
        .map(|lane| lane.name.as_str())
        .collect();
    assert_eq!(others, Vec::<&str>::new(), "lanes the cut never held");
}

#[test]
fn a_subclip_is_written_as_its_file_from_the_subclip_start() {
    let session = session();
    let shot = session.copy(PICTURE, "shot.mp4");
    let parent = session.import(&shot);
    session.apply(Command::Edit(EditCommand::CreateSubclip {
        parent,
        name: "Middle".to_owned(),
        source: TimeRange::at_rate(24, 24, R24),
        bin: None,
    }));
    let project = session.project();
    let subclip = project
        .media_iter()
        .find(|media| media.is_subclip())
        .expect("the subclip is in the pool")
        .id;
    let ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedTrack(picture)) = edit(
        &session,
        EditCommand::AddTrack {
            kind: TrackKind::Video,
            name: "Picture".to_owned(),
            index: None,
            pinned: false,
        },
    ) else {
        panic!("adding a track created none");
    };
    edit(
        &session,
        EditCommand::AddClip {
            track: picture,
            media: subclip,
            source: TimeRange::at_rate(6, 12, R24),
            start: RationalTime::new(0, R24),
        },
    );

    let read = read(&session.project());
    let lane = lane_named(&read.cut, "Picture");
    assert_eq!(
        lane.events,
        vec![event("Middle", 0, 12, text(&shot), at(30, R24))]
    );
    assert_eq!(read.skipped, Vec::<String>::new());
}

#[test]
fn what_a_cut_cannot_hold_is_written_the_nearest_way_and_reported() {
    let session = session();
    session.copy(PICTURE, "shot.mp4");
    session.copy(TONE, "tone.wav");
    let base = cut(
        "Base",
        R24,
        vec![
            lane(
                LaneKind::Video,
                "Picture",
                vec![
                    event("a", 0, 24, "shot.mp4", at(0, R24)),
                    event("b", 24, 24, "shot.mp4", at(24, R24)),
                    event("c", 48, 24, "shot.mp4", at(48, R24)),
                ],
            ),
            lane(
                LaneKind::Video,
                "Hidden",
                vec![event("shot", 0, 12, "shot.mp4", at(0, R24))],
            ),
            lane(
                LaneKind::Audio,
                "Sound",
                vec![Event {
                    sound: true,
                    ..event("t", 0, 24, "tone.wav", at(0, R24))
                }],
            ),
        ],
        vec![],
    );
    assert_eq!(session.place(&base), Vec::<String>::new());

    let [a, _, c] = clips(&session, "Picture")[..] else {
        panic!("the picture lane holds three clips");
    };
    let effect = &shipped_effect_catalog()[0].id;
    edit(
        &session,
        EditCommand::AddEffect {
            clip: a,
            effect_id: effect.clone(),
        },
    );
    edit(
        &session,
        EditCommand::SetClipTransform {
            clip: a,
            transform: ClipTransform {
                position: [0.25, 0.0],
                ..ClipTransform::IDENTITY
            },
            at: None,
        },
    );
    let wipe = shipped_transition_catalog()
        .iter()
        .find(|spec| spec.id != DISSOLVE)
        .expect("a shipped transition besides the dissolve");
    edit(
        &session,
        EditCommand::AddTransition {
            clip: a,
            transition_id: wipe.id.clone(),
        },
    );
    edit(
        &session,
        EditCommand::SetTransition {
            clip: a,
            duration: 8,
        },
    );
    edit(
        &session,
        EditCommand::SetSpeedCurve {
            clip: c,
            curve: Some(Param::Constant(2.0)),
        },
    );
    let shot = project_media(&session, "shot.mp4");
    edit(
        &session,
        EditCommand::CreateMulticamGroup {
            name: "Cameras".to_owned(),
            angles: vec![MulticamAngle::new(shot, 0)],
        },
    );
    let group = session
        .project()
        .multicam_group_iter()
        .next()
        .expect("the group is in the project")
        .id;
    let picture = track(&session.project(), "Picture").id;
    edit(
        &session,
        EditCommand::AddMulticamClip {
            track: picture,
            group,
            source: TimeRange::at_rate(0, 12, R24),
            start: RationalTime::new(96, R24),
        },
    );

    let hidden = track(&session.project(), "Hidden").id;
    edit(
        &session,
        EditCommand::SetTrackEnabled {
            track: hidden,
            enabled: false,
        },
    );
    let [held] = clips(&session, "Hidden")[..] else {
        panic!("the hidden lane holds one clip");
    };
    edit(
        &session,
        EditCommand::FreezeFrame {
            clip: held,
            at: RationalTime::new(0, R24),
            duration: RationalTime::new(6, R24),
        },
    );

    let sound = track(&session.project(), "Sound").id;
    edit(
        &session,
        EditCommand::SetTrackMuted {
            track: sound,
            muted: true,
        },
    );
    let [tone] = clips(&session, "Sound")[..] else {
        panic!("the sound lane holds one clip");
    };
    edit(
        &session,
        EditCommand::SetClipAudio {
            clip: tone,
            volume: Some(0.5),
            fade_in: RationalTime::new(0, R24),
            fade_out: RationalTime::new(0, R24),
        },
    );

    let ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedTrack(titles)) = edit(
        &session,
        EditCommand::AddTrack {
            kind: TrackKind::Text,
            name: "Titles".to_owned(),
            index: None,
            pinned: false,
        },
    ) else {
        panic!("adding a track created none");
    };
    edit(
        &session,
        EditCommand::AddGenerated {
            track: titles,
            generator: Generator::Text {
                content: "Hello".to_owned(),
                style: TextStyle::default(),
            },
            timeline: TimeRange::at_rate(0, 24, R24),
        },
    );
    // An empty lane of a kind a cut cannot hold has nothing to report.
    edit(
        &session,
        EditCommand::AddTrack {
            kind: TrackKind::Sticker,
            name: "Stickers".to_owned(),
            index: None,
            pinned: true,
        },
    );

    let project = session.project();
    let frozen: Vec<_> = track(&project, "Hidden")
        .clips_ordered()
        .filter(|clip| clip.freeze_frame)
        .collect();
    let [frozen] = frozen[..] else {
        panic!("the freeze frame made one frozen clip");
    };
    let frozen_name = frozen.name.as_deref().unwrap_or("shot.mp4");
    let frozen_at = crate::timecode::format(frozen.timeline.start.value, R24, false);

    let mut skipped = read(&project).skipped;
    skipped.sort();
    let mut expected = vec![
        "Hidden: a hidden lane, written as shown".to_owned(),
        format!("Hidden {frozen_name} at {frozen_at}: freeze frames are not written"),
        "Picture a at 00:00:00:00: effects, position, scale and crop not written".to_owned(),
        format!(
            "Picture b: the {} transition is written as a dissolve",
            wipe.label
        ),
        "Picture c at 00:00:02:00: the speed ramp (written at its base speed) not written"
            .to_owned(),
        "Picture at 00:00:04:00: multicam clips are not written".to_owned(),
        "Sound: a muted lane, written as audible".to_owned(),
        "Sound t at 00:00:00:00: volume, pan, fades and audio effects not written".to_owned(),
        format!("Titles: {} lanes are not written", TrackKind::Text.label()),
    ];
    expected.sort();
    assert_eq!(skipped, expected);
}
