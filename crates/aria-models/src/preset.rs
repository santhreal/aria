//! Animation presets (`.ariapreset`): reusable effects, audio inserts, and
//! property curves anchored at the playhead.
//!
//! Every keyframe tick in a [`Preset`] is relative to the anchor: tick `0` is
//! the playhead at save time, and applying a preset at `anchor` shifts every
//! effect, audio insert, and property keyframe by `+anchor`.

use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

use crate::audio_effects::AudioEffectInstance;
use crate::clip::{Clip, ClipParam, ParamValue, property_row, visit_clip_params};
use crate::effects::EffectInstance;
use crate::error::ModelError;
use crate::param::Param;
use crate::param_walk::ParamWalk;

/// File extension for standalone and packaged preset files (without leading dot).
pub const PRESET_FILE_EXTENSION: &str = "ariapreset";

/// Current `.ariapreset` schema version.
pub const PRESET_VERSION: u32 = 1;

/// Maximum `.ariapreset` file size accepted by [`Preset::parse`] (1 MiB).
pub const PRESET_MAX_BYTES: u64 = 1 << 20;

/// Maximum length of a preset name in Unicode scalar values.
pub const PRESET_MAX_NAME_CHARS: usize = 128;

/// Maximum number of visual effects in one preset.
pub const MAX_PRESET_EFFECTS: usize = 64;

/// Maximum number of audio effects in one preset.
pub const MAX_PRESET_AUDIO_EFFECTS: usize = 64;

/// Maximum number of property curves in one preset.
pub const MAX_PRESET_PROPERTIES: usize = 256;

/// Maximum number of keyframes on any single curve in a preset.
pub const MAX_PRESET_KEYFRAMES: usize = 4096;

/// A saved animation and effect preset (`.ariapreset`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// Format version; must equal [`PRESET_VERSION`].
    pub version: u32,
    /// Display name: trimmed, non-empty, at most [`PRESET_MAX_NAME_CHARS`] chars.
    pub name: String,
    /// Visual effects appended to the target clip's effect chain.
    #[serde(default)]
    pub effects: Vec<EffectInstance>,
    /// Audio inserts appended to the target clip's audio effect chain.
    #[serde(default)]
    pub audio_effects: Vec<AudioEffectInstance>,
    /// Animatable clip properties with curves relative to the anchor tick.
    #[serde(default)]
    pub properties: Vec<PresetProperty>,
}

/// One clip property curve stored inside a [`Preset`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetProperty {
    /// A row id [`property_row`] resolves: a row every clip declares, or a mask
    /// row. Never an effect parameter row; effects travel whole.
    pub id: String,
    /// Constant value, or keyframes with ticks relative to the anchor (`0` = playhead).
    pub curve: Param<ParamValue>,
}

/// Which parts of a clip to include when capturing a [`Preset`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetParts {
    /// Indices into [`Clip::effects`].
    pub effects: Vec<usize>,
    /// Indices into [`Clip::audio_effects`].
    pub audio_effects: Vec<usize>,
    /// Property ids, each one [`property_row`] resolves.
    pub properties: Vec<String>,
}

impl PresetParts {
    /// Every effect, every audio insert, and every property of `clip`,
    /// including each mask's, that is available and keyframed. Effect
    /// parameters travel with their effects.
    pub fn all_of(clip: &Clip) -> Self {
        let mut properties = Vec::new();
        visit_clip_params(clip, |spec| {
            if !matches!(spec.param, ClipParam::Effect { .. }) && !spec.keyframes(clip).is_empty() {
                properties.push(spec.id.to_string());
            }
        });
        Self {
            effects: (0..clip.effects.len()).collect(),
            audio_effects: (0..clip.audio_effects.len()).collect(),
            properties,
        }
    }

    /// True when no effect, audio insert, or property is selected.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty() && self.audio_effects.is_empty() && self.properties.is_empty()
    }
}

impl Preset {
    /// Construct a preset at [`PRESET_VERSION`] with `name` trimmed.
    pub fn new(
        name: impl Into<String>,
        effects: Vec<EffectInstance>,
        audio_effects: Vec<AudioEffectInstance>,
        properties: Vec<PresetProperty>,
    ) -> Self {
        Self {
            version: PRESET_VERSION,
            name: name.into().trim().to_string(),
            effects,
            audio_effects,
            properties,
        }
    }

