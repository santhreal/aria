//! `source_time_2` cases.

use super::*;
use crate::time::Rational;

#[test]
fn source_time_at_reversed_double_speed() {
    // 2× + reverse: timeline [0, 25) covers source [100, 150) backward.
    let mut clip = media_clip(MediaId::from_raw(1), tr(100, 50, R24), tr(0, 25, R24));
    clip.speed = Rational::new(2, 1);
    clip.reversed = true;
    assert_eq!(clip.source_time_at(rt(0, R24)).unwrap(), Some(rt(149, R24)));
    assert_eq!(
        clip.source_time_at(rt(10, R24)).unwrap(),
        Some(rt(129, R24))
    );
    assert_eq!(
        clip.source_time_at(rt(24, R24)).unwrap(),
        Some(rt(101, R24))
    );
}

#[test]
fn source_time_at_clamps_rounding_into_the_window() {
    // src dur 3 ÷ (2/3 speed) = 4.5 → 4 timeline ticks (truncating);
    // every timeline tick must still land inside the source window.
    let mut clip = media_clip(MediaId::from_raw(1), tr(10, 3, R24), tr(0, 4, R24));
    clip.speed = Rational::new(2, 3);
    for t in 0..4 {
        let src = clip.source_time_at(rt(t, R24)).unwrap().unwrap().value;
        assert!((10..13).contains(&src), "tick {t} mapped to {src}");
    }
}

// --- speed serde shape --------------------------------------------------

#[test]
fn never_retimed_clips_serialize_without_speed_fields() {
    let clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(0, 10, R24));
    let value = serde_json::to_value(&clip).expect("serialize");
    let map = value.as_object().expect("clip serializes to a map");
    assert!(!map.contains_key("speed"), "1× speed must stay absent");
    assert!(!map.contains_key("reversed"), "forward must stay absent");

    // And a pre-speed save (no fields) loads as forward 1×.
    let loaded: Clip = serde_json::from_value(value).expect("deserialize");
    assert_eq!(loaded.speed, Rational::new(1, 1));
    assert!(!loaded.reversed);
    assert!(!loaded.is_retimed());
}

#[test]
fn retimed_clip_roundtrips_speed_through_serde() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(0, 5, R24));
    clip.speed = Rational::new(2, 1);
    clip.reversed = true;
    let json = serde_json::to_string(&clip).expect("serialize");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded.speed, Rational::new(2, 1));
    assert!(loaded.reversed);
}

// --- pitch lock (M8 Phase 3) --------------------------------------------

#[test]
fn pitch_lock_defaults_on_and_is_omitted_from_saves() {
    let clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(0, 5, R24));
    assert!(clip.preserve_pitch, "pitch is locked by default");
    let map = serde_json::to_value(&clip).unwrap();
    assert!(
        !map.as_object().unwrap().contains_key("preserve_pitch"),
        "the locked default stays absent so old files are byte-identical"
    );
    // A pre-Phase-3 save (no field) loads pitch-locked.
    let loaded: Clip = serde_json::from_value(map).unwrap();
    assert!(loaded.preserve_pitch);
}

#[test]
fn pitch_unlock_roundtrips_and_drives_the_transpose_factor() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(0, 5, R24));
    clip.speed = Rational::new(2, 1);
    // Locked: no pitch shift regardless of speed.
    assert_eq!(clip.audio_pitch_factor(), 1.0);
    // Unlocked (chipmunk): pitch rides the 2× speed.
    clip.preserve_pitch = false;
    assert!((clip.audio_pitch_factor() - 2.0).abs() < 1e-6);
    let json = serde_json::to_string(&clip).expect("serialize");
    assert!(json.contains("preserve_pitch"), "the off state is saved");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert!(!loaded.preserve_pitch);
    assert!((loaded.audio_pitch_factor() - 2.0).abs() < 1e-6);
}

#[test]
fn flat_curve_is_not_retimed_and_omitted_from_saves() {
    let clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(0, 10, R24));
    assert!(!clip.has_speed_curve());
    assert_eq!(clip.speed_curve_average(), 1.0);
    let map = serde_json::to_value(&clip).unwrap();
    assert!(!map.as_object().unwrap().contains_key("speed_curve"));
}

#[test]
fn curve_integral_matches_analytic_linear_ramp() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(0, 100, R24));
    // Rate ramps 1 → 3 linearly; average = 2, ∫₀ᵖ (1+2q) dq = p + p².
    clip.speed_curve = linear_ramp(1.0, 3.0);
    assert!(clip.has_speed_curve());
    assert!((clip.speed_curve_average() - 2.0).abs() < 1e-6);
    assert!((clip.speed_curve_integral(0.5) - (0.5 + 0.25)).abs() < 1e-6);
    assert!((clip.speed_curve_integral(1.0) - 2.0).abs() < 1e-6);
    // Outside the unit range clamps.
    assert_eq!(clip.speed_curve_integral(0.0), 0.0);
    assert!((clip.speed_curve_integral(2.0) - 2.0).abs() < 1e-6);
}

