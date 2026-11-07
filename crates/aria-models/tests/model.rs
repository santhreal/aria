//! WHY: Closes the class of model invariant violations, unchecked tick
//! overflows, partial mutations on refused edits, and save/load persistence
//! drift across `Project` and `Timeline` operations. Runs a deterministic
//! seeded generator over multi-track, multi-sequence, linked, parented,
//! transitioned, marker-annotated, multicam, bin, and bus-routed projects,
//! asserting full structural invariants and `save_to_file -> load_from_file ->
//! save_to_file` byte idempotence after every step. Does not cover GPU shader
//! execution or container demuxing.

mod common;

use std::collections::BTreeSet;

use aria_models::{
    ClipId, ClipParam, ClipSource, ClipTransform, Easing, Generator, Marker, MarkerColor,
    MediaSource, ModelError, MulticamAngle, ParamValue, Project, Rational, SplitRefusal, Timeline,
    TrackKind, TrackOutput, TransitionAlignment,
};
use common::{FPS_24, FPS_30, rt, sample_media, tr, tr_at};
#[test]
fn set_transform_updates_visual_and_rejects_audio() {
    let mut project = Project::new("demo", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 1000));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let a1 = project.add_track(TrackKind::Audio, "A1");

    let video = project.add_clip(v1, media_id, tr(0, 48), rt(0)).unwrap();
    let audio = project.add_clip(a1, media_id, tr(0, 48), rt(0)).unwrap();

    // New clips start at identity (aspect-fit, centered).
    assert!(project.clip(video).unwrap().transform.is_identity());

    let t = ClipTransform {
        position: [0.1, 0.2],
        scale: 2.0.into(),
        rotation: -30.0,
        opacity: 0.5,
        ..ClipTransform::IDENTITY
    };
    project
        .set_transform(video, t, None)
        .expect("set transform");
    assert_eq!(project.clip(video).unwrap().transform, t.into());

    // Audio clips have nothing to place on the canvas.
    assert!(matches!(
        project.set_transform(audio, t, None),
        Err(ModelError::NotOnPicture { .. })
    ));

    // Invalid values are rejected and leave the stored transform unchanged.
    for bad in [
        ClipTransform {
            scale: 0.0.into(),
            ..ClipTransform::IDENTITY
        },
        ClipTransform {
            opacity: 1.5,
            ..ClipTransform::IDENTITY
        },
        ClipTransform {
            position: [f32::NAN, 0.0],
            ..ClipTransform::IDENTITY
        },
        ClipTransform {
            rotation: f32::INFINITY,
            ..ClipTransform::IDENTITY
        },
    ] {
        assert!(matches!(
            project.set_transform(video, bad, None),
            Err(ModelError::InvalidTransform(_))
        ));
    }
    assert_eq!(project.clip(video).unwrap().transform, t.into());
}

#[test]
fn unknown_refs_error() {
    let mut project = Project::new("demo", FPS_24);
    let v1 = project.add_track(TrackKind::Video, "V1");
    let media_id = project.add_media(sample_media(FPS_24, 1000));

    let bad_media = MediaSource::new("/x", 1, 1, FPS_24, 10, false).id;
    assert!(matches!(
        project.add_clip(v1, bad_media, tr(0, 10), rt(0)),
        Err(ModelError::UnknownMedia(_))
    ));

    assert_eq!(
        project.add_clip(v1, media_id, tr(900, 200), rt(0)),
        Err(ModelError::SourceOutOfBounds)
    );
}

#[test]
fn rate_conform_adjusts_timeline_duration() {
    let mut project = Project::new("demo", FPS_24);
    let media_id = project.add_media(sample_media(FPS_30, 1000));
    let v1 = project.add_track(TrackKind::Video, "V1");

    let clip_id = project
        .add_clip(v1, media_id, tr_at(0, 120, FPS_30), rt(0))
        .unwrap();
    assert_eq!(project.clip(clip_id).unwrap().timeline.duration.value, 96);
}

