//! Why a menu row cannot run: one function per condition, shared by every row
//! that has it.
//!
//! A guard reads the state the row would act on and returns the reason the row
//! is drawn disabled, or `None` when the row can run. The reason is the
//! disabled row's tooltip, and the status line when the row is chosen anyway
//! from the keyboard or the palette. Each reads what the row's own code reads,
//! so a row is not drawn enabled over a state it would refuse.

use crate::app::AppState;
use crate::app::decline::Decline;

/// The row can always run.
pub(crate) fn ready(_: &str, _: &AppState) -> Option<&'static str> {
    None
}

/// The row acts on the selected clips.
pub(crate) fn selection(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .selection
        .is_empty()
        .then_some(Decline::NoSelection.reason())
}

/// The row opens the take of a media clip under the playhead, selected or not.
pub(crate) fn media_clip_at_playhead(_: &str, state: &AppState) -> Option<&'static str> {
    let at = state.playhead.value;
    let under = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .any(|clip| {
            clip.media_id.is_some()
                && clip.timeline_start_ticks <= at
                && at < clip.timeline_end_ticks
        });
    (!under).then_some(Decline::NoMediaClipAtPlayhead.reason())
}

/// The row acts on the selected clips or on the selected gap.
pub(crate) fn clip_or_gap(_: &str, state: &AppState) -> Option<&'static str> {
    (state.selection.is_empty() && state.selected_gap.is_none())
        .then_some(Decline::NoClipOrGap.reason())
}

/// The row acts on the selected clip's active mask.
pub(crate) fn mask(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .active_mask()
        .is_none()
        .then_some("The selected clip has no mask")
}

/// The row acts on the selected clip's color grade.
pub(crate) fn grade(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        state
            .selected_model()
            .is_none_or(|clip| clip.adjust == aria_models::ColorAdjustments::default())
            .then_some("The selected clip has no grade")
    })
}

/// The row acts on the selected clip's chroma key.
pub(crate) fn chroma(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        state
            .selected_model()
            .is_none_or(|clip| clip.chroma_key.is_none())
            .then_some("The selected clip has no chroma key")
    })
}

/// The row acts on selected clips that carry sound.
pub(crate) fn sound(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.selection_has_sound()).then_some("Select a clip with sound")
}

/// The row imports a file that is still where it was imported from.
pub(crate) fn import_recent(id: &str, state: &AppState) -> Option<&'static str> {
    let path = id.strip_prefix(super::menu_file::IMPORT_RECENT_PREFIX)?;
    state
        .import_recent_missing(path)
        .then_some("The file is missing")
}

/// The row empties an Import Recent list that holds a file.
pub(crate) fn import_recents(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .import_recents()
        .is_empty()
        .then_some("No file has been imported yet")
}

/// The row acts on two or more selected clips.
pub(crate) fn several(_: &str, state: &AppState) -> Option<&'static str> {
    (state.selection.len() < 2).then_some("Select two or more clips")
}

/// The row acts on a selection that is linked.
pub(crate) fn linked(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.selection_is_linked()).then_some("The selected clips are not linked")
}

/// The row acts on a selection that forms a clip group.
pub(crate) fn grouped(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.selection_is_grouped()).then_some("The selected clips are not a group")
}

/// The row links two or more selected clips that are not already one link
/// group.
pub(crate) fn linkable(id: &str, state: &AppState) -> Option<&'static str> {
    several(id, state).or_else(|| {
        state
            .selection_is_linked()
            .then_some(Decline::AlreadyLinked.reason())
    })
}

/// The row groups two or more selected clips that are not already one clip
/// group.
pub(crate) fn groupable(id: &str, state: &AppState) -> Option<&'static str> {
    several(id, state).or_else(|| {
        state
            .selection_is_grouped()
            .then_some(Decline::AlreadyGrouped.reason())
    })
}

/// The row remaps the time of a selected clip that plays source media.
pub(crate) fn remappable(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state)
        .or_else(|| (!state.selected_can_ramp()).then_some(Decline::NoSourceToRemap.reason()))
}

