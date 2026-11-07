//! Why a menu row cannot run: one function per condition, shared by every row
//! that has it.
//!
//! A guard reads the state the row would act on and returns the [`Decline`]
//! the row is drawn disabled for, or `None` when the row can run. Its reason is
//! the disabled row's tooltip, and the status line when the row is chosen
//! anyway from the keyboard or the palette. Each reads what the row's own code
//! reads, so a row is not drawn enabled over a state it would refuse. A guard
//! returns a `Decline` rather than text, so every reason a row is withheld for
//! is in `Decline::ALL` and its wording has one owner.

use crate::app::AppState;
use crate::app::decline::Decline;

/// The row can always run.
pub(crate) fn ready(_: &str, _: &AppState) -> Option<Decline> {
    None
}

/// The row acts on the selected clips.
pub(crate) fn selection(_: &str, state: &AppState) -> Option<Decline> {
    state.selection.is_empty().then_some(Decline::NoSelection)
}

/// The row opens the take of a media clip under the playhead, selected or not.
pub(crate) fn media_clip_at_playhead(_: &str, state: &AppState) -> Option<Decline> {
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
    (!under).then_some(Decline::NoMediaClipAtPlayhead)
}

/// The row acts on the selected clips or on the selected gap.
pub(crate) fn clip_or_gap(_: &str, state: &AppState) -> Option<Decline> {
    (state.selection.is_empty() && state.selected_gap.is_none()).then_some(Decline::NoClipOrGap)
}

/// The row acts on the selected clip's active mask.
pub(crate) fn mask(_: &str, state: &AppState) -> Option<Decline> {
    state.active_mask().is_none().then_some(Decline::NoMask)
}

/// The row acts on the selected clip's color grade.
pub(crate) fn grade(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        state
            .selected_model()
            .is_none_or(|clip| clip.adjust == aria_models::ColorAdjustments::default())
            .then_some(Decline::NoGrade)
    })
}

/// The row acts on the selected clip's chroma key.
pub(crate) fn chroma(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        state
            .selected_model()
            .is_none_or(|clip| clip.chroma_key.is_none())
            .then_some(Decline::NoChromaKey)
    })
}

/// The row acts on selected clips that carry sound.
pub(crate) fn sound(_: &str, state: &AppState) -> Option<Decline> {
    (!state.selection_has_sound()).then_some(Decline::SelectSound)
}

/// The row imports a file that is still where it was imported from.
pub(crate) fn import_recent(id: &str, state: &AppState) -> Option<Decline> {
    let path = id.strip_prefix(super::menu_file::IMPORT_RECENT_PREFIX)?;
    state
        .import_recent_missing(path)
        .then_some(Decline::ImportMissing)
}

/// The row empties an Import Recent list that holds a file.
pub(crate) fn import_recents(_: &str, state: &AppState) -> Option<Decline> {
    state
        .import_recents()
        .is_empty()
        .then_some(Decline::NoImports)
}

/// The row acts on two or more selected clips.
pub(crate) fn several(_: &str, state: &AppState) -> Option<Decline> {
    (state.selection.len() < 2).then_some(Decline::FewerThanTwo)
}

/// The row acts on a selection that is linked.
pub(crate) fn linked(_: &str, state: &AppState) -> Option<Decline> {
    (!state.selection_is_linked()).then_some(Decline::NotLinked)
}

/// The row acts on a selection that forms a clip group.
pub(crate) fn grouped(_: &str, state: &AppState) -> Option<Decline> {
    (!state.selection_is_grouped()).then_some(Decline::NotGrouped)
}

/// The row links two or more selected clips that are not already one link
/// group.
pub(crate) fn linkable(id: &str, state: &AppState) -> Option<Decline> {
    several(id, state).or_else(|| {
        state
            .selection_is_linked()
            .then_some(Decline::AlreadyLinked)
    })
}

/// The row groups two or more selected clips that are not already one clip
/// group.
pub(crate) fn groupable(id: &str, state: &AppState) -> Option<Decline> {
    several(id, state).or_else(|| {
        state
            .selection_is_grouped()
            .then_some(Decline::AlreadyGrouped)
    })
}

/// The row remaps the time of a selected clip that plays source media.
pub(crate) fn remappable(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state)
        .or_else(|| (!state.selected_can_ramp()).then_some(Decline::NoSourceToRemap))
}

