//! The handler rows.
//!
//! One row per command, and nothing else: the outcome columns, the guard and
//! the dispatchers live in the parent module. Adding a command is adding a row
//! here, because [`apply_edit`] and [`apply_project`] are generated from these
//! tables and are exhaustive over their axis.
//!
//! A row is `Variant { payload } => outcome(handler(ctx, payload)?)`. The
//! outcome word says what the edit changed; the handler is the module under
//! `action::edit` or `action::project` that does it and returns the inverse.
//! Nothing else belongs in a row: a row that needs a statement is a handler
//! that is not finished.

use super::*;

#[rustfmt::skip]
edit_ops! { ctx;
    // Placements and duplications: the handler returns the new clip's id.
    AddTrack { kind, name, index, pinned } => created_track(edit::add_track::execute(ctx, kind, name, index, pinned)?),
    AddClip { track, media, source, start } => created(edit::add_clip::execute(ctx, track, media, source, start)?),
    AddGenerated { track, generator, timeline } => created(edit::add_generated::execute(ctx, track, generator, timeline)?),
    AddMulticamClip { track, group, source, start } => created(edit::multicam::add_multicam_clip(ctx, track, group, source, start)?),
    PlaceNestedSequence { track, sequence, source, start } => created(edit::nested_sequence::place_nested_sequence(ctx, track, sequence, source, start)?),
    CreateNestedSequence { name, clips } => created(edit::nested_sequence::create_nested_sequence(ctx, name, &clips)?),
    ExtractAudio { clip, to_track } => created(edit::extract_audio::execute(ctx, clip, to_track)?),
    DuplicateClip { clip, to_track, start } => created(edit::duplicate_clip::execute(ctx, clip, to_track, start)?),
    PasteRetimed { clip, to_track, start, target_duration, target_speed } => created(edit::paste_retimed::execute(ctx, clip, to_track, start, target_duration, target_speed)?),
    PasteAttributes { source, clip } => updated(clip, edit::paste_attributes::execute(ctx, source, clip)?),
    RemoveAttributes { clip, attributes } => updated(clip, edit::remove_attributes::execute(ctx, clip, &attributes)?),
    FreezeFrame { clip, at, duration } => created(edit::freeze_frame::execute(ctx, clip, at, duration)?),
    BakeClip { clip, media } => updated(clip, edit::bake::bake_clip(ctx, clip, media)?),
    RestoreBakedClip { clip } => updated(clip, edit::bake::restore_baked_clip(ctx, clip)?),
    SplitClip { clip, at } => created(edit::split_clip::execute(ctx, clip, at)?),
    RippleInsert { track, media, source, at } => created(edit::ripple_insert::execute(ctx, track, media, source, at)?),
    OverwriteClip { track, media, source, at } => created(edit::overwrite::execute(ctx, track, media, source, at)?),
    FitToFill { track, media, source, at, duration, ripple } => created(edit::fit_to_fill::execute(ctx, track, media, source, at, duration, ripple)?),

    // Clip content.
    SetGenerator { clip, generator } => updated(clip, edit::set_generator::execute(ctx, clip, generator)?),
    SetClipMedia { clip, media, source } => updated(clip, edit::set_clip_media::execute(ctx, clip, media, source)?),
    SetReplaceable { clip, replaceable } => updated(clip, edit::template_marks::set_replaceable(ctx, clip, replaceable)?),
    SetTextEditable { clip, editable } => updated(clip, edit::template_marks::set_text_editable(ctx, clip, editable)?),

    // Geometry and parameters.
    SetClipTransform { clip, transform, at } => updated(clip, edit::set_transform::execute(ctx, clip, transform, at)?),
    SetClipParent { clip, parent, at, keep } => updated(clip, edit::set_parent::execute(ctx, clip, parent, at, keep)?),
    SetParamKeyframe { clip, param, at, value, easing, tangents } => updated(clip, edit::set_param::set_keyframe(ctx, clip, param, at, value, easing, tangents)?),
    RemoveParamKeyframe { clip, param, at } => updated(clip, edit::set_param::remove_keyframe(ctx, clip, param, at)?),
    SetParamDriver { clip, param, code } => updated(clip, edit::set_driver::set_driver(ctx, clip, param, code)?),
    SetParamConstant { clip, param, value } => updated(clip, edit::set_param::set_constant(ctx, clip, param, value)?),
    ApplyEasingPreset { clip, param, at, preset } => updated(clip, edit::set_param::apply_easing_preset(ctx, clip, param, at, preset)?),
    SetClipCrop { clip, crop, flip_h, flip_v, at } => updated(clip, edit::set_crop::set_crop(ctx, clip, crop, flip_h, flip_v, at)?),

    // Time.
    SetClipSpeed { clip, speed, reversed } => updated(clip, edit::set_speed::set_speed(ctx, clip, speed, reversed)?),
    SetSpeedCurve { clip, curve } => updated(clip, edit::set_speed_curve::set_speed_curve(ctx, clip, curve)?),
    SetClipBeats { clip, beats } => updated(clip, edit::clip_beats::set(ctx, clip, beats)?),
    ClearBeats { clip } => updated(clip, edit::clip_beats::clear(ctx, clip)?),
    // A slip changes neither the clip's extent nor its neighbours, so no
    // junction can appear or disappear: its row keeps them.
    SlipClip { clip, by } => updated(clip, edit::slip_clip::execute(ctx, clip, by)?),
    TrimClip { clip, timeline } => updated(clip, edit::trim_clip::execute(ctx, clip, timeline)?),
    TrimClipUnlinked { clip, timeline } => updated(clip, edit::trim_clip::trim_one(ctx, clip, timeline)?),
    MoveClip { clip, to_track, start } => updated(clip, edit::move_clip::execute(ctx, clip, to_track, start)?),
    MoveClipUnlinked { clip, to_track, start } => updated(clip, edit::move_clip::execute_unlinked(ctx, clip, to_track, start)?),
    OverwriteMoveClip { clip, to_track, start } => updated(clip, edit::overwrite_move::execute(ctx, clip, to_track, start)?),
    OverwriteMoveClipUnlinked { clip, to_track, start } => updated(clip, edit::overwrite_move::execute_unlinked(ctx, clip, to_track, start)?),
    RippleMoveClip { clip, to_track, start } => updated(clip, edit::ripple_move::execute(ctx, clip, to_track, start)?),

    // Audio.
    SetClipPitch { clip, preserve_pitch } => updated(clip, edit::set_pitch::set_pitch(ctx, clip, preserve_pitch)?),
    SetClipAudio { clip, volume, fade_in, fade_out } => updated(clip, edit::set_audio::set_audio(ctx, clip, volume, fade_in, fade_out)?),
    SetClipDenoise { clip, denoise } => updated(clip, edit::set_denoise::set_denoise(ctx, clip, denoise)?),
    SetAudioRole { clip, role } => updated(clip, edit::set_look::set_audio_role(ctx, clip, role)?),

    // Identity.
    SetClipLabel { clip, label } => updated(clip, edit::set_label::set_label(ctx, clip, label)?),
    SetClipName { clip, name } => updated(clip, edit::set_clip_name::set_clip_name(ctx, clip, name)?),
    SetClipType { clip, clip_type } => updated(clip, edit::set_clip_name::set_clip_type(ctx, clip, clip_type)?),
    SetClipLineParam { clip, param } => updated(clip, edit::set_clip_name::set_clip_line_param(ctx, clip, param)?),

    // Look: one handler module, one function per property.
    AddClipMask { clip, mask } => updated(clip, edit::set_mask::add_mask(ctx, clip, mask)?),
    SetClipMask { clip, index, mask } => updated(clip, edit::set_mask::set_mask(ctx, clip, index, mask)?),
    RemoveClipMask { clip, index } => updated(clip, edit::set_mask::remove_mask(ctx, clip, index)?),
    MoveClipMask { clip, from_index, to_index } => updated(clip, edit::set_mask::move_mask(ctx, clip, from_index, to_index)?),
    SetClipBlendMode { clip, mode } => updated(clip, edit::set_look::set_blend_mode(ctx, clip, mode)?),
    SetClipMotionBlur { clip, motion_blur } => updated(clip, edit::set_look::set_motion_blur(ctx, clip, motion_blur)?),
    SetClipLayerStyles { clip, styles } => updated(clip, edit::set_look::set_layer_styles(ctx, clip, styles)?),
    SetClipChroma { clip, chroma } => updated(clip, edit::set_look::set_chroma(ctx, clip, chroma)?),
    SetClipMatte { clip, matte } => updated(clip, edit::set_look::set_matte(ctx, clip, matte)?),
    SetClipStabilize { clip, stabilize } => updated(clip, edit::set_look::set_stabilize(ctx, clip, stabilize)?),
    SetClipFilter { clip, filter } => updated(clip, edit::set_look::set_filter(ctx, clip, filter)?),
    SetClipLut { clip, lut } => updated(clip, edit::set_look::set_lut(ctx, clip, lut)?),
    SetClipAdjustments { clip, adjust } => updated(clip, edit::set_look::set_adjustments(ctx, clip, adjust)?),
    SetClipAnimation { clip, slot, animation } => updated(clip, edit::set_look::set_animation(ctx, clip, slot, animation)?),

    // Effect chain and transitions.
    AddEffect { clip, effect_id } => updated(clip, edit::set_effect::add_effect(ctx, clip, &effect_id)?),
    RemoveEffect { clip, index } => updated(clip, edit::set_effect::remove_effect(ctx, clip, index)?),
    MoveEffect { clip, from_index, to_index } => updated(clip, edit::set_effect::move_effect(ctx, clip, from_index, to_index)?),
    SetEffectParam { clip, index, param, value } => updated(clip, edit::set_effect::set_effect_param(ctx, clip, index, param, value)?),
    SetEffectInstance { clip, index, effect } => updated(clip, edit::set_effect::set_effect_instance(ctx, clip, index, effect)?),
    SetEffectBypass { clip, index, bypass } => updated(clip, edit::set_effect::set_effect_bypass(ctx, clip, index, bypass)?),
    ApplyPreset { clip, preset, at } => updated(clip, edit::set_effect::apply_preset(ctx, clip, &preset, at)?),
    AddTransition { clip, transition_id } => updated(clip, edit::set_transition::add_transition(ctx, clip, &transition_id)?),
    RemoveTransition { clip } => updated(clip, edit::set_transition::remove_transition(ctx, clip)?),
    SetTransition { clip, duration } => updated(clip, edit::set_transition::set_transition(ctx, clip, duration)?),
    SetTransitionAlignment { clip, alignment } => updated(clip, edit::set_transition::set_transition_alignment(ctx, clip, alignment)?),
    SetTransitionState { clip, transition } => updated(clip, edit::set_transition::set_transition_state(ctx, clip, transition)?),

    // Linking. The outcome names the selection's first clip; an empty
    // selection is refused by `first_of`.
    LinkClips { clips } => updated(first_of(&clips)?, edit::link_clips::execute(ctx, &clips)?),
    UnlinkClips { clips } => updated(first_of(&clips)?, edit::link_clips::unlink(ctx, &clips)?),
    SetClipsEnabled { clips, enabled } => updated(first_of(&clips)?, edit::link_clips::set_enabled(ctx, &clips, enabled)?),
    GroupClips { clips } => updated(first_of(&clips)?, edit::link_clips::group(ctx, &clips)?),
    UngroupClips { clips } => updated(first_of(&clips)?, edit::link_clips::ungroup(ctx, &clips)?),

    // Removals.
    RemoveClip { clip } => removed(clip, Box::new(RemoveClipAction { clip }).apply(ctx)?),
    RippleDelete { clip } => removed(clip, edit::ripple_delete::execute(ctx, clip)?),
    RemoveTrack { track } => removed_track(track, Box::new(RemoveTrackAction { track_id: track }).apply(ctx)?),

    // Lane order, flags and view. Every flag is one patch through one handler.
    MoveTrack { track, index } => updated_track(track, edit::move_track::execute(ctx, track, index)?),
    SetTrackName { track, name } => updated_track(track, edit::set_track_name::set_track_name(ctx, track, name)?),
    SetTrackEnabled { track, enabled } => updated_track(track, edit::set_track_flags::execute(ctx, track, FlagPatch::enabled(enabled))?),
    SetTrackMuted { track, muted } => updated_track(track, edit::set_track_flags::execute(ctx, track, FlagPatch::muted(muted))?),
    SetTrackSolo { track, solo } => updated_track(track, edit::set_track_flags::execute(ctx, track, FlagPatch::solo(solo))?),
    SetTrackLocked { track, locked } => updated_track(track, edit::set_track_flags::execute(ctx, track, FlagPatch::locked(locked))?),
    SetTrackDuckSource { track, duck_source } => updated_track(track, edit::set_track_flags::execute(ctx, track, FlagPatch::duck_source(duck_source))?),
    SetTrackTargeted { track, targeted } => updated_track(track, edit::set_track_flags::execute(ctx, track, FlagPatch::targeted(targeted))?),
    SetTrackSyncLock { track, sync_lock } => updated_track(track, edit::set_track_flags::execute(ctx, track, FlagPatch::sync_lock(sync_lock))?),
    SetTrackDisplay { track, height, waveform, filmstrip, collapsed } => updated_track(track, edit::set_track_display::execute(ctx, track, height, waveform, filmstrip, collapsed)?),
    SetTrackGain { track, gain } => updated_track(track, edit::set_track_routing::execute(ctx, track, RouteEdit::Gain(gain))?),
    SetTrackPan { track, pan } => updated_track(track, edit::set_track_routing::execute(ctx, track, RouteEdit::Pan(pan))?),
    SetTrackAutomation { track, mode, volume, pan } => updated_track(track, edit::set_track_routing::execute(ctx, track, RouteEdit::Automation { mode, volume, pan })?),
    SetTrackOutput { track, to } => updated_track(track, edit::set_track_routing::execute(ctx, track, RouteEdit::Output(to.map_or(TrackOutput::Master, TrackOutput::Bus)))?),
    AddSend { track, to, level, pre_fader } => updated_track(track, edit::set_track_routing::execute(ctx, track, RouteEdit::AddSend { to, level, pre_fader })?),
    SetSend { track, index, level, pre_fader } => updated_track(track, edit::set_track_routing::execute(ctx, track, RouteEdit::SetSend { index, level, pre_fader })?),
    RemoveSend { track, index } => updated_track(track, edit::set_track_routing::execute(ctx, track, RouteEdit::RemoveSend { index })?),
    ShiftClips { track, from, delta } => shifted(track, edit::shift_clips::execute(ctx, track, from, delta)?),

    // Insert chains. Every gesture is one handler that snapshots the chain.
    AddAudioEffect { target, effect_id } => updated_inserts(target, edit::audio_effects::execute(ctx, target, InsertEdit::Add { effect_id })?),
    RemoveAudioEffect { target, index } => updated_inserts(target, edit::audio_effects::execute(ctx, target, InsertEdit::Remove { index })?),
    MoveAudioEffect { target, from_index, to_index } => updated_inserts(target, edit::audio_effects::execute(ctx, target, InsertEdit::Move { from_index, to_index })?),
    SetAudioEffectParam { target, index, param, value } => updated_inserts(target, edit::audio_effects::execute(ctx, target, InsertEdit::Param { index, param, value })?),
    SetAudioEffectBypass { target, index, bypass } => updated_inserts(target, edit::audio_effects::execute(ctx, target, InsertEdit::Bypass { index, bypass })?),
    SetAudioEffectSidechain { target, index, key } => updated_inserts(target, edit::audio_effects::execute(ctx, target, InsertEdit::Sidechain { index, key })?),

    // Ruler markers.
    AddMarker { at, name, color } => created_marker(edit::marker::add(ctx, at, name, color)?),
    RemoveMarker { marker } => removed_marker(marker, Box::new(edit::marker::RemoveMarkerAction { marker }).apply(ctx)?),
    SetMarker { marker, at, name, color, comment } => updated_marker(marker, edit::marker::set(ctx, marker, at, name, color, comment)?),

    // Project-wide settings and the pool's folder structure.
    SetCanvas { aspect, background, resolution } => updated_canvas(edit::set_canvas::set_canvas(ctx, aspect, background, resolution)?),
    SetSequenceRate { frame_rate } => updated_sequence_rate(edit::set_sequence_rate::set_sequence_rate(ctx, frame_rate)?),
    SetAudioLayout { layout } => updated_audio_layout(edit::set_audio_layout::set_audio_layout(ctx, layout)?),
    SetTimeDisplay { display } => updated_time_display(edit::set_time_display::set_time_display(ctx, display)?),
    SetMasterMix { gain, pan, muted, automation, volume, pan_envelope } => updated_master(edit::set_track_routing::execute_master(ctx, gain, pan, muted, automation, volume, pan_envelope)?),
    SetProjectName { name } => updated_project(edit::set_project_name::set_project_name(ctx, name)?),
    CreateBin { name, parent } => created_bin(edit::bin::create_bin(ctx, name, parent)?),
    RenameBin { bin, name } => updated_pool(edit::bin::rename_bin(ctx, bin, name)?),
    RemoveBin { bin } => updated_pool(edit::bin::remove_bin(ctx, bin)?),
    MoveBin { bin, parent } => updated_pool(edit::bin::move_bin(ctx, bin, parent)?),
    MoveMediaToBin { media, bin } => updated_pool(edit::bin::move_media_to_bin(ctx, media, bin)?),
    SetPoolItemPosition { media, bin, x, y } => updated_pool(edit::bin::set_pool_item_position(ctx, media, bin, x, y)?),
    CreateSubclip { parent, name, source, bin } => updated_pool(edit::bin::create_subclip(ctx, parent, name, source, bin)?),
    CreateMulticamGroup { name, angles } => updated_pool(edit::multicam::create_group(ctx, name, angles)?),
    SwitchMulticamAngle { group, at, angle } => updated_pool(edit::multicam::switch_angle(ctx, group, at, angle)?),
    SetMulticamSync { group, angle, offset } => updated_pool(edit::multicam::set_angle_offset(ctx, group, angle, offset)?),
    OpenSequence { sequence } => opened_sequence(edit::nested_sequence::open_sequence(ctx, sequence)?),
    CreateSequence { name } => opened_sequence(edit::nested_sequence::create_sequence(ctx, name)?),
    SetKeyframeEase { clip, param, at, in_ease, out_ease, hold } => updated(clip, edit::set_param::set_keyframe_ease(ctx, clip, param, at, in_ease, out_ease, hold)?),
    SetKeyframeRoving { clip, param, at, roving } => updated(clip, edit::set_param::set_keyframe_roving(ctx, clip, param, at, roving)?),
    SetSourcePatch { channel, patch } => updated_source_patch(edit::source_patch::execute(ctx, channel, patch)?),
    SourceEdit { media, source, at, fill, ripple, track, sound } => created(edit::source_edit::execute(ctx, media, source, at, fill, ripple, track, sound)?),
}

