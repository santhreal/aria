//! Project the engine model into [`UiEditorStore`].

use crate::types::{
    UiAnimation, UiChroma, UiClip, UiClipKind, UiClipType, UiClips, UiEditorStore,
    UiEffectInstance, UiFilter, UiGenerator, UiLut, UiMarker, UiMask, UiMatte, UiMediaItem,
    UiMulticamAngle, UiMulticamGroup, UiSequence, UiSequenceRef, UiSlot, UiStyles, UiTrack,
    UiTrackKind, UiTransition, UiTypeHandle, clip_id, media_id, track_id,
};
use aria_models::{
    Clip, ClipAction, ClipCapabilities, ClipId, ClipSource, EffectInstance, Filter, Generator,
    MediaKind, MediaSource, Project, Track, TrackKind,
};
/// Convert engine project state into the GPUI view model.
pub fn project_to_ui(
    project: &Project,
    playhead: aria_models::RationalTime,
    selection: &[ClipId],
) -> UiEditorStore {
    project_to_ui_with(project, playhead, selection, false, false, false)
}
/// Same as [`project_to_ui`], with session flags the engine owns.
pub fn project_to_ui_with(
    project: &Project,
    playhead: aria_models::RationalTime,
    selection: &[ClipId],
    is_dirty: bool,
    can_undo: bool,
    can_redo: bool,
) -> UiEditorStore {
    project_to_ui_from(
        project, playhead, selection, is_dirty, can_undo, can_redo, None,
    )
    .0
}

/// Same as [`project_to_ui_with`], taking from `previous` the clips of every
/// lane that did not move since it was built (see [`lanes`]), and returning
/// the basis the next projection compares against.
pub(crate) fn project_to_ui_from(
    project: &Project,
    playhead: aria_models::RationalTime,
    selection: &[ClipId],
    is_dirty: bool,
    can_undo: bool,
    can_redo: bool,
    previous: Option<Previous<'_>>,
) -> (UiEditorStore, LaneBasis) {
    let timeline = project.timeline();
    let media: Vec<UiMediaItem> = project
        .media_iter()
        .map(|m| media_to_ui(m, project))
        .collect();
    let sequences: Vec<UiSequenceRef> = project
        .sequences()
        .map(|sequence| UiSequenceRef {
            id: sequence.id.raw(),
            name: sequence.name.clone().into(),
            duration_ticks: sequence.timeline.duration().value,
        })
        .collect();
    let multicam = multicam_to_ui(project);
    let globals = lanes::Globals::of(project, &media, &sequences, &multicam);
    let (drawn, basis) = lanes::lanes(project, selection, globals, previous);
    let mut tracks: Vec<UiTrack> = Vec::new();
    let mut audio_tracks: Vec<UiTrack> = Vec::new();
    for (track, ui) in timeline.tracks_ordered().zip(drawn) {
        if matches!(track.kind, TrackKind::Audio | TrackKind::Submix) {
            audio_tracks.push(ui);
        } else {
            tracks.push(ui);
        }
    }
    // Visual lanes stack upward from the A/V divider (V1 closest to the divider,
    // V2 above V1, V3 above V2); audio lanes stack downward from the divider
    // (A1 closest to the divider, A2 below A1, A3 below A2).
    tracks.reverse();
    tracks.extend(audio_tracks);

    let mut markers: Vec<UiMarker> = timeline
        .markers()
        .iter()
        .map(|marker| UiMarker {
            id: marker.id.raw(),
            tick: marker.tick.value,
            duration_ticks: marker.duration.value,
            name: marker.name.clone().into(),
            color: marker.color,
            comment: marker.comment.clone().into(),
            kind: marker.kind,
            clip_id: None,
            clip_tick: None,
            clip_duration_ticks: None,
            clip_rate: None,
        })
        .collect();
    if basis.clip_marker_count() > 0 {
        for track in &tracks {
            if basis.lane_has_clip_markers(track.id) {
                for clip in &track.clips {
                    markers.extend(clip.markers.iter().cloned());
                }
            }
        }
    }
    // A project file can list sequence markers out of order, and the ruler
    // stops at the first marker past the viewport. The sort is linear on
    // input that is already in order.
    markers.sort_by_key(|m| (m.tick, m.id));

    let mut sequence = UiSequence {
        tracks,
        duration_ticks: timeline.duration().value,
        frame_rate: timeline.frame_rate,
        markers: markers.into(),
        source_patching: timeline.source_patching(),
        time_display: timeline.time_display(),
    };
    sequence.mark_patched_lanes();

    let store = UiEditorStore {
        project_name: project.name.clone().into(),
        is_dirty,
        can_undo,
        can_redo,
        playhead_ticks: playhead.value,
        zoom: 1.0,
        scroll_x: 0.0,
        selection: selection.iter().copied().map(clip_id).collect(),
        sequence,
        media,
        bins: bins_to_ui(project),
        multicam,
        sequences,
        open_sequence: project.active_sequence().map(|id| id.raw()),
    };
    (store, basis)
}

/// Multicam groups in id order, each carrying its cut list.
///
/// The list, not a resolved angle: which camera is live depends on where the
/// playhead is, and the playhead moves without the project changing.
fn multicam_to_ui(project: &Project) -> std::sync::Arc<[UiMulticamGroup]> {
    project
        .multicam_group_iter()
        .map(|group| UiMulticamGroup {
            id: group.id.raw(),
            name: group.name.clone().into(),
            angles: group
                .angles
                .iter()
                .map(|angle| UiMulticamAngle {
                    media: angle.media.raw(),
                    name: angle.name.as_ref().map(|name| name.to_string().into()),
                    sync_offset: angle.sync_offset,
                })
                .collect(),
            cuts: group.cuts.to_vec(),
        })
        .collect()
}
#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;

// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
#[path = "projection_bar.rs"]
mod bar;
#[path = "projection_clip.rs"]
mod clip;
#[path = "projection_effect.rs"]
mod effect;
use clip::{bins_to_ui, media_to_ui};
#[path = "projection_lanes.rs"]
mod lanes;
pub(crate) use lanes::{LaneBasis, Previous, reselect};

/// Forget cached file metadata after an explicit filesystem boundary.
pub(crate) fn invalidate_media_statuses() {
    clip::invalidate_media_statuses();
}
#[cfg(test)]
pub(crate) use lanes::{built_clip_probes, reset_built_clip_probes};