/// The row holds a frame of a selected clip that has video frames.
pub(crate) fn freezable(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state)
        .or_else(|| (!state.selected_can_freeze_frame()).then_some(Decline::NoFrameToHold))
}

/// The row moves a selected clip's sound to a lane of its own: the clip
/// carries sound it can extract, and shares its link with no other clip.
pub(crate) fn extractable(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        let why = match state.selected_ui_clip_ref() {
            Some(ui) if ui.can_extract_audio => return None,
            Some(ui) if ui.extract_needs_unlink => Decline::LinkedSound,
            _ => Decline::NoSoundToExtract,
        };
        Some(why)
    })
}

/// The row sets the gain of the selected clips and their linked or grouped
/// companions that carry sound.
pub(crate) fn gain(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        state
            .selected_audio_clips()
            .is_empty()
            .then_some(Decline::NoSound)
    })
}

/// The row pastes what Copy or Cut put on the clipboard.
pub(crate) fn clipboard(_: &str, state: &AppState) -> Option<Decline> {
    state.clipboard.is_none().then_some(Decline::NothingCopied)
}

/// The row steps back through the history.
pub(crate) fn undo(_: &str, state: &AppState) -> Option<Decline> {
    (!state.store.can_undo).then_some(Decline::NothingToUndo)
}

/// The row steps forward through the history.
pub(crate) fn redo(_: &str, state: &AppState) -> Option<Decline> {
    (!state.store.can_redo).then_some(Decline::NothingToRedo)
}

/// The row clears the In or Out point of the Source tab in front of the
/// Viewer, else of the sequence.
pub(crate) fn marks(_: &str, state: &AppState) -> Option<Decline> {
    let (inn, out) = state.marks_here();
    (inn.is_none() && out.is_none()).then_some(Decline::NoMarks)
}

/// The row clears the In point of the Source tab in front of the Viewer, else
/// of the sequence.
pub(crate) fn mark_in(_: &str, state: &AppState) -> Option<Decline> {
    state.marks_here().0.is_none().then_some(Decline::NoInPoint)
}

/// The row clears the Out point of the Source tab in front of the Viewer, else
/// of the sequence.
pub(crate) fn mark_out(_: &str, state: &AppState) -> Option<Decline> {
    state
        .marks_here()
        .1
        .is_none()
        .then_some(Decline::NoOutPoint)
}

/// The row seeks to an In point. A Source tab with the transport keys goes to
/// its first frame without one; the sequence needs its In set.
pub(crate) fn go_in(_: &str, state: &AppState) -> Option<Decline> {
    (!state.source_has_keys() && state.mark_in.is_none()).then_some(Decline::NoInPoint)
}

/// The row seeks to an Out point. A Source tab with the transport keys goes to
/// its last frame without one; the sequence needs its Out set.
pub(crate) fn go_out(_: &str, state: &AppState) -> Option<Decline> {
    (!state.source_has_keys() && state.mark_out.is_none()).then_some(Decline::NoOutPoint)
}

/// The row acts on the sequence's markers.
pub(crate) fn markers(_: &str, state: &AppState) -> Option<Decline> {
    state
        .store
        .sequence
        .markers
        .is_empty()
        .then_some(Decline::NoMarkers)
}

/// The row acts on the marker within a second of the playhead.
pub(crate) fn marker_at_playhead(_: &str, state: &AppState) -> Option<Decline> {
    state
        .marker_at_playhead()
        .is_none()
        .then_some(Decline::NoMarkerAtPlayhead)
}

/// The row acts on the clips of the sequence.
pub(crate) fn clips(_: &str, state: &AppState) -> Option<Decline> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .all(|track| track.clips.is_empty())
        .then_some(Decline::NoClips)
}

/// The row acts on the span between the In and Out points.
pub(crate) fn range(_: &str, state: &AppState) -> Option<Decline> {
    state
        .marked_range()
        .is_none()
        .then_some(Decline::NoMarkedRange)
}

/// The row acts on the selected keyframes, or on the keyframes under the
/// playhead on the selected clip.
pub(crate) fn keyframes(_: &str, state: &AppState) -> Option<Decline> {
    state
        .ease_target_keyframes()
        .is_empty()
        .then_some(Decline::NoKeyframeTarget)
}

/// The row sets the target keyframes that are not Linear to Linear.
pub(crate) fn keyframes_to_linear(_: &str, state: &AppState) -> Option<Decline> {
    state.linear_targets().err()
}