/// The row holds a frame of a selected clip that has video frames.
pub(crate) fn freezable(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state)
        .or_else(|| (!state.selected_can_freeze_frame()).then_some(Decline::NoFrameToHold.reason()))
}

/// The row moves a selected clip's sound to a lane of its own.
pub(crate) fn extractable(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        (!state.selected_can_extract_audio()).then_some(Decline::NoSoundToExtract.reason())
    })
}

/// The row sets the gain of the selected clips and their linked or grouped
/// companions that carry sound.
pub(crate) fn gain(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        state
            .selected_audio_clips()
            .is_empty()
            .then_some(Decline::NoSound.reason())
    })
}

/// The row pastes what Copy or Cut put on the clipboard.
pub(crate) fn clipboard(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .clipboard
        .is_none()
        .then_some(Decline::NothingCopied.reason())
}

/// The row steps back through the history.
pub(crate) fn undo(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.store.can_undo).then_some("Nothing to undo")
}

/// The row steps forward through the history.
pub(crate) fn redo(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.store.can_redo).then_some("Nothing to redo")
}

/// The row clears the In or Out point of the Source tab in front of the
/// Viewer, else of the sequence.
pub(crate) fn marks(_: &str, state: &AppState) -> Option<&'static str> {
    let (inn, out) = state.marks_here();
    (inn.is_none() && out.is_none()).then_some("Neither an In nor an Out point is set")
}

/// The row clears the In point of the Source tab in front of the Viewer, else
/// of the sequence.
pub(crate) fn mark_in(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .marks_here()
        .0
        .is_none()
        .then_some("No In point is set")
}

/// The row clears the Out point of the Source tab in front of the Viewer, else
/// of the sequence.
pub(crate) fn mark_out(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .marks_here()
        .1
        .is_none()
        .then_some("No Out point is set")
}

/// The row seeks to an In point. A Source tab with the transport keys goes to
/// its first frame without one; the sequence needs its In set.
pub(crate) fn go_in(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.source_has_keys() && state.mark_in.is_none()).then_some("No In point is set")
}

/// The row seeks to an Out point. A Source tab with the transport keys goes to
/// its last frame without one; the sequence needs its Out set.
pub(crate) fn go_out(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.source_has_keys() && state.mark_out.is_none()).then_some("No Out point is set")
}

/// The row acts on the sequence's markers.
pub(crate) fn markers(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .store
        .sequence
        .markers
        .is_empty()
        .then_some(Decline::NoMarkers.reason())
}

/// The row acts on the marker within a second of the playhead.
pub(crate) fn marker_at_playhead(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .marker_at_playhead()
        .is_none()
        .then_some(Decline::NoMarkerAtPlayhead.reason())
}

/// The row acts on the clips of the sequence.
pub(crate) fn clips(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .all(|track| track.clips.is_empty())
        .then_some("The sequence has no clips")
}

/// The row acts on the span between the In and Out points.
pub(crate) fn range(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .marked_range()
        .is_none()
        .then_some(Decline::NoMarkedRange.reason())
}

/// The row acts on the selected keyframes, or on the keyframes under the
/// playhead on the selected clip.
pub(crate) fn keyframes(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .ease_target_keyframes()
        .is_empty()
        .then_some(Decline::NoKeyframeTarget.reason())
}

/// The row sets the target keyframes that are not Linear to Linear.
pub(crate) fn keyframes_to_linear(_: &str, state: &AppState) -> Option<&'static str> {
    state.linear_targets().err().map(|why| why.reason())
}

/// The row acts on the property rows selected in the timeline outline.
pub(crate) fn property_rows(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .with_prop_rows(|rows| rows.graph_params.is_empty())
        .then_some("Select a property row in the timeline")
}

/// The row reverses selected keyframes that do not play the same reversed.
pub(crate) fn keyframes_to_reverse(_: &str, state: &AppState) -> Option<&'static str> {
    state.time_reverse_plan().err().map(|why| why.reason())
}

