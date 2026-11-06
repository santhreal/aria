//! Seeded generator of valid and hostile [`EditCommand`] values.
//!
//! Enumerates [`edit_specs`] at run time so adding a variant to [`EditCommand`]
//! fails any test that drives this generator until the new variant is covered.
//! Combines live entity ids read from a [`Project`] with stale ids, negative and
//! extreme `i64` ticks, zero and negative durations, mismatched and invalid
//! frame rates, locked-lane targets, and non-finite floats.

#[path = "fuzz_tracks.rs"]
mod tracks;

use aria_models::{
    AnimationRef, AnimationSlot, AudioInsertTarget, AudioRole, BinId, BlendMode, ChromaKey, Clip,
    ClipAttribute, ClipId, ClipParam, ClipTransform, ColorAdjustments, CropRect, DriverCode,
    Easing, EffectInstance, Filter, Generator, Keyframe, LabelColor, LayerShadow, LayerStyles,
    LookParam, Lut, MarkerId, Mask, MaskKind, MaskMode, MatteSource, MediaId, MotionBlur,
    MulticamGroupId, Param, ParamValue, ParentKeep, PiecewiseEasingPreset, Preset, Project,
    Rational, RationalTime, Replaceable, SequenceId, Shape, SpatialTangents, StabilizeLevel,
    TimeRange, TrackId, TrackKind, TrackMatte, Transition, TransitionAlignment,
};

use crate::{EditCommand, edit_specs};

/// Deterministic SplitMix64 generator for reproducible command sequences.
#[derive(Debug, Clone)]
pub struct CommandRng {
    state: u64,
}

impl CommandRng {
    /// Create a generator from `seed`.
    pub const fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Next pseudo-random `u64`.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform index in `0..len` (returns `0` when `len == 0`).
    pub fn next_usize(&mut self, len: usize) -> usize {
        if len <= 1 {
            return 0;
        }
        (self.next_u64() as usize) % len
    }

    /// Next boolean.
    pub fn next_bool(&mut self) -> bool {
        (self.next_u64() & 1) != 0
    }

