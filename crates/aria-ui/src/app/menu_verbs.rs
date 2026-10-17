//! Verbs the menu bar offers that had no other surface: reverting to the saved
//! file, opening Settings at one section, picking the edit point nearest the
//! playhead and extending it to the playhead, replacing a clip from the Source
//! monitor, removing a clip's effects, deleting empty tracks, and setting the
//! interpolation of the targeted keyframes.

use std::path::Path;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, MediaId, RationalTime, TimeRange, TrackId, resample};

use super::{AppState, DialogKind};
use crate::types::{SelectedEdge, TrimSide};

/// A temporal interpolation the Animation menu sets on the targeted keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyInterpolation {
    /// Constant speed into and out of the keyframe.
    Linear,
    /// Easy Ease on both sides.
    Bezier,
    /// The value holds until the next keyframe.
    Hold,
}

impl AppState {
    /// Open Settings with its filter set to one section's title.
    ///
    /// The filter rather than a scroll position: the dialog narrows itself by
    /// section title, so setting it shows the section without the dialog
    /// recording which surface opened it.
    pub fn open_settings_at(&mut self, section: &str) {
        self.settings_form.filter = section.to_string();
        self.settings_form.focused = None;
        self.settings_form.draft.clear();
        self.open_dialog(DialogKind::Settings);
    }

    /// File > Revert: reopen the saved file, dropping every edit made since.
    pub fn revert_project(&mut self) {
        let Some(path) = self.project_path.clone() else {
            self.status_message = Some("The project has never been saved.".into());
            return;
        };
        if !self.store.is_dirty {
            self.status_message = Some("There are no changes since the last save.".into());
            return;
        }
        match self.open_project(path) {
            Ok(()) => self.status_message = Some("Reverted to the saved project".into()),
            Err(err) => self.report_error(&err),
        }
    }

    /// File > Open Recent: open one listed project through the unsaved-changes
    /// prompt.
    pub fn open_recent(&mut self, path: &Path) {
        if !path.exists() {
            self.report_error(format!(
                "{} is missing. Choose Open… to find the project.",
                path.display()
            ));
            return;
        }
        self.request_open_project(path.to_path_buf());
    }

    /// Sequence > Trim Edit: select the edit point nearest the playhead on an
    /// unlocked lane, ready for the trim keys.
    pub fn select_nearest_edit(&mut self) {
        let at = self.playhead.value;
        let mut nearest: Option<(i64, u64, TrimSide)> = None;
        for track in self.store.sequence.tracks.iter().filter(|t| !t.locked) {
            for clip in track.clips.iter().filter(|c| !c.locked) {
                for (tick, side) in [
                    (clip.timeline_start_ticks, TrimSide::In),
                    (clip.timeline_end_ticks, TrimSide::Out),
                ] {
                    let distance = (tick - at).abs();
                    if nearest.is_none_or(|(best, ..)| distance < best) {
                        nearest = Some((distance, clip.id, side));
                    }
                }
            }
        }
        match nearest {
            Some((_, clip, side)) => {
                self.select_edge(ClipId::from_raw(clip), side);
                self.status_message = Some(match side {
                    TrimSide::In => "Selected the In point nearest the playhead".into(),
                    TrimSide::Out => "Selected the Out point nearest the playhead".into(),
                });
            }
            None => self.status_message = Some("The sequence has no edit points.".into()),
        }
    }

    /// Sequence > Extend Edit: move the selected edit point to the playhead,
    /// with the active trim tool and edit mode.
    pub fn extend_edit_to_playhead(&mut self) -> Result<(), EngineError> {
        let Some(SelectedEdge { clip, side }) = self.selected_edge else {
            self.status_message = Some("Select an edit point first.".into());
            return Ok(());
        };
        let Some(ui) = self.ui_clip_ref(ClipId::from_raw(clip)) else {
            return Ok(());
        };
        let edge = match side {
            TrimSide::In => ui.timeline_start_ticks,
            TrimSide::Out => ui.timeline_end_ticks,
        };
        let delta = self.playhead.value - edge;
        if delta == 0 {
            self.status_message = Some("The edit point is already at the playhead.".into());
            return Ok(());
        }
        self.trim_selected_edge(delta)
    }

    /// The source in the Source monitor: the focused Source tab, or the one
    /// focused last while Program is on screen.
    pub(crate) fn monitor_source(&self) -> Option<&super::pool_preview::SourceView> {
        self.sources.focused().or_else(|| self.sources.recent())
    }