#[test]
fn removing_referenced_media_fails_then_succeeds() {
    let mut project = Project::new("demo", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 1000));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let clip_id = project.add_clip(v1, media_id, tr(0, 100), rt(0)).unwrap();

    assert_eq!(
        project.remove_media(media_id),
        Err(ModelError::MediaReferenced(media_id))
    );

    project.timeline_mut().remove_clip(clip_id).unwrap();
    assert!(project.remove_media(media_id).is_ok());
    assert_eq!(project.media_count(), 0);
}

#[test]
fn split_at_or_outside_boundary_is_rejected() {
    let mut project = Project::new("demo", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 1000));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(v1, media_id, tr(0, 100), rt(10)).unwrap();

    for at in [10, 110, 200] {
        assert_eq!(
            project.split_clip(clip, rt(at)),
            Err(ModelError::Split(SplitRefusal::OffClip)),
            "a cut at {at}"
        );
    }
}

#[test]
fn trim_past_source_bounds_is_rejected() {
    let mut project = Project::new("demo", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 100));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(v1, media_id, tr(90, 10), rt(0)).unwrap();

    assert_eq!(
        project.trim_clip(clip, tr(0, 40)),
        Err(ModelError::SourceOutOfBounds)
    );
}

#[test]
fn trim_into_neighbor_is_rejected() {
    let mut project = Project::new("demo", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 1000));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let a = project.add_clip(v1, media_id, tr(0, 100), rt(0)).unwrap();
    project.add_clip(v1, media_id, tr(0, 100), rt(100)).unwrap();

    assert_eq!(
        project.trim_clip(a, tr(0, 150)),
        Err(ModelError::Overlap(v1))
    );
}

#[test]
fn move_clip_across_tracks_and_rejects_overlap() {
    let mut project = Project::new("demo", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 1000));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let v2 = project.add_track(TrackKind::Video, "V2");
    let clip = project.add_clip(v1, media_id, tr(0, 100), rt(0)).unwrap();
    project.add_clip(v2, media_id, tr(0, 100), rt(0)).unwrap();

    assert_eq!(
        project.move_clip(clip, v2, rt(0)),
        Err(ModelError::Overlap(v2))
    );
    assert_eq!(project.timeline().track_of(clip), Some(v1));

    project.move_clip(clip, v2, rt(200)).unwrap();
    assert_eq!(project.timeline().track_of(clip), Some(v2));
    assert_eq!(project.clip(clip).unwrap().timeline, tr(200, 100));
}

#[test]
fn move_clip_rejects_incompatible_track_kind() {
    let mut project = Project::new("demo", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 1000));
    let video = project.add_track(TrackKind::Video, "V1");
    let overlay = project.add_track(TrackKind::Text, "T1");
    let clip = project
        .add_clip(video, media_id, tr(0, 100), rt(0))
        .unwrap();

    assert_eq!(
        project.move_clip(clip, overlay, rt(0)),
        Err(ModelError::IncompatibleTrackKind {
            track: overlay,
            kind: TrackKind::Text,
        })
    );
}

#[test]
fn generated_clip_rejects_wrong_track_kind() {
    let mut project = Project::new("demo", FPS_24);
    let video = project.add_track(TrackKind::Video, "V1");

    assert_eq!(
        project.add_generated(video, Generator::text("nope"), tr(0, 24),),
        Err(ModelError::IncompatibleTrackKind {
            track: video,
            kind: TrackKind::Video,
        })
    );
}

#[test]
fn editing_unknown_clip_errors() {
    let mut project = Project::new("demo", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 1000));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(v1, media_id, tr(0, 100), rt(0)).unwrap();
    let gone = project.ripple_delete(clip).unwrap().id;

    assert!(matches!(
        project.split_clip(gone, rt(5)),
        Err(ModelError::UnknownClip(_))
    ));
    assert!(matches!(
        project.trim_clip(gone, tr(0, 10)),
        Err(ModelError::UnknownClip(_))
    ));
    assert!(matches!(
        project.move_clip(gone, v1, rt(0)),
        Err(ModelError::UnknownClip(_))
    ));
}