/// The row reverses selected keyframes that do not play the same reversed.
pub(crate) fn keyframes_to_reverse(_: &str, state: &AppState) -> Option<Decline> {
    state.time_reverse_plan().err()
}

/// The row acts on a selected property row of the selected clip.
pub(crate) fn clip_property_row(_: &str, state: &AppState) -> Option<Decline> {
    let clip = state.selected_clip();
    let picked = state.with_prop_rows(|rows| {
        rows.graph_params
            .iter()
            .any(|(owner, _)| Some(*owner) == clip)
    });
    (!picked).then_some(Decline::NoPropertyRow)
}

/// The row selects the edit point Trim Edit finds.
pub(crate) fn edit_point(_: &str, state: &AppState) -> Option<Decline> {
    state
        .nearest_edit()
        .is_none()
        .then_some(Decline::NoEditPoint)
}

/// The row moves the selected edit point to the playhead.
pub(crate) fn edge_to_extend(_: &str, state: &AppState) -> Option<Decline> {
    state.extend_delta().err()
}

/// The row shifts the selected clip's lane one second earlier.
pub(crate) fn lane_shift_earlier(_: &str, state: &AppState) -> Option<Decline> {
    state.lane_shift(-state.one_second()).err()
}

/// The row reopens the saved file over unsaved edits.
pub(crate) fn revertible(_: &str, state: &AppState) -> Option<Decline> {
    if state.project_path.is_none() {
        return Some(Decline::NeverSaved);
    }
    (!state.store.is_dirty).then_some(Decline::NoUnsavedChanges)
}

/// The row acts on the marked range of the source in the Source monitor.
pub(crate) fn source_range(_: &str, state: &AppState) -> Option<Decline> {
    state
        .source_range()
        .is_none()
        .then_some(Decline::NoMonitorSource)
}

/// The row places the media selected in the pool.
pub(crate) fn pool_selection(_: &str, state: &AppState) -> Option<Decline> {
    (!state.pool_selection_online()).then_some(Decline::NoPoolSelection)
}

/// The row acts on the source in the Source monitor.
pub(crate) fn source(_: &str, state: &AppState) -> Option<Decline> {
    state
        .monitor_source()
        .is_none()
        .then_some(Decline::NoMonitorSource)
}

/// The row repoints the selected clip at the Source monitor's frames.
pub(crate) fn source_replacement(_: &str, state: &AppState) -> Option<Decline> {
    state.source_replacement().err()
}

/// The row resets a crop or a mirror on the selected clip.
pub(crate) fn framing(_: &str, state: &AppState) -> Option<Decline> {
    state.framing_to_reset().err()
}

/// The row removes the selected clip's track matte.
pub(crate) fn matte(_: &str, state: &AppState) -> Option<Decline> {
    state.matte_to_clear().err()
}

/// The row acts on the comparison the Viewer draws.
pub(crate) fn comparing(_: &str, state: &AppState) -> Option<Decline> {
    (!state.viewer_compare.comparing()).then_some(Decline::NotComparing)
}

/// The row applies the effect applied last.
pub(crate) fn last_effect(_: &str, state: &AppState) -> Option<Decline> {
    if state.last_effect.is_none() {
        return Some(Decline::NoEffectApplied);
    }
    selection("", state)
}

/// The row pastes the copied effect onto the selected clips.
pub(crate) fn copied_effect(_: &str, state: &AppState) -> Option<Decline> {
    if state.copied_effect.is_none() {
        return Some(Decline::NoEffectCopied);
    }
    selection("", state)
}

/// The row acts on the effects of the selected clips.
pub(crate) fn effects(_: &str, state: &AppState) -> Option<Decline> {
    if let reason @ Some(_) = selection("", state) {
        return reason;
    }
    let any = state.selection.iter().any(|clip| {
        state
            .ui_clip_ref(*clip)
            .is_some_and(|ui| !ui.effects.is_empty())
    });
    (!any).then_some(Decline::NoEffects)
}

/// The row opens a project from the recent list, by the path in its id.
pub(crate) fn recent(id: &str, state: &AppState) -> Option<Decline> {
    let path = id.strip_prefix(super::menu_file::RECENT_PREFIX)?;
    state
        .recents()
        .iter()
        .any(|view| view.missing && view.path.as_os_str() == path)
        .then_some(Decline::ProjectMissing)
}

