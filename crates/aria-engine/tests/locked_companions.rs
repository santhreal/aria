//! A clip linked or grouped to the edited one, on a locked lane.
//!
//! # Why
//!
//! A lock rejects every edit addressed to its lane, but a move, trim, retime
//! or slip of a clip carried its linked and grouped companions along on every
//! lane, locked or not, and the host's lock check reads only the lanes an edit
//! names. So dragging a picture moved or trimmed its sound on a locked lane.
//! The class closed here is a companion set that ignores the lock: every
//! command that carries companions reads one set, and a companion on a locked
//! lane stays while one on an unlocked lane still follows.
//!
//! Not caught here: the drawn drag, which reads the same companion set, and a
//! sync-locked lane that rides a ripple, which `sync_lock` covers.

mod common;

use aria_commands::{Command, EditCommand};
use aria_engine::{Engine, EngineConfig};
use aria_models::{ClipId, MediaSource, Project, Rational, TimeRange, TrackId, TrackKind};
use common::{rt, tr};

/// A sound counted in milliseconds.
const MS: Rational = Rational::new(1_000, 1);

/// Frames 48 to 96 of a 300-frame shot at 24 on V1, and 2 s to 4 s of a 10 s
/// sound at 24 on A1 and on A2, the three tied by `tie`: a link, or a group.
/// A1 is locked.
struct Tied {
    engine: Engine,
    v1: TrackId,
    picture: ClipId,
    locked: ClipId,
    free: ClipId,
}

fn tied(grouped: bool) -> Tied {
    let mut project = Project::new("locked-companions", Rational::FPS_24);
    let shot = project.add_media(MediaSource::new(
        "/srv/shot.mp4",
        1920,
        1080,
        Rational::FPS_24,
        300,
        false,
    ));
    let tone = project.add_media(MediaSource::new("/srv/tone.wav", 0, 0, MS, 10_000, true));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let a1 = project.add_track(TrackKind::Audio, "A1");
    let a2 = project.add_track(TrackKind::Audio, "A2");
    let picture = project
        .add_clip(v1, shot, tr(48, 48), rt(24))
        .expect("place the picture");
    let sound = |project: &mut Project, lane| {
        project
            .add_clip(lane, tone, TimeRange::at_rate(2_000, 2_000, MS), rt(24))
            .expect("place a sound")
    };
    let locked = sound(&mut project, a1);
    let free = sound(&mut project, a2);
    let clips = [picture, locked, free];
    if grouped {
        let plan = project.group_plan(&clips).expect("plan the group");
        project.set_clip_groups(&plan).expect("group the three");
    } else {
        project.link_clips(&clips).expect("link the three");
    }
    project.timeline_mut().track_mut(a1).expect("A1").locked = true;
    let engine = Engine::with_project(EngineConfig { undo_limit: 32 }, project).expect("engine");
    Tied {
        engine,
        v1,
        picture,
        locked,
        free,
    }
}

/// Where `clip` sits on its lane and which stretch of its take it plays.
fn placed(engine: &Engine, clip: ClipId) -> (i64, i64, Option<TimeRange>) {
    let clip = engine.project().clip(clip).expect("the clip is placed");
    (
        clip.timeline.start.value,
        clip.timeline.duration.value,
        clip.source_range(),
    )
}

#[test]
fn a_companion_on_a_locked_lane_stays_through_every_edit_that_carries_companions() {
    // (case, grouped, the edit of the picture).
    let cases: [(&str, bool, fn(TrackId, ClipId) -> EditCommand); 8] = [
        ("move", false, |to_track, clip| EditCommand::MoveClip {
            clip,
            to_track,
            start: rt(48),
        }),
        ("overwrite move", false, |to_track, clip| {
            EditCommand::OverwriteMoveClip {
                clip,
                to_track,
                start: rt(48),
            }
        }),
        ("ripple move", false, |to_track, clip| {
            EditCommand::RippleMoveClip {
                clip,
                to_track,
                start: rt(96),
            }
        }),
        ("trim", false, |_, clip| EditCommand::TrimClip {
            clip,
            timeline: tr(30, 42),
        }),
        ("speed", false, |_, clip| EditCommand::SetClipSpeed {
            clip,
            speed: Rational::new(2, 1),
            reversed: false,
        }),
        ("slip", false, |_, clip| EditCommand::SlipClip {
            clip,
            by: 12,
        }),
        ("grouped move", true, |to_track, clip| {
            EditCommand::MoveClipUnlinked {
                clip,
                to_track,
                start: rt(48),
            }
        }),
        ("grouped overwrite move", true, |to_track, clip| {
            EditCommand::OverwriteMoveClipUnlinked {
                clip,
                to_track,
                start: rt(48),
            }
        }),
    ];
    for (case, grouped, edit) in cases {
        let Tied {
            mut engine,
            v1,
            picture,
            locked,
            free,
        } = tied(grouped);
        let before = [picture, locked, free].map(|clip| placed(&engine, clip));
        engine
            .apply(Command::Edit(edit(v1, picture)))
            .unwrap_or_else(|err| panic!("{case}: {err}"));
        let after = [picture, locked, free].map(|clip| placed(&engine, clip));
        assert_ne!(after[0], before[0], "{case}: the picture did not change");
        assert_eq!(
            after[1], before[1],
            "{case}: a companion on a locked lane changed"
        );
        assert_ne!(
            after[2], before[2],
            "{case}: a companion on an unlocked lane stayed"
        );
        assert!(engine.undo(), "{case}: no undo step");
        assert_eq!(
            [picture, locked, free].map(|clip| placed(&engine, clip)),
            before,
            "{case}: one undo did not put the three back"
        );
    }
}
