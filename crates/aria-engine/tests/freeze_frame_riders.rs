//! `freeze_frame` tests of the lanes a freeze carries along. A `#[path]`
//! child of the `freeze_frame` test binary; the tests reach their fixtures
//! through `super::*`.

use super::*;
use aria_models::ClipSource;

/// Every media clip that is not a freeze frame: its id, lane, first tick, end
/// and source start, lanes in stack order.
fn pieces(project: &Project) -> Vec<(ClipId, TrackId, i64, i64, i64)> {
    let mut pieces = Vec::new();
    for lane in project.timeline().tracks_ordered() {
        for clip in lane.clips_ordered().filter(|clip| !clip.freeze_frame) {
            if let ClipSource::Media { source, .. } = &clip.content {
                pieces.push((
                    clip.id,
                    lane.id,
                    clip.timeline.start.value,
                    clip.timeline.end_tick(),
                    source.start.value,
                ));
            }
        }
    }
    pieces
}

/// A freeze opens its hold on the source's lane and on every unlocked
/// sync-locked lane, cutting a clip that lies across the tick there, and
/// leaves a lane with its sync lock cleared and a locked lane where they were.
/// The source's sound is cut with it and both tails are linked again. Undo
/// restores every lane, the transition the cut broke on a riding lane
/// included, and redo applies the edit again.
///
/// WHY: the freeze opened its hold on the source's lane only, so the lanes in
/// sync with it drifted by the hold, and a clip linked to its sound could not
/// be frozen at all. Not covered: the drawn layouts and links, which
/// `a_freeze_moves_every_link_in_step_or_is_refused` reads.
#[test]
fn a_freeze_opens_its_hold_on_every_riding_lane() {
    let mut fixture = video_fixture(80);
    let sound_lane = fixture.project.add_track(TrackKind::Audio, "A1");
    let music_lane = fixture.project.add_track(TrackKind::Audio, "A2");
    let pinned_lane = fixture.project.add_track(TrackKind::Audio, "A3");
    let locked_lane = fixture.project.add_track(TrackKind::Video, "V2");
    let blend_lane = fixture.project.add_track(TrackKind::Video, "V3");
    let media = fixture.media;
    let mut place = |lane, source, at, length| {
        fixture
            .project
            .add_clip(lane, media, tr(source, length), rt(at))
            .unwrap()
    };
    let sound = place(sound_lane, 100, 10, 50);
    let music = place(music_lane, 0, 0, 100);
    let pinned = place(pinned_lane, 0, 0, 100);
    let locked = place(locked_lane, 0, 0, 100);
    let left = place(blend_lane, 0, 0, 30);
    let right = place(blend_lane, 30, 30, 30);
    let link = LinkId::next();
    for clip in [fixture.source, sound] {
        fixture.project.timeline_mut().clip_mut(clip).unwrap().link = Some(link);
    }
    let timeline = fixture.project.timeline_mut();
    timeline.track_mut(pinned_lane).unwrap().sync_lock = false;
    timeline.track_mut(locked_lane).unwrap().locked = true;
    let mut engine = engine(fixture.project);
    engine
        .apply(Command::Edit(EditCommand::AddTransition {
            clip: left,
            transition_id: "crossfade".into(),
        }))
        .expect("a crossfade between the V3 clips");
    let before = snapshot(engine.project());

    let freeze = apply_freeze(&mut engine, fixture.source, rt(30), rt(5));

    let project = engine.project();
    let span = |clip: ClipId| {
        let clip = project.clip(clip).unwrap();
        (clip.timeline.start.value, clip.timeline.end_tick())
    };
    let layout = |lane| -> Vec<(i64, i64)> {
        project
            .timeline()
            .track(lane)
            .unwrap()
            .clips_ordered()
            .map(|clip| (clip.timeline.start.value, clip.timeline.end_tick()))
            .collect()
    };
    assert_eq!(span(freeze), (30, 35), "the hold");
    assert_eq!(
        layout(fixture.track),
        [(10, 30), (30, 35), (35, 65), (85, 105)]
    );
    assert_eq!(layout(sound_lane), [(10, 30), (35, 65)], "the sound");
    assert_eq!(layout(music_lane), [(0, 30), (35, 105)], "the music");
    assert_eq!(layout(blend_lane), [(0, 30), (35, 65)], "the V3 pair");
    assert_eq!(span(music).0, 0);
    assert_eq!(span(pinned), (0, 100), "the lane with sync lock cleared");
    assert_eq!(span(locked), (0, 100), "the locked lane");
    assert_eq!(span(right), (35, 65));
    assert!(!project.has_transitions(), "the cut broke the crossfade");
    let tail = |lane| {
        project
            .timeline()
            .track(lane)
            .unwrap()
            .clips_ordered()
            .find(|clip| clip.timeline.start.value == 35)
            .unwrap()
            .link
    };
    let tails = tail(fixture.track);
    assert!(tails.is_some(), "the picture's tail is linked");
    assert_eq!(tails, tail(sound_lane), "to the sound's tail");
    for head in [fixture.source, sound] {
        assert_eq!(project.clip(head).unwrap().link, Some(link), "the heads");
    }

    assert!(engine.undo());
    assert_eq!(
        snapshot(engine.project()),
        before,
        "undo restores every lane"
    );
    assert!(engine.project().has_transitions(), "and the crossfade");
    assert!(engine.redo());
    assert_eq!(
        engine.project().clip(right).unwrap().timeline.start.value,
        35
    );
    assert!(!engine.project().has_transitions());
}

/// A seeded splitmix64 stream, so a failing layout is named by its seed.
struct Draw(u64);

impl Draw {
    fn below(&mut self, bound: u64) -> i64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % bound) as i64
    }
}