#[test]
fn source_fraction_is_normalized_and_matches_source_time_at() {
    // Linear 1 → 3 ramp: ∫₀ᵖ (1+2q) dq / 2 = (p + p²)/2.
    let curve = linear_ramp(1.0, 3.0);
    assert_eq!(speed_curve_source_fraction(&curve, 0.0), 0.0);
    assert!(
        (speed_curve_source_fraction(&curve, 1.0) - 1.0).abs() < 1e-9,
        "ends at 1"
    );
    assert!(
        (speed_curve_source_fraction(&curve, 0.5) - (0.5 + 0.25) / 2.0).abs() < 1e-9,
        "midpoint sweeps the analytic fraction"
    );
    // It is exactly the placement `source_time_at` uses for the picture, so
    // the audio render warps in lockstep: fraction · window == source offset.
    let clip = {
        let mut c = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(0, 100, R24));
        c.speed_curve = curve.clone();
        c
    };
    let src_dur = clip.source_range().unwrap().duration.value as f64;
    let mid = clip.start().value + clip.timeline.duration.value / 2;
    let src = clip
        .source_time_at(RationalTime::new(mid, R24))
        .unwrap()
        .unwrap();
    let expected = (src_dur * speed_curve_source_fraction(&curve, 0.5)).round() as i64;
    assert_eq!(
        src.value, expected,
        "audio fraction matches the video mapping"
    );
}

#[test]
fn curve_integral_holds_flat_outside_keyframes() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(0, 100, R24));
    // One mid keyframe: constant 2.0 everywhere (flat extrapolation).
    clip.speed_curve = Param::Keyframed {
        keyframes: vec![Keyframe {
            tick: SPEED_CURVE_SCALE / 2,
            value: 2.0,
            easing: Easing::Linear,
            tangents: None,
        }],
    };
    assert!((clip.speed_curve_integral(0.25) - 0.5).abs() < 1e-6);
    assert!((clip.speed_curve_average() - 2.0).abs() < 1e-6);
}

#[test]
fn source_time_at_curve_sweeps_full_window_symmetrically() {
    // A symmetric slow-fast-slow ramp must still consume the whole source
    // window across the clip, and the midpoint sweeps exactly half.
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(0, 100, R24));
    clip.speed_curve = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: 0.5,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: SPEED_CURVE_SCALE / 2,
                value: 2.0,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: SPEED_CURVE_SCALE,
                value: 0.5,
                easing: Easing::Linear,
                tangents: None,
            },
        ],
    };
    let start = clip.source_time_at(rt(0, R24)).unwrap().unwrap();
    let mid = clip.source_time_at(rt(50, R24)).unwrap().unwrap();
    let endish = clip.source_time_at(rt(99, R24)).unwrap().unwrap();
    assert_eq!(start.value, 0);
    // By symmetry the middle of the clip is the middle of the source.
    assert_eq!(mid.value, 50);
    // The last frame clamps to the final source frame (window fully swept).
    assert_eq!(endish.value, 99);
}

#[test]
fn source_time_at_flat_curve_matches_constant_speed_exact_path() {
    // A clip with a flat curve and a constant 2× must map identically to
    // the exact rational path (no f64 drift).
    let mut curved = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(0, 50, R24));
    curved.speed = Rational::new(2, 1);
    for tick in [0, 7, 23, 49] {
        let got = curved.source_time_at(rt(tick, R24)).unwrap().unwrap();
        assert_eq!(got.value, (tick * 2).min(99), "tick {tick}");
    }
}

#[test]
fn validate_speed_curve_rejects_out_of_range_values_and_ticks() {
    // Value below the floor.
    assert!(validate_speed_curve(&linear_ramp(0.0, 1.0)).is_err());
    // Tick outside the normalized span.
    let bad_tick = Param::Keyframed {
        keyframes: vec![Keyframe {
            tick: SPEED_CURVE_SCALE + 1,
            value: 1.0,
            easing: Easing::Linear,
            tangents: None,
        }],
    };
    assert!(validate_speed_curve(&bad_tick).is_err());
    // A sane ramp passes.
    assert!(validate_speed_curve(&linear_ramp(0.5, 2.0)).is_ok());
}

#[test]
fn speed_presets_are_valid_curves() {
    for name in ["ramp_up", "ramp_down", "montage", "hero", "bullet"] {
        let curve = speed_preset(name).unwrap_or_else(|| panic!("missing preset {name}"));
        validate_speed_curve(&curve).unwrap_or_else(|e| panic!("{name} invalid: {e:?}"));
    }
    assert!(speed_preset("nope").is_none());
}