/// The row acts on a selected property row of the selected clip.
pub(crate) fn clip_property_row(_: &str, state: &AppState) -> Option<&'static str> {
    let clip = state.selected_clip();
    let picked = state.with_prop_rows(|rows| {
        rows.graph_params
            .iter()
            .any(|(owner, _)| Some(*owner) == clip)
    });
    (!picked).then_some("Select a property row of the selected clip")
}

/// The row selects the edit point Trim Edit finds.
pub(crate) fn edit_point(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .nearest_edit()
        .is_none()
        .then_some(Decline::NoEditPoint.reason())
}

/// The row moves the selected edit point to the playhead.
pub(crate) fn edge_to_extend(_: &str, state: &AppState) -> Option<&'static str> {
    state.extend_delta().err().map(|why| why.reason())
}

/// The row shifts the selected clip's lane one second earlier.
pub(crate) fn lane_shift_earlier(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .lane_shift(-state.one_second())
        .err()
        .map(|why| why.reason())
}

/// The row reopens the saved file over unsaved edits.
pub(crate) fn revertible(_: &str, state: &AppState) -> Option<&'static str> {
    if state.project_path.is_none() {
        return Some("The project has never been saved");
    }
    (!state.store.is_dirty).then_some("There are no changes since the last save")
}

/// The row acts on the marked range of the source in the Source monitor.
pub(crate) fn source_range(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .source_range()
        .is_none()
        .then_some(Decline::NoMonitorSource.reason())
}

/// The row places the media selected in the pool.
pub(crate) fn pool_selection(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.pool_selection_online()).then_some("No media is selected in the pool")
}

/// The row acts on the source in the Source monitor.
pub(crate) fn source(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .monitor_source()
        .is_none()
        .then_some(Decline::NoMonitorSource.reason())
}

/// The row repoints the selected clip at the Source monitor's frames.
pub(crate) fn source_replacement(_: &str, state: &AppState) -> Option<&'static str> {
    state.source_replacement().err().map(|why| why.reason())
}

/// The row resets a crop or a mirror on the selected clip.
pub(crate) fn framing(_: &str, state: &AppState) -> Option<&'static str> {
    state.framing_to_reset().err().map(|why| why.reason())
}

/// The row removes the selected clip's track matte.
pub(crate) fn matte(_: &str, state: &AppState) -> Option<&'static str> {
    state.matte_to_clear().err().map(|why| why.reason())
}

/// The row acts on the comparison the Viewer draws.
pub(crate) fn comparing(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.viewer_compare.comparing()).then_some("Nothing is being compared")
}

/// The row applies the effect applied last.
pub(crate) fn last_effect(_: &str, state: &AppState) -> Option<&'static str> {
    if state.last_effect.is_none() {
        return Some("No effect has been applied yet");
    }
    selection("", state)
}

/// The row pastes the copied effect onto the selected clips.
pub(crate) fn copied_effect(_: &str, state: &AppState) -> Option<&'static str> {
    if state.copied_effect.is_none() {
        return Some("No effect has been copied");
    }
    selection("", state)
}

/// The row acts on the effects of the selected clips.
pub(crate) fn effects(_: &str, state: &AppState) -> Option<&'static str> {
    if let reason @ Some(_) = selection("", state) {
        return reason;
    }
    let any = state.selection.iter().any(|clip| {
        state
            .ui_clip_ref(*clip)
            .is_some_and(|ui| !ui.effects.is_empty())
    });
    (!any).then_some("The selected clips have no effects")
}

/// The row opens a project from the recent list, by the path in its id.
pub(crate) fn recent(id: &str, state: &AppState) -> Option<&'static str> {
    let path = id.strip_prefix(super::menu_file::RECENT_PREFIX)?;
    state
        .recents()
        .iter()
        .any(|view| view.missing && view.path.as_os_str() == path)
        .then_some("The project file is missing")
}

/// The row exports through a timeline exporter.
pub(crate) fn exporters(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .ctx
        .exporters()
        .is_empty()
        .then_some("No timeline exporter is mounted")
}

/// The row deletes tracks that hold no clips.
pub(crate) fn empty_tracks(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .empty_track_ids()
        .is_empty()
        .then_some("Every track holds a clip")
}

