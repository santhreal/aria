//! Audio effects registered while the process runs, and insert chains that
//! hold them.
//!
//! Its own test binary: the audio effect catalog is process-wide. Every test
//! registers ids no other test uses, so the tests stay independent while they
//! share it.
//!
//! Covers registration, removal and every refusal; parameter writes, defaults,
//! ranges and sampling on a registered spec; and the insert chain edits that
//! need one (adding, sidechain keys). Processing belongs to `aria-render`.

use aria_models::{
    AudioEffectInstance, AudioEffectSpec, AudioInsertTarget, Easing, EffectParamKind,
    EffectParamSpec, Keyframe, MAX_AUDIO_LATENCY, ModelError, Param, Project, Rational,
    STRIP_INSERTS, TrackKind, audio_effect_catalog, audio_effect_spec, register_audio_effect,
};

const R24: Rational = Rational::FPS_24;

fn scalar(name: &str, default: f32, min: f32, max: f32) -> EffectParamSpec {
    EffectParamSpec {
        name: name.into(),
        label: name.into(),
        kind: EffectParamKind::Scalar,
        default,
        min,
        max,
        default_color: [0; 4],
        default_vec2: [0.0; 2],
    }
}

fn compressor(id: &str, sidechain: bool) -> AudioEffectSpec {
    AudioEffectSpec {
        id: id.into(),
        label: format!("{id} label"),
        params: vec![
            scalar("threshold", -12.0, -60.0, 0.0),
            scalar("ratio", 4.0, 1.0, 20.0),
        ],
        latency: 64,
        sidechain,
    }
}

fn ids() -> Vec<String> {
    audio_effect_catalog()
        .iter()
        .map(|spec| spec.id.clone())
        .collect()
}

#[test]
fn a_registered_audio_effect_is_listed_and_resolves_until_its_guard_drops() {
    let before = audio_effect_catalog().revision();
    let (guard, held) = register_audio_effect(compressor("test.listed", false)).unwrap();
    assert_eq!(held.latency, 64);
    assert!(ids().contains(&"test.listed".to_owned()));
    assert_eq!(audio_effect_spec("test.listed").as_deref(), Some(&*held));
    let during = audio_effect_catalog().revision();
    assert!(during > before);

    drop(guard);
    assert!(!ids().contains(&"test.listed".to_owned()));
    assert!(audio_effect_spec("test.listed").is_none());
    assert!(audio_effect_catalog().revision() > during);
    // A spec held across the removal stays whole.
    assert_eq!(held.params.len(), 2);
}

/// Other tests in this binary register while this one runs, so each refusal is
/// checked by the refused id alone, not by the whole catalog.
#[test]
fn every_refusal_rule_rejects_and_lists_nothing() {
    let (_held, held) = register_audio_effect(compressor("test.taken", false)).unwrap();
    let mut colour = scalar("tint", 0.0, 0.0, 1.0);
    colour.kind = EffectParamKind::Color;
    let refusals = [
        ("no namespace", compressor("bare", false)),
        (
            "an id already listed",
            AudioEffectSpec {
                latency: 0,
                ..compressor("test.taken", false)
            },
        ),
        (
            "an empty label",
            AudioEffectSpec {
                label: String::new(),
                ..compressor("test.unlabelled", false)
            },
        ),
        (
            "a colour parameter",
            AudioEffectSpec {
                params: vec![colour],
                ..compressor("test.colour", false)
            },
        ),
        (
            "a default outside its range",
            AudioEffectSpec {
                params: vec![scalar("gain", 2.0, 0.0, 1.0)],
                ..compressor("test.default", false)
            },
        ),
        (
            "latency past one second",
            AudioEffectSpec {
                latency: MAX_AUDIO_LATENCY + 1,
                ..compressor("test.slow", false)
            },
        ),
    ];
    for (case, spec) in refusals {
        let id = spec.id.clone();
        assert!(register_audio_effect(spec).is_err(), "{case} was accepted");
        if id == "test.taken" {
            assert_eq!(audio_effect_spec(&id), Some(held.clone()), "{case}");
        } else {
            assert!(audio_effect_spec(&id).is_none(), "{case} was listed");
        }
        assert_eq!(
            ids().iter().filter(|listed| **listed == id).count(),
            usize::from(id == "test.taken"),
            "{case}"
        );
    }
}