#[test]
fn curve_roundtrips_through_serde_and_marks_retimed() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(0, 100, R24));
    clip.speed_curve = speed_preset("montage").unwrap();
    assert!(clip.is_retimed());
    let json = serde_json::to_string(&clip).unwrap();
    let loaded: Clip = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.speed_curve, clip.speed_curve);
    assert!(loaded.has_speed_curve());
}

// --- audio mix: volume + fades (M1) --------------------------------------

#[test]
fn default_audio_serializes_without_fields() {
    let clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(0, 10, R24));
    assert!(!clip.has_custom_audio());
    let value = serde_json::to_value(&clip).expect("serialize");
    let map = value.as_object().expect("clip serializes to a map");
    assert!(!map.contains_key("volume"), "unit volume must stay absent");
    assert!(!map.contains_key("pan"), "center pan must stay absent");
    assert!(!map.contains_key("fade_in"), "zero fade must stay absent");
    assert!(!map.contains_key("fade_out"), "zero fade must stay absent");

    // And a pre-volume / pre-pan save loads with the defaults.
    let loaded: Clip = serde_json::from_value(value).expect("deserialize");
    assert_eq!(loaded.volume, Param::Constant(1.0));
    assert_eq!(loaded.pan, Param::Constant(0.0));
    assert_eq!((loaded.fade_in, loaded.fade_out), (0, 0));
}

#[test]
fn custom_audio_roundtrips_through_serde() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 48, R24), tr(0, 48, R24));
    clip.volume = Param::Constant(0.5);
    clip.pan = Param::Constant(-0.25);
    clip.fade_in = 12;
    clip.fade_out = 24;
    assert!(clip.has_custom_audio());
    let json = serde_json::to_string(&clip).expect("serialize");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded.volume, Param::Constant(0.5));
    assert_eq!(loaded.pan, Param::Constant(-0.25));
    assert_eq!((loaded.fade_in, loaded.fade_out), (12, 24));
}

#[test]
fn constant_volume_serializes_as_a_bare_value() {
    // M8 migrated `volume` to a `Param`, but a constant gain must stay
    // byte-identical to the pre-M8 bare-`f32` shape so old files load
    // unchanged and constant-only saves never grow a `{"kf":..}` wrapper.
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 48, R24), tr(0, 48, R24));
    clip.volume = Param::Constant(0.5);
    let value = serde_json::to_value(&clip).expect("serialize");
    assert_eq!(value.get("volume"), Some(&serde_json::json!(0.5)));
    // A pre-M8 bare value still loads as a constant.
    let loaded: Clip = serde_json::from_value(value).expect("deserialize");
    assert_eq!(loaded.volume, Param::Constant(0.5));
}

#[test]
fn constant_pan_serializes_as_a_bare_value() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 48, R24), tr(0, 48, R24));
    clip.pan = Param::Constant(0.5);
    let value = serde_json::to_value(&clip).expect("serialize");
    assert_eq!(value.get("pan"), Some(&serde_json::json!(0.5)));
    // A legacy save without pan loads centered.
    let mut legacy = value.as_object().cloned().expect("map");
    legacy.remove("pan");
    let loaded: Clip = serde_json::from_value(serde_json::Value::Object(legacy)).expect("load");
    assert_eq!(loaded.pan, Param::Constant(0.0));
    // Bare constant round-trips without a `{"kf":..}` wrapper.
    let roundtrip: Clip = serde_json::from_value(value).expect("deserialize");
    assert_eq!(roundtrip.pan, Param::Constant(0.5));
}

#[test]
fn volume_envelope_roundtrips_and_validates() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 48, R24), tr(0, 48, R24));
    clip.volume = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: 0.0,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: 24,
                value: 1.0,
                easing: Easing::EaseOut,
                tangents: None,
            },
        ],
    };
    assert!(clip.has_volume_envelope());
    assert!(!clip.is_silent(), "an envelope is non-zero somewhere");
    validate_volume_envelope(&clip.volume).expect("in-range envelope");
    let json = serde_json::to_string(&clip).expect("serialize");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded.volume, clip.volume);

    // Out-of-range gain is rejected.
    let hot = Param::Keyframed {
        keyframes: vec![Keyframe {
            tick: 0,
            value: MAX_CLIP_VOLUME + 1.0,
            easing: Easing::Linear,
            tangents: None,
        }],
    };
    assert!(validate_volume_envelope(&hot).is_err());
}