/// The row steps out of the open folder in the media pool.
pub(crate) fn bin_up(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .current_bin
        .is_none()
        .then_some("Already at the top of the media pool")
}

/// The row seeks to the next or previous detected analysis moment.
pub(crate) fn moment(id: &str, state: &AppState) -> Option<&'static str> {
    if state.analysis.moments.is_empty() {
        return Some("No detected moments");
    }
    let now = state.store.playhead_ticks;
    if id == "pal-previous-moment" {
        (!state.analysis.moments.iter().any(|m| m.start_tick < now))
            .then_some("No detected moment before the playhead")
    } else {
        (!state.analysis.moments.iter().any(|m| m.start_tick > now))
            .then_some("No detected moment after the playhead")
    }
}

/// The row marks or unmarks the selected media clip as a template slot.
pub(crate) fn template_slot(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        (state.selected_slot().is_none() && !state.selected_can_be_slot())
            .then_some("Only media clips can be template slots")
    })
}

/// The row acts on a selected title clip.
pub(crate) fn title_clip(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state)
        .or_else(|| (!state.selected_is_text()).then_some("Select a title clip first"))
}

/// Why `param` cannot be keyframed at the playhead on the selected clip, or
/// `None` when it can.
pub(crate) fn keyframe_param_blocked(
    state: &AppState,
    param: aria_models::ClipParam,
) -> Option<&'static str> {
    let Some(clip) = state.selected_model() else {
        return Some(Decline::NoSelection.reason());
    };
    if state.playhead.value < clip.timeline.start.value
        || state.playhead.value >= clip.timeline.end_tick()
    {
        return Some("Move the playhead inside the selected clip");
    }
    let Some(row) = state.key_param(param) else {
        return Some("The selected clip does not have that property");
    };
    row.spec
        .sample(clip, clip.param_tick(state.playhead.value))
        .is_none()
        .then_some("The selected clip does not have that property")
}

/// The row sets a keyframe at the playhead on the property named by `id`.
pub(crate) fn keyframe_at_playhead(id: &str, state: &AppState) -> Option<&'static str> {
    let slug = id.strip_prefix("pal-key-")?;
    let param = aria_models::ClipParam::CENSUS.into_iter().find(
        |param| matches!(param.class(), aria_models::ClipParamClass::Direct(s) if s == slug),
    )?;
    keyframe_param_blocked(state, *param)
}

/// The row removes the opacity keyframe under the playhead on the selected clip.
pub(crate) fn opacity_keyframe_at_playhead(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        (!state.has_keyframe_at_playhead(aria_models::ClipParam::Opacity))
            .then_some("The selected clip has no opacity keyframe at the playhead")
    })
}

/// Why the selected clip's track cannot move one row up (`up`) or down, or
/// `None` when it can.
fn track_move(state: &AppState, up: bool) -> Option<&'static str> {
    let Some(track) = state.selected_track() else {
        return Some(Decline::NoSelection.reason());
    };
    let tracks = &state.store.sequence.tracks;
    let count = tracks.len();
    let Some(from_row) = tracks.iter().position(|lane| lane.id == track.raw()) else {
        return Some(Decline::NoSelection.reason());
    };
    let visual_count = tracks
        .iter()
        .take_while(|lane| {
            !matches!(
                lane.kind,
                crate::types::UiTrackKind::Audio | crate::types::UiTrackKind::Submix
            )
        })
        .count();
    let is_audio = from_row >= visual_count;
    if up {
        let top = if is_audio { visual_count } else { 0 };
        (from_row <= top).then_some("The track is already at the top")
    } else {
        let bottom = if is_audio { count } else { visual_count };
        (from_row + 1 >= bottom).then_some("The track is already at the bottom")
    }
}

/// The row moves the selected clip's track up one slot.
pub(crate) fn track_up(_: &str, state: &AppState) -> Option<&'static str> {
    track_move(state, true)
}

/// The row moves the selected clip's track down one slot.
pub(crate) fn track_down(_: &str, state: &AppState) -> Option<&'static str> {
    track_move(state, false)
}

