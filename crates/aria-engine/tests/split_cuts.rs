//! What a cut does to the transitions at a clip's ends and to the link it
//! shares.
//!
//! A clip's in-point transition stays with the head and its out-point
//! transition moves to the tail, which ends at that cut now. `SplitClip`
//! refuses a cut that leaves a half shorter than a transition window reaching
//! into it, for every alignment, and names the transition. An edit that cuts
//! as one step of a larger change and is then refused puts back whatever its
//! cuts moved. A cut through linked clips on several lanes keeps each source
//! link's tails linked to each other and to nothing else.
//!
//! Not covered: the editor gestures that send these cuts (Add Edit, the razor,
//! Lift and Extract), asserted in `aria-ui`.

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    ClipId, LinkId, MediaId, MediaSource, Project, Rational, RationalTime, TimeRange, TrackId,
    TrackKind, TransitionAlignment, transition_spec,
};

const FPS: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS)
}

fn engine(project: Project) -> Engine {
    Engine::with_project(EngineConfig { undo_limit: 32 }, project).expect("engine")
}

fn apply(engine: &mut Engine, command: EditCommand) -> ApplyOutcome {
    engine
        .apply(Command::Edit(command))
        .unwrap_or_else(|err| panic!("edit refused: {err}"))
}

fn created(outcome: ApplyOutcome) -> ClipId {
    match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected a new clip, got {other:?}"),
    }
}

fn project_with_media() -> (Project, MediaId) {
    let mut project = Project::new("cuts", FPS);
    let media = project.add_media(MediaSource::new(
        "/tmp/cuts.mp4",
        1920,
        1080,
        FPS,
        1_000,
        true,
    ));
    (project, media)
}

fn json(engine: &Engine) -> serde_json::Value {
    serde_json::to_value(engine.project()).expect("project json")
}

/// The catalog name a refusal prints for `id`.
fn label(id: &str) -> String {
    transition_spec(id)
        .expect("catalog transition")
        .label
        .clone()
}

/// Every transition on `track` as `(left, right, id, alignment)`, by left clip.
fn junctions(
    engine: &Engine,
    track: TrackId,
) -> Vec<(ClipId, ClipId, String, TransitionAlignment)> {
    let mut junctions: Vec<_> = engine
        .project()
        .timeline()
        .track(track)
        .expect("track")
        .transitions()
        .iter()
        .map(|t| (t.left, t.right, t.transition_id.clone(), t.alignment))
        .collect();
    junctions.sort_by_key(|junction| junction.0);
    junctions
}

/// The clip on `track` that starts at `start`.
fn clip_at(engine: &Engine, track: TrackId, start: i64) -> ClipId {
    engine
        .project()
        .timeline()
        .track(track)
        .expect("track")
        .clips()
        .find(|clip| clip.timeline.start.value == start)
        .unwrap_or_else(|| panic!("no clip starts at {start}"))
        .id
}

/// A [0,48), B [48,120), C [120,168) on V1, with a 24-tick crossfade at A|B
/// and a 24-tick dip to black at B|C, both aligned `alignment`.
struct Row {
    project: Project,
    media: MediaId,
    track: TrackId,
    a: ClipId,
    b: ClipId,
    c: ClipId,
}

fn row(alignment: TransitionAlignment) -> Row {
    let (mut project, media) = project_with_media();
    let track = project.add_track(TrackKind::Video, "V1");
    let [a, b, c] = [(0, 48), (48, 72), (120, 48)].map(|(start, length)| {
        project
            .add_clip(track, media, tr(start, length), rt(start))
            .expect("clip")
    });
    for (clip, id) in [(a, "crossfade"), (b, "dip_to_black")] {
        project.add_transition(clip, id).expect("transition");
        project.set_transition_duration(clip, 24).expect("duration");
        project
            .set_transition_alignment(clip, alignment)
            .expect("alignment");
    }
    Row {
        project,
        media,
        track,
        a,
        b,
        c,
    }
}

