//! One constructed sample of every command variant.
//!
//! This is the command census. Anything that has to cover every command reads
//! it: the wire-format lock, the reach declaration's tests, and the editor's
//! own conformance suite, which lives in another crate and so cannot reach a
//! `#[cfg(test)]` helper.
//!
//! The census is library code rather than a test fixture because its
//! completeness is the contract. `the_census_and_the_registry_describe_the_same_commands`
//! in `tests/wire_format.rs` compares this list against the registry tables in
//! `spec_rows.rs`, which the compiler already keeps exhaustive over the axis,
//! so a command cannot exist without appearing here.

use std::path::PathBuf;

use crate::{
    AnimationRef, AnimationSlot, AudioLayout, AudioRole, BlendMode, CanvasAspect, ChromaKey,
    ClipId, ClipParam, ClipTransform, ColorAdjustments, CropRect, Easing, EditCommand,
    EffectInstance, EffectPreset, ExportFormat, Filter, Generator, LayerShadow, LayerStyles, Lut,
    MarkerColor, MarkerId, Mask, MaskKind, MediaId, MotionBlur, MulticamAngle, MulticamGroupId,
    Param, ParamValue, PiecewiseEasingPreset, ProjectCommand, Rational, RationalTime, Replaceable,
    SequenceId, SpatialTangents, StabilizeLevel, TemplateMeta, TemplatePick, TimeRange, TrackId,
    TrackKind, TrackMatte, VideoCodec,
};

const FPS: Rational = Rational::FPS_30;

fn t(value: i64) -> RationalTime {
    RationalTime::new(value, FPS)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS)
}

fn clip(raw: u64) -> ClipId {
    ClipId::from_raw(raw)
}

fn track(raw: u64) -> TrackId {
    TrackId::from_raw(raw)
}

fn media(raw: u64) -> MediaId {
    MediaId::from_raw(raw)
}

/// One sample per [`ProjectCommand`] variant, in declaration order.
pub fn every_project_command() -> Vec<ProjectCommand> {
    vec![
        ProjectCommand::Import {
            path: PathBuf::from("/media/clip.mp4"),
        },
        ProjectCommand::RemoveMedia { media: media(3) },
        ProjectCommand::Save {
            path: PathBuf::from("/projects/cut.aria"),
        },
        ProjectCommand::Open {
            path: PathBuf::from("/projects/cut.aria"),
        },
        ProjectCommand::Load {
            path: PathBuf::from("/projects/cut.aria"),
        },
        ProjectCommand::RelinkMedia {
            media: media(3),
            path: PathBuf::from("/media/moved.mp4"),
        },
        ProjectCommand::SetMediaLabel {
            media: media(3),
            label: Some(MarkerColor::Green),
        },
        ProjectCommand::Export {
            path: PathBuf::from("/out/final.mp4"),
            format: ExportFormat {
                video_codec: Some(VideoCodec::H265),
                bitrate_kbps: Some(8_000),
                ..ExportFormat::PROJECT
            },
        },
        ProjectCommand::SaveTemplate {
            path: PathBuf::from("/templates/intro.ariat"),
            meta: TemplateMeta::new("Intro"),
        },
        ProjectCommand::ApplyTemplate {
            path: PathBuf::from("/templates/intro.ariat"),
            picks: vec![
                TemplatePick {
                    path: PathBuf::from("/media/a.mp4"),
                    source_in: Some(t(30)),
                },
                TemplatePick {
                    path: PathBuf::from("/media/b.mp4"),
                    source_in: None,
                },
            ],
        },
    ]
}