/// The row flags or unflags the selected clip's audio track as the duck source.
pub(crate) fn audio_track(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        let Some(track) = state.selected_track() else {
            return Some(Decline::NoSelection.reason());
        };
        let is_audio =
            state.store.sequence.tracks.iter().any(|lane| {
                lane.id == track.raw() && lane.kind == crate::types::UiTrackKind::Audio
            });
        (!is_audio).then_some("Select a clip on an audio track")
    })
}

/// The row selects a clip under the playhead.
pub(crate) fn clip_under_playhead(_: &str, state: &AppState) -> Option<&'static str> {
    let at = state.store.playhead_ticks;
    let any = state.store.sequence.tracks.iter().any(|track| {
        track
            .clips
            .iter()
            .any(|clip| at >= clip.timeline_start_ticks && at < clip.timeline_end_ticks)
    });
    (!any).then_some("No clip under the playhead")
}

/// The row acts on the unlocked clips under the playhead on the targeted lanes.
pub(crate) fn targeted_clip_at_playhead(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .clips_at_playhead()
        .next()
        .is_none()
        .then_some(Decline::NoTargetedClipAtPlayhead.reason())
}

/// The row cuts the clips Add Edit takes at the playhead.
pub(crate) fn clip_to_cut(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .add_edit_targets(false)
        .is_empty()
        .then_some(Decline::NoClipToCut.reason())
}

/// The row puts the default picture transition on the cut Ctrl+T finds.
pub(crate) fn picture_cut(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .default_transition_cut(aria_models::TransitionMedia::Picture)
        .err()
        .map(|why| why.reason())
}

/// The row cuts every unlocked lane's clip at the playhead.
pub(crate) fn clip_to_cut_all(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .add_edit_targets(true)
        .is_empty()
        .then_some(Decline::NoClipToCut.reason())
}

/// The row duplicates the selected clip onto another track that accepts it.
pub(crate) fn other_compatible_track(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        let Some(clip) = state.selected_ui_clip_ref() else {
            return Some(Decline::NoSelection.reason());
        };
        let kind = state
            .store
            .sequence
            .tracks
            .iter()
            .find(|track| track.id == clip.track_id)
            .map(|track| track.kind);
        let any = state
            .store
            .sequence
            .tracks
            .iter()
            .any(|track| track.id != clip.track_id && Some(track.kind) == kind);
        (!any).then_some("No other track accepts this clip")
    })
}

/// The row names the pending undo and redo steps.
pub(crate) fn history_steps(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.store.can_undo && !state.store.can_redo).then_some("No steps to undo or redo")
}

/// The row reports the drawn size of a selected generated clip.
pub(crate) fn measurable_content(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        state
            .selected_ui_clip_ref()
            .is_none_or(|clip| clip.generator.is_none())
            .then_some("The selection draws no measurable content")
    })
}

/// The row clears the active media pool filter query.
pub(crate) fn media_filter(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .media_query
        .is_empty()
        .then_some("The pool filter is already empty")
}

/// The row clears active media proxies when any pool item uses one.
pub(crate) fn proxies_in_use(_: &str, state: &AppState) -> Option<&'static str> {
    (!state.store.media.iter().any(|item| item.proxied)).then_some("No proxies in use")
}

/// The row acts on the items in the media pool.
pub(crate) fn media_pool(_: &str, state: &AppState) -> Option<&'static str> {
    state.store.media.is_empty().then_some("The pool is empty")
}

/// The row reloads the focused media pool item from disk.
pub(crate) fn pool_focus(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .pool_selection
        .focus()
        .is_none()
        .then_some("Pick a pool item to reload")
}

/// The row reports the cost of the last rendered frame.
pub(crate) fn rendered_frame(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .frame_rows()
        .is_empty()
        .then_some("No frame has been rendered yet")
}

/// The row acts on the media pool item played by the selected clip.
pub(crate) fn selected_media_source(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        let has_item = state
            .selected_ui_clip_ref()
            .and_then(|clip| clip.media_id)
            .is_some_and(|media| state.store.media.iter().any(|item| item.id == media));
        (!has_item).then_some("Select a clip that plays a pool entry")
    })
}