#[test]
fn a_parameter_write_is_ranged_and_a_default_is_not_stored() {
    let (_guard, _) = register_audio_effect(compressor("test.params", false)).unwrap();
    let mut insert = AudioEffectInstance::new("test.params");
    assert_eq!(insert.sample_param("threshold", 0.0), Some(-12.0));
    assert_eq!(insert.sample_param("missing", 0.0), None);

    insert.set_param(1, Param::Constant(8.0)).unwrap();
    assert_eq!(insert.sample_param("ratio", 0.0), Some(8.0));
    assert!(matches!(
        insert.set_param(1, Param::Constant(30.0)),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(matches!(
        insert.set_param(1, Param::Constant(f32::NAN)),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(matches!(
        insert.set_param(2, Param::Constant(1.0)),
        Err(ModelError::InvalidParam(_))
    ));
    // A keyframe outside the range is refused like a constant.
    let reaching = Param::Keyframed {
        keyframes: vec![
            Keyframe::new(0, -24.0, Easing::Linear),
            Keyframe::new(24, 6.0, Easing::Linear),
        ],
    };
    assert!(insert.set_param(0, reaching).is_err());
    assert_eq!(insert.sample_param("ratio", 0.0), Some(8.0));

    let ramp = Param::Keyframed {
        keyframes: vec![
            Keyframe::new(0, -24.0, Easing::Linear),
            Keyframe::new(24, -12.0, Easing::Linear),
        ],
    };
    insert.set_param(0, ramp).unwrap();
    assert_eq!(insert.sample_param("threshold", 12.0), Some(-18.0));

    // Writing the default back clears the entry, so the save shrinks again.
    insert.set_param(1, Param::Constant(4.0)).unwrap();
    assert!(!insert.params.contains_key("ratio"));
    assert_eq!(insert.sample_param("ratio", 0.0), Some(4.0));
}

/// A project with an audio lane holding a clip of `score`, a second audio lane
/// to key from, and a bus.
fn stage() -> (
    Project,
    aria_models::TrackId,
    aria_models::TrackId,
    aria_models::ClipId,
) {
    let mut project = Project::new("inserts", R24);
    let music = project.add_track(TrackKind::Audio, "Music");
    let voice = project.add_track(TrackKind::Audio, "Voice");
    let media = project.add_media(aria_models::MediaSource::new(
        "/media/score.wav",
        0,
        0,
        R24,
        480,
        true,
    ));
    let clip = project
        .add_clip(
            music,
            media,
            aria_models::TimeRange::at_rate(0, 96, R24),
            aria_models::RationalTime::new(0, R24),
        )
        .unwrap();
    (project, music, voice, clip)
}

#[test]
fn an_effect_is_added_to_every_kind_of_chain_until_a_strip_is_full() {
    let (_guard, _) = register_audio_effect(compressor("test.add", false)).unwrap();
    let (mut project, music, _, clip) = stage();
    for target in [
        AudioInsertTarget::Clip(clip),
        AudioInsertTarget::Track(music),
        AudioInsertTarget::Master,
    ] {
        for expected in 0..STRIP_INSERTS {
            assert_eq!(project.add_audio_effect(target, "test.add"), Ok(expected));
        }
        let more = project.add_audio_effect(target, "test.add");
        match target.limit() {
            Some(limit) => assert_eq!(more, Err(ModelError::InsertsFull(limit))),
            None => assert_eq!(more, Ok(STRIP_INSERTS), "a clip chain has no limit"),
        }
    }
}

#[test]
fn a_key_is_set_only_on_an_effect_that_reads_one_and_from_a_lane_that_may() {
    let (_plain, _) = register_audio_effect(compressor("test.plain", false)).unwrap();
    let (_keyed, _) = register_audio_effect(compressor("test.keyed", true)).unwrap();
    let (mut project, music, voice, clip) = stage();
    let lane = AudioInsertTarget::Track(music);
    project.add_audio_effect(lane, "test.plain").unwrap();
    project.add_audio_effect(lane, "test.keyed").unwrap();

    assert!(matches!(
        project.set_audio_effect_sidechain(lane, 0, Some(voice)),
        Err(ModelError::InvalidParam(_))
    ));
    assert_eq!(
        project.set_audio_effect_sidechain(lane, 1, Some(music)),
        Err(ModelError::RoutingCycle(music))
    );
    project
        .set_audio_effect_sidechain(lane, 1, Some(voice))
        .unwrap();
    assert_eq!(
        project.audio_inserts(lane).unwrap()[1].sidechain,
        Some(voice)
    );
    // With the voice keying the music, the voice cannot key back from it.
    project
        .add_audio_effect(AudioInsertTarget::Track(voice), "test.keyed")
        .unwrap();
    assert_eq!(
        project.set_audio_effect_sidechain(AudioInsertTarget::Track(voice), 0, Some(music)),
        Err(ModelError::RoutingCycle(music))
    );

    let on_clip = AudioInsertTarget::Clip(clip);
    project.add_audio_effect(on_clip, "test.keyed").unwrap();
    assert!(matches!(
        project.set_audio_effect_sidechain(on_clip, 0, Some(voice)),
        Err(ModelError::InvalidParam(_))
    ));

    project
        .add_audio_effect(AudioInsertTarget::Master, "test.keyed")
        .unwrap();
    project
        .set_audio_effect_sidechain(AudioInsertTarget::Master, 0, Some(voice))
        .unwrap();
    project
        .set_audio_effect_sidechain(AudioInsertTarget::Master, 0, None)
        .unwrap();
    assert_eq!(
        project.audio_inserts(AudioInsertTarget::Master).unwrap()[0].sidechain,
        None
    );
}