    /// Capture the selected `parts` of `clip` into a named preset, shifting
    /// every keyframe tick by `-anchor_local` so tick `0` is the anchor.
    pub fn capture(
        name: impl Into<String>,
        clip: &Clip,
        anchor_local: i64,
        parts: &PresetParts,
    ) -> Result<Self, ModelError> {
        let name = name.into().trim().to_string();
        let mut effects = Vec::with_capacity(parts.effects.len());
        for &index in &parts.effects {
            let effect = clip.effects.get(index).ok_or_else(|| {
                ModelError::InvalidParam(format!(
                    "preset effect index {index} out of range for chain length {}; select a valid effect index",
                    clip.effects.len()
                ))
            })?;
            effects.push(effect.clone());
        }

        let mut audio_effects = Vec::with_capacity(parts.audio_effects.len());
        for &index in &parts.audio_effects {
            let insert = clip.audio_effects.get(index).ok_or_else(|| {
                ModelError::InvalidParam(format!(
                    "preset audio effect index {index} out of range for chain length {}; select a valid audio insert index",
                    clip.audio_effects.len()
                ))
            })?;
            audio_effects.push(insert.clone());
        }

        let mut properties = Vec::with_capacity(parts.properties.len());
        for id in &parts.properties {
            let spec = property_row(id).ok_or_else(|| {
                ModelError::InvalidParam(format!(
                    "property `{id}`: unknown property id; choose a property from clip_param_catalog() or a mask row"
                ))
            })?;
            let curve = spec.curve(clip).ok_or_else(|| {
                ModelError::InvalidParam(format!(
                    "property `{id}`: not available on clip {}; enable its block or select a compatible clip",
                    clip.id
                ))
            })?;
            properties.push(PresetProperty {
                id: id.clone(),
                curve,
            });
        }

        let mut preset = Self {
            version: PRESET_VERSION,
            name,
            effects,
            audio_effects,
            properties,
        };
        let shift = anchor_local.checked_neg().ok_or(ModelError::TimeOverflow)?;
        preset.walk_params(&mut |curve| curve.shift_fits(shift))?;
        preset.walk_params(&mut |curve| curve.shift(shift))?;
        preset.validate()?;
        Ok(preset)
    }

