//! Project the engine model into [`UiEditorStore`].

use crate::types::{
    UiAnimation, UiChroma, UiClip, UiClipKind, UiClipTransform, UiEditorStore, UiEffectInstance,
    UiFilter, UiGenerator, UiLut, UiMarker, UiMask, UiMatte, UiMediaItem, UiMulticamAngle,
    UiMulticamGroup, UiSequence, UiSequenceRef, UiSlot, UiStyles, UiTrack, UiTrackKind,
    UiTransition, clip_id, media_id, track_id,
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
    let timeline = project.timeline();
    let mut tracks: Vec<UiTrack> = timeline
        .tracks_ordered()
        .map(|track| track_to_ui(project, track, selection))
        .collect();
    tracks.reverse();

    UiEditorStore {
        project_name: project.name.clone().into(),
        is_dirty,
        can_undo,
        can_redo,
        playhead_ticks: playhead.value,
        zoom: 1.0,
        scroll_x: 0.0,
        selection: selection.iter().copied().map(clip_id).collect(),
        sequence: UiSequence {
            tracks,
            duration_ticks: timeline.duration().value,
            frame_rate: timeline.frame_rate,
            markers: timeline
                .markers()
                .iter()
                .map(|marker| UiMarker {
                    id: marker.id.raw(),
                    tick: marker.tick.value,
                    name: marker.name.clone().into(),
                    color: marker.color,
                    comment: marker.comment.clone().into(),
                })
                .collect(),
        },
        media: project
            .media_iter()
            .map(|m| media_to_ui(m, project))
            .collect(),
        bins: bins_to_ui(project),
        multicam: multicam_to_ui(project),
        sequences: project
            .sequences()
            .map(|sequence| UiSequenceRef {
                id: sequence.id.raw(),
                name: sequence.name.clone().into(),
                duration_ticks: sequence.timeline.duration().value,
            })
            .collect(),
        open_sequence: project.active_sequence().map(|id| id.raw()),
    }
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
#[path = "projection_clip.rs"]
mod clip;
#[path = "projection_effect.rs"]
mod effect;
use clip::{bins_to_ui, media_to_ui, track_to_ui};

/// Forget cached file metadata after an explicit filesystem boundary.
pub(crate) fn invalidate_media_statuses() {
    clip::invalidate_media_statuses();
}