/// The row acts on the audio stream of the media pool item played by the selected clip.
pub(crate) fn selected_audio_source(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        let Some(media) = state.selected_ui_clip_ref().and_then(|clip| clip.media_id) else {
            return Some("Select a clip that plays a pool entry");
        };
        let Some(item) = state.store.media.iter().find(|item| item.id == media) else {
            return Some("Select a clip that plays a pool entry");
        };
        (!item.has_audio).then_some(Decline::NoSound.reason())
    })
}

/// The row measures the frame currently shown in the Viewer.
pub(crate) fn picture_on_screen(_: &str, state: &AppState) -> Option<&'static str> {
    (state.last_frame.is_none() && state.preview_worker.shown_texture().is_none())
        .then_some("No frame on screen to measure")
}

/// The row normalizes the selected clip from a measured loudness contour.
pub(crate) fn normalisable(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        state
            .loudness_peak_dbfs()
            .is_none()
            .then_some("Measure loudness in the Analysis panel first, then normalize")
    })
}

/// The row runs shot detection on the selected clip's media.
pub(crate) fn detect_shots(_: &str, state: &AppState) -> Option<&'static str> {
    use crate::app::analysis::{AnalysisKind, AnalysisRefusal};
    match state.analysis_target(AnalysisKind::Shots) {
        Ok(_) => None,
        Err(AnalysisRefusal::Unselected) => Some(Decline::NoSelection.reason()),
        Err(AnalysisRefusal::Generated) => Some("A generated clip has no media file to analyze"),
        Err(AnalysisRefusal::Gone) => Some("The clip's media is no longer in the pool"),
        Err(AnalysisRefusal::Offline(_)) => Some("The clip's media is offline"),
        Err(AnalysisRefusal::Silent(_)) => Some(Decline::NoSound.reason()),
    }
}

/// The row runs audio analysis or loudness measurement on the selected clip.
pub(crate) fn detect_audio(_: &str, state: &AppState) -> Option<&'static str> {
    use crate::app::analysis::{AnalysisKind, AnalysisRefusal};
    match state.analysis_target(AnalysisKind::Silence) {
        Ok(_) => None,
        Err(AnalysisRefusal::Unselected) => Some(Decline::NoSelection.reason()),
        Err(AnalysisRefusal::Generated) => Some("A generated clip has no media file to analyze"),
        Err(AnalysisRefusal::Gone) => Some("The clip's media is no longer in the pool"),
        Err(AnalysisRefusal::Offline(_)) => Some("The clip's media is offline"),
        Err(AnalysisRefusal::Silent(_)) => Some(Decline::NoSound.reason()),
    }
}

/// The row transcribes the selected clip's audio when a model is installed.
pub(crate) fn transcribe(id: &str, state: &AppState) -> Option<&'static str> {
    detect_audio(id, state).or_else(|| {
        state
            .settings_form
            .installed_model
            .is_none()
            .then_some("Install a transcription model in Settings first")
    })
}

/// The row ducks the selected music clips under the flagged voice track.
pub(crate) fn duck(id: &str, state: &AppState) -> Option<&'static str> {
    selection(id, state).or_else(|| {
        state
            .duck_precheck()
            .is_err()
            .then_some("Flag a voice track and select an audio clip to duck")
    })
}

/// The row tracks the active mask forward from the playhead.
pub(crate) fn mask_track(_: &str, state: &AppState) -> Option<&'static str> {
    if state.selected_clip().is_none() {
        return Some("Select a clip with a mask to track");
    }
    if state.active_mask().is_none() {
        return Some("Draw a mask on the clip before tracking it");
    }
    let span = state
        .selected_ui_clip_ref()
        .map_or(0, |ui| ui.timeline_end_ticks - 1 - state.playhead.value);
    (span <= 0)
        .then_some("The playhead is at the end of the clip, so there is nothing ahead to track")
}

/// The row queries the configured AI endpoint when no probe is already in flight.
pub(crate) fn ai_idle(_: &str, state: &AppState) -> Option<&'static str> {
    state
        .ai_probe
        .busy
        .then_some("An AI endpoint check is already running")
}