#[test]
fn media_pool_lookup_and_iteration() {
    let mut project = Project::new("pool", FPS_24);
    let a = project.add_media(sample_media(FPS_24, 100));
    let b = project.add_media(sample_media(FPS_30, 200));

    assert_eq!(project.media(a).unwrap().duration.value, 100);
    assert_eq!(project.media(b).unwrap().frame_rate, FPS_30);

    let mut durations: Vec<i64> = project.media_iter().map(|m| m.duration.value).collect();
    durations.sort();
    assert_eq!(durations, vec![100, 200]);
}

#[test]
fn resample_preserved_across_split_trim_chain() {
    let mut project = Project::new("chain", FPS_24);
    let media_id = project.add_media(sample_media(FPS_30, 600));
    let track = project.add_track(TrackKind::Video, "V1");

    let clip = project
        .add_clip(track, media_id, tr_at(0, 300, FPS_30), rt(0))
        .unwrap();
    assert_eq!(project.clip(clip).unwrap().timeline.duration.value, 240);

    let right = project.split_clip(clip, rt(120)).unwrap();
    project.trim_clip(right, tr(120, 96)).unwrap();

    assert_eq!(project.clip(clip).unwrap().timeline, tr(0, 120));
    assert_eq!(project.clip(right).unwrap().timeline, tr(120, 96));
    assert_eq!(project.timeline().duration().value, 216);
}

/// WHY: `Timeline::resample_frame_rate` documented that it resamples
/// "clip-relative keyframe parameters" and did not. `Clip::resample_timeline_params`
/// existed, was `pub(crate)`, and was reached only from unit tests, so changing a
/// sequence's rate rewrote every clip placement, fade, transition, and marker onto
/// the new grid and left keyframe ticks on the old one. A keyframe at tick 24 is one
/// second at 24 fps and half a second at 48, so every animation in the project slid
/// earlier by the ratio of the two rates.
///
/// The assertion is on the sampled value at a wall-clock instant, not on tick
/// numbers: that is what playback shows, and it stays correct if the resampling
/// arithmetic is ever rounded differently. Against the unfixed code the midpoint
/// reads fully faded instead of half faded.
///
/// It does not cover sequence-addressed params on a retimed clip, or a rate change
/// whose ratio is not an integer.
#[test]
fn changing_the_sequence_rate_keeps_every_keyframe_at_its_wall_clock_time() {
    use aria_models::{ClipParam, Easing, ParamValue, Rational};

    const FPS_48: Rational = Rational::new(48, 1);

    let mut project = Project::new("rate", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 600));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(track, media_id, tr(0, 48), rt(0))
        .expect("a two-second clip");

    // Opaque at 0 s, gone at 1 s: at 24 fps that is ticks 0 and 24.
    project
        .set_param_keyframe(
            clip,
            ClipParam::Opacity,
            rt(0),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .expect("opening keyframe");
    project
        .set_param_keyframe(
            clip,
            ClipParam::Opacity,
            rt(24),
            ParamValue::Scalar(0.0),
            Easing::Linear,
            None,
        )
        .expect("closing keyframe");

    let midpoint = |project: &Project, timeline_tick: i64| -> f32 {
        let clip = project
            .clip(clip)
            .expect("the clip survives the rate change");
        clip.transform
            .opacity
            .sample_at(clip.param_tick_f(timeline_tick as f64))
    };

    // Half a second in: 12 ticks at 24 fps, 24 ticks at 48 fps.
    let before = midpoint(&project, 12);
    assert!(
        (before - 0.5).abs() < 1e-4,
        "half a second in must be half faded, got {before}"
    );

    project
        .resample_frame_rate(FPS_48)
        .expect("48 fps is a valid rate");
    assert_eq!(project.timeline().frame_rate.num, 48);

    let after = midpoint(&project, 24);
    assert!(
        (after - 0.5).abs() < 1e-4,
        "the same half second must still be half faded after the rate change, got {after}"
    );

    // And the endpoints must land on the new grid rather than the old one.
    let opening = midpoint(&project, 0);
    let closing = midpoint(&project, 48);
    assert!(
        (opening - 1.0).abs() < 1e-4,
        "the clip must still open opaque, got {opening}"
    );
    assert!(
        (closing - 0.0).abs() < 1e-4,
        "one second in must still be fully faded, got {closing}"
    );
}

