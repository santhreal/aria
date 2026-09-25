//! `duplicate_clip` cases.

use super::*;
use crate::wire;

#[test]
fn duplicate_clip_rejects_unknown_ids_and_invalid_starts() {
    let (project, _, video, _, clip, _) = fixture();

    let msg = reject(
        &project,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip: 999,
            to_track: video,
            start: 12.0,
        }),
    );
    assert!(msg.contains("clip 999 does not exist"), "{msg}");

    let msg = reject(
        &project,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip,
            to_track: 999,
            start: 12.0,
        }),
    );
    assert!(msg.contains("track 999 does not exist"), "{msg}");

    for start in [f64::NAN, f64::INFINITY] {
        let msg = reject(
            &project,
            WireCommand::DuplicateClip(wire::DuplicateClip {
                clip,
                to_track: video,
                start,
            }),
        );
        assert!(msg.contains("start must be a finite number"), "{msg}");
    }

    let msg = reject(
        &project,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip,
            to_track: video,
            start: -0.5,
        }),
    );
    assert!(msg.contains("start must not be negative"), "{msg}");
}

#[test]
fn duplicate_clip_rejects_incompatible_locked_and_overlapping_destinations() {
    let (mut project, media, _, text, clip, _) = fixture();

    let msg = reject(
        &project,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip,
            to_track: text,
            start: 12.0,
        }),
    );
    assert!(msg.contains("cannot be duplicated"), "{msg}");
    assert!(msg.contains("text lane"), "{msg}");

    let destination = project.add_track(TrackKind::Video, "V2");
    project
        .timeline_mut()
        .track_mut(destination)
        .unwrap()
        .locked = true;
    let msg = reject(
        &project,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip,
            to_track: destination.raw(),
            start: 12.0,
        }),
    );
    assert!(msg.contains("destination track"), "{msg}");
    assert!(msg.contains("locked"), "{msg}");
    assert!(msg.contains("unlock it or choose another"), "{msg}");

    project
        .timeline_mut()
        .track_mut(destination)
        .unwrap()
        .locked = false;
    project
        .add_clip(
            destination,
            MediaId::from_raw(media),
            TimeRange::at_rate(0, 24, R24),
            RationalTime::new(480, R24),
        )
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip,
            to_track: destination.raw(),
            start: 12.0,
        }),
    );
    assert!(
        msg.contains("exact destination range 12.000s to 22.000s"),
        "{msg}"
    );
    assert!(msg.contains("10.000s of free space"), "{msg}");
    assert!(msg.contains("does not ripple or search for space"), "{msg}");
}

#[test]
fn extract_audio_rejections_tell_the_model_how_to_recover() {
    let (mut project, _media, _video, _text, video_clip, title) = fixture();
    let audio = project.add_track(TrackKind::Audio, "A1");
    let link = aria_models::LinkId::next();
    for raw in [video_clip, title] {
        project
            .timeline_mut()
            .clip_mut(ClipId::from_raw(raw))
            .unwrap()
            .link = Some(link);
    }
    let msg = reject(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip,
            track: audio.raw(),
        }),
    );
    assert!(msg.contains("unlink_clips first"), "{msg}");
    assert!(msg.contains(&title.to_string()), "{msg}");

    let (mut project, media, _video, _text, video_clip, _) = fixture();
    let audio = project.add_track(TrackKind::Audio, "A1");
    let companion = project
        .add_clip(
            audio,
            aria_models::MediaId::from_raw(media),
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let link = aria_models::LinkId::next();
    for id in [ClipId::from_raw(video_clip), companion] {
        project.timeline_mut().clip_mut(id).unwrap().link = Some(link);
    }
    let msg = reject(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip,
            track: audio.raw(),
        }),
    );
    assert!(msg.contains("already has extracted audio"), "{msg}");

    let (mut project, media, _video, _text, video_clip, _) = fixture();
    project
        .media_mut(aria_models::MediaId::from_raw(media))
        .unwrap()
        .is_image = true;
    let audio = project.add_track(TrackKind::Audio, "A1");
    let msg = reject(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip,
            track: audio.raw(),
        }),
    );
    assert!(msg.contains("video media"), "{msg}");

    let (mut project, media, _video, _text, video_clip, _) = fixture();
    project
        .media_mut(aria_models::MediaId::from_raw(media))
        .unwrap()
        .has_audio = false;
    let audio = project.add_track(TrackKind::Audio, "A1");
    let msg = reject(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip,
            track: audio.raw(),
        }),
    );
    assert!(msg.contains("no audio stream"), "{msg}");
}

