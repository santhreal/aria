//! Pool, lane, insert-chain, marker, sequence, and source-edit arms of the
//! seeded [`EditCommand`] generator.

use aria_models::{
    AudioLayout, AutomationMode, CanvasAspect, MarkerColor, MulticamAngle, Patch, Project,
    Rational, SoundPlacement, SourceChannel, TemporalEase, TimeDisplay,
};

use crate::EditCommand;
use crate::fuzz::CommandRng;

pub(super) fn random_track_or_project_command(
    rng: &mut CommandRng,
    project: &Project,
    variant_id: &str,
) -> EditCommand {
    let rate = project.timeline().frame_rate;
    match variant_id {
        "CreateBin" => EditCommand::CreateBin {
            name: if rng.next_bool() {
                "Footage".into()
            } else {
                String::new()
            },
            parent: rng.opt_bin_id(project),
        },
        "RenameBin" => EditCommand::RenameBin {
            bin: rng.bin_id(project),
            name: if rng.next_bool() {
                "B Roll".into()
            } else {
                String::new()
            },
        },
        "RemoveBin" => EditCommand::RemoveBin {
            bin: rng.bin_id(project),
        },
        "MoveBin" => EditCommand::MoveBin {
            bin: rng.bin_id(project),
            parent: rng.opt_bin_id(project),
        },
        "MoveMediaToBin" => EditCommand::MoveMediaToBin {
            media: rng.media_id(project),
            bin: rng.opt_bin_id(project),
        },
        "SetPoolItemPosition" => EditCommand::SetPoolItemPosition {
            media: rng.media_id(project),
            bin: rng.opt_bin_id(project),
            x: rng.float(),
            y: rng.float(),
        },
        "CreateSubclip" => EditCommand::CreateSubclip {
            parent: rng.media_id(project),
            name: if rng.next_bool() {
                "Subclip".into()
            } else {
                String::new()
            },
            source: rng.range(rate),
            bin: rng.opt_bin_id(project),
        },
        "CreateMulticamGroup" => {
            let count = *rng.pick(&[0usize, 1, 2]);
            let angles = (0..count)
                .map(|_| MulticamAngle::new(rng.media_id(project), rng.tick()))
                .collect();
            EditCommand::CreateMulticamGroup {
                name: if rng.next_bool() {
                    "Cam Group".into()
                } else {
                    String::new()
                },
                angles,
            }
        }
        "AddMulticamClip" => EditCommand::AddMulticamClip {
            track: rng.track_id(project),
            group: rng.multicam_id(project),
            source: rng.range(rate),
            start: rng.time(rate),
        },
        "SwitchMulticamAngle" => EditCommand::SwitchMulticamAngle {
            group: rng.multicam_id(project),
            at: rng.tick(),
            angle: rng.small_index_usize(),
        },
        "SetMulticamSync" => EditCommand::SetMulticamSync {
            group: rng.multicam_id(project),
            angle: rng.small_index_usize(),
            offset: rng.tick(),
        },
        "MoveClip" => EditCommand::MoveClip {
            clip: rng.clip_id(project),
            to_track: rng.track_id(project),
            start: rng.time(rate),
        },
        "MoveClipUnlinked" => EditCommand::MoveClipUnlinked {
            clip: rng.clip_id(project),
            to_track: rng.track_id(project),
            start: rng.time(rate),
        },
        "OverwriteMoveClip" => EditCommand::OverwriteMoveClip {
            clip: rng.clip_id(project),
            to_track: rng.track_id(project),
            start: rng.time(rate),
        },
        "OverwriteMoveClipUnlinked" => EditCommand::OverwriteMoveClipUnlinked {
            clip: rng.clip_id(project),
            to_track: rng.track_id(project),
            start: rng.time(rate),
        },
        "RippleMoveClip" => EditCommand::RippleMoveClip {
            clip: rng.clip_id(project),
            to_track: rng.track_id(project),
            start: rng.time(rate),
        },
        "RemoveClip" => EditCommand::RemoveClip {
            clip: rng.clip_id(project),
        },
        "RemoveTrack" => EditCommand::RemoveTrack {
            track: rng.track_id(project),
        },
        "MoveTrack" => EditCommand::MoveTrack {
            track: rng.track_id(project),
            index: rng.small_index_usize(),
        },
        "SetTrackName" => EditCommand::SetTrackName {
            track: rng.track_id(project),
            name: if rng.next_bool() {
                "Dialogue".into()
            } else {
                String::new()
            },
        },
        "SetTrackEnabled" => EditCommand::SetTrackEnabled {
            track: rng.track_id(project),
            enabled: rng.next_bool(),
        },
        "SetTrackMuted" => EditCommand::SetTrackMuted {
            track: rng.track_id(project),
            muted: rng.next_bool(),
        },
        "SetTrackSolo" => EditCommand::SetTrackSolo {
            track: rng.track_id(project),
            solo: rng.next_bool(),
        },
        "SetTrackLocked" => EditCommand::SetTrackLocked {
            track: rng.track_id(project),
            locked: rng.next_bool(),
        },
        "SetTrackDuckSource" => EditCommand::SetTrackDuckSource {
            track: rng.track_id(project),
            duck_source: rng.next_bool(),
        },
        "SetTrackTargeted" => EditCommand::SetTrackTargeted {
            track: rng.track_id(project),
            targeted: rng.next_bool(),
        },
        "SetTrackSyncLock" => EditCommand::SetTrackSyncLock {
            track: rng.track_id(project),
            sync_lock: rng.next_bool(),
        },
        "SetTrackDisplay" => EditCommand::SetTrackDisplay {
            track: rng.track_id(project),
            height: rng
                .next_bool()
                .then(|| *rng.pick(&[0, 20, 48, 200, u32::MAX])),
            waveform: rng.next_bool().then(|| rng.next_bool()),
            filmstrip: rng.next_bool().then(|| rng.next_bool()),
            collapsed: rng.next_bool().then(|| rng.next_bool()),
        },
        "SetTrackGain" => EditCommand::SetTrackGain {
            track: rng.track_id(project),
            gain: rng.float(),
        },
        "SetTrackPan" => EditCommand::SetTrackPan {
            track: rng.track_id(project),
            pan: rng.float(),
        },
        "SetTrackAutomation" => EditCommand::SetTrackAutomation {
            track: rng.track_id(project),
            mode: *rng.pick(&[
                AutomationMode::Off,
                AutomationMode::Read,
                AutomationMode::Touch,
                AutomationMode::Latch,
                AutomationMode::Write,
            ]),
            volume: rng.param_f32(),
            pan: rng.param_f32(),
        },
        "SetTrackOutput" => EditCommand::SetTrackOutput {
            track: rng.track_id(project),
            to: rng.next_bool().then(|| rng.track_id(project)),
        },
        "AddSend" => EditCommand::AddSend {
            track: rng.track_id(project),
            to: rng.track_id(project),
            level: rng.float(),
            pre_fader: rng.next_bool(),
        },
        "SetSend" => EditCommand::SetSend {
            track: rng.track_id(project),
            index: rng.small_index_usize(),
            level: rng.next_bool().then(|| rng.float()),
            pre_fader: rng.next_bool().then(|| rng.next_bool()),
        },
        "RemoveSend" => EditCommand::RemoveSend {
            track: rng.track_id(project),
            index: rng.small_index_usize(),
        },
        "AddAudioEffect" => EditCommand::AddAudioEffect {
            target: rng.audio_target(project),
            effect_id: if rng.next_bool() {
                "aria.compressor".into()
            } else {
                "unknown.audio.effect".into()
            },
        },
        "RemoveAudioEffect" => EditCommand::RemoveAudioEffect {
            target: rng.audio_target(project),
            index: rng.small_index_usize(),
        },
        "MoveAudioEffect" => EditCommand::MoveAudioEffect {
            target: rng.audio_target(project),
            from_index: rng.small_index_usize(),
            to_index: rng.small_index_usize(),
        },
        "SetAudioEffectParam" => EditCommand::SetAudioEffectParam {
            target: rng.audio_target(project),
            index: rng.small_index_usize(),
            param: rng.small_index_usize(),
            value: rng.param_f32(),
        },
        "SetAudioEffectBypass" => EditCommand::SetAudioEffectBypass {
            target: rng.audio_target(project),
            index: rng.small_index_usize(),
            bypass: rng.next_bool(),
        },
        "SetAudioEffectSidechain" => EditCommand::SetAudioEffectSidechain {
            target: rng.audio_target(project),
            index: rng.small_index_usize(),
            key: rng.next_bool().then(|| rng.track_id(project)),
        },
        "RippleDelete" => EditCommand::RippleDelete {
            clip: rng.clip_id(project),
        },
        "ShiftClips" => EditCommand::ShiftClips {
            track: rng.track_id(project),
            from: rng.time(rate),
            delta: rng.time(rate),
        },
        "OpenGap" => EditCommand::OpenGap {
            tracks: {
                let count = *rng.pick(&[0usize, 1, 2, 3]);
                (0..count).map(|_| rng.track_id(project)).collect()
            },
            at: rng.time(rate),
            duration: rng.time(rate),
        },
        "RippleInsert" => EditCommand::RippleInsert {
            track: rng.track_id(project),
            media: rng.media_id(project),
            source: rng.range(rate),
            at: rng.time(rate),
        },
        "OverwriteClip" => EditCommand::OverwriteClip {
            track: rng.track_id(project),
            media: rng.media_id(project),
            source: rng.range(rate),
            at: rng.time(rate),
        },
        "FitToFill" => EditCommand::FitToFill {
            track: rng.track_id(project),
            media: rng.media_id(project),
            source: rng.range(rate),
            at: rng.time(rate),
            duration: rng.time(rate),
            ripple: rng.next_bool(),
        },
        "LinkClips" => EditCommand::LinkClips {
            clips: rng.clip_ids(project),
        },
        "UnlinkClips" => EditCommand::UnlinkClips {
            clips: rng.clip_ids(project),
        },
        "SetClipsEnabled" => EditCommand::SetClipsEnabled {
            clips: rng.clip_ids(project),
            enabled: rng.next_bool(),
        },
        "GroupClips" => EditCommand::GroupClips {
            clips: rng.clip_ids(project),
        },
        "UngroupClips" => EditCommand::UngroupClips {
            clips: rng.clip_ids(project),
        },
        "SetClipBeats" => {
            let beats = (0..rng.next_usize(5)).map(|_| rng.tick()).collect();
            EditCommand::SetClipBeats {
                clip: rng.clip_id(project),
                beats,
            }
        }
        "ClearBeats" => EditCommand::ClearBeats {
            clip: rng.clip_id(project),
        },
        "AddMarker" => EditCommand::AddMarker {
            at: rng.time(rate),
            name: if rng.next_bool() {
                "Marker".into()
            } else {
                String::new()
            },
            color: rng.next_bool().then(|| MarkerColor::Teal),
        },
        "RemoveMarker" => EditCommand::RemoveMarker {
            marker: rng.marker_id(project),
        },
        "SetMarker" => EditCommand::SetMarker {
            marker: rng.marker_id(project),
            at: rng.time(rate),
            name: if rng.next_bool() {
                "Marker".into()
            } else {
                String::new()
            },
            color: MarkerColor::Pink,
            comment: if rng.next_bool() {
                "Note".into()
            } else {
                String::new()
            },
        },
        "SetCanvas" => EditCommand::SetCanvas {
            aspect: *rng.pick(&[
                CanvasAspect::Auto,
                CanvasAspect::Wide16x9,
                CanvasAspect::Tall9x16,
                CanvasAspect::Square1x1,
            ]),
            background: [0, 0, 0],
            resolution: rng.next_bool().then(|| {
                *rng.pick(&[
                    [1920, 1080],
                    [1080, 1920],
                    [0, 1080],
                    [1921, 1080],
                    [u32::MAX, u32::MAX],
                ])
            }),
        },
        "SetSequenceRate" => EditCommand::SetSequenceRate {
            frame_rate: rng.rate(Rational::FPS_24),
        },
        "SetAudioLayout" => EditCommand::SetAudioLayout {
            layout: *rng.pick(&[
                AudioLayout::Mono,
                AudioLayout::Stereo,
                AudioLayout::Surround51,
            ]),
        },
        "SetTimeDisplay" => EditCommand::SetTimeDisplay {
            display: *rng.pick(&[
                TimeDisplay::Timecode,
                TimeDisplay::Frames,
                TimeDisplay::FeetFrames,
            ]),
        },
        "SetMasterMix" => EditCommand::SetMasterMix {
            gain: rng.float(),
            pan: rng.float(),
            muted: rng.next_bool(),
            automation: *rng.pick(&[AutomationMode::Off, AutomationMode::Read]),
            volume: rng.param_f32(),
            pan_envelope: rng.param_f32(),
        },
        "SetProjectName" => EditCommand::SetProjectName {
            name: if rng.next_bool() {
                "Cut".into()
            } else {
                String::new()
            },
        },
        "CreateNestedSequence" => EditCommand::CreateNestedSequence {
            name: if rng.next_bool() {
                "Nest".into()
            } else {
                String::new()
            },
            clips: rng.clip_ids(project),
        },
        "PlaceNestedSequence" => EditCommand::PlaceNestedSequence {
            track: rng.track_id(project),
            sequence: rng.sequence_id(project),
            source: rng.range(rate),
            start: rng.time(rate),
        },
        "OpenSequence" => EditCommand::OpenSequence {
            sequence: rng.next_bool().then(|| rng.sequence_id(project)),
        },
        "CreateSequence" => EditCommand::CreateSequence {
            name: if rng.next_bool() {
                "Seq".into()
            } else {
                String::new()
            },
        },
        "SetKeyframeEase" => {
            let clip = rng.clip_id(project);
            let param = rng.keyed_param(project, clip);
            EditCommand::SetKeyframeEase {
                clip,
                param,
                at: rng.keyframe_time(project, clip, param, false),
                in_ease: rng.next_bool().then(|| TemporalEase::EASY_EASE),
                out_ease: rng.next_bool().then(|| TemporalEase::EASY_EASE),
                hold: rng.next_bool(),
            }
        }
        "SetKeyframeRoving" => {
            let clip = rng.clip_id(project);
            let param = rng.keyed_param(project, clip);
            EditCommand::SetKeyframeRoving {
                clip,
                param,
                at: rng.keyframe_time(project, clip, param, false),
                roving: rng.next_bool(),
            }
        }
        "SetSourcePatch" => EditCommand::SetSourcePatch {
            channel: *rng.pick(&[SourceChannel::Video, SourceChannel::Audio]),
            patch: match rng.next_usize(3) {
                0 => Patch::Off,
                1 => Patch::Auto,
                _ => Patch::Track(rng.track_id(project)),
            },
        },
        "SourceEdit" => EditCommand::SourceEdit {
            media: rng.media_id(project),
            source: rng.range(rate),
            at: rng.time(rate),
            fill: rng.next_bool().then(|| rng.time(rate)),
            ripple: rng.next_bool(),
            track: rng.next_bool().then(|| rng.track_id(project)),
            sound: match rng.next_usize(3) {
                0 => SoundPlacement::Off,
                1 => SoundPlacement::InPicture,
                _ => SoundPlacement::Lane(rng.track_id(project)),
            },
        },
        other => panic!("unhandled EditCommand variant in fuzz generator: {other}"),
    }
}