/// The row exports through a timeline exporter.
pub(crate) fn exporters(_: &str, state: &AppState) -> Option<Decline> {
    state
        .ctx
        .exporters()
        .is_empty()
        .then_some(Decline::NoExporter)
}

/// The row imports through a timeline importer.
pub(crate) fn importers(_: &str, state: &AppState) -> Option<Decline> {
    state
        .ctx
        .importers()
        .is_empty()
        .then_some(Decline::NoImporter)
}

/// The row deletes tracks that hold no clips.
pub(crate) fn empty_tracks(_: &str, state: &AppState) -> Option<Decline> {
    state
        .empty_track_ids()
        .is_empty()
        .then_some(Decline::NoEmptyTrack)
}

/// The row steps out of the open folder in the media pool.
pub(crate) fn bin_up(_: &str, state: &AppState) -> Option<Decline> {
    state.current_bin.is_none().then_some(Decline::AtPoolTop)
}

/// The row seeks to the next or previous detected analysis moment.
pub(crate) fn moment(id: &str, state: &AppState) -> Option<Decline> {
    if state.analysis.moments.is_empty() {
        return Some(Decline::NoMoments);
    }
    let now = state.store.playhead_ticks;
    if id == "pal-previous-moment" {
        (!state.analysis.moments.iter().any(|m| m.start_tick < now))
            .then_some(Decline::NoMomentBefore)
    } else {
        (!state.analysis.moments.iter().any(|m| m.start_tick > now))
            .then_some(Decline::NoMomentAfter)
    }
}

/// The row marks or unmarks the selected media clip as a template slot.
pub(crate) fn template_slot(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        (state.selected_slot().is_none() && !state.selected_can_be_slot())
            .then_some(Decline::NotSlotMedia)
    })
}

/// The row acts on a selected title clip.
pub(crate) fn title_clip(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| (!state.selected_is_text()).then_some(Decline::NotATitle))
}

/// Why `param` cannot be keyframed at the playhead on the selected clip, or
/// `None` when it can.
pub(crate) fn keyframe_param_blocked(
    state: &AppState,
    param: aria_models::ClipParam,
) -> Option<Decline> {
    let Some(clip) = state.selected_model() else {
        return Some(Decline::NoSelection);
    };
    if state.playhead.value < clip.timeline.start.value
        || state.playhead.value >= clip.timeline.end_tick()
    {
        return Some(Decline::PlayheadOutsideClip);
    }
    let Some(row) = state.key_param(param) else {
        return Some(Decline::NoSuchProperty);
    };
    row.spec
        .sample(clip, clip.param_tick(state.playhead.value))
        .is_none()
        .then_some(Decline::NoSuchProperty)
}

/// The row sets a keyframe at the playhead on the property named by `id`.
pub(crate) fn keyframe_at_playhead(id: &str, state: &AppState) -> Option<Decline> {
    let slug = id.strip_prefix("pal-key-")?;
    let param = aria_models::ClipParam::CENSUS.into_iter().find(
        |param| matches!(param.class(), aria_models::ClipParamClass::Direct(s) if s == slug),
    )?;
    keyframe_param_blocked(state, *param)
}

/// The row removes the opacity keyframe under the playhead on the selected clip.
pub(crate) fn opacity_keyframe_at_playhead(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        (!state.has_keyframe_at_playhead(aria_models::ClipParam::Opacity))
            .then_some(Decline::NoOpacityKeyAtPlayhead)
    })
}

/// Why the selected clip's track cannot move one row up (`up`) or down, or
/// `None` when it can. Read from the plan the move follows.
fn track_move(state: &AppState, up: bool) -> Option<Decline> {
    let Some(track) = state.selected_track() else {
        return Some(Decline::NoSelection);
    };
    state.track_step(track, up).err()
}

/// The row moves the selected clip's track up one slot.
pub(crate) fn track_up(_: &str, state: &AppState) -> Option<Decline> {
    track_move(state, true)
}

/// The row moves the selected clip's track down one slot.
pub(crate) fn track_down(_: &str, state: &AppState) -> Option<Decline> {
    track_move(state, false)
}

/// The row opens a one-second gap at the playhead on the selected clip's
/// track: read from the check the shift makes.
pub(crate) fn open_gap(id: &str, state: &AppState) -> Option<Decline> {
    gap_shift(id, state, 1)
}