    /// Clip > Replace with Clip > From Source Monitor: repoint the selected
    /// clip at the source in the Source monitor, so the source frame under the
    /// Source playhead lands on the sequence frame under the sequence playhead.
    pub fn replace_with_source(&mut self) -> Result<(), EngineError> {
        let Some((media, source_tick)) = self
            .monitor_source()
            .map(|view| (MediaId::from_raw(view.media), view.tick))
        else {
            self.status_message = Some("Open a source in the Source monitor first.".into());
            return Ok(());
        };
        let Some(clip) = self.selected_clip() else {
            self.status_message = Some("No clip is selected.".into());
            return Ok(());
        };
        let Some(ui) = self.ui_clip_ref(clip) else {
            return Ok(());
        };
        let Some(old_media) = ui.media_id.filter(|_| ui.generator.is_none()) else {
            self.status_message = Some("A generated clip has no media to replace.".into());
            return Ok(());
        };
        let (start, end) = (ui.timeline_start_ticks, ui.timeline_end_ticks);
        let (old_from, old_to, reversed) =
            (ui.source_start_ticks, ui.source_end_ticks, ui.reversed);
        let (media_rate, still, available, old_rate) = {
            let engine = self.engine.lock();
            let project = engine.project();
            let Some(item) = project.media(media) else {
                return Ok(());
            };
            let old_rate = project
                .media(MediaId::from_raw(old_media))
                .map_or(item.frame_rate, |old| old.frame_rate);
            (
                item.frame_rate,
                item.is_image,
                item.duration.value,
                old_rate,
            )
        };
        // The clip keeps its source span, speed included, measured in the new
        // source's frames.
        let length = resample(RationalTime::new(old_to - old_from, old_rate), media_rate)
            .value
            .max(1);
        let span = (end - start).max(1);
        let into = (self.playhead.value - start).clamp(0, span);
        let offset = i64::try_from(i128::from(length) * i128::from(into) / i128::from(span))
            .unwrap_or(length);
        let offset = if reversed { length - offset } else { offset };
        let from = source_tick - offset;
        if !still && (from < 0 || from + length > available) {
            self.status_message = Some(
                "The source does not hold enough media around its playhead to fill the clip."
                    .into(),
            );
            return Ok(());
        }
        let from = if still { 0 } else { from };
        self.apply_command(Command::Edit(EditCommand::SetClipMedia {
            clip,
            media,
            source: TimeRange::at_rate(from, length, media_rate),
        }))?;
        self.status_message = Some("Replaced from the Source monitor".into());
        Ok(())
    }

    /// Effect > Remove All: take every effect off the selected clips, as one
    /// undo step.
    pub fn remove_all_effects_selected(&mut self) -> Result<(), EngineError> {
        let chains: Vec<(ClipId, usize)> = self
            .selection
            .iter()
            .filter_map(|clip| Some((*clip, self.ui_clip_ref(*clip)?.effects.len())))
            .filter(|(_, count)| *count > 0)
            .collect();
        if chains.is_empty() {
            self.status_message = Some("The selected clips have no effects.".into());
            return Ok(());
        }
        let removed: usize = chains.iter().map(|(_, count)| count).sum();
        self.one_step(|this| {
            for (clip, count) in chains {
                for index in (0..count).rev() {
                    this.apply_command(Command::Edit(EditCommand::RemoveEffect { clip, index }))?;
                }
            }
            Ok(())
        })?;
        self.status_message = Some(format!("Removed {removed} effect(s)"));
        Ok(())
    }

    /// Apply the effect applied last to the selection.
    pub fn apply_last_effect(&mut self) -> Result<(), EngineError> {
        let Some(effect) = self.last_effect.clone() else {
            self.status_message = Some("No effect has been applied yet.".into());
            return Ok(());
        };
        self.add_effect_to_selected(effect)
    }

    /// Tracks that hold no clip, are unlocked, and are not the last lane of
    /// their kind.
    pub(crate) fn empty_track_ids(&self) -> Vec<TrackId> {
        let tracks = &self.store.sequence.tracks;
        let mut out = Vec::new();
        for kind in [
            crate::types::UiTrackKind::Video,
            crate::types::UiTrackKind::Audio,
        ] {
            let of_kind: Vec<_> = tracks.iter().filter(|t| t.kind == kind).collect();
            let empty: Vec<_> = of_kind
                .iter()
                .filter(|t| t.clips.is_empty() && !t.locked)
                .collect();
            // A sequence keeps one lane of each kind it has, so dropping media
            // onto it still has somewhere to land.
            let keep = usize::from(empty.len() == of_kind.len());
            out.extend(
                empty
                    .into_iter()
                    .skip(keep)
                    .map(|t| TrackId::from_raw(t.id)),
            );
        }
        out
    }

    /// Sequence > Delete Tracks: delete every empty track, as one undo step.
    pub fn delete_empty_tracks(&mut self) -> Result<(), EngineError> {
        let tracks = self.empty_track_ids();
        if tracks.is_empty() {
            self.status_message = Some("Every track holds a clip.".into());
            return Ok(());
        }
        let count = tracks.len();
        self.one_step(|this| {
            for track in tracks {
                this.remove_track(track)?;
            }
            Ok(())
        })?;
        self.status_message = Some(format!("Deleted {count} empty track(s)"));
        Ok(())
    }

    /// Animation > Keyframe Interpolation: set the interpolation of the
    /// selected keyframes, or of the keyframes under the playhead.
    pub fn set_keyframe_interpolation(
        &mut self,
        interpolation: KeyInterpolation,
    ) -> Result<(), EngineError> {
        match interpolation {
            KeyInterpolation::Bezier => self.easy_ease_selected_or_playhead(),
            KeyInterpolation::Hold => self.hold_selected_or_playhead(),
            KeyInterpolation::Linear => {
                let targets = self.ease_target_keyframes();
                if targets.is_empty() {
                    self.status_message = Some("Select a keyframe first".into());
                    return Ok(());
                }
                self.one_step(|this| {
                    for target in targets {
                        this.set_keyframe_linear_at(target.clip, target.param, target.tick)?;
                    }
                    Ok(())
                })
            }
        }
    }
}

#[cfg(test)]
#[path = "menu_verbs_tests.rs"]
mod tests;