#[test]
fn a_cut_keeps_the_in_point_transition_on_the_head_and_moves_the_out_point_one_to_the_tail() {
    for alignment in TransitionAlignment::ALL {
        let Row {
            project,
            track,
            a,
            b,
            c,
            ..
        } = row(alignment);
        let mut engine = engine(project);
        let before = junctions(&engine, track);

        let tail = created(apply(
            &mut engine,
            EditCommand::SplitClip {
                clip: b,
                at: rt(72),
            },
        ));
        let after = vec![
            (a, b, "crossfade".to_owned(), alignment),
            (tail, c, "dip_to_black".to_owned(), alignment),
        ];
        assert_eq!(junctions(&engine, track), after, "{alignment:?}");

        assert!(engine.undo());
        assert_eq!(junctions(&engine, track), before, "{alignment:?} undo");
        assert!(engine.redo());
        assert_eq!(junctions(&engine, track), after, "{alignment:?} redo");
    }
}

/// The end of B a cut lands 12 ticks from.
#[derive(Debug, Clone, Copy)]
enum Side {
    /// At 60: the head is 12 ticks long.
    In,
    /// At 108: the tail is 12 ticks long.
    Out,
}

#[test]
fn a_razor_cut_that_leaves_a_half_shorter_than_a_transition_window_is_refused() {
    for alignment in TransitionAlignment::ALL {
        for side in [Side::In, Side::Out] {
            let Row {
                project, track, b, ..
            } = row(alignment);
            let mut engine = engine(project);
            // A centered window takes 12 ticks from each clip at its cut, one
            // that starts at its cut takes all 24 from the clip after it, and
            // one that ends there takes all 24 from the clip before it.
            let window_reaches_short_half = match (alignment, side) {
                (TransitionAlignment::Center, _) => true,
                (TransitionAlignment::Start, Side::In) => true,
                (TransitionAlignment::Start, Side::Out) => false,
                (TransitionAlignment::End, Side::In) => false,
                (TransitionAlignment::End, Side::Out) => true,
            };
            let (at, transition) = match side {
                Side::In => (60, "crossfade"),
                Side::Out => (108, "dip_to_black"),
            };
            let before = json(&engine);
            let revision = engine.revision();

            let result = engine.apply(Command::Edit(EditCommand::SplitClip {
                clip: b,
                at: rt(at),
            }));

            let case = format!("{alignment:?} cut at {at}");
            if window_reaches_short_half {
                let err = result.expect_err(&case).to_string();
                assert!(
                    err.contains(&label(transition)),
                    "{case}: '{err}' names the {transition}"
                );
                assert_eq!(
                    json(&engine),
                    before,
                    "{case}: the refused cut left a trace"
                );
                assert_eq!(engine.revision(), revision, "{case}");
            } else {
                result.unwrap_or_else(|err| panic!("{case}: {err}"));
                assert_eq!(junctions(&engine, track).len(), 2, "{case}");
            }
        }
    }
}

#[test]
fn an_overwrite_inside_a_clip_keeps_its_out_point_transition_on_the_surviving_tail() {
    let Row {
        project,
        media,
        track,
        a,
        b,
        c,
    } = row(TransitionAlignment::Center);
    let mut engine = engine(project);

    created(apply(
        &mut engine,
        EditCommand::OverwriteClip {
            track,
            media,
            source: tr(0, 12),
            at: rt(72),
        },
    ));
    let tail = clip_at(&engine, track, 84);
    assert_eq!(
        junctions(&engine, track),
        vec![
            (a, b, "crossfade".to_owned(), TransitionAlignment::Center),
            (
                tail,
                c,
                "dip_to_black".to_owned(),
                TransitionAlignment::Center
            ),
        ]
    );

    // A cover across the cut itself leaves nothing for the window to join.
    created(apply(
        &mut engine,
        EditCommand::OverwriteClip {
            track,
            media,
            source: tr(0, 24),
            at: rt(108),
        },
    ));
    assert_eq!(
        junctions(&engine, track),
        vec![(a, b, "crossfade".to_owned(), TransitionAlignment::Center)]
    );
}

