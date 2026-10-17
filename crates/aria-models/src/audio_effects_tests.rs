//! Audio effect specs, instances and insert chains.
//!
//! The library tests never register: the catalogs are process-wide and other
//! tests read them while they run. What needs a spec uses a shipped effect, or
//! registers one in `tests/registered_audio_effects.rs`. Here: the registration
//! rules as a pure function, the shipped catalog's override rule, the
//! serialized shape, what an unregistered instance does, and every refusal an
//! insert chain edit makes before it reaches the catalog.

use super::*;
use crate::effects::EffectParamKind;
use crate::param::{Easing, Keyframe};
use crate::time::{Rational, TimeRange};
use crate::track::TrackKind;
use crate::{Generator, MediaSource, Project, RationalTime};

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
        ..Default::default()
    }
}

fn spec(params: Vec<EffectParamSpec>, latency: u32) -> AudioEffectSpec {
    AudioEffectSpec {
        id: "example.comp".into(),
        category: String::new(),
        label: "Comp".into(),
        params,
        latency,
        sidechain: false,
    }
}

#[test]
fn the_registration_rules_accept_a_sound_spec_and_name_each_refusal() {
    let sound = spec(
        vec![
            scalar("threshold", -12.0, -60.0, 0.0),
            scalar("ratio", 4.0, 1.0, 20.0),
        ],
        MAX_AUDIO_LATENCY,
    );
    assert_eq!(check_audio_effect(&sound), Ok(()));

    let mut colour = scalar("tint", 0.0, 0.0, 1.0);
    colour.kind = EffectParamKind::Color;
    let mut vec2 = scalar("xy", 0.0, 0.0, 1.0);
    vec2.kind = EffectParamKind::Vec2;
    for (case, refused, says) in [
        (
            "an unnamed parameter",
            spec(vec![scalar("", 0.0, 0.0, 1.0)], 0),
            "parameter 0 has no name",
        ),
        (
            "a repeated parameter",
            spec(
                vec![scalar("a", 0.0, 0.0, 1.0), scalar("a", 0.0, 0.0, 1.0)],
                0,
            ),
            "repeats parameter a",
        ),
        (
            "an inverted range",
            spec(vec![scalar("a", 0.0, 1.0, 0.0)], 0),
            "inverted range",
        ),
        ("a colour parameter", spec(vec![colour], 0), "scalar"),
        ("a vector parameter", spec(vec![vec2], 0), "scalar"),
        (
            "a default above the range",
            spec(vec![scalar("a", 2.0, 0.0, 1.0)], 0),
            "outside its range",
        ),
        (
            "a default below the range",
            spec(vec![scalar("a", -1.0, 0.0, 1.0)], 0),
            "outside its range",
        ),
        (
            "a default that is not a number",
            spec(vec![scalar("a", f32::NAN, 0.0, 1.0)], 0),
            "outside its range",
        ),
        (
            "latency past one second",
            spec(Vec::new(), MAX_AUDIO_LATENCY + 1),
            "frames of latency",
        ),
    ] {
        let err = check_audio_effect(&refused).expect_err(case);
        assert!(err.contains(says), "{case}: {err}");
    }
}

#[test]
fn a_lane_and_the_master_hold_five_inserts_and_a_clip_holds_any_number() {
    let clip = AudioInsertTarget::Clip(crate::ClipId::from_raw(7));
    let lane = AudioInsertTarget::Track(crate::TrackId::from_raw(3));
    assert_eq!(clip.limit(), None);
    assert_eq!(lane.limit(), Some(STRIP_INSERTS));
    assert_eq!(AudioInsertTarget::Master.limit(), Some(STRIP_INSERTS));
    assert_eq!(clip.clip(), Some(crate::ClipId::from_raw(7)));
    assert_eq!(clip.track(), None);
    assert_eq!(lane.track(), Some(crate::TrackId::from_raw(3)));
    assert_eq!(AudioInsertTarget::Master.clip(), None);
}

#[test]
fn a_fresh_insert_saves_as_its_id_and_every_field_round_trips() {
    let fresh = AudioEffectInstance::new("example.comp");
    assert_eq!(
        serde_json::to_string(&fresh).unwrap(),
        r#"{"effect_id":"example.comp"}"#
    );

    let mut dressed = AudioEffectInstance::new("example.comp");
    dressed.params.insert(
        "threshold".into(),
        Param::Keyframed {
            keyframes: vec![
                Keyframe::new(0, -20.0, Easing::Linear),
                Keyframe::new(24, -6.0, Easing::Linear),
            ],
        },
    );
    dressed.bypass = true;
    dressed.sidechain = Some(crate::TrackId::from_raw(9));
    let json = serde_json::to_string(&dressed).unwrap();
    let back: AudioEffectInstance = serde_json::from_str(&json).unwrap();
    assert_eq!(back, dressed);
}