fn assert_timeline_invariants(project: &Project, timeline: &Timeline) {
    assert!(
        timeline.frame_rate.is_valid(),
        "invalid timeline frame_rate: {:?}",
        timeline.frame_rate
    );
    let order = timeline.order();
    let mut seen_tracks = BTreeSet::new();
    let mut total_clips = 0usize;
    let mut prev_zone = 0u8;
    let mut video_tracks = Vec::new();
    let mut main_tracks = Vec::new();

    for &track_id in order {
        assert!(
            seen_tracks.insert(track_id),
            "duplicate track {track_id:?} in timeline order"
        );
        let track = timeline.track(track_id).expect("ordered track exists");
        assert_eq!(track.id, track_id);

        let zone = match track.kind {
            TrackKind::Submix => 0,
            TrackKind::Audio => 1,
            TrackKind::Video => 2,
            TrackKind::Text
            | TrackKind::Sticker
            | TrackKind::Effect
            | TrackKind::Filter
            | TrackKind::Adjustment => 3,
        };
        assert!(
            zone >= prev_zone,
            "lane zone order violated: {track_id:?} ({:?}, zone {zone}) after zone {prev_zone}",
            track.kind
        );
        prev_zone = zone;
        if track.kind == TrackKind::Video {
            video_tracks.push(track_id);
        }
        if track.main {
            main_tracks.push(track_id);
            assert_eq!(
                track.kind,
                TrackKind::Video,
                "non-video track {track_id:?} flagged as main"
            );
        }

        if let TrackOutput::Bus(bus) = track.output {
            let bus_track = timeline.track(bus).expect("output bus exists");
            assert_eq!(bus_track.kind, TrackKind::Submix);
            assert_ne!(bus, track_id);
        }
        for send in &track.sends {
            let bus_track = timeline.track(send.to).expect("send bus exists");
            assert_eq!(bus_track.kind, TrackKind::Submix);
            assert_ne!(send.to, track_id);
        }

        let ordered: Vec<_> = track.clips_ordered().collect();
        assert_eq!(ordered.len(), track.len(), "track.order len mismatch");
        total_clips += ordered.len();

        for clip in &ordered {
            assert_eq!(
                timeline.track_of(clip.id),
                Some(track_id),
                "clip_index mismatch for {:?}",
                clip.id
            );
            assert!(
                track.kind.accepts_clip(clip),
                "track {track_id:?} ({:?}) holds incompatible clip {:?}",
                track.kind,
                clip.id
            );
            assert!(clip.timeline.start.value >= 0);
            assert!(clip.timeline.duration.value > 0);
            assert_eq!(clip.timeline.start.rate, timeline.frame_rate);
            assert_eq!(clip.timeline.duration.rate, timeline.frame_rate);
            let end = clip.timeline.end().expect("clip timeline end valid");
            assert!(end.value > clip.timeline.start.value);

            match &clip.content {
                ClipSource::Media { media, source } => {
                    let asset = project.media(*media).expect("clip media exists");
                    assert!(source.start.value >= 0);
                    assert!(source.duration.value > 0);
                    assert_eq!(source.start.rate, asset.frame_rate);
                    assert_eq!(source.duration.rate, asset.frame_rate);
                    let src_end = source.end().expect("clip source end valid").value;
                    if !asset.is_image && !clip.freeze_frame {
                        assert!(
                            src_end <= asset.duration.value,
                            "clip {:?} source end {src_end} > media duration {}",
                            clip.id,
                            asset.duration.value
                        );
                    }
                }
                ClipSource::Sequence { sequence, source } => {
                    let seq = project.sequence(*sequence).expect("clip sequence exists");
                    assert!(source.start.value >= 0);
                    assert!(source.duration.value > 0);
                    assert_eq!(source.start.rate, seq.timeline.frame_rate);
                    assert_eq!(source.duration.rate, seq.timeline.frame_rate);
                    source.end().expect("sequence source end valid");
                }
                ClipSource::Multicam { group, source } => {
                    assert!(project.multicam_group(*group).is_some());
                    assert!(source.start.value >= 0);
                    assert!(source.duration.value > 0);
                    source.end().expect("multicam source end valid");
                }
                ClipSource::Generated(_) => {}
            }

            let mut cur = clip.parent;
            let mut steps = 0usize;
            while let Some(parent) = cur {
                assert_ne!(parent, clip.id, "parent cycle on {:?}", clip.id);
                let Some(parent_clip) = timeline.clip(parent) else {
                    break;
                };
                cur = parent_clip.parent;
                steps += 1;
                assert!(steps <= timeline.clip_count(), "unbounded parent chain");
            }

            let window = clip.clip_time_range();
            let win_end = window.end().expect("clip window end").value;
            for m in &clip.markers {
                assert!(m.tick.value >= window.start.value && m.tick.value < win_end);
                assert!(m.duration.value >= 0);
                let m_end = m.tick.value.checked_add(m.duration.value).unwrap();
                assert!(m_end <= win_end);
            }
        }

        for pair in ordered.windows(2) {
            let left_end = pair[0].timeline.end().unwrap().value;
            let right_start = pair[1].timeline.start.value;
            assert!(
                left_end <= right_start,
                "overlapping clips {:?} and {:?} on {track_id:?}",
                pair[0].id,
                pair[1].id
            );
        }

        for t in track.transitions() {
            assert!(t.duration > 0);
            let l = track.clip(t.left).expect("transition left clip exists");
            let r = track.clip(t.right).expect("transition right clip exists");
            assert_eq!(
                l.timeline.end_tick(),
                r.timeline.start.value,
                "transition clips {:?} and {:?} do not abut",
                t.left,
                t.right
            );
            let limit = t
                .alignment
                .junction_limit(l.timeline.duration.value, r.timeline.duration.value);
            assert!(
                t.duration <= limit,
                "transition duration {} exceeds junction_limit {limit}",
                t.duration
            );
        }
    }

    assert_eq!(seen_tracks.len(), timeline.track_count());
    assert_eq!(total_clips, timeline.clip_count());
    if video_tracks.is_empty() {
        assert!(main_tracks.is_empty());
    } else {
        assert_eq!(main_tracks, vec![video_tracks[0]]);
    }

    let seq_end = timeline.duration().value;
    for m in timeline.markers() {
        assert!(m.tick.value >= 0 && m.duration.value >= 0);
        let m_end = m.tick.value.checked_add(m.duration.value).unwrap();
        if m.duration.value > 0 {
            assert!(m_end <= seq_end);
        }
    }
}