    /// Parse and validate a `.ariapreset` payload from raw bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, ModelError> {
        if bytes.len() as u64 > PRESET_MAX_BYTES {
            return Err(ModelError::InvalidParam(format!(
                "preset file is {} bytes, exceeding the maximum of {PRESET_MAX_BYTES} bytes; reduce the preset size",
                bytes.len()
            )));
        }
        let text = std::str::from_utf8(bytes).map_err(|err| {
            ModelError::InvalidParam(format!(
                "preset file is not valid UTF-8 ({err}); save the preset as UTF-8 JSON"
            ))
        })?;
        let mut preset: Self = serde_json::from_str(text).map_err(|err| {
            ModelError::InvalidParam(format!(
                "invalid preset JSON ({err}); check the .ariapreset syntax and schema"
            ))
        })?;
        preset.name = preset.name.trim().to_string();
        preset.validate()?;
        Ok(preset)
    }

    /// Serialize this preset as pretty-printed JSON after validating it.
    pub fn to_json(&self) -> Result<String, ModelError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|err| {
            ModelError::InvalidParam(format!("failed to serialize preset `{}`: {err}", self.name))
        })
    }

    /// Validate all structural and catalog invariants on this preset.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.version != PRESET_VERSION {
            return Err(ModelError::InvalidParam(format!(
                "preset `{}` has unsupported version {} (expected version {PRESET_VERSION}); update the preset version to {PRESET_VERSION}",
                self.name, self.version
            )));
        }
        let trimmed = self.name.trim();
        if trimmed.is_empty() {
            return Err(ModelError::InvalidParam(
                "preset name cannot be empty; provide a non-empty name".into(),
            ));
        }
        let char_count = trimmed.chars().count();
        if char_count > PRESET_MAX_NAME_CHARS {
            return Err(ModelError::InvalidParam(format!(
                "preset name `{trimmed}` is {char_count} characters, exceeding the maximum of {PRESET_MAX_NAME_CHARS} characters; shorten the name"
            )));
        }
        if self.effects.len() > MAX_PRESET_EFFECTS {
            return Err(ModelError::InvalidParam(format!(
                "preset `{trimmed}` has {} effects, exceeding the maximum of {MAX_PRESET_EFFECTS}",
                self.effects.len()
            )));
        }
        if self.audio_effects.len() > MAX_PRESET_AUDIO_EFFECTS {
            return Err(ModelError::InvalidParam(format!(
                "preset `{trimmed}` has {} audio effects, exceeding the maximum of {MAX_PRESET_AUDIO_EFFECTS}",
                self.audio_effects.len()
            )));
        }
        if self.properties.len() > MAX_PRESET_PROPERTIES {
            return Err(ModelError::InvalidParam(format!(
                "preset `{trimmed}` has {} properties, exceeding the maximum of {MAX_PRESET_PROPERTIES}",
                self.properties.len()
            )));
        }

        for (index, effect) in self.effects.iter().enumerate() {
            for (param, curve) in &effect.params {
                check_keyframe_limit(
                    curve.keyframes().len(),
                    format_args!(
                        "effect `{}` (index {index}) parameter `{param}`",
                        effect.effect_id
                    ),
                )?;
            }
            for (param, curve) in &effect.color_params {
                check_keyframe_limit(
                    curve.keyframes().len(),
                    format_args!(
                        "effect `{}` (index {index}) parameter `{param}`",
                        effect.effect_id
                    ),
                )?;
            }
            for (param, curve) in &effect.linear_color_params {
                check_keyframe_limit(
                    curve.keyframes().len(),
                    format_args!(
                        "effect `{}` (index {index}) parameter `{param}`",
                        effect.effect_id
                    ),
                )?;
            }
            for (param, curve) in &effect.vec2_params {
                check_keyframe_limit(
                    curve.keyframes().len(),
                    format_args!(
                        "effect `{}` (index {index}) parameter `{param}`",
                        effect.effect_id
                    ),
                )?;
            }
            for (param, curve) in &effect.vec3_params {
                check_keyframe_limit(
                    curve.keyframes().len(),
                    format_args!(
                        "effect `{}` (index {index}) parameter `{param}`",
                        effect.effect_id
                    ),
                )?;
            }
            effect.validate()?;
        }

        for (index, insert) in self.audio_effects.iter().enumerate() {
            for (param, curve) in &insert.params {
                check_keyframe_limit(
                    curve.keyframes().len(),
                    format_args!(
                        "audio effect `{}` (index {index}) parameter `{param}`",
                        insert.effect_id
                    ),
                )?;
            }
            insert.validate()?;
        }

        let mut seen_props = FxHashSet::default();
        for prop in &self.properties {
            let id = prop.id.as_str();
            if !seen_props.insert(id) {
                return Err(ModelError::InvalidParam(format!(
                    "property `{id}`: duplicate property entry in preset `{trimmed}`; list each property at most once"
                )));
            }
            let spec = property_row(id).ok_or_else(|| {
                ModelError::InvalidParam(format!(
                    "property `{id}`: unknown property id; choose a non-effect property from clip_param_catalog() or a mask row"
                ))
            })?;
            check_keyframe_limit(
                prop.curve.keyframes().len(),
                format_args!("property `{id}`"),
            )?;
            prop.curve
                .validate_shape()
                .map_err(|err| ModelError::InvalidParam(format!("property `{id}`: {err}")))?;
            if spec.param != ClipParam::Position
                && prop
                    .curve
                    .keyframes()
                    .iter()
                    .any(|kf| kf.tangents.is_some())
            {
                return Err(ModelError::InvalidParam(format!(
                    "property `{id}`: spatial tangents are only supported on position"
                )));
            }
            prop.curve.for_each_value(|value| {
                spec.param
                    .validate_value(*value)
                    .map_err(|err| ModelError::InvalidParam(format!("property `{id}`: {err}")))
            })?;
        }

        Ok(())
    }
}