#[test]
fn set_denoise_lowers_steers_and_rejects_generated() {
    let (mut project, media, _, _, video_clip, title) = fixture();
    // An audio lane carrying the linked companion of the video clip.
    let lane = project.add_track(TrackKind::Audio, "A1");
    let audio_clip = project
        .add_clip(
            lane,
            aria_models::MediaId::from_raw(media),
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let link = aria_models::LinkId::next();
    for id in [ClipId::from_raw(video_clip), audio_clip] {
        project.timeline_mut().clip_mut(id).unwrap().link = Some(link);
    }

    // An audio-lane target lowers straight through.
    let edit = lower(
        &project,
        WireCommand::SetDenoise(wire::SetDenoise {
            clip: audio_clip.raw(),
            denoise: true,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipDenoise {
            clip: audio_clip,
            denoise: true,
        }
    );

    // A video-lane target is steered to its linked audio companion.
    let msg = reject(
        &project,
        WireCommand::SetDenoise(wire::SetDenoise {
            clip: video_clip,
            denoise: true,
        }),
    );
    assert!(
        msg.contains(&format!("linked clip {}", audio_clip.raw())),
        "{msg}"
    );

    // A generated clip has no footage to clean.
    let msg = reject(
        &project,
        WireCommand::SetDenoise(wire::SetDenoise {
            clip: title,
            denoise: true,
        }),
    );
    assert!(msg.contains("generated clip"), "{msg}");
}

#[test]
fn clip_audio_lowers_volume_and_fades() {
    let (mut project, media, _, _, video_clip, title) = fixture();
    // An audio lane carrying the linked companion of the video clip.
    let lane = project.add_track(TrackKind::Audio, "A1");
    let audio_clip = project
        .add_clip(
            lane,
            aria_models::MediaId::from_raw(media),
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let link = aria_models::LinkId::next();
    for id in [ClipId::from_raw(video_clip), audio_clip] {
        project.timeline_mut().clip_mut(id).unwrap().link = Some(link);
    }

    // Volume + fades lower to ticks at the timeline rate (1s = 24).
    let edit = lower(
        &project,
        WireCommand::SetClipAudio(wire::SetClipAudio {
            clip: audio_clip.raw(),
            volume: Some(0.5),
            fade_in: Some(1.0),
            fade_out: Some(0.5),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipAudio {
            clip: audio_clip,
            volume: Some(0.5),
            fade_in: RationalTime::new(24, R24),
            fade_out: RationalTime::new(12, R24),
        }
    );

    // Omitted fields keep the clip's current mix.
    let edit = lower(
        &project,
        WireCommand::SetClipAudio(wire::SetClipAudio {
            clip: audio_clip.raw(),
            volume: Some(0.0),
            fade_in: None,
            fade_out: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipAudio {
            clip: audio_clip,
            volume: Some(0.0),
            fade_in: RationalTime::new(0, R24),
            fade_out: RationalTime::new(0, R24),
        }
    );

    // Omitting volume lowers to `None`, so the clip's gain (a flat level
    // or an M8 envelope) is preserved — only the fades change.
    let edit = lower(
        &project,
        WireCommand::SetClipAudio(wire::SetClipAudio {
            clip: audio_clip.raw(),
            volume: None,
            fade_in: Some(0.5),
            fade_out: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipAudio {
            clip: audio_clip,
            volume: None,
            fade_in: RationalTime::new(12, R24),
            fade_out: RationalTime::new(0, R24),
        }
    );

    // A video-lane target is steered to its linked audio companion.
    let msg = reject(
        &project,
        WireCommand::SetClipAudio(wire::SetClipAudio {
            clip: video_clip,
            volume: Some(0.5),
            fade_in: None,
            fade_out: None,
        }),
    );
    assert!(
        msg.contains(&format!("linked clip {}", audio_clip.raw())),
        "{msg}"
    );

    // Out-of-range volume, over-long fades, generated clips: rejected.
    let msg = reject(
        &project,
        WireCommand::SetClipAudio(wire::SetClipAudio {
            clip: audio_clip.raw(),
            volume: Some(11.0),
            fade_in: None,
            fade_out: None,
        }),
    );
    assert!(msg.contains("between 0 (mute) and 10"), "{msg}");
    let msg = reject(
        &project,
        WireCommand::SetClipAudio(wire::SetClipAudio {
            clip: audio_clip.raw(),
            volume: None,
            fade_in: Some(60.0),
            fade_out: None,
        }),
    );
    assert!(msg.contains("longer than clip"), "{msg}");
    let msg = reject(
        &project,
        WireCommand::SetClipAudio(wire::SetClipAudio {
            clip: title,
            volume: Some(0.5),
            fade_in: None,
            fade_out: None,
        }),
    );
    assert!(msg.contains("generated clip"), "{msg}");
}

#[test]
fn audio_edits_on_a_video_clip_with_sound_target_the_clip_itself() {
    // CapCut keeps a video's audio on the clip — a drop lands a single
    // clip, not a linked audio companion — so volume/fades and denoise on
    // the video clip adjust it directly, with no steering.
    let (project, _, _, _, video_clip, _) = fixture();

    let edit = lower(
        &project,
        WireCommand::SetClipAudio(wire::SetClipAudio {
            clip: video_clip,
            volume: Some(0.5),
            fade_in: None,
            fade_out: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipAudio {
            clip: ClipId::from_raw(video_clip),
            volume: Some(0.5),
            fade_in: RationalTime::new(0, R24),
            fade_out: RationalTime::new(0, R24),
        }
    );

    let edit = lower(
        &project,
        WireCommand::SetDenoise(wire::SetDenoise {
            clip: video_clip,
            denoise: true,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipDenoise {
            clip: ClipId::from_raw(video_clip),
            denoise: true,
        }
    );
}

#[test]
fn split_outside_clip_names_its_extent() {
    let (project, _, _, _, clip, _) = fixture();
    let msg = reject(
        &project,
        WireCommand::SplitClip(wire::SplitClip { clip, at: 10.0 }),
    );
    assert!(msg.contains("not strictly inside clip"), "{msg}");
    assert!(msg.contains("0.000s"), "{msg}");
    assert!(msg.contains("10.000s"), "{msg}");
}