fn assert_project_invariants(project: &Project) {
    assert_timeline_invariants(project, project.timeline());
    assert_timeline_invariants(project, project.root_timeline());
    for seq in project.sequences() {
        assert_timeline_invariants(project, &seq.timeline);
        for clip in seq
            .timeline
            .tracks_ordered()
            .flat_map(|t| t.clips_ordered())
        {
            if let Some(child_seq) = clip.sequence() {
                assert!(!project.would_create_sequence_cycle(Some(seq.id), child_seq));
            }
        }
    }
    for bin in project.bin_iter() {
        if let Some(parent) = bin.parent {
            assert!(project.bin(parent).is_some());
        }
        assert!(!project.bin_ancestors(bin.id).any(|a| a == bin.id));
    }
}

fn assert_save_load_idempotent(project: &Project, dir: &std::path::Path) {
    let path_a = dir.join("roundtrip_a.aria");
    let path_b = dir.join("roundtrip_b.aria");
    project.save_to_file(&path_a).expect("first save_to_file");
    let loaded = Project::load_from_file(&path_a).expect("load_from_file");
    assert_project_invariants(&loaded);
    loaded.save_to_file(&path_b).expect("second save_to_file");
    let bytes_a = std::fs::read(&path_a).expect("read first save");
    let bytes_b = std::fs::read(&path_b).expect("read second save");
    assert_eq!(bytes_a, bytes_b, "save -> load -> save bytes drifted");
}

struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo) as u64) as i64
    }
}