fn check_keyframe_limit(len: usize, owner: std::fmt::Arguments<'_>) -> Result<(), ModelError> {
    if len > MAX_PRESET_KEYFRAMES {
        return Err(ModelError::InvalidParam(format!(
            "{owner}: curve has {len} keyframes, exceeding the maximum of {MAX_PRESET_KEYFRAMES}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::MediaSource;
    use crate::param::{Easing, Keyframe, SpatialTangents};
    use crate::project::Project;
    use crate::time::{Rational, RationalTime, TimeRange};
    use crate::track::TrackKind;

    fn fps24() -> Rational {
        Rational::new(24, 1)
    }

    fn t(ticks: i64) -> RationalTime {
        RationalTime::new(ticks, fps24())
    }

    fn range(start: i64, duration: i64) -> TimeRange {
        TimeRange::at_rate(start, duration, fps24())
    }

    fn project_with_two_video_clips() -> (Project, crate::ClipId, crate::ClipId) {
        let mut project = Project::new("Preset Test", fps24());
        let media = project.add_media(MediaSource::new(
            "/media/sample.mp4",
            1920,
            1080,
            fps24(),
            240,
            true,
        ));
        let track = project.add_track(TrackKind::Video, "V1");
        let clip_a = Clip::from_media(media, range(0, 100), range(0, 100));
        let clip_b = Clip::from_media(media, range(0, 100), range(100, 100));
        let id_a = clip_a.id;
        let id_b = clip_b.id;
        project.timeline_mut().add_clip(track, clip_a).unwrap();
        project.timeline_mut().add_clip(track, clip_b).unwrap();
        (project, id_a, id_b)
    }

    #[test]
    fn capture_and_apply_shifts_effects_audio_and_keyframes_relative_to_anchor() {
        let (mut project, clip_a, clip_b) = project_with_two_video_clips();

        // Add two visual effects (one animated), one audio effect (animated), and
        // keyframed position with spatial tangents at ticks 20 and 50 on clip A.
        let fx0 = project.add_effect(clip_a, "gaussian_blur").unwrap();
        project
            .set_param_keyframe(
                clip_a,
                ClipParam::Effect {
                    effect: fx0 as u32,
                    param: 0,
                },
                t(20),
                ParamValue::Scalar(8.0),
                Easing::Linear,
                None,
            )
            .unwrap();
        project
            .set_param_keyframe(
                clip_a,
                ClipParam::Effect {
                    effect: fx0 as u32,
                    param: 0,
                },
                t(50),
                ParamValue::Scalar(24.0),
                Easing::EaseOut,
                None,
            )
            .unwrap();
        project.add_effect(clip_a, "vignette").unwrap();

        let afx = project
            .add_audio_effect(crate::AudioInsertTarget::Clip(clip_a), "gain")
            .unwrap();
        let mut gain_curve = Param::Constant(0.0f32);
        gain_curve.set_keyframe(20, -6.0, Easing::Linear);
        gain_curve.set_keyframe(50, 3.0, Easing::EaseIn);
        project
            .set_audio_effect_param(crate::AudioInsertTarget::Clip(clip_a), afx, 0, gain_curve)
            .unwrap();

        let tangents = SpatialTangents {
            out_t: [0.25, -0.1],
            in_t: [-0.2, 0.15],
        };
        project
            .set_param_keyframe(
                clip_a,
                ClipParam::Position,
                t(20),
                ParamValue::Vec2([0.1, 0.2]),
                Easing::EaseInOut,
                Some(tangents),
            )
            .unwrap();
        project
            .set_param_keyframe(
                clip_a,
                ClipParam::Position,
                t(50),
                ParamValue::Vec2([0.8, 0.9]),
                Easing::Linear,
                None,
            )
            .unwrap();

        // Capture at playhead 30 (clip_a starts at 0, so anchor_local = 30).
        let source_clip = project.timeline().clip(clip_a).unwrap();
        let parts = PresetParts::all_of(source_clip);
        assert_eq!(parts.effects, vec![0, 1]);
        assert_eq!(parts.audio_effects, vec![0]);
        assert_eq!(parts.properties, vec!["position".to_string()]);

        let preset = Preset::capture("Whip Pan Blur", source_clip, 30, &parts).unwrap();
        let json = preset.to_json().unwrap();
        let parsed = Preset::parse(json.as_bytes()).unwrap();
        assert_eq!(parsed, preset);

        // Captured keyframes must be shifted by -30: 20 -> -10, 50 -> +20.
        let pos_kfs = parsed.properties[0].curve.keyframes();
        assert_eq!(pos_kfs.len(), 2);
        assert_eq!(pos_kfs[0].tick, -10);
        assert_eq!(pos_kfs[0].value, ParamValue::Vec2([0.1, 0.2]));
        assert_eq!(pos_kfs[0].easing, Easing::EaseInOut);
        assert_eq!(pos_kfs[0].tangents, Some(tangents));
        assert_eq!(pos_kfs[1].tick, 20);
        assert_eq!(pos_kfs[1].value, ParamValue::Vec2([0.8, 0.9]));

        // Clip B starts at timeline tick 100. Give it an existing effect so we
        // prove preset effects append rather than replace.
        project.add_effect(clip_b, "sharpen").unwrap();

        // Apply at timeline tick 110 -> local anchor 10 on clip B.
        // Keyframes (-10, +20) must land at local ticks (0, 30).
        project.apply_preset(clip_b, &parsed, t(110)).unwrap();

        let target = project.timeline().clip(clip_b).unwrap();
        assert_eq!(target.effects.len(), 3);
        assert_eq!(target.effects[0].effect_id, "sharpen");
        assert_eq!(target.effects[1].effect_id, "gaussian_blur");
        assert_eq!(target.effects[2].effect_id, "vignette");
        let blur_kfs = target.effects[1].params["radius"].keyframes();
        assert_eq!(blur_kfs[0].tick, 0);
        assert_eq!(blur_kfs[0].value, 8.0);
        assert_eq!(blur_kfs[1].tick, 30);
        assert_eq!(blur_kfs[1].value, 24.0);
        assert_eq!(blur_kfs[1].easing, Easing::EaseOut);

        assert_eq!(target.audio_effects.len(), 1);
        let gain_kfs = target.audio_effects[0].params["gain"].keyframes();
        assert_eq!(gain_kfs[0].tick, 0);
        assert_eq!(gain_kfs[0].value, -6.0);
        assert_eq!(gain_kfs[1].tick, 30);
        assert_eq!(gain_kfs[1].value, 3.0);

        let target_pos = target.transform.position.keyframes();
        assert_eq!(target_pos.len(), 2);
        assert_eq!(target_pos[0].tick, 0);
        assert_eq!(target_pos[0].value, [0.1, 0.2]);
        assert_eq!(target_pos[0].easing, Easing::EaseInOut);
        assert_eq!(target_pos[0].tangents, Some(tangents));
        assert_eq!(target_pos[1].tick, 30);
        assert_eq!(target_pos[1].value, [0.8, 0.9]);
    }

    #[test]
    fn constant_property_writes_constant_or_keyframe_when_target_already_animated() {
        let (mut project, clip_a, clip_b) = project_with_two_clips_for_constants();
        let preset = Preset::new(
            "Half Opacity",
            Vec::new(),
            Vec::new(),
            vec![PresetProperty {
                id: "opacity".into(),
                curve: Param::Constant(ParamValue::Scalar(0.5)),
            }],
        );

        // Clip A is not keyframed on opacity -> constant is set to 0.5.
        project.apply_preset(clip_a, &preset, t(15)).unwrap();
        assert_eq!(
            project.timeline().clip(clip_a).unwrap().transform.opacity,
            Param::Constant(0.5)
        );

        // Clip B is already keyframed on opacity at tick 0 -> applying constant
        // preset at timeline tick 125 (local tick 25) inserts a keyframe at 25.
        project
            .set_param_keyframe(
                clip_b,
                ClipParam::Opacity,
                t(100),
                ParamValue::Scalar(1.0),
                Easing::Linear,
                None,
            )
            .unwrap();
        project.apply_preset(clip_b, &preset, t(125)).unwrap();
        let kfs = project
            .timeline()
            .clip(clip_b)
            .unwrap()
            .transform
            .opacity
            .keyframes();
        assert_eq!(kfs.len(), 2);
        assert_eq!(kfs[0].tick, 0);
        assert_eq!(kfs[0].value, 1.0);
        assert_eq!(kfs[1].tick, 25);
        assert_eq!(kfs[1].value, 0.5);
    }

    fn project_with_two_clips_for_constants() -> (Project, crate::ClipId, crate::ClipId) {
        project_with_two_video_clips()
    }

    #[test]
    fn validation_and_apply_refuse_invalid_inputs_with_named_reasons_and_no_partial_mutation() {
        let (mut project, clip_a, _) = project_with_two_video_clips();

        // Unknown property id.
        let unknown_prop = Preset::new(
            "Bad Prop",
            Vec::new(),
            Vec::new(),
            vec![PresetProperty {
                id: "not-a-real-property".into(),
                curve: Param::Constant(ParamValue::Scalar(1.0)),
            }],
        );
        let err = unknown_prop.validate().unwrap_err().to_string();
        assert!(err.contains("property `not-a-real-property`"), "got: {err}");

        // Effect parameter id in properties is refused.
        let effect_prop = Preset::new(
            "Effect Row Prop",
            Vec::new(),
            Vec::new(),
            vec![PresetProperty {
                id: "effect-0-0".into(),
                curve: Param::Constant(ParamValue::Scalar(1.0)),
            }],
        );
        let err = effect_prop.validate().unwrap_err().to_string();
        assert!(err.contains("property `effect-0-0`"), "got: {err}");

        // Kind mismatch.
        let kind_mismatch = Preset::new(
            "Wrong Kind",
            Vec::new(),
            Vec::new(),
            vec![PresetProperty {
                id: "position".into(),
                curve: Param::Constant(ParamValue::Scalar(0.5)),
            }],
        );
        let err = kind_mismatch.validate().unwrap_err().to_string();
        assert!(
            err.contains("property `position`") && err.contains("vec2"),
            "got: {err}"
        );

        // Out-of-range value.
        let out_of_range = Preset::new(
            "Bad Range",
            Vec::new(),
            Vec::new(),
            vec![PresetProperty {
                id: "opacity".into(),
                curve: Param::Constant(ParamValue::Scalar(2.5)),
            }],
        );
        let err = out_of_range.validate().unwrap_err().to_string();
        assert!(err.contains("property `opacity`"), "got: {err}");

        // Stale/wrong version.
        let mut wrong_version = Preset::new("Stale", Vec::new(), Vec::new(), Vec::new());
        wrong_version.version = 2;
        let err = wrong_version.validate().unwrap_err().to_string();
        assert!(err.contains("version 2"), "got: {err}");

        // Oversized payload.
        let oversized = vec![b' '; (PRESET_MAX_BYTES as usize) + 1];
        let err = Preset::parse(&oversized).unwrap_err().to_string();
        assert!(err.contains(&PRESET_MAX_BYTES.to_string()), "got: {err}");

        // Empty name.
        let empty_name = Preset::new("   ", Vec::new(), Vec::new(), Vec::new());
        let err = empty_name.validate().unwrap_err().to_string();
        assert!(err.contains("empty"), "got: {err}");

        // Anchor outside clip.
        let valid = Preset::new(
            "Valid",
            vec![EffectInstance::new("vignette")],
            Vec::new(),
            Vec::new(),
        );
        let err = project
            .apply_preset(clip_a, &valid, t(500))
            .unwrap_err()
            .to_string();
        assert!(err.contains("outside clip"), "got: {err}");

        // Audio part on a video clip without sound is refused and leaves
        // existing effects untouched (all-or-nothing).
        let silent_media = project.add_media(MediaSource::new(
            "/media/silent.mp4",
            1920,
            1080,
            fps24(),
            240,
            false,
        ));
        let silent_track = project.add_track(TrackKind::Video, "V2");
        let silent_clip = Clip::from_media(silent_media, range(0, 48), range(0, 48));
        let silent_id = silent_clip.id;
        project
            .timeline_mut()
            .add_clip(silent_track, silent_clip)
            .unwrap();

        let mixed = Preset::new(
            "Mixed",
            vec![EffectInstance::new("vignette")],
            vec![AudioEffectInstance::new("gain")],
            Vec::new(),
        );
        let err = project.apply_preset(silent_id, &mixed, t(0)).unwrap_err();
        assert!(matches!(err, ModelError::ClipCarriesNoSound(id) if id == silent_id));
        assert!(
            project
                .timeline()
                .clip(silent_id)
                .unwrap()
                .effects
                .is_empty(),
            "failed apply_preset must not partially mutate the clip"
        );

        // Property not available on target clip (text-size on a media clip)
        // is refused naming the property, without appending the preset's effects.
        let unavailable_prop = Preset::new(
            "Text Size Preset",
            vec![EffectInstance::new("vignette")],
            Vec::new(),
            vec![PresetProperty {
                id: "text-size".into(),
                curve: Param::Keyframed {
                    keyframes: vec![Keyframe::new(0, ParamValue::Scalar(48.0), Easing::Linear)],
                },
            }],
        );
        let err = project
            .apply_preset(clip_a, &unavailable_prop, t(10))
            .unwrap_err()
            .to_string();
        assert!(err.contains("text-size"), "got: {err}");
        assert!(
            project.timeline().clip(clip_a).unwrap().effects.is_empty(),
            "failed apply_preset must not append effects when a property is refused"
        );
    }
}

#[cfg(test)]
#[path = "preset_mask_tests.rs"]
mod mask_tests;