/// One sample per [`EditCommand`] variant, in declaration order.
pub fn every_edit_command() -> Vec<EditCommand> {
    vec![
        EditCommand::AddTrack {
            kind: TrackKind::Video,
            name: "V2".into(),
            index: Some(1),
            pinned: false,
        },
        EditCommand::AddClip {
            track: track(1),
            media: media(2),
            source: tr(0, 90),
            start: t(30),
        },
        EditCommand::ExtractAudio {
            clip: clip(4),
            to_track: Some(track(2)),
        },
        EditCommand::DuplicateClip {
            clip: clip(4),
            to_track: track(2),
            start: t(120),
        },
        EditCommand::PasteRetimed {
            clip: clip(4),
            to_track: track(2),
            start: t(120),
            target_duration: Some(t(60)),
            target_speed: None,
        },
        EditCommand::PasteAttributes {
            source: clip(4),
            clip: clip(5),
        },
        EditCommand::FreezeFrame {
            clip: clip(4),
            at: t(45),
            duration: t(30),
        },
        EditCommand::BakeClip {
            clip: clip(4),
            media: media(2),
        },
        EditCommand::RestoreBakedClip { clip: clip(4) },
        EditCommand::AddGenerated {
            track: track(1),
            generator: Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            timeline: tr(0, 60),
        },
        EditCommand::SetGenerator {
            clip: clip(4),
            generator: Generator::Text {
                content: "Title".into(),
                style: Default::default(),
            },
        },
        EditCommand::SetClipMedia {
            clip: clip(4),
            media: media(2),
            source: tr(15, 45),
        },
        EditCommand::SetReplaceable {
            clip: clip(4),
            replaceable: Some(Replaceable::new(0)),
        },
        EditCommand::SetTextEditable {
            clip: clip(4),
            editable: true,
        },
        EditCommand::SetClipTransform {
            clip: clip(4),
            transform: ClipTransform {
                position: [0.25, -0.5],
                anchor_point: [0.5, 0.5],
                scale: 2.0.into(),
                rotation: 90.0,
                opacity: 0.75,
            },
            at: Some(t(45)),
        },
        EditCommand::SetParamKeyframe {
            clip: clip(4),
            param: ClipParam::Position,
            at: t(45),
            value: ParamValue::Vec2([0.25, -0.5]),
            easing: Easing::Linear,
            tangents: Some(SpatialTangents {
                out_t: [0.0, 0.5],
                in_t: [-0.25, 0.0],
            }),
        },
        EditCommand::RemoveParamKeyframe {
            clip: clip(4),
            param: ClipParam::Position,
            at: t(45),
        },
        EditCommand::ApplyEasingPreset {
            clip: clip(4),
            param: ClipParam::Scale,
            at: t(45),
            preset: PiecewiseEasingPreset::BounceOut,
        },
        EditCommand::SetParamConstant {
            clip: clip(4),
            param: ClipParam::Opacity,
            value: ParamValue::Scalar(0.5),
        },
        EditCommand::SetClipSpeed {
            clip: clip(4),
            speed: Rational::new(2, 1),
            reversed: true,
        },
        EditCommand::SetSpeedCurve {
            clip: clip(4),
            curve: Some(Param::Constant(1.0)),
        },
        EditCommand::SetClipPitch {
            clip: clip(4),
            preserve_pitch: true,
        },
        EditCommand::SetClipCrop {
            clip: clip(4),
            crop: CropRect {
                x: 0.25,
                y: 0.25,
                w: 0.5,
                h: 0.5,
            },
            flip_h: true,
            flip_v: false,
            at: None,
        },
        EditCommand::SetClipAudio {
            clip: clip(4),
            volume: Some(0.5),
            fade_in: t(15),
            fade_out: t(30),
        },
        EditCommand::SetClipDenoise {
            clip: clip(4),
            denoise: true,
        },
        EditCommand::SetClipLabel {
            clip: clip(4),
            label: Some(MarkerColor::Teal),
        },
        EditCommand::SetClipName {
            clip: clip(4),
            name: Some("Interview take 3".into()),
        },
        EditCommand::SetClipMask {
            clip: clip(4),
            mask: Some(Mask {
                kind: MaskKind::Circle,
                feather: 0.5.into(),
                invert: true,
                ..Mask::new(MaskKind::Circle)
            }),
        },
        EditCommand::SetClipBlendMode {
            clip: clip(4),
            mode: BlendMode::Multiply,
        },
        EditCommand::SetClipMotionBlur {
            clip: clip(4),
            motion_blur: MotionBlur {
                enabled: true,
                shutter_deg: 180.0,
                samples: 8,
            },
        },
        EditCommand::SetClipLayerStyles {
            clip: clip(4),
            styles: LayerStyles {
                shadow: Some(LayerShadow {
                    rgba: Param::Constant([0, 0, 0, 128]),
                    offset: Param::Constant([4.0, 4.0]),
                    blur: Param::Constant(8.0),
                }),
                ..Default::default()
            },
        },
        EditCommand::SetClipChroma {
            clip: clip(4),
            chroma: Some(ChromaKey {
                rgb: [0, 255, 0],
                strength: 0.7.into(),
                shadow: 0.2.into(),
            }),
        },
        EditCommand::SetClipMatte {
            clip: clip(4),
            matte: Some(TrackMatte {
                source: crate::MatteSource::Luma,
                invert: true,
                softness: 0.25.into(),
            }),
        },
        EditCommand::SetClipStabilize {
            clip: clip(4),
            stabilize: Some(StabilizeLevel::MaxSmooth),
        },
        EditCommand::SetClipFilter {
            clip: clip(4),
            filter: Some(Filter {
                id: "vivid".into(),
                intensity: 0.6.into(),
            }),
        },
        EditCommand::SetClipLut {
            clip: clip(4),
            lut: Some(Lut {
                path: "/luts/teal-orange.cube".into(),
                intensity: 0.7.into(),
            }),
        },
        EditCommand::SetClipAdjustments {
            clip: clip(4),
            adjust: ColorAdjustments {
                brightness: 0.1.into(),
                contrast: (-0.2).into(),
                saturation: 0.3.into(),
                exposure: 0.0.into(),
                temperature: (-0.4).into(),
                ..Default::default()
            },
        },
        EditCommand::SetClipAnimation {
            clip: clip(4),
            slot: AnimationSlot::In,
            animation: Some(AnimationRef::new("fade_in")),
        },
        EditCommand::SetAudioRole {
            clip: clip(4),
            role: Some(AudioRole::Voiceover),
        },
        EditCommand::AddEffect {
            clip: clip(4),
            effect_id: "gaussian_blur".into(),
        },
        EditCommand::RemoveEffect {
            clip: clip(4),
            index: 0,
        },
        EditCommand::MoveEffect {
            clip: clip(4),
            from_index: 0,
            to_index: 2,
        },
        EditCommand::SetEffectParam {
            clip: clip(4),
            index: 0,
            param: 0,
            value: 8.0,
        },
        EditCommand::ApplyEffectPreset {
            clip: clip(4),
            preset: EffectPreset::new("sample_blur", vec![EffectInstance::new("gaussian_blur")]),
        },
        EditCommand::AddTransition {
            clip: clip(4),
            transition_id: "crossfade".into(),
        },
        EditCommand::RemoveTransition { clip: clip(4) },
        EditCommand::SetTransition {
            clip: clip(4),
            duration: 15,
        },
        EditCommand::SplitClip {
            clip: clip(4),
            at: t(45),
        },
        EditCommand::TrimClip {
            clip: clip(4),
            timeline: tr(30, 60),
        },
        EditCommand::SlipClip {
            clip: clip(4),
            by: -12,
        },
        EditCommand::CreateBin {
            name: "Footage".into(),
            parent: Some(aria_models::BinId::from_raw(3)),
        },
        EditCommand::RenameBin {
            bin: aria_models::BinId::from_raw(3),
            name: "B roll".into(),
        },
        EditCommand::RemoveBin {
            bin: aria_models::BinId::from_raw(3),
        },
        EditCommand::MoveBin {
            bin: aria_models::BinId::from_raw(3),
            parent: None,
        },
        EditCommand::MoveMediaToBin {
            media: aria_models::MediaId::from_raw(7),
            bin: Some(aria_models::BinId::from_raw(3)),
        },
        EditCommand::SetPoolItemPosition {
            media: aria_models::MediaId::from_raw(7),
            bin: Some(aria_models::BinId::from_raw(3)),
            x: 100.0,
            y: 150.0,
        },
        EditCommand::CreateSubclip {
            parent: media(2),
            name: "Subclip 1".into(),
            source: tr(10, 50),
            bin: Some(aria_models::BinId::from_raw(3)),
        },
        EditCommand::CreateMulticamGroup {
            name: "Concert Multicam".into(),
            angles: vec![
                MulticamAngle::new(media(1), 0),
                MulticamAngle::new(media(2), 24),
            ],
        },
        EditCommand::AddMulticamClip {
            track: track(1),
            group: MulticamGroupId::from_raw(1),
            source: tr(0, 120),
            start: t(0),
        },
        EditCommand::SwitchMulticamAngle {
            group: MulticamGroupId::from_raw(1),
            at: 24,
            angle: 1,
        },
        EditCommand::SetMulticamSync {
            group: MulticamGroupId::from_raw(1),
            angle: 1,
            offset: -18,
        },
        EditCommand::MoveClip {
            clip: clip(4),
            to_track: track(2),
            start: t(90),
        },
        EditCommand::RippleMoveClip {
            clip: clip(4),
            to_track: track(2),
            start: t(90),
        },
        EditCommand::RemoveClip { clip: clip(4) },
        EditCommand::RemoveTrack { track: track(2) },
        EditCommand::MoveTrack {
            track: track(2),
            index: 1,
        },
        EditCommand::SetTrackName {
            track: track(2),
            name: "Dialogue".into(),
        },
        EditCommand::SetTrackEnabled {
            track: track(2),
            enabled: false,
        },
        EditCommand::SetTrackMuted {
            track: track(2),
            muted: true,
        },
        EditCommand::SetTrackSolo {
            track: track(2),
            solo: true,
        },
        EditCommand::SetTrackLocked {
            track: track(2),
            locked: true,
        },
        EditCommand::SetTrackDuckSource {
            track: track(2),
            duck_source: true,
        },
        EditCommand::SetTrackTargeted {
            track: track(2),
            targeted: false,
        },
        EditCommand::SetTrackSyncLock {
            track: track(2),
            sync_lock: false,
        },
        EditCommand::SetTrackDisplay {
            track: track(2),
            height: Some(96),
            waveform: Some(false),
            filmstrip: None,
            collapsed: Some(true),
        },
        EditCommand::SetTrackGain {
            track: track(2),
            gain: 0.75,
        },
        EditCommand::SetTrackOutput {
            track: track(2),
            to: Some(track(3)),
        },
        EditCommand::AddSend {
            track: track(2),
            to: track(3),
            level: 0.5,
            pre_fader: true,
        },
        EditCommand::SetSend {
            track: track(2),
            index: 0,
            level: Some(0.25),
            pre_fader: Some(false),
        },
        EditCommand::RemoveSend {
            track: track(2),
            index: 0,
        },
        EditCommand::RippleDelete { clip: clip(4) },
        EditCommand::ShiftClips {
            track: track(1),
            from: t(60),
            delta: t(-30),
        },
        EditCommand::RippleInsert {
            track: track(1),
            media: media(2),
            source: tr(0, 90),
            at: t(60),
        },
        EditCommand::OverwriteClip {
            track: track(1),
            media: media(2),
            source: tr(0, 90),
            at: t(60),
        },
        EditCommand::FitToFill {
            track: track(1),
            media: media(2),
            source: tr(0, 90),
            at: t(60),
            duration: t(45),
            ripple: true,
        },
        EditCommand::LinkClips {
            clips: vec![clip(4), clip(5)],
        },
        EditCommand::UnlinkClips {
            clips: vec![clip(4), clip(5)],
        },
        EditCommand::SetClipBeats {
            clip: clip(4),
            beats: vec![12, 24, 36],
        },
        EditCommand::ClearBeats { clip: clip(4) },
        EditCommand::AddMarker {
            at: t(45),
            name: "Beat drop".into(),
            color: Some(MarkerColor::Teal),
        },
        EditCommand::RemoveMarker {
            marker: MarkerId::from_raw(7),
        },
        EditCommand::SetMarker {
            marker: MarkerId::from_raw(7),
            at: t(60),
            name: "Chorus".into(),
            color: MarkerColor::Pink,
            comment: "hold for the vocal".into(),
        },
        EditCommand::SetCanvas {
            aspect: CanvasAspect::Tall9x16,
            background: [0, 0, 0],
            resolution: Some([1080, 1920]),
        },
        EditCommand::SetSequenceRate {
            frame_rate: Rational::FPS_30,
        },
        EditCommand::SetAudioLayout {
            layout: AudioLayout::Stereo,
        },
        EditCommand::SetProjectName {
            name: "Sunday cut".into(),
        },
        EditCommand::CreateNestedSequence {
            name: "Nested 1".into(),
            clips: vec![clip(4), clip(5)],
        },
        EditCommand::PlaceNestedSequence {
            track: track(1),
            sequence: SequenceId::from_raw(10),
            source: tr(0, 48),
            start: t(0),
        },
        EditCommand::OpenSequence {
            sequence: Some(SequenceId::from_raw(10)),
        },
    ]
}