#[test]
fn generative_project_mutations_preserve_invariants_and_save_load_idempotence() {
    let dir = tempfile::tempdir().expect("tempdir");
    for seed in 0..8u64 {
        let mut rng = Lcg::new(seed);
        let mut project = Project::new(format!("gen_{seed}"), FPS_24);
        let m24 = project.add_media(sample_media(FPS_24, 600));
        let m30 = project.add_media(sample_media(FPS_30, 600));
        let mut img = sample_media(FPS_24, 120);
        img.is_image = true;
        let m_img = project.add_media(img);

        let v1 = project.add_track(TrackKind::Video, "V1");
        let v2 = project.add_track(TrackKind::Video, "V2");
        let a1 = project.add_track(TrackKind::Audio, "A1");
        let t1 = project.add_track(TrackKind::Text, "T1");
        let bus = project.add_track(TrackKind::Submix, "Bus 1");
        project
            .timeline_mut()
            .set_track_output(a1, TrackOutput::Bus(bus))
            .unwrap();
        project
            .timeline_mut()
            .add_send(v1, bus, 1.0, false)
            .unwrap();

        let bin_root = project.add_bin("Root", None).unwrap();
        let _bin_child = project.add_bin("Child", Some(bin_root)).unwrap();
        let mc = project
            .create_multicam_group(
                "CamGroup",
                vec![MulticamAngle::new(m24, 0), MulticamAngle::new(m30, 12)],
            )
            .unwrap();
        let seq_id = project.create_sequence("SubSeq", FPS_24);
        project.set_active_sequence(Some(seq_id)).unwrap();
        let sv1 = project.add_track(TrackKind::Video, "SV1");
        project.add_clip(sv1, m24, tr(0, 72), rt(0)).unwrap();
        project.set_active_sequence(None).unwrap();

        let c1 = project.add_clip(v1, m24, tr(0, 96), rt(0)).unwrap();
        let c2 = project.add_clip(v1, m24, tr(96, 96), rt(96)).unwrap();
        let ca = project.add_clip(a1, m24, tr(0, 96), rt(0)).unwrap();
        let ct = project
            .add_generated(t1, Generator::text("Title"), tr(0, 48))
            .unwrap();
        project.link_clips(&[c1, ca]).unwrap();
        project
            .set_clip_parent(ct, Some(c1), ClipTransform::IDENTITY.into())
            .unwrap();
        project.add_transition(c1, "crossfade").unwrap();

        assert_project_invariants(&project);
        assert_save_load_idempotent(&project, dir.path());

        for _step in 0..40 {
            let clips: Vec<ClipId> = project
                .timeline()
                .tracks_ordered()
                .flat_map(|t| t.clips_ordered().map(|c| c.id))
                .collect();
            let op = rng.range(0, 14);
            match op {
                0 => {
                    let start = rng.range(-12, 320);
                    let dur = rng.range(0, 120);
                    let src_start = rng.range(-10, 520);
                    let track = if rng.range(0, 2) == 0 { v1 } else { v2 };
                    let media = match rng.range(0, 3) {
                        0 => m24,
                        1 => m30,
                        _ => m_img,
                    };
                    let rate = project.media(media).unwrap().frame_rate;
                    let _ = project.add_clip(track, media, tr_at(src_start, dur, rate), rt(start));
                }
                1 if !clips.is_empty() => {
                    let c = clips[rng.range(0, clips.len() as i64) as usize];
                    let at = rng.range(0, 300);
                    let _ = project.split_clip(c, rt(at));
                }
                2 if !clips.is_empty() => {
                    let c = clips[rng.range(0, clips.len() as i64) as usize];
                    let start = rng.range(-10, 260);
                    let dur = rng.range(0, 140);
                    let _ = project.trim_clip(c, tr(start, dur));
                }
                3 if !clips.is_empty() => {
                    let c = clips[rng.range(0, clips.len() as i64) as usize];
                    let to = match rng.range(0, 4) {
                        0 => v1,
                        1 => v2,
                        2 => a1,
                        _ => t1,
                    };
                    let _ = project.move_clip(c, to, rt(rng.range(-5, 300)));
                }
                4 => {
                    let track = if rng.range(0, 2) == 0 { v1 } else { v2 };
                    let _ =
                        project.shift_clips(track, rt(rng.range(0, 200)), rt(rng.range(-48, 48)));
                }
                5 if !clips.is_empty() => {
                    let c = clips[rng.range(0, clips.len() as i64) as usize];
                    let num = rng.range(1, 5) as i32;
                    let den = rng.range(1, 4) as i32;
                    let _ =
                        project.set_clip_speed(c, Rational::new(num, den), rng.range(0, 2) == 0);
                }
                6 if !clips.is_empty() => {
                    let c = clips[rng.range(0, clips.len() as i64) as usize];
                    let _ = project.add_transition(c, "crossfade");
                    if rng.range(0, 2) == 0 {
                        let align = TransitionAlignment::ALL[rng.range(0, 3) as usize];
                        let _ = project.set_transition_alignment(c, align);
                    }
                    let _ = project.set_transition_duration(c, rng.range(0, 40));
                }
                7 if !clips.is_empty() => {
                    let c = clips[rng.range(0, clips.len() as i64) as usize];
                    let _ = project.slip_clip(c, rng.range(-40, 40));
                }
                8 => {
                    let _ = project.add_sequence_clip(
                        v2,
                        seq_id,
                        tr(0, rng.range(12, 72)),
                        rt(rng.range(0, 280)),
                    );
                }
                9 => {
                    let _ = project.add_multicam_clip(
                        v2,
                        mc,
                        tr(0, rng.range(12, 72)),
                        rt(rng.range(0, 280)),
                    );
                }
                10 if !clips.is_empty() => {
                    let c = clips[rng.range(0, clips.len() as i64) as usize];
                    let _ = project.set_param_keyframe(
                        c,
                        ClipParam::Opacity,
                        rt(rng.range(0, 200)),
                        ParamValue::Scalar(0.75),
                        Easing::Linear,
                        None,
                    );
                }
                11 => {
                    let m = Marker::new(rt(rng.range(0, 120)), "M", MarkerColor::Blue)
                        .with_duration(rt(rng.range(0, 24)));
                    let _ = project.timeline_mut().add_marker(m);
                }
                12 if clips.len() > 2 => {
                    let c = clips[rng.range(0, clips.len() as i64) as usize];
                    let _ = project.ripple_delete(c);
                }
                _ => {
                    let _ = project.set_clip_beats(c2, vec![-5, 0, 12, 24, 24, 9999]);
                }
            }
            assert_project_invariants(&project);
            assert_save_load_idempotent(&project, dir.path());
        }
    }
}