/// Where each link of `members` broke, read from `pieces` of the project a
/// freeze left: the clips still holding a member's id start at one offset
/// from their source, and every two clips playing the same source frames of
/// one link play them at the same tick. `members` is each link's member ids
/// and the source start its material is drawn from.
fn broken_links(
    members: &[(Vec<ClipId>, i64)],
    pieces: &[(ClipId, TrackId, i64, i64, i64)],
) -> Vec<String> {
    let mut broken = Vec::new();
    for (index, (ids, base)) in members.iter().enumerate() {
        let carrying: Vec<i64> = pieces
            .iter()
            .filter(|piece| ids.contains(&piece.0))
            .map(|&(_, _, start, _, source)| source - start)
            .collect();
        if carrying.windows(2).any(|pair| pair[0] != pair[1]) {
            broken.push(format!("link {index} holds offsets {carrying:?}"));
        }
        let material: Vec<_> = pieces
            .iter()
            .filter(|piece| (*base..base + 1_000).contains(&piece.4))
            .map(|&(_, lane, start, end, source)| {
                (lane, source, source + end - start, source - start)
            })
            .collect();
        for (a, first) in material.iter().enumerate() {
            for second in &material[a + 1..] {
                let overlap = first.1 < second.2 && second.1 < first.2;
                if first.0 != second.0 && overlap && first.3 != second.3 {
                    broken.push(format!("link {index} plays {first:?} beside {second:?}"));
                }
            }
        }
    }
    broken
}

/// Over drawn layouts of four lanes, each beside the source's sync-locked or
/// not and locked or not, with clips in up to three links whose members play
/// the same source frames at the same ticks, a freeze at every tick of a V1
/// clip either applies and leaves every link in step, or is refused with
/// [`FreezeRefusal::Linked`] or [`FreezeRefusal::SplitsLink`] where the same
/// freeze with the links cleared pulls a link apart, or is refused by the cut
/// it makes on a lane.
///
/// Closes the class "the freeze check and the edit disagree about which lanes
/// move": a rider rule read differently by the check, a pinned partner the
/// check misses, and a refusal of a freeze that keeps every link in step each
/// show up as a seed and a tick. Not covered: groups, which a freeze does not
/// read, and speed changes, which every drawn clip leaves at one.
#[test]
fn a_freeze_moves_every_link_in_step_or_is_refused() {
    let (mut applied, mut refused) = (0, 0);
    for seed in 0..120 {
        let mut draw = Draw(seed);
        let mut project = Project::new("drawn", FPS);
        let media = project.add_media(MediaSource::new("take.mp4", 1920, 1080, FPS, 9_000, true));
        let lanes = [
            project.add_track(TrackKind::Video, "V1"),
            project.add_track(TrackKind::Video, "V2"),
            project.add_track(TrackKind::Audio, "A1"),
            project.add_track(TrackKind::Audio, "A2"),
        ];
        let links = [LinkId::next(), LinkId::next(), LinkId::next()];
        let mut members: Vec<(Vec<ClipId>, i64)> =
            (0..3).map(|n| (Vec::new(), 1_000 * (n + 1))).collect();
        let mut first_lane = Vec::new();
        for (index, &lane) in lanes.iter().enumerate() {
            let mut at = draw.below(12);
            for _ in 0..1 + draw.below(4) {
                let length = 4 + draw.below(24);
                let which = draw.below(5) as usize;
                let source = members
                    .get(which)
                    .map_or(draw.below(400), |(_, base)| base + at);
                let placed = project
                    .add_clip(lane, media, TimeRange::at_rate(source, length, FPS), rt(at))
                    .expect("a drawn clip");
                if let Some(link) = links.get(which) {
                    project.timeline_mut().clip_mut(placed).unwrap().link = Some(*link);
                    members[which].0.push(placed);
                }
                if index == 0 {
                    first_lane.push(placed);
                }
                at += length + draw.below(8);
            }
        }
        for &lane in &lanes[1..] {
            let flags = draw.below(4);
            let track = project.timeline_mut().track_mut(lane).unwrap();
            track.sync_lock = flags & 1 == 0;
            track.locked = flags & 2 != 0;
        }
        let clip = first_lane[draw.below(first_lane.len() as u64) as usize];
        let span = project.clip(clip).unwrap().timeline;
        for at in span.start.value..=span.end_tick() {
            let case = format!("seed {seed}, at {at}");
            let command = || Command::Edit(freeze_command(clip, rt(at), rt(3)));
            let mut real = engine(project.clone());
            match real.apply(command()) {
                Ok(_) => {
                    applied += 1;
                    let broken = broken_links(&members, &pieces(real.project()));
                    assert!(broken.is_empty(), "{case}: {broken:?}");
                }
                Err(EngineError::Model(ModelError::Freeze(
                    FreezeRefusal::Linked | FreezeRefusal::SplitsLink,
                ))) => {
                    let mut unlinked = project.clone();
                    for lane in lanes {
                        let ids: Vec<ClipId> = unlinked
                            .timeline()
                            .track(lane)
                            .unwrap()
                            .clips()
                            .map(|clip| clip.id)
                            .collect();
                        for id in ids {
                            unlinked.timeline_mut().clip_mut(id).unwrap().link = None;
                        }
                    }
                    let mut replay = engine(unlinked);
                    if replay.apply(command()).is_ok() {
                        refused += 1;
                        let broken = broken_links(&members, &pieces(replay.project()));
                        assert!(!broken.is_empty(), "{case}: refused a freeze in step");
                    }
                }
                Err(EngineError::Model(ModelError::Split(_))) => {}
                Err(other) => panic!("{case}: {other:?}"),
            }
        }
    }
    assert!(applied > 300, "only {applied} freezes applied");
    assert!(refused > 30, "only {refused} link refusals replayed");
}