#[test]
fn an_unregistered_insert_keeps_its_values_and_refuses_writes() {
    let mut insert = AudioEffectInstance::new("example.not_loaded");
    insert.params.insert("gain".into(), Param::Constant(0.5));
    assert!(insert.spec().is_none());
    assert_eq!(insert.sample_param("gain", 0.0), None);
    assert_eq!(
        insert.set_param(0, Param::Constant(0.25)),
        Err(ModelError::UnknownAudioEffect("example.not_loaded".into()))
    );
    assert_eq!(
        insert.set_sidechain(Some(crate::TrackId::from_raw(1))),
        Err(ModelError::UnknownAudioEffect("example.not_loaded".into()))
    );
    // Clearing a key needs no spec: there is nothing to check.
    assert_eq!(insert.set_sidechain(None), Ok(()));
    assert_eq!(insert.params.get("gain"), Some(&Param::Constant(0.5)));
}

/// A project with a media clip on an audio lane, a solid on its own lane, a
/// clip of silent footage on a video lane, and a text lane.
struct Stage {
    project: Project,
    audio: crate::TrackId,
    text: crate::TrackId,
    sound: crate::ClipId,
    solid: crate::ClipId,
    silent: crate::ClipId,
}

fn stage() -> Stage {
    let mut project = Project::new("inserts", R24);
    let audio = project.add_track(TrackKind::Audio, "A1");
    let video = project.add_track(TrackKind::Video, "V1");
    let text = project.add_track(TrackKind::Text, "T1");
    let score = project.add_media(MediaSource::new("/media/score.wav", 0, 0, R24, 480, true));
    let broll = project.add_media(MediaSource::new(
        "/media/broll.mp4",
        1920,
        1080,
        R24,
        480,
        false,
    ));
    let sound = project
        .add_clip(
            audio,
            score,
            TimeRange::at_rate(0, 96, R24),
            RationalTime::new(0, R24),
        )
        .expect("an audio clip");
    let solid_color = Generator::SolidColor { rgba: [0; 4] };
    let solid_lane = project.add_track(
        TrackKind::for_generator(&solid_color).expect("a solid has a lane"),
        "Solid",
    );
    let solid = project
        .add_generated(solid_lane, solid_color, TimeRange::at_rate(0, 48, R24))
        .expect("a solid");
    let silent = project
        .add_clip(
            video,
            broll,
            TimeRange::at_rate(0, 48, R24),
            RationalTime::new(48, R24),
        )
        .expect("silent footage");
    Stage {
        project,
        audio,
        text,
        sound,
        solid,
        silent,
    }
}

fn chain(n: usize) -> Vec<AudioEffectInstance> {
    (0..n)
        .map(|i| AudioEffectInstance::new(format!("example.fx{i}")))
        .collect()
}

#[test]
fn an_insert_chain_sits_only_where_there_is_sound() {
    let Stage {
        mut project,
        audio,
        text,
        sound,
        solid,
        silent,
    } = stage();
    for (case, target, want) in [
        (
            "a generated clip has nothing to hear",
            AudioInsertTarget::Clip(solid),
            ModelError::ClipCarriesNoSound(solid),
        ),
        (
            "footage without an audio stream has nothing to hear",
            AudioInsertTarget::Clip(silent),
            ModelError::ClipCarriesNoSound(silent),
        ),
        (
            "a text lane is not in the mix",
            AudioInsertTarget::Track(text),
            ModelError::LaneCarriesNoSound(text),
        ),
    ] {
        assert_eq!(
            project.set_audio_inserts(target, chain(1)),
            Err(want),
            "{case}"
        );
    }
    let gone = crate::ClipId::from_raw(u64::MAX);
    assert_eq!(
        project.audio_inserts(AudioInsertTarget::Clip(gone)),
        Err(ModelError::UnknownClip(gone))
    );
    for target in [
        AudioInsertTarget::Clip(sound),
        AudioInsertTarget::Track(audio),
        AudioInsertTarget::Master,
    ] {
        project.set_audio_inserts(target, chain(2)).unwrap();
        assert_eq!(project.audio_inserts(target).unwrap(), chain(2).as_slice());
    }
}

#[test]
fn an_unknown_effect_is_refused_before_the_chain_changes() {
    let Stage {
        mut project, audio, ..
    } = stage();
    let target = AudioInsertTarget::Track(audio);
    assert_eq!(
        project.add_audio_effect(target, "example.not_loaded"),
        Err(ModelError::UnknownAudioEffect("example.not_loaded".into()))
    );
    assert!(project.audio_inserts(target).unwrap().is_empty());
}