#[test]
fn transition_clamps_on_short_clips_and_prunes_when_shrunk_below_junction_limit() {
    let mut project = Project::new("trans_invariant", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 500));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let left = project.add_clip(v1, media_id, tr(0, 10), rt(0)).unwrap();
    let right = project.add_clip(v1, media_id, tr(10, 48), rt(10)).unwrap();

    // Adding a transition on a 10-tick clip clamps its default duration (24)
    // to junction_limit(10, 48) == 10 so the transition is immediately valid.
    project.add_transition(left, "crossfade").unwrap();
    assert_eq!(project.transition_at(left).unwrap().duration, 10);
    project
        .set_transition_alignment(left, TransitionAlignment::Center)
        .unwrap();
    assert_project_invariants(&project);

    // Trimming `right`'s tail to 6 ticks keeps `left` and `right` abutting at
    // tick 10, but reduces `junction_limit(10, 6)` to 6 < 10, which prunes the
    // now-overlong transition and invalidates the track revision.
    let rev_before = project.timeline().track(v1).unwrap().revision();
    project.trim_clip(right, tr(10, 6)).unwrap();
    let rev_after = project.timeline().track(v1).unwrap().revision();
    assert_ne!(rev_before, rev_after);
    assert!(project.transition_at(left).is_none());
    assert_project_invariants(&project);
}