/// The row closes a one-second gap at the playhead on the selected clip's
/// track: read from the check the shift makes.
pub(crate) fn close_gap(id: &str, state: &AppState) -> Option<Decline> {
    gap_shift(id, state, -1)
}

/// Why a shift of one second in the direction of `sign` from the playhead on
/// the selected clip's track moves nothing.
fn gap_shift(id: &str, state: &AppState, sign: i64) -> Option<Decline> {
    selection(id, state).or_else(|| {
        let track = state.selected_track()?;
        let delta = sign * state.store.sequence.frame_rate.nominal_fps();
        state.playhead_shift_decline(track, delta)
    })
}

/// The row flags or unflags the selected clip's audio track as the duck source.
pub(crate) fn audio_track(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        let Some(track) = state.selected_track() else {
            return Some(Decline::NoSelection);
        };
        let is_audio =
            state.store.sequence.tracks.iter().any(|lane| {
                lane.id == track.raw() && lane.kind == crate::types::UiTrackKind::Audio
            });
        (!is_audio).then_some(Decline::NotOnAudioTrack)
    })
}

/// The row selects a clip under the playhead.
pub(crate) fn clip_under_playhead(_: &str, state: &AppState) -> Option<Decline> {
    let at = state.store.playhead_ticks;
    let any = state.store.sequence.tracks.iter().any(|track| {
        track
            .clips
            .iter()
            .any(|clip| at >= clip.timeline_start_ticks && at < clip.timeline_end_ticks)
    });
    (!any).then_some(Decline::NoClipAtPlayhead)
}

/// The row acts on the unlocked clips under the playhead on the targeted lanes.
pub(crate) fn targeted_clip_at_playhead(_: &str, state: &AppState) -> Option<Decline> {
    state
        .clips_at_playhead()
        .next()
        .is_none()
        .then_some(Decline::NoTargetedClipAtPlayhead)
}

/// The row cuts the clips Add Edit takes at the playhead.
pub(crate) fn clip_to_cut(_: &str, state: &AppState) -> Option<Decline> {
    state
        .add_edit_targets(false)
        .is_empty()
        .then_some(Decline::NoClipToCut)
}

/// The row puts the default picture transition on the cut Ctrl+T finds,
/// which does not already hold it.
pub(crate) fn picture_cut(_: &str, state: &AppState) -> Option<Decline> {
    let media = aria_models::TransitionMedia::Picture;
    state
        .default_transition_cut(media)
        .and_then(|clip| {
            crate::app::clip_transitions::default_transition(media)
                .map_or(Ok(()), |id| state.transition_to_set(clip, &id))
        })
        .err()
}

/// The row cuts every unlocked lane's clip at the playhead.
pub(crate) fn clip_to_cut_all(_: &str, state: &AppState) -> Option<Decline> {
    state
        .add_edit_targets(true)
        .is_empty()
        .then_some(Decline::NoClipToCut)
}

/// The row duplicates the selected clip onto another track that accepts it.
pub(crate) fn other_compatible_track(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        let Some(clip) = state.selected_ui_clip_ref() else {
            return Some(Decline::NoSelection);
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
        (!any).then_some(Decline::NoOtherTrack)
    })
}

/// The row names the pending undo and redo steps.
pub(crate) fn history_steps(_: &str, state: &AppState) -> Option<Decline> {
    (!state.store.can_undo && !state.store.can_redo).then_some(Decline::NoHistory)
}

/// The row reports the drawn size of a selected generated clip.
pub(crate) fn measurable_content(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        state
            .selected_ui_clip_ref()
            .is_none_or(|clip| clip.generator.is_none())
            .then_some(Decline::NothingToMeasure)
    })
}

/// The row clears the active media pool filter query.
pub(crate) fn media_filter(_: &str, state: &AppState) -> Option<Decline> {
    state.media_query.is_empty().then_some(Decline::FilterEmpty)
}

/// The row clears active media proxies when any pool item uses one.
pub(crate) fn proxies_in_use(_: &str, state: &AppState) -> Option<Decline> {
    (!state.store.media.iter().any(|item| item.proxied)).then_some(Decline::NoProxies)
}

/// The row acts on the items in the media pool.
pub(crate) fn media_pool(_: &str, state: &AppState) -> Option<Decline> {
    state.store.media.is_empty().then_some(Decline::PoolEmpty)
}