#[test]
fn a_refused_overwrite_puts_back_the_transition_its_first_cut_moved() {
    let Row {
        mut project,
        media,
        track,
        b,
        ..
    } = row(TransitionAlignment::Center);
    // The first cut, at 72, leaves the 20-tick fade-out on a 48-tick rest and
    // moves the dip to black onto it; the second, at 108, would leave a
    // 12-tick tail inside the fade-out.
    project.timeline_mut().clip_mut(b).expect("b").fade_out = 20;
    let mut engine = engine(project);
    let before = json(&engine);

    let err = engine
        .apply(Command::Edit(EditCommand::OverwriteClip {
            track,
            media,
            source: tr(0, 36),
            at: rt(72),
        }))
        .expect_err("the second cut falls inside the fade-out");
    assert!(err.to_string().contains("fade-out"), "{err}");
    assert_eq!(json(&engine), before, "the refused overwrite left a trace");
}

/// V1 and A1 hold one linked pair and V2 and A2 another, all [0,48).
/// Returns the project, the media, and the lanes `[v1, a1, v2, a2]`.
fn two_linked_pairs() -> (Project, MediaId, [TrackId; 4]) {
    let (mut project, media) = project_with_media();
    let lanes = [
        project.add_track(TrackKind::Video, "V1"),
        project.add_track(TrackKind::Audio, "A1"),
        project.add_track(TrackKind::Video, "V2"),
        project.add_track(TrackKind::Audio, "A2"),
    ];
    let clips = lanes.map(|lane| {
        project
            .add_clip(lane, media, tr(0, 48), rt(0))
            .expect("clip")
    });
    project.link_clips(&clips[..2]).expect("first pair");
    project.link_clips(&clips[2..]).expect("second pair");
    (project, media, lanes)
}

/// The links of the clips starting at `start` on each lane.
fn links_at(engine: &Engine, lanes: [TrackId; 4], start: i64) -> [Option<LinkId>; 4] {
    lanes.map(|lane| {
        let clip = clip_at(engine, lane, start);
        engine.project().clip(clip).expect("clip").link
    })
}

/// Heads keep the links they had; each pair's tails share a link of their own.
fn assert_pairs_kept(
    heads: [Option<LinkId>; 4],
    before: [Option<LinkId>; 4],
    tails: [Option<LinkId>; 4],
) {
    assert_eq!(heads, before, "the heads keep their links");
    assert!(tails[0].is_some(), "the tails are linked");
    assert_eq!(tails[0], tails[1], "the first pair's tails share a link");
    assert_eq!(tails[2], tails[3], "the second pair's tails share a link");
    assert_ne!(tails[0], tails[2], "the two pairs stay apart");
    assert!(
        !before.contains(&tails[0]) && !before.contains(&tails[2]),
        "a tail joins no head's link"
    );
}

#[test]
fn a_ripple_insert_through_linked_pairs_keeps_each_pair_linked_on_both_sides() {
    let (project, media, lanes) = two_linked_pairs();
    let mut engine = engine(project);
    let before = links_at(&engine, lanes, 0);

    created(apply(
        &mut engine,
        EditCommand::RippleInsert {
            track: lanes[0],
            media,
            source: tr(0, 12),
            at: rt(24),
        },
    ));
    assert_pairs_kept(
        links_at(&engine, lanes, 0),
        before,
        links_at(&engine, lanes, 36),
    );

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().clip_count(), 4);
    assert_eq!(links_at(&engine, lanes, 0), before);
}

#[test]
fn a_ripple_delete_through_linked_pairs_on_rider_lanes_keeps_each_pair_linked() {
    let (mut project, media, lanes) = two_linked_pairs();
    let shot_lane = project.add_track(TrackKind::Video, "V3");
    let shot = project
        .add_clip(shot_lane, media, tr(0, 24), rt(12))
        .expect("shot");
    let mut engine = engine(project);
    let before = links_at(&engine, lanes, 0);

    apply(&mut engine, EditCommand::RippleDelete { clip: shot });
    assert_pairs_kept(
        links_at(&engine, lanes, 0),
        before,
        links_at(&engine, lanes, 12),
    );

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().clip_count(), 5);
    assert_eq!(links_at(&engine, lanes, 0), before);
}
