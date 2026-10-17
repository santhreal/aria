//! Every lane kind answers the lane guards, and the catalog is what answers.
//!
//! WHY: the guards that decide "this gesture needs a frame" and "this gesture
//! needs sound" each hand-wrote `kind == TrackKind::Audio`. Four copies of one
//! predicate, none of which knew about a lane kind added after it was written —
//! `Submix` landed with five such comparisons already in the tree, and nothing
//! went red. The columns `is_audio` and `supports_transitions` are the one
//! owner of those questions, so these cases derive both sides from
//! [`aria_models::track_kind_specs`] at run time: a new lane row is covered by
//! existing, and a guard that answers it differently from its own row fails
//! here.
//!
//! What this does not catch: a guard that reads the right column and then
//! writes the wrong refusal *sentence*. The assertions match on the phrase, so
//! a reworded refusal is a failure here too, which is the trade — the phrase is
//! what an agent reads back, so it is worth pinning.

use super::*;
use crate::wire;
use aria_models::{MediaSource, generator_specs, track_kind_spec, track_kind_specs};

/// A project holding one lane of `kind` with one clip on it, or `None` when the
/// kind takes no clip at all (a bus).
///
/// Media lanes get a media clip; every other lane gets the census value of the
/// one generator whose row names it, so the fixture needs no per-kind arm.
fn lane_with_clip(kind: TrackKind) -> Option<(Project, u64)> {
    let mut project = Project::new("lane census", R24);
    let lane = project.add_track(kind, "L1");
    let span = TimeRange::at_rate(0, 48, R24);
    if track_kind_spec(kind).accepts_media {
        let media = project.add_media(MediaSource::new(
            "/tmp/aria-lane-census.mp4",
            1920,
            1080,
            R24,
            240,
            true,
        ));
        let clip = project
            .add_clip(lane, media, span, RationalTime::new(0, R24))
            .expect("a media lane takes a media clip");
        return Some((project, clip.raw()));
    }
    let generator = generator_specs().iter().find(|spec| spec.lane == kind)?;
    let clip = project
        .add_generated(lane, (generator.census)(), span)
        .expect("a generator's own lane takes it");
    Some((project, clip.raw()))
}

/// The refusal message for `cmd`, or `None` when it validates.
fn verdict(project: &Project, cmd: WireCommand) -> Option<String> {
    validate(&cmd, project).err().map(|r| r.message)
}

#[test]
fn every_lane_answers_the_audio_guards() {
    for spec in track_kind_specs() {
        let Some((project, clip)) = lane_with_clip(spec.kind) else {
            assert!(
                spec.is_bus,
                "lane '{}' holds no clip and is not a bus; give it a fixture",
                spec.id
            );
            continue;
        };

        let picture = verdict(
            &project,
            WireCommand::AddClipMask(wire::AddClipMask {
                clip,
                mask: wire_mask("circle"),
            }),
        );
        let refused_for_sound = picture
            .as_deref()
            .is_some_and(|m| m.contains("is on an audio lane"));
        assert_eq!(
            refused_for_sound, spec.is_audio,
            "lane '{}': the picture guard disagrees with its is_audio column ({picture:?})",
            spec.id
        );

        let sound = verdict(
            &project,
            WireCommand::SetAudioRole(wire::SetAudioRole {
                clip,
                role: Some(wire::WireAudioRole::Music),
            }),
        );
        let refused_for_picture = sound
            .as_deref()
            .is_some_and(|m| m.contains("is not on an audio lane"));
        assert_eq!(
            refused_for_picture, !spec.is_audio,
            "lane '{}': the sound guard disagrees with its is_audio column ({sound:?})",
            spec.id
        );
    }
}