#[test]
fn a_strip_holds_five_inserts_and_a_clip_holds_more() {
    let Stage {
        mut project,
        audio,
        sound,
        ..
    } = stage();
    for target in [AudioInsertTarget::Track(audio), AudioInsertTarget::Master] {
        assert_eq!(
            project.set_audio_inserts(target, chain(STRIP_INSERTS + 1)),
            Err(ModelError::InsertsFull(STRIP_INSERTS))
        );
        project
            .set_audio_inserts(target, chain(STRIP_INSERTS))
            .unwrap();
    }
    project
        .set_audio_inserts(AudioInsertTarget::Clip(sound), chain(STRIP_INSERTS * 2))
        .unwrap();
}

#[test]
fn every_index_past_the_chain_is_refused_by_name() {
    let Stage {
        mut project, audio, ..
    } = stage();
    let target = AudioInsertTarget::Track(audio);
    project.set_audio_inserts(target, chain(2)).unwrap();
    let past = || Err(ModelError::UnknownInsert { index: 2, len: 2 });
    assert_eq!(project.remove_audio_effect(target, 2).map(|_| ()), past());
    assert_eq!(project.move_audio_effect(target, 2, 0), past());
    assert_eq!(project.move_audio_effect(target, 0, 2), past());
    assert_eq!(project.set_audio_effect_bypass(target, 2, true), past());
    assert_eq!(
        project.set_audio_effect_param(target, 2, 0, Param::Constant(0.0)),
        past()
    );
    assert_eq!(project.set_audio_effect_sidechain(target, 2, None), past());
    assert_eq!(project.audio_inserts(target).unwrap(), chain(2).as_slice());
}

#[test]
fn moving_removing_and_bypassing_an_insert_reorder_the_chain_in_place() {
    let Stage {
        mut project, audio, ..
    } = stage();
    let target = AudioInsertTarget::Track(audio);
    project.set_audio_inserts(target, chain(3)).unwrap();
    let ids = |project: &Project| -> Vec<String> {
        project
            .audio_inserts(target)
            .unwrap()
            .iter()
            .map(|fx| fx.effect_id.clone())
            .collect()
    };

    project.move_audio_effect(target, 0, 2).unwrap();
    assert_eq!(ids(&project), ["example.fx1", "example.fx2", "example.fx0"]);
    project.move_audio_effect(target, 2, 0).unwrap();
    assert_eq!(ids(&project), ["example.fx0", "example.fx1", "example.fx2"]);

    project.set_audio_effect_bypass(target, 1, true).unwrap();
    assert!(project.audio_inserts(target).unwrap()[1].bypass);

    let removed = project.remove_audio_effect(target, 1).unwrap();
    assert_eq!(removed.effect_id, "example.fx1");
    assert!(removed.bypass);
    assert_eq!(ids(&project), ["example.fx0", "example.fx2"]);
}