#[test]
fn load_normalizes_secondary_sequence_lanes_and_preserves_save_load_idempotence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = Project::new("multi_seq", FPS_24);
    let _v_root = project.add_track(TrackKind::Video, "V_Root");
    let _a_root = project.add_track(TrackKind::Audio, "A_Root");

    let seq_id = project.create_sequence("Nested", FPS_24);
    project.set_active_sequence(Some(seq_id)).unwrap();
    let _v_sub = project.add_track(TrackKind::Video, "V_Sub");
    let _a_sub = project.add_track(TrackKind::Audio, "A_Sub");

    let mut raw: serde_json::Value =
        serde_json::from_str(&project.save_to_string().unwrap()).unwrap();
    // Scramble the secondary sequence's track order and clear its `main` flag
    // to simulate an unnormalized secondary sequence on disk while root is active.
    raw["active_sequence"] = serde_json::Value::Null;
    if let Some(seq_arr) = raw["sequences"].as_array_mut()
        && let Some(seq_entry) = seq_arr.first_mut()
    {
        let tl = &mut seq_entry[1]["timeline"];
        if let Some(order) = tl["order"].as_array_mut() {
            order.reverse();
        }
        if let Some(tracks) = tl["tracks"].as_array_mut() {
            for pair in tracks {
                pair[1]["main"] = serde_json::Value::Bool(false);
            }
        }
    }
    let loaded = Project::load_from_string(&serde_json::to_string(&raw).unwrap()).unwrap();
    assert_project_invariants(&loaded);
    assert_save_load_idempotent(&loaded, dir.path());
}

#[test]
fn model_boundary_overflows_fail_closed_without_partial_mutation() {
    let mut project = Project::new("bounds", FPS_24);
    let media_id = project.add_media(sample_media(FPS_24, 500));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let c1 = project.add_clip(v1, media_id, tr(0, 48), rt(10)).unwrap();
    let c2 = project
        .add_clip(v1, media_id, tr(48, 48), rt(i64::MAX - 60))
        .unwrap();
    let before_json = project.save_to_string().unwrap();

    // 1. Multi-clip `shift_clips` where clip 1 fits (`10 + 20`) but clip 2
    // overflows `start + duration` (`(i64::MAX - 60) + 20 + 48 > i64::MAX`)
    // must fail atomically without moving clip 1.
    assert_eq!(
        project.shift_clips(v1, rt(0), rt(20)),
        Err(ModelError::TimeOverflow)
    );
    assert_eq!(project.clip(c1).unwrap().timeline.start.value, 10);
    assert_eq!(project.save_to_string().unwrap(), before_json);

    // 2. `Track::has_overlap` / `Timeline::add_clip` with overflowing range.
    assert_eq!(
        project
            .timeline()
            .track(v1)
            .unwrap()
            .has_overlap(tr(10, i64::MAX), None),
        Err(ModelError::TimeOverflow)
    );

    // 3. `Timeline::add_marker` and `Clip::add_marker` with overflowing duration.
    let bad_seq_marker = Marker::new(rt(10), "M", MarkerColor::Red).with_duration(rt(i64::MAX));
    assert_eq!(
        project.timeline_mut().add_marker(bad_seq_marker),
        Err(ModelError::TimeOverflow)
    );
    let bad_clip_marker = Marker::new(rt(10), "CM", MarkerColor::Blue).with_duration(rt(i64::MAX));
    assert_eq!(
        project.timeline_mut().add_clip_marker(c1, bad_clip_marker),
        Err(ModelError::TimeOverflow)
    );

    // 4. `Project::multicam_group_span` with `sync_offset == i64::MAX`.
    let mc = project
        .create_multicam_group("OverflowCam", vec![MulticamAngle::new(media_id, i64::MAX)])
        .unwrap();
    assert_eq!(
        project.multicam_group_span(mc),
        Err(ModelError::TimeOverflow)
    );

    // 5. `Clip::animation_tick` and `scale_by_speed` at `i64::MIN` / `i64::MAX`.
    let clip_ref = project.clip(c2).unwrap();
    assert_eq!(clip_ref.animation_tick(i64::MIN), 0);
    assert_eq!(clip_ref.scale_by_speed(i64::MAX), i64::MAX);

    // 6. `Project::remove_multicam_group` refuses deletion when a clip inside a
    // secondary sequence references the group while the root sequence is active.
    let sub_seq = project.create_sequence("SubMulticam", FPS_24);
    project.set_active_sequence(Some(sub_seq)).unwrap();
    let sub_v1 = project.add_track(TrackKind::Video, "SV1");
    project
        .add_multicam_clip(sub_v1, mc, tr(0, 24), rt(0))
        .unwrap();
    project.set_active_sequence(None).unwrap();
    assert!(project.remove_multicam_group(mc).is_err());
    assert_project_invariants(&project);
}