#[test]
fn every_lane_answers_the_crop_guard() {
    for spec in track_kind_specs() {
        let Some((project, clip)) = lane_with_clip(spec.kind) else {
            continue;
        };
        let msg = verdict(
            &project,
            WireCommand::SetClipCrop(wire::SetClipCrop {
                clip,
                left: Some(0.1),
                right: None,
                top: None,
                bottom: None,
                flip_h: None,
                flip_v: None,
            }),
        );
        let refused_for_sound = msg
            .as_deref()
            .is_some_and(|m| m.contains("is on an audio lane"));
        assert_eq!(
            refused_for_sound, spec.is_audio,
            "lane '{}': the crop guard disagrees with its is_audio column ({msg:?})",
            spec.id
        );
    }
}

/// Every lane takes the transitions its catalog row names: a lane that
/// composites frames takes a picture blend, a sound lane that holds clips an
/// audio crossfade, and any other lane neither. A lone clip has no cut, so a
/// transition the lane takes is refused only for the missing neighbour.
#[test]
fn every_lane_takes_the_transitions_its_row_names() {
    let picture = ["crossfade".to_string()];
    let audio = aria_models::AudioTransition::ALL.map(|t| t.id().to_string());
    for spec in track_kind_specs() {
        let Some((project, clip)) = lane_with_clip(spec.kind) else {
            continue;
        };
        let takes_picture = spec.supports_transitions;
        let takes_audio = spec.is_audio && !spec.is_bus;
        for (ids, takes) in [(&picture[..], takes_picture), (&audio[..], takes_audio)] {
            for id in ids {
                let msg = verdict(
                    &project,
                    WireCommand::AddTransition(wire::AddTransition {
                        clip,
                        transition: id.clone(),
                    }),
                )
                .expect("a lone clip has no cut to transition across");
                let refused_for_lane = !msg.contains("has no clip butting against its right edge");
                assert_eq!(
                    refused_for_lane, !takes,
                    "lane '{}' and transition '{id}': the refusal disagrees with the row: {msg}",
                    spec.id
                );
            }
        }
    }
}

/// A lane that takes no transition says which lanes take each kind, from the
/// same rows.
#[test]
fn the_transition_refusal_names_the_lanes_that_take_one() {
    let none = track_kind_specs()
        .iter()
        .find(|spec| {
            !spec.supports_transitions && !spec.is_audio && lane_with_clip(spec.kind).is_some()
        })
        .expect("a lane that takes no transition ships");
    let (project, clip) = lane_with_clip(none.kind).expect("checked above");
    let msg = verdict(
        &project,
        WireCommand::AddTransition(wire::AddTransition {
            clip,
            transition: "crossfade".to_string(),
        }),
    )
    .expect("a transition on a canvas-pass lane is refused");
    for spec in track_kind_specs() {
        let takes = spec.supports_transitions || (spec.is_audio && !spec.is_bus);
        assert_eq!(
            msg.contains(spec.id),
            takes,
            "the refusal and the '{}' row disagree: {msg}",
            spec.id
        );
    }
}

/// A transition of the other kind is refused with what the lane does take.
#[test]
fn a_transition_of_the_other_kind_lists_what_the_lane_takes() {
    let (sound, clip) = lane_with_clip(TrackKind::Audio).expect("a sound lane takes a clip");
    let msg = verdict(
        &sound,
        WireCommand::AddTransition(wire::AddTransition {
            clip,
            transition: "crossfade".to_string(),
        }),
    )
    .expect("a picture blend on a sound lane is refused");
    assert_eq!(
        msg,
        format!(
            "'crossfade' is a picture transition and clip {clip} is on an audio lane; \
             available transitions: constant_power, constant_gain, exponential_fade"
        )
    );
    let (frames, clip) = lane_with_clip(TrackKind::Video).expect("a video lane takes a clip");
    let msg = verdict(
        &frames,
        WireCommand::AddTransition(wire::AddTransition {
            clip,
            transition: "constant_gain".to_string(),
        }),
    )
    .expect("an audio crossfade on a video lane is refused");
    assert!(
        msg.starts_with(&format!(
            "'constant_gain' is an audio transition and clip {clip} is on a Video lane; \
             available transitions: crossfade, "
        )),
        "{msg}"
    );
}