#[test]
fn a_restored_chain_is_checked_for_shape_and_for_a_key_on_a_clip() {
    let Stage {
        mut project, sound, ..
    } = stage();
    let target = AudioInsertTarget::Clip(sound);
    let mut malformed = AudioEffectInstance::new("example.fx");
    malformed
        .params
        .insert("gain".into(), Param::Keyframed { keyframes: vec![] });
    assert!(matches!(
        project.set_audio_inserts(target, vec![malformed]),
        Err(ModelError::InvalidParam(_))
    ));

    let mut keyed = AudioEffectInstance::new("example.fx");
    keyed.sidechain = Some(crate::TrackId::from_raw(1));
    assert!(matches!(
        project.set_audio_inserts(target, vec![keyed]),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(project.audio_inserts(target).unwrap().is_empty());
}

#[test]
fn a_rate_change_moves_lane_and_master_insert_keys_onto_the_new_grid() {
    let Stage {
        mut project, audio, ..
    } = stage();
    let mut animated = AudioEffectInstance::new("example.fx");
    animated.params.insert(
        "gain".into(),
        Param::Keyframed {
            keyframes: vec![
                Keyframe::new(24, 0.0, Easing::Linear),
                Keyframe::new(48, 1.0, Easing::Linear),
            ],
        },
    );
    for target in [AudioInsertTarget::Track(audio), AudioInsertTarget::Master] {
        project
            .set_audio_inserts(target, vec![animated.clone()])
            .unwrap();
    }
    project
        .timeline_mut()
        .resample_frame_rate(Rational::new(30, 1))
        .unwrap();
    for target in [AudioInsertTarget::Track(audio), AudioInsertTarget::Master] {
        let ticks: Vec<i64> = project.audio_inserts(target).unwrap()[0].params["gain"]
            .keyframes()
            .iter()
            .map(|key| key.tick)
            .collect();
        assert_eq!(ticks, [30, 60], "{target:?}");
    }
}

/// The shipped catalog, as owned entries an override file would hold.
fn shipped_entries() -> Vec<AudioEffectSpec> {
    shipped_audio_effect_catalog()
        .iter()
        .map(|spec| AudioEffectSpec::clone(spec))
        .collect()
}

fn edited(id: &str, edit: impl FnOnce(&mut AudioEffectSpec)) -> Vec<AudioEffectSpec> {
    let mut entries = shipped_entries();
    let entry = entries
        .iter_mut()
        .find(|spec| spec.id == id)
        .unwrap_or_else(|| panic!("{id} ships"));
    edit(entry);
    entries
}

/// A processor reads its parameters by slot and reports a fixed latency, so
/// an override may relabel and re-range a shipped effect but not reshape it.
/// Each refusal names the effect, so the Catalogs list can say which entry.
#[test]
fn an_override_keeps_each_shipped_effects_slots_latency_and_key() {
    assert_eq!(check_audio_catalog(&shipped_entries()), Ok(()));
    let allowed = [
        (
            "relabelled",
            edited("limiter", |spec| spec.label = "Brickwall".into()),
        ),
        (
            "re-ranged",
            edited("compressor", |spec| {
                spec.params[1].max = 40.0;
                spec.params[1].default = 8.0;
            }),
        ),
        ("dropped", {
            let mut entries = shipped_entries();
            entries.retain(|spec| spec.id != "reverb");
            entries
        }),
        ("added", {
            let mut entries = shipped_entries();
            entries.push(AudioEffectSpec {
                id: "house_gain".into(),
                category: String::new(),
                label: "House Gain".into(),
                params: vec![scalar("drive", 0.0, 0.0, 1.0)],
                latency: 12,
                sidechain: true,
            });
            entries
        }),
    ];
    for (case, entries) in allowed {
        assert_eq!(check_audio_catalog(&entries), Ok(()), "{case}");
    }
    let refused = [
        (
            "reordered",
            "parametric_eq",
            edited("parametric_eq", |spec| spec.params.swap(0, 1)),
        ),
        (
            "renamed",
            "gain",
            edited("gain", |spec| spec.params[0].name = "level".into()),
        ),
        (
            "shortened",
            "delay",
            edited("delay", |spec| {
                spec.params.pop();
            }),
        ),
        (
            "lengthened",
            "mono",
            edited("mono", |spec| {
                spec.params.push(scalar("blend", 0.0, 0.0, 1.0));
            }),
        ),
        (
            "latency",
            "limiter",
            edited("limiter", |spec| spec.latency = 0),
        ),
        (
            "key",
            "compressor",
            edited("compressor", |spec| spec.sidechain = false),
        ),
        (
            "gained a key",
            "reverb",
            edited("reverb", |spec| spec.sidechain = true),
        ),
    ];
    for (case, id, entries) in refused {
        let err = check_audio_catalog(&entries).expect_err(case);
        assert!(err.starts_with(id), "{case}: {err}");
    }
}

/// A shipped effect resolves with no registration, takes writes inside its
/// ranges, and reads a key only when it declares one.
#[test]
fn a_shipped_effect_takes_writes_in_range_and_keys_only_when_declared() {
    let mut compressor = AudioEffectInstance::new("compressor");
    assert_eq!(compressor.set_param(0, Param::Constant(-30.0)), Ok(()));
    assert_eq!(compressor.sample_param("threshold", 0.0), Some(-30.0));
    assert!(compressor.set_param(0, Param::Constant(-61.0)).is_err());
    assert!(compressor.set_param(6, Param::Constant(0.0)).is_err());
    assert_eq!(
        compressor.set_sidechain(Some(crate::TrackId::from_raw(3))),
        Ok(())
    );

    let mut gain = AudioEffectInstance::new("gain");
    assert!(
        gain.set_sidechain(Some(crate::TrackId::from_raw(3)))
            .is_err()
    );
    assert_eq!(gain.sample_param("gain", 0.0), Some(0.0));
}

/// Every shipped audio effect names the group it is listed under, and an entry
/// that names none is listed under Other Audio. A shipped entry added without a
/// category fails here.
#[test]
fn every_shipped_audio_effect_names_its_group_and_a_nameless_one_is_other_audio() {
    for spec in shipped_audio_effect_catalog() {
        assert!(!spec.category.is_empty(), "{} names no category", spec.id);
        assert_eq!(spec.category(), spec.category);
    }
    let spec: AudioEffectSpec = toml::from_str("id = \"x\"\nlabel = \"X\"").expect("parses");
    assert_eq!(spec.category(), OTHER_AUDIO_CATEGORY);
}