/// The row reloads the focused media pool item from disk.
pub(crate) fn pool_focus(_: &str, state: &AppState) -> Option<Decline> {
    state
        .pool_selection
        .focus()
        .is_none()
        .then_some(Decline::NoPoolFocus)
}

/// The row reports the cost of the last rendered frame.
pub(crate) fn rendered_frame(_: &str, state: &AppState) -> Option<Decline> {
    state
        .frame_rows()
        .is_empty()
        .then_some(Decline::NoFrameRendered)
}

/// The row acts on the media pool item played by the selected clip.
pub(crate) fn selected_media_source(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        let has_item = state
            .selected_ui_clip_ref()
            .and_then(|clip| clip.media_id)
            .is_some_and(|media| state.store.media.iter().any(|item| item.id == media));
        (!has_item).then_some(Decline::NoPoolEntry)
    })
}

/// The row acts on the audio stream of the media pool item played by the selected clip.
pub(crate) fn selected_audio_source(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        let Some(media) = state.selected_ui_clip_ref().and_then(|clip| clip.media_id) else {
            return Some(Decline::NoPoolEntry);
        };
        let Some(item) = state.store.media.iter().find(|item| item.id == media) else {
            return Some(Decline::NoPoolEntry);
        };
        (!item.has_audio).then_some(Decline::NoSound)
    })
}

/// The row measures the frame currently shown in the Viewer.
pub(crate) fn picture_on_screen(_: &str, state: &AppState) -> Option<Decline> {
    (state.last_frame.is_none() && state.preview_worker.shown_texture().is_none())
        .then_some(Decline::NoFrameOnScreen)
}

/// The row normalizes the selected clip from a measured loudness contour.
pub(crate) fn normalisable(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| {
        state
            .loudness_peak_dbfs()
            .is_none()
            .then_some(Decline::NoLoudness)
    })
}

/// The row runs shot detection on the selected clip's media.
pub(crate) fn detect_shots(_: &str, state: &AppState) -> Option<Decline> {
    analysable(state, crate::app::analysis::AnalysisKind::Shots)
}

/// The row runs audio analysis or loudness measurement on the selected clip.
pub(crate) fn detect_audio(_: &str, state: &AppState) -> Option<Decline> {
    analysable(state, crate::app::analysis::AnalysisKind::Silence)
}

/// Why a `kind` analysis cannot read the selected clip's media, or `None`
/// when it can.
fn analysable(state: &AppState, kind: crate::app::analysis::AnalysisKind) -> Option<Decline> {
    use crate::app::analysis::AnalysisRefusal;
    Some(match state.analysis_target(kind).err()? {
        AnalysisRefusal::Unselected => Decline::NoSelection,
        AnalysisRefusal::Generated => Decline::GeneratedAnalysis,
        AnalysisRefusal::Gone => Decline::MediaGone,
        AnalysisRefusal::Offline(_) => Decline::MediaOffline,
        AnalysisRefusal::Silent(_) => Decline::NoSound,
    })
}

/// The row transcribes the selected clip's audio when a model is installed.
pub(crate) fn transcribe(id: &str, state: &AppState) -> Option<Decline> {
    detect_audio(id, state).or_else(|| {
        state
            .settings_form
            .installed_model
            .is_none()
            .then_some(Decline::NoTranscriptionModel)
    })
}

/// The row ducks the selected music clips under the flagged voice track, and
/// states the reason the run would post when it cannot.
pub(crate) fn duck(id: &str, state: &AppState) -> Option<Decline> {
    selection(id, state).or_else(|| state.duck_precheck().err().map(Decline::Duck))
}

/// The row tracks the active mask forward from the playhead.
pub(crate) fn mask_track(_: &str, state: &AppState) -> Option<Decline> {
    if state.selected_clip().is_none() {
        return Some(Decline::NoMaskClip);
    }
    if state.active_mask().is_none() {
        return Some(Decline::NoMaskToTrack);
    }
    let span = state
        .selected_ui_clip_ref()
        .map_or(0, |ui| ui.timeline_end_ticks - 1 - state.playhead.value);
    (span <= 0).then_some(Decline::NothingAheadToTrack)
}

/// The row queries the configured AI endpoint when no probe is already in flight.
pub(crate) fn ai_idle(_: &str, state: &AppState) -> Option<Decline> {
    state.ai_probe.busy.then_some(Decline::AiCheckRunning)
}