#[test]
fn volume_and_pan_validate_boundaries() {
    assert!(validate_volume(0.0).is_ok());
    assert!(validate_volume(1.0).is_ok());
    assert!(validate_volume(MAX_CLIP_VOLUME).is_ok());
    assert!(validate_volume(-0.01).is_err());
    assert!(validate_volume(MAX_CLIP_VOLUME + 0.01).is_err());
    assert!(validate_volume(f32::NAN).is_err());
    assert!(validate_volume(f32::INFINITY).is_err());

    assert!(validate_pan(MIN_CLIP_PAN).is_ok());
    assert!(validate_pan(0.0).is_ok());
    assert!(validate_pan(MAX_CLIP_PAN).is_ok());
    assert!(validate_pan(MIN_CLIP_PAN - 0.1).is_err());
    assert!(validate_pan(MAX_CLIP_PAN + 0.1).is_err());
    assert!(validate_pan(f32::NAN).is_err());
    assert!(validate_pan(f32::INFINITY).is_err());
}

#[test]
fn pan_envelope_roundtrips_and_validates() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 48, R24), tr(0, 48, R24));
    clip.pan = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: -1.0,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: 24,
                value: 1.0,
                easing: Easing::EaseOut,
                tangents: None,
            },
        ],
    };
    assert!(clip.has_pan_envelope());
    assert!(clip.has_custom_audio());
    validate_pan_envelope(&clip.pan).expect("in-range envelope");
    let json = serde_json::to_string(&clip).expect("serialize");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded.pan, clip.pan);

    for bad in [
        MAX_CLIP_PAN + 0.01,
        MIN_CLIP_PAN - 0.01,
        f32::NAN,
        f32::INFINITY,
    ] {
        assert!(validate_pan(bad).is_err());
    }
    let hot = Param::Keyframed {
        keyframes: vec![Keyframe {
            tick: 0,
            value: MAX_CLIP_PAN + 1.0,
            easing: Easing::Linear,
            tangents: None,
        }],
    };
    assert!(validate_pan_envelope(&hot).is_err());
}

#[test]
fn audio_gain_ramps_linearly_at_both_edges() {
    let vol = |v: f32| Param::Constant(v);
    // No fades: flat volume everywhere.
    assert_eq!(audio_gain_at(0, 100, &vol(0.8), 0, 0), 0.8);
    assert_eq!(audio_gain_at(99, 100, &vol(0.8), 0, 0), 0.8);

    // Fade-in over the first 10: silence at 0, half at 5, full at 10.
    assert_eq!(audio_gain_at(0, 100, &vol(1.0), 10, 0), 0.0);
    assert_eq!(audio_gain_at(5, 100, &vol(1.0), 10, 0), 0.5);
    assert_eq!(audio_gain_at(10, 100, &vol(1.0), 10, 0), 1.0);

    // Fade-out over the last 10: full until 90, half at 95, ~0 at the end.
    assert_eq!(audio_gain_at(90, 100, &vol(1.0), 0, 10), 1.0);
    assert_eq!(audio_gain_at(95, 100, &vol(1.0), 0, 10), 0.5);
    assert!(audio_gain_at(99, 100, &vol(1.0), 0, 10) <= 0.11);

    // Ramps scale by the volume and overlapping fades multiply.
    assert_eq!(audio_gain_at(5, 100, &vol(2.0), 10, 0), 1.0);
    assert_eq!(audio_gain_at(5, 10, &vol(1.0), 10, 10), 0.25);

    // Out-of-span positions never go negative.
    assert_eq!(audio_gain_at(-3, 100, &vol(1.0), 10, 0), 0.0);
    assert_eq!(audio_gain_at(105, 100, &vol(1.0), 0, 10), 0.0);
}

#[test]
fn enabled_and_group_default_for_old_saves_and_roundtrip_when_set() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 48, R24), tr(0, 48, R24));
    assert!(clip.enabled);
    assert_eq!(clip.group, None);
    assert!(!clip.is_silent());

    let map = serde_json::to_value(&clip).unwrap();
    assert!(!map.as_object().unwrap().contains_key("enabled"));
    assert!(!map.as_object().unwrap().contains_key("group"));

    // Old project JSON without `enabled` or `group` loads enabled and ungrouped.
    let loaded: Clip = serde_json::from_value(map).unwrap();
    assert!(loaded.enabled);
    assert_eq!(loaded.group, None);

    // Disabled and grouped states round-trip and silence the clip.
    clip.enabled = false;
    clip.group = Some(crate::GroupId::from_raw(7));
    assert!(clip.is_silent(), "disabled clips are silent in audio mixes");
    let json = serde_json::to_string(&clip).unwrap();
    assert!(json.contains("\"enabled\":false"));
    assert!(json.contains("\"group\":7"));
    let loaded: Clip = serde_json::from_str(&json).unwrap();
    assert!(!loaded.enabled);
    assert_eq!(loaded.group, Some(crate::GroupId::from_raw(7)));
}