#[rustfmt::skip]
project_ops! { ctx;
    Import { path } => imported(project::import::execute(ctx, &path)?),
    RemoveMedia { media } => undoable(ApplyOutcome::RemovedMedia { media }, Box::new(RemoveMediaAction { media }).apply(ctx)?),
    SetMediaLabel { media, label } => undoable(ApplyOutcome::LabeledMedia { media }, project::set_media_label::execute(ctx, media, label)?),

    // Session and file actions. Each records no undo step, for the reason its
    // row in `aria-commands` states.
    Save { path } => { project::save::execute(ctx, path)?; settled(ApplyOutcome::Saved) },
    Open { path } => { project::open::execute(ctx, path)?; settled(ApplyOutcome::Opened) },
    Load { path } => { project::load::execute(ctx, path)?; settled(ApplyOutcome::Loaded) },
    RelinkMedia { media, path } => { project::relink::execute(ctx, media, &path)?; settled(ApplyOutcome::Relinked { media }) },
    SaveTemplate { path, meta } => { project::save_template::execute(ctx, path, meta)?; settled(ApplyOutcome::SavedTemplate) },
    ApplyTemplate { path, picks } => { project::apply_template::execute(ctx, path, picks)?; settled(ApplyOutcome::AppliedTemplate) },

    // Export renders through the engine's own renderer, which has to suspend
    // its proxy first; `Engine::apply` intercepts it before dispatch is
    // reached, so arriving here is a routing bug rather than an input error.
    Export { .. } => return Err(EngineError::Export(
        "export is handled by Engine::apply, not dispatch".into(),
    )),
}