    /// Pick one element from a non-empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next_usize(items.len())]
    }

    /// Pick a timeline or source tick mixing valid frames with negative and
    /// extreme `i64` boundaries.
    pub fn tick(&mut self) -> i64 {
        const TICKS: &[i64] = &[
            0,
            1,
            12,
            15,
            24,
            30,
            45,
            48,
            60,
            90,
            120,
            240,
            -1,
            -12,
            -100,
            i64::MIN,
            i64::MIN + 1,
            i64::MAX - 120,
            i64::MAX - 1,
            i64::MAX,
        ];
        *self.pick(TICKS)
    }

    /// Pick a duration tick count mixing valid spans with zero, negative, and
    /// extreme `i64` values.
    pub fn duration_tick(&mut self) -> i64 {
        const DURS: &[i64] = &[
            1,
            12,
            24,
            30,
            45,
            48,
            60,
            90,
            120,
            0,
            -1,
            -30,
            i64::MIN,
            i64::MAX - 60,
            i64::MAX,
        ];
        *self.pick(DURS)
    }

    /// Pick a frame rate: usually `timeline_rate`, sometimes another valid or
    /// invalid rate.
    pub fn rate(&mut self, timeline_rate: Rational) -> Rational {
        match self.next_usize(8) {
            0..=4 => timeline_rate,
            5 => *self.pick(&[
                Rational::FPS_24,
                Rational::FPS_25,
                Rational::FPS_30,
                Rational::FPS_60,
            ]),
            6 => *self.pick(&[
                Rational::new(0, 1),
                Rational::new(24, 0),
                Rational::new(-24, 1),
                Rational::new(24, -1),
            ]),
            _ => *self.pick(&[
                Rational::new(1, 1000),
                Rational::new(i32::MAX, 1),
                Rational::new(1, i32::MAX),
                Rational::new(i32::MIN, 1),
            ]),
        }
    }

    /// Pick a [`RationalTime`] mixing valid timeline times with hostile ticks
    /// and mismatched rates.
    pub fn time(&mut self, timeline_rate: Rational) -> RationalTime {
        let rate = self.rate(timeline_rate);
        RationalTime::new(self.tick(), rate)
    }

    /// Pick a [`TimeRange`] mixing valid ranges with zero-length, negative, and
    /// overflowing spans.
    pub fn range(&mut self, timeline_rate: Rational) -> TimeRange {
        let rate = self.rate(timeline_rate);
        TimeRange::at_rate(self.tick(), self.duration_tick(), rate)
    }

    /// Pick an `f32` value including normal values, negative values, and
    /// non-finite edge cases.
    pub fn float(&mut self) -> f32 {
        const FLOATS: &[f32] = &[
            0.0,
            0.25,
            0.5,
            0.75,
            1.0,
            2.0,
            -0.25,
            -1.0,
            -10.0,
            100.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        *self.pick(FLOATS)
    }

    pub(crate) fn small_index_usize(&mut self) -> usize {
        *self.pick(&[0, 1, 2, 3, 16, usize::MAX])
    }

    pub(crate) fn small_index_u32(&mut self) -> u32 {
        *self.pick(&[0, 1, 2, 3, 16, u32::MAX])
    }

    pub(crate) fn clip_id(&mut self, project: &Project) -> ClipId {
        let live: Vec<ClipId> = project
            .timeline()
            .tracks_ordered()
            .flat_map(|t| t.clips_ordered().map(|c| c.id))
            .collect();
        if !live.is_empty() && self.next_usize(4) < 3 {
            *self.pick(&live)
        } else {
            ClipId::from_raw(*self.pick(&[0, 1, 999_999, u64::MAX]))
        }
    }

    pub(crate) fn clip_ids(&mut self, project: &Project) -> Vec<ClipId> {
        let count = *self.pick(&[0usize, 1, 2, 3]);
        (0..count).map(|_| self.clip_id(project)).collect()
    }

    /// A clipboard copy: a live clip as it is now, or a stand-alone clip under
    /// a stale id that plays a live or stale media id over hostile ranges. The
    /// stand-alone copy takes the stale id the draw picked rather than one
    /// from the process-wide counter, so one seed draws one command list.
    pub(crate) fn copied_clip(&mut self, project: &Project) -> Box<Clip> {
        let rate = project.timeline().frame_rate;
        let live = self.clip_id(project);
        if let Some(clip) = project.clip(live) {
            return Box::new(clip.clone());
        }
        let media = self.media_id(project);
        let mut clip = if self.next_bool() {
            Clip::from_media(media, self.range(rate), self.range(rate))
        } else {
            Clip::generated(self.generator(), self.range(rate))
        };
        clip.id = live;
        Box::new(clip)
    }

    pub(crate) fn track_id(&mut self, project: &Project) -> TrackId {
        let live = project.timeline().order();
        if !live.is_empty() && self.next_usize(4) < 3 {
            *self.pick(live)
        } else {
            TrackId::from_raw(*self.pick(&[0, 1, 999_999, u64::MAX]))
        }
    }

    pub(crate) fn media_id(&mut self, project: &Project) -> MediaId {
        let live: Vec<MediaId> = project.media_iter().map(|m| m.id).collect();
        if !live.is_empty() && self.next_usize(4) < 3 {
            *self.pick(&live)
        } else {
            MediaId::from_raw(*self.pick(&[0, 1, 999_999, u64::MAX]))
        }
    }

    pub(crate) fn bin_id(&mut self, project: &Project) -> BinId {
        let live: Vec<BinId> = project.bin_iter().map(|b| b.id).collect();
        if !live.is_empty() && self.next_usize(4) < 3 {
            *self.pick(&live)
        } else {
            BinId::from_raw(*self.pick(&[0, 1, 999_999, u64::MAX]))
        }
    }

    pub(crate) fn opt_bin_id(&mut self, project: &Project) -> Option<BinId> {
        if self.next_bool() {
            None
        } else {
            Some(self.bin_id(project))
        }
    }

    pub(crate) fn marker_id(&mut self, project: &Project) -> MarkerId {
        let live: Vec<MarkerId> = project.timeline().markers().iter().map(|m| m.id).collect();
        if !live.is_empty() && self.next_usize(4) < 3 {
            *self.pick(&live)
        } else {
            MarkerId::from_raw(*self.pick(&[0, 1, 999_999, u64::MAX]))
        }
    }

    pub(crate) fn multicam_id(&mut self, project: &Project) -> MulticamGroupId {
        let live: Vec<MulticamGroupId> = project.multicam_group_iter().map(|g| g.id).collect();
        if !live.is_empty() && self.next_usize(4) < 3 {
            *self.pick(&live)
        } else {
            MulticamGroupId::from_raw(*self.pick(&[0, 1, 999_999, u64::MAX]))
        }
    }

    pub(crate) fn sequence_id(&mut self, project: &Project) -> SequenceId {
        let live: Vec<SequenceId> = project.sequences().map(|s| s.id).collect();
        if !live.is_empty() && self.next_usize(4) < 3 {
            *self.pick(&live)
        } else {
            SequenceId::from_raw(*self.pick(&[0, 1, 999_999, u64::MAX]))
        }
    }

    pub(crate) fn track_kind(&mut self) -> TrackKind {
        *self.pick(&[
            TrackKind::Video,
            TrackKind::Audio,
            TrackKind::Text,
            TrackKind::Sticker,
            TrackKind::Effect,
            TrackKind::Filter,
            TrackKind::Adjustment,
            TrackKind::Submix,
        ])
    }

    pub(crate) fn clip_param(&mut self) -> ClipParam {
        match self.next_usize(12) {
            0 => ClipParam::Position,
            1 => ClipParam::AnchorPoint,
            2 => ClipParam::Scale,
            3 => ClipParam::Rotation,
            4 => ClipParam::Opacity,
            5 => ClipParam::Crop,
            6 => ClipParam::Volume,
            7 => ClipParam::Pan,
            8 => ClipParam::Speed,
            9 => ClipParam::Look {
                param: LookParam::AdjustBrightness,
            },
            10 => ClipParam::Mask {
                mask: self.small_index_u32(),
                param: aria_models::MaskParam::Feather,
            },
            _ => ClipParam::Effect {
                effect: self.small_index_u32(),
                param: self.small_index_u32(),
            },
        }
    }

    pub(crate) fn param_value(&mut self) -> ParamValue {
        match self.next_usize(4) {
            0 => ParamValue::Scalar(self.float()),
            1 => ParamValue::Vec2([self.float(), self.float()]),
            2 => ParamValue::Color([255, 128, 64, 255]),
            _ => ParamValue::Rect([self.float(), self.float(), self.float(), self.float()]),
        }
    }

    pub(crate) fn param_f32(&mut self) -> Param<f32> {
        if self.next_bool() {
            Param::Constant(self.float())
        } else {
            Param::Keyframed {
                keyframes: vec![
                    Keyframe::new(self.tick(), self.float(), Easing::Linear),
                    Keyframe::new(self.tick(), self.float(), Easing::EaseInOut),
                ],
            }
        }
    }

    pub(crate) fn audio_target(&mut self, project: &Project) -> AudioInsertTarget {
        match self.next_usize(3) {
            0 => AudioInsertTarget::Clip(self.clip_id(project)),
            1 => AudioInsertTarget::Track(self.track_id(project)),
            _ => AudioInsertTarget::Master,
        }
    }

    pub(crate) fn generator(&mut self) -> Generator {
        match self.next_usize(5) {
            0 => Generator::SolidColor {
                rgba: [255, 0, 128, 255],
            },
            1 => Generator::text(if self.next_bool() { "Title" } else { "" }),
            2 => Generator::shape(Shape::Ellipse, [0, 200, 100, 255]),
            3 => Generator::Adjustment,
            _ => Generator::Filter,
        }
    }
}

