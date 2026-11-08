//! Consolidated integration test binary for `aria-engine`.
#![allow(clippy::duplicate_mod)]

#[path = "../audio_inserts.rs"]
mod audio_inserts;
#[path = "../clip_beats.rs"]
mod clip_beats;
#[path = "../command_registry.rs"]
mod command_registry;
#[path = "../compound_undo.rs"]
mod compound_undo;
#[path = "../drivers.rs"]
mod drivers;
#[path = "../duplicate_clip.rs"]
mod duplicate_clip;
#[path = "../effects_drift.rs"]
mod effects_drift;
#[path = "../export.rs"]
mod export;
#[path = "../extract_audio.rs"]
mod extract_audio;
#[path = "../fit_to_fill.rs"]
mod fit_to_fill;
#[path = "../frame_cancellation.rs"]
mod frame_cancellation;
#[path = "../freeze_frame.rs"]
mod freeze_frame;
#[cfg(target_vendor = "apple")]
#[path = "../import_probe.rs"]
mod import_probe;
#[path = "../import_still.rs"]
mod import_still;
#[path = "../inverse_edits.rs"]
mod inverse_edits;
#[path = "../inverse_undo.rs"]
mod inverse_undo;
#[path = "../linked_slip.rs"]
mod linked_slip;
#[path = "../locked_companions.rs"]
mod locked_companions;
#[path = "../multicam.rs"]
mod multicam;
#[path = "../nested_sequence.rs"]
mod nested_sequence;
#[path = "../open_gap.rs"]
mod open_gap;
#[path = "../orient.rs"]
mod orient;
#[path = "../overwrite.rs"]
mod overwrite;
#[path = "../overwrite_move.rs"]
mod overwrite_move;
#[path = "../paste_retimed.rs"]
mod paste_retimed;
#[path = "../preview.rs"]
mod preview;
#[path = "../project_file.rs"]
mod project_file;
#[path = "../relink.rs"]
mod relink;
#[path = "../remove_attributes.rs"]
mod remove_attributes;
#[path = "../ripple.rs"]
mod ripple;
#[path = "../session_reload_undo.rs"]
mod session_reload_undo;
#[path = "../set_sequence_rate.rs"]
mod set_sequence_rate;
#[path = "../source_edit.rs"]
mod source_edit;
#[path = "../source_frame.rs"]
mod source_frame;
#[path = "../split_cuts.rs"]
mod split_cuts;
#[path = "../subclip.rs"]
mod subclip;
#[path = "../sync_lock.rs"]
mod sync_lock;
#[path = "../template.rs"]
mod template;
#[path = "../unlink_clips.rs"]
mod unlink_clips;
#[path = "../unlinked_edits.rs"]
mod unlinked_edits;
#[path = "../workflow.rs"]
mod workflow;