/// Build one fuzzed [`EditCommand`] per shipped [`edit_specs`] entry, in
/// registry order.
pub fn every_fuzzed_edit_command(rng: &mut CommandRng, project: &Project) -> Vec<EditCommand> {
    edit_specs()
        .iter()
        .map(|spec| {
            let cmd = random_edit_command_for(rng, project, spec.id);
            assert_eq!(
                cmd.spec().id,
                spec.id,
                "fuzz generator returned wrong variant for {}",
                spec.id
            );
            cmd
        })
        .collect()
}

/// Pick a random [`EditCommand`] variant from [`edit_specs`] and populate it
/// with fuzzed arguments.
pub fn random_edit_command(rng: &mut CommandRng, project: &Project) -> EditCommand {
    let specs = edit_specs();
    let spec = rng.pick(specs);
    let cmd = random_edit_command_for(rng, project, spec.id);
    debug_assert_eq!(cmd.spec().id, spec.id);
    cmd
}

/// Build a fuzzed [`EditCommand`] for the registry variant named `variant_id`.
///
/// Panics if `variant_id` is not handled, so any new [`EditCommand`] variant
/// added to [`edit_specs`] turns callers red until covered here.
pub fn random_edit_command_for(
    rng: &mut CommandRng,
    project: &Project,
    variant_id: &str,
) -> EditCommand {
    let rate = project.timeline().frame_rate;
    match variant_id {
        "AddTrack" => EditCommand::AddTrack {
            kind: rng.track_kind(),
            name: if rng.next_bool() {
                "Lane".into()
            } else {
                String::new()
            },
            index: rng.next_bool().then(|| rng.small_index_usize()),
            pinned: rng.next_bool(),
        },
        "AddClip" => EditCommand::AddClip {
            track: rng.track_id(project),
            media: rng.media_id(project),
            source: rng.range(rate),
            start: rng.time(rate),
        },
        "ExtractAudio" => EditCommand::ExtractAudio {
            clip: rng.clip_id(project),
            to_track: rng.next_bool().then(|| rng.track_id(project)),
        },
        "DuplicateClip" => EditCommand::DuplicateClip {
            clip: rng.clip_id(project),
            to_track: rng.track_id(project),
            start: rng.time(rate),
        },
        "PasteClip" => EditCommand::PasteClip {
            copy: rng.copied_clip(project),
            track: rng.track_id(project),
            start: rng.time(rate),
        },
        "PasteRetimed" => EditCommand::PasteRetimed {
            copy: rng.copied_clip(project),
            track: rng.track_id(project),
            start: rng.time(rate),
            target_duration: rng.next_bool().then(|| rng.time(rate)),
            target_speed: rng.next_bool().then(|| rng.rate(rate)),
        },
        "PasteAttributes" => EditCommand::PasteAttributes {
            copy: rng.copied_clip(project),
            clip: rng.clip_id(project),
        },
        "RemoveAttributes" => {
            let attributes = (0..rng.next_usize(4))
                .map(|_| *rng.pick(&ClipAttribute::ALL))
                .collect();
            EditCommand::RemoveAttributes {
                clip: rng.clip_id(project),
                attributes,
            }
        }
        "FreezeFrame" => EditCommand::FreezeFrame {
            clip: rng.clip_id(project),
            at: rng.time(rate),
            duration: rng.time(rate),
        },
        "BakeClip" => EditCommand::BakeClip {
            clip: rng.clip_id(project),
            media: rng.media_id(project),
        },
        "RestoreBakedClip" => EditCommand::RestoreBakedClip {
            clip: rng.clip_id(project),
        },
        "AddGenerated" => EditCommand::AddGenerated {
            track: rng.track_id(project),
            generator: rng.generator(),
            timeline: rng.range(rate),
        },
        "SetGenerator" => EditCommand::SetGenerator {
            clip: rng.clip_id(project),
            generator: rng.generator(),
        },
        "SetClipMedia" => EditCommand::SetClipMedia {
            clip: rng.clip_id(project),
            media: rng.media_id(project),
            source: rng.range(rate),
        },
        "SetReplaceable" => EditCommand::SetReplaceable {
            clip: rng.clip_id(project),
            replaceable: rng
                .next_bool()
                .then(|| Replaceable::new(rng.small_index_u32())),
        },
        "SetTextEditable" => EditCommand::SetTextEditable {
            clip: rng.clip_id(project),
            editable: rng.next_bool(),
        },
        "SetClipTransform" => EditCommand::SetClipTransform {
            clip: rng.clip_id(project),
            transform: ClipTransform {
                position: [rng.float(), rng.float()],
                anchor_point: [rng.float(), rng.float()],
                scale: rng.float().into(),
                rotation: rng.float(),
                opacity: rng.float(),
            },
            at: rng.next_bool().then(|| rng.time(rate)),
        },
        "SetClipParent" => EditCommand::SetClipParent {
            clip: rng.clip_id(project),
            parent: rng.next_bool().then(|| rng.clip_id(project)),
            at: rng.time(rate),
            keep: if rng.next_bool() {
                ParentKeep::Place
            } else {
                ParentKeep::Snap
            },
        },
        "SetParamKeyframe" => EditCommand::SetParamKeyframe {
            clip: rng.clip_id(project),
            param: rng.clip_param(),
            at: rng.time(rate),
            value: rng.param_value(),
            easing: *rng.pick(&[
                Easing::Linear,
                Easing::Hold,
                Easing::EaseIn,
                Easing::EaseOut,
                Easing::EaseInOut,
            ]),
            tangents: rng.next_bool().then(|| SpatialTangents {
                out_t: [rng.float(), rng.float()],
                in_t: [rng.float(), rng.float()],
            }),
        },
        "RemoveParamKeyframe" => EditCommand::RemoveParamKeyframe {
            clip: rng.clip_id(project),
            param: rng.clip_param(),
            at: rng.time(rate),
        },
        "ApplyEasingPreset" => EditCommand::ApplyEasingPreset {
            clip: rng.clip_id(project),
            param: rng.clip_param(),
            at: rng.time(rate),
            preset: *rng.pick(&[
                PiecewiseEasingPreset::BounceOut,
                PiecewiseEasingPreset::ElasticOut,
                PiecewiseEasingPreset::BackOut,
            ]),
        },
        "SetParamConstant" => EditCommand::SetParamConstant {
            clip: rng.clip_id(project),
            param: rng.clip_param(),
            value: rng.param_value(),
        },
        "SetParamDriver" => EditCommand::SetParamDriver {
            clip: rng.clip_id(project),
            param: rng.clip_param(),
            code: rng.next_bool().then(|| {
                DriverCode::new(
                    if rng.next_bool() {
                        "aria.expressions"
                    } else {
                        ""
                    },
                    if rng.next_bool() { "value" } else { "" },
                )
            }),
        },
        "SetClipSpeed" => EditCommand::SetClipSpeed {
            clip: rng.clip_id(project),
            speed: rng.rate(Rational::new(2, 1)),
            reversed: rng.next_bool(),
        },
        "SetSpeedCurve" => EditCommand::SetSpeedCurve {
            clip: rng.clip_id(project),
            curve: rng.next_bool().then(|| rng.param_f32()),
        },
        "SetClipPitch" => EditCommand::SetClipPitch {
            clip: rng.clip_id(project),
            preserve_pitch: rng.next_bool(),
        },
        "SetClipCrop" => EditCommand::SetClipCrop {
            clip: rng.clip_id(project),
            crop: CropRect {
                x: rng.float(),
                y: rng.float(),
                w: rng.float(),
                h: rng.float(),
            },
            flip_h: rng.next_bool(),
            flip_v: rng.next_bool(),
            at: rng.next_bool().then(|| rng.time(rate)),
        },
        "SetClipAudio" => EditCommand::SetClipAudio {
            clip: rng.clip_id(project),
            volume: rng.next_bool().then(|| rng.float()),
            fade_in: rng.time(rate),
            fade_out: rng.time(rate),
        },
        "SetClipDenoise" => EditCommand::SetClipDenoise {
            clip: rng.clip_id(project),
            denoise: rng.next_bool(),
        },
        "SetClipLabel" => EditCommand::SetClipLabel {
            clip: rng.clip_id(project),
            label: rng.next_bool().then(|| LabelColor::Teal),
        },
        "SetClipName" => EditCommand::SetClipName {
            clip: rng.clip_id(project),
            name: rng.next_bool().then(|| {
                if rng.next_bool() {
                    "Take".into()
                } else {
                    "   ".into()
                }
            }),
        },
        "SetClipType" => EditCommand::SetClipType {
            clip: rng.clip_id(project),
            clip_type: rng.next_bool().then(|| {
                if rng.next_bool() {
                    "com.example.card".into()
                } else {
                    "invalid space".into()
                }
            }),
        },
        "SetClipLineParam" => EditCommand::SetClipLineParam {
            clip: rng.clip_id(project),
            param: rng.next_bool().then(|| rng.clip_param()),
        },
        "AddClipMask" => EditCommand::AddClipMask {
            clip: rng.clip_id(project),
            mask: Mask {
                feather: rng.float().into(),
                opacity: rng.float().into(),
                expansion: rng.float().into(),
                invert: rng.next_bool(),
                ..Mask::new(*rng.pick(&[MaskKind::Rectangle, MaskKind::Circle]))
            },
        },
        "SetClipMask" => EditCommand::SetClipMask {
            clip: rng.clip_id(project),
            index: rng.small_index_u32(),
            mask: Mask {
                mode: *rng.pick(&[MaskMode::Add, MaskMode::Subtract, MaskMode::Intersect]),
                feather: rng.float().into(),
                opacity: rng.float().into(),
                expansion: rng.float().into(),
                ..Mask::new(MaskKind::Rectangle)
            },
        },
        "RemoveClipMask" => EditCommand::RemoveClipMask {
            clip: rng.clip_id(project),
            index: rng.small_index_u32(),
        },
        "MoveClipMask" => EditCommand::MoveClipMask {
            clip: rng.clip_id(project),
            from_index: rng.small_index_u32(),
            to_index: rng.small_index_u32(),
        },
        "SetClipBlendMode" => EditCommand::SetClipBlendMode {
            clip: rng.clip_id(project),
            mode: *rng.pick(&[BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen]),
        },
        "SetClipMotionBlur" => EditCommand::SetClipMotionBlur {
            clip: rng.clip_id(project),
            motion_blur: MotionBlur {
                enabled: rng.next_bool(),
                shutter_deg: rng.float(),
                samples: *rng.pick(&[0, 1, 8, 64, u32::MAX]),
            },
        },
        "SetClipLayerStyles" => EditCommand::SetClipLayerStyles {
            clip: rng.clip_id(project),
            styles: LayerStyles {
                shadow: rng.next_bool().then(|| LayerShadow {
                    rgba: Param::Constant([0, 0, 0, 128]),
                    offset: Param::Constant([rng.float(), rng.float()]),
                    blur: Param::Constant(rng.float()),
                }),
                ..Default::default()
            },
        },
        "SetClipChroma" => EditCommand::SetClipChroma {
            clip: rng.clip_id(project),
            chroma: rng.next_bool().then(|| ChromaKey {
                rgb: [0, 255, 0],
                strength: rng.float().into(),
                shadow: rng.float().into(),
            }),
        },
        "SetClipMatte" => EditCommand::SetClipMatte {
            clip: rng.clip_id(project),
            matte: rng.next_bool().then(|| TrackMatte {
                source: *rng.pick(&[MatteSource::Alpha, MatteSource::Luma]),
                invert: rng.next_bool(),
                softness: rng.float().into(),
            }),
        },
        "SetClipStabilize" => EditCommand::SetClipStabilize {
            clip: rng.clip_id(project),
            stabilize: rng.next_bool().then(|| StabilizeLevel::Recommended),
        },
        "SetClipFilter" => EditCommand::SetClipFilter {
            clip: rng.clip_id(project),
            filter: rng.next_bool().then(|| Filter {
                id: if rng.next_bool() {
                    "vivid".into()
                } else {
                    "unknown_filter".into()
                },
                intensity: rng.float().into(),
            }),
        },
        "SetClipLut" => EditCommand::SetClipLut {
            clip: rng.clip_id(project),
            lut: rng.next_bool().then(|| Lut {
                path: if rng.next_bool() {
                    "/luts/look.cube".into()
                } else {
                    String::new()
                },
                intensity: rng.float().into(),
            }),
        },
        "SetClipAdjustments" => EditCommand::SetClipAdjustments {
            clip: rng.clip_id(project),
            adjust: ColorAdjustments {
                brightness: rng.float().into(),
                contrast: rng.float().into(),
                saturation: rng.float().into(),
                exposure: rng.float().into(),
                temperature: rng.float().into(),
                ..Default::default()
            },
        },
        "SetClipAnimation" => EditCommand::SetClipAnimation {
            clip: rng.clip_id(project),
            slot: *rng.pick(&[AnimationSlot::In, AnimationSlot::Out, AnimationSlot::Combo]),
            animation: rng.next_bool().then(|| {
                AnimationRef::new(if rng.next_bool() {
                    "fade_in"
                } else {
                    "unknown_anim"
                })
            }),
        },
        "SetAudioRole" => EditCommand::SetAudioRole {
            clip: rng.clip_id(project),
            role: rng.next_bool().then(|| AudioRole::Voiceover),
        },
        "AddEffect" => EditCommand::AddEffect {
            clip: rng.clip_id(project),
            effect_id: if rng.next_bool() {
                "gaussian_blur".into()
            } else {
                "unknown_effect".into()
            },
        },
        "RemoveEffect" => EditCommand::RemoveEffect {
            clip: rng.clip_id(project),
            index: rng.small_index_usize(),
        },
        "MoveEffect" => EditCommand::MoveEffect {
            clip: rng.clip_id(project),
            from_index: rng.small_index_usize(),
            to_index: rng.small_index_usize(),
        },
        "SetEffectParam" => EditCommand::SetEffectParam {
            clip: rng.clip_id(project),
            index: rng.small_index_usize(),
            param: rng.small_index_usize(),
            value: rng.float(),
        },
        "SetEffectInstance" => EditCommand::SetEffectInstance {
            clip: rng.clip_id(project),
            index: rng.small_index_usize(),
            effect: EffectInstance::new(if rng.next_bool() {
                "gaussian_blur"
            } else {
                "unknown_effect"
            }),
        },
        "SetEffectBypass" => EditCommand::SetEffectBypass {
            clip: rng.clip_id(project),
            index: rng.small_index_usize(),
            bypass: rng.next_bool(),
        },
        "ApplyPreset" => EditCommand::ApplyPreset {
            clip: rng.clip_id(project),
            preset: Preset::new(
                if rng.next_bool() { "blur" } else { "" },
                vec![EffectInstance::new(if rng.next_bool() {
                    "gaussian_blur"
                } else {
                    "unknown_effect"
                })],
                Vec::new(),
                Vec::new(),
            ),
            at: rng.time(rate),
        },
        "AddTransition" => EditCommand::AddTransition {
            clip: rng.clip_id(project),
            transition_id: if rng.next_bool() {
                "crossfade".into()
            } else {
                "unknown_transition".into()
            },
        },
        "RemoveTransition" => EditCommand::RemoveTransition {
            clip: rng.clip_id(project),
        },
        "SetTransition" => EditCommand::SetTransition {
            clip: rng.clip_id(project),
            duration: rng.duration_tick(),
        },
        "SetTransitionAlignment" => EditCommand::SetTransitionAlignment {
            clip: rng.clip_id(project),
            alignment: *rng.pick(&[
                TransitionAlignment::Center,
                TransitionAlignment::Start,
                TransitionAlignment::End,
            ]),
        },
        "SetTransitionState" => EditCommand::SetTransitionState {
            clip: rng.clip_id(project),
            transition: Transition::new(
                rng.clip_id(project),
                rng.clip_id(project),
                if rng.next_bool() {
                    "crossfade"
                } else {
                    "unknown_transition"
                },
                rng.duration_tick(),
            ),
        },
        "SplitClip" => EditCommand::SplitClip {
            clip: rng.clip_id(project),
            at: rng.time(rate),
        },
        "TrimClip" => EditCommand::TrimClip {
            clip: rng.clip_id(project),
            timeline: rng.range(rate),
        },
        "TrimClipUnlinked" => EditCommand::TrimClipUnlinked {
            clip: rng.clip_id(project),
            timeline: rng.range(rate),
        },
        "SlipClip" => EditCommand::SlipClip {
            clip: rng.clip_id(project),
            by: rng.tick(),
        },
        other => tracks::random_track_or_project_command(rng, project, other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fuzzed_edit_command_covers_every_spec_and_is_deterministic() {
        let project = Project::new("Fuzz", Rational::FPS_24);
        let mut rng_a = CommandRng::new(42);
        let mut rng_b = CommandRng::new(42);
        let list_a = every_fuzzed_edit_command(&mut rng_a, &project);
        let list_b = every_fuzzed_edit_command(&mut rng_b, &project);
        assert_eq!(list_a.len(), edit_specs().len());
        assert_eq!(format!("{list_a:?}"), format!("{list_b:?}"));
        for (cmd, spec) in list_a.iter().zip(edit_specs()) {
            assert_eq!(cmd.spec().id, spec.id);
        }
    }
}
