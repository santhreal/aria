//! Project-wide verbs: what the sequence carries, and where a source lives.
//!
//! Every read here goes through a catalog that owns the axis, so this module has
//! no table of its own: [`aria_models::clip_facts`] names the per-clip questions
//! and [`aria_models::clips_with_fact`] answers one. A question added there is
//! walkable here without an edit, and a clip property added without a question
//! does not compile.
//!
//! What the report *counts* is [`crate::app::project_report`]; this module walks
//! to the clips a count found.
//!
//! Nothing here restates a rule the model owns. Which lanes accept the selected
//! clip is [`aria_models::TrackKind::accepts_clip`]; which folder a source sits
//! in is walked with [`aria_models::Project::bin_ancestors`]; a clip under a
//! position is [`aria_models::Track::clip_at`]. This module turns a click into
//! one of those and reports what came back.

use std::path::Path;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    BinId, ClipId, RationalTime, Template, TrackId, clip_fact_by_id, clips_with_fact,
};

use super::AppState;
use super::picker::{Answer, Ask, FileSpec};

// The report's row types live with the report: [`crate::app::project_report`].

/// A lane the selected clip may be duplicated onto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneChoice {
    pub track: u64,
    pub name: String,
}

/// File extension a template bundle is described from.
const TEMPLATE_EXTENSION: &str = aria_models::TEMPLATE_FILE_EXTENSION;

impl AppState {
    // --- what the project carries ------------------------------------------

    /// Whether a report has anything to say: a project with no sources and no
    /// clips describes nothing.
    pub fn project_report_available(&self) -> bool {
        !self.store.media.is_empty()
            || self
                .store
                .sequence
                .tracks
                .iter()
                .any(|track| !track.clips.is_empty())
    }

    // Every project-wide number and every per-clip count is owned by
    // [`crate::app::project_report`], which measures them when the report is
    // asked for rather than once per repaint.

    /// Select and seek to the next clip carrying `fact`, wrapping at the end.
    ///
    /// The playhead is the cursor: repeated presses walk the matches in
    /// sequence order instead of returning to the first one. Inert on an
    /// unknown id and on a question nothing answers, so a stale row is not an
    /// error.
    pub fn go_to_clip_with_fact(&mut self, fact_id: &str) {
        let Some(spec) = clip_fact_by_id(fact_id) else {
            return;
        };
        let matches: Vec<(ClipId, i64)> = {
            let engine = self.engine.lock();
            let timeline = engine.project().timeline();
            clips_with_fact(timeline, spec.fact)
                .into_iter()
                .filter_map(|id| {
                    timeline
                        .clip(id)
                        .map(|clip| (id, clip.timeline.start.value))
                })
                .collect()
        };
        if matches.is_empty() {
            self.status_message = Some(format!("No clip carries: {}", spec.label));
            return;
        }
        let at = self.playhead.value;
        let (clip, start) = matches
            .iter()
            .find(|(_, start)| *start > at)
            .copied()
            .unwrap_or(matches[0]);
        self.select_clip(clip);
        self.set_playhead(start);
        self.status_message = Some(format!(
            "{} — {} of {}",
            spec.label,
            matches
                .iter()
                .position(|(id, _)| *id == clip)
                .map(|index| index + 1)
                .unwrap_or(1),
            matches.len()
        ));
    }

    /// Walk to the next clip carrying anything the audit asks about.
    ///
    /// The question is chosen from the catalog at run time — the first row any
    /// clip answers — so this row cannot name a fact that stops shipping, and a
    /// new question is reachable from it the moment it has a row.
    pub fn go_to_next_flagged_clip(&mut self) {
        self.measure_project_report();
        let Some(id) = self
            .clip_fact_rows()
            .iter()
            .find(|row| row.count > 0)
            .map(|row| row.id)
        else {
            self.status_message = Some("No clip carries anything the audit asks about".into());
            return;
        };
        self.go_to_clip_with_fact(id);
    }

    /// Put the last frame's stage costs on the status line, for an operator
    /// without the report open.
    pub fn report_frame_cost(&mut self) {
        let total = self.frame_total_ms();
        if total == 0.0 {
            self.status_message = Some("No frame measured yet".into());
            return;
        }
        let parts: Vec<String> = self
            .frame_stage_rows()
            .iter()
            .map(|row| format!("{} {:.2}ms", row.label, row.milliseconds))
            .collect();
        self.status_message = Some(format!("{} · total {total:.2}ms", parts.join(" · ")));
    }

    /// Report the drawn size of the selected clip's content.
    pub fn report_content_size(&mut self) {
        self.measure_project_report();
        self.status_message = Some(match self.selected_content_size() {
            Some((width, height)) => format!("Content draws {width} × {height} px"),
            None => "The selection draws no measurable content".to_string(),
        });
    }

    /// Select whichever clip on the selected clip's lane sits under the
    /// playhead. Falls back to the first lane that has one, so this works with
    /// nothing selected.
    ///
    /// The lane owns "which clip is at this position"; a projected scan here
    /// would be a second copy of that rule.
    pub fn select_clip_under_playhead(&mut self) {
        let at = self.playhead;
        let found = {
            let engine = self.engine.lock();
            let timeline = engine.project().timeline();
            let preferred = self
                .selection
                .first()
                .and_then(|clip| timeline.track_of(*clip));
            let lanes = preferred
                .into_iter()
                .chain(timeline.tracks_ordered().map(|track| track.id));
            lanes
                .filter_map(|id| timeline.track(id))
                .find_map(|track| track.clip_at(at).ok().flatten().map(|clip| clip.id))
        };
        match found {
            Some(clip) => self.select_clip(clip),
            None => self.status_message = Some("No clip under the playhead".into()),
        }
    }

    // --- duplicating onto another lane --------------------------------------

    /// The lanes the selected clip could be duplicated onto, excluding the one
    /// it is already on.
    ///
    /// The lane kind decides what it accepts, so a text clip is never offered a
    /// video lane and a generated clip is only offered the lane its generator
    /// declares.
    pub fn lanes_accepting_selection(&self) -> Vec<LaneChoice> {
        let Some(selected) = self.selected_clip() else {
            return Vec::new();
        };
        let engine = self.engine.lock();
        let timeline = engine.project().timeline();
        let Some(clip) = timeline.clip(selected) else {
            return Vec::new();
        };
        let on = timeline.track_of(selected);
        timeline
            .tracks_ordered()
            .filter(|track| Some(track.id) != on && track.kind.accepts_clip(clip))
            .map(|track| LaneChoice {
                track: track.id.raw(),
                name: track.name.clone(),
            })
            .collect()
    }

    /// Deep-copy the selected clip onto `track` at the same timeline position.
    ///
    /// Same position rather than the playhead: stacking a copy above the
    /// original is what a duplicate onto another lane is for, and the engine
    /// refuses the placement if that lane is already occupied there.
    pub fn duplicate_selection_to_lane(&mut self, track: u64) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let start = RationalTime::new(ui.timeline_start_ticks.max(0), self.playhead.rate);
        self.apply_command(Command::Edit(EditCommand::DuplicateClip {
            clip,
            to_track: TrackId::from_raw(track),
            start,
        }))?;
        Ok(())
    }

    /// Duplicate the selected clip onto the first lane that accepts it. The
    /// palette's one-press form of the lane list above.
    pub fn duplicate_selection_to_next_lane(&mut self) -> Result<(), EngineError> {
        let Some(choice) = self.lanes_accepting_selection().into_iter().next() else {
            self.status_message = Some("No other lane accepts this clip".into());
            return Ok(());
        };
        let name = choice.name.clone();
        self.duplicate_selection_to_lane(choice.track)?;
        self.status_message = Some(format!("Duplicated onto {name}"));
        Ok(())
    }

    // --- history, named ------------------------------------------------------

    /// The step Undo would take back, by name.
    ///
    /// The engine has always labelled its steps and the history dialog has
    /// always drawn them; a keystroke with the dialog closed was still a step
    /// into the dark, because nothing named the pending one anywhere else.
    pub fn next_undo_label(&self) -> Option<String> {
        let depth = self.history_depth();
        self.history_entries()
            .get(depth.checked_sub(1)?)
            .map(|entry| entry.label.clone())
    }

    /// The step Redo would put back, by name.
    pub fn next_redo_label(&self) -> Option<String> {
        let depth = self.history_depth();
        self.history_entries()
            .get(depth)
            .map(|entry| entry.label.clone())
    }

    /// Name both pending steps on the status line, for an operator who does not
    /// have the report open. Moves nothing: this is what the keys would do, not
    /// a second way of doing it.
    pub fn report_pending_history(&mut self) {
        let undo = self.next_undo_label();
        let redo = self.next_redo_label();
        self.status_message = Some(match (undo, redo) {
            (None, None) => "No steps to undo or redo".to_string(),
            (Some(undo), None) => format!("Undo: {undo}"),
            (None, Some(redo)) => format!("Redo: {redo}"),
            (Some(undo), Some(redo)) => format!("Undo: {undo} · Redo: {redo}"),
        });
    }

    // --- finding a source in the pool ---------------------------------------

    /// Open the folder holding the source at `path` and select it.
    ///
    /// The pool shows one folder at a time, so a source three folders deep is
    /// invisible until the right one is open. The project owns the lookup and
    /// the ancestor walk; this only moves the panel to what they found.
    pub fn reveal_media_in_pool(&mut self, path: &Path) -> bool {
        let found = {
            let engine = self.engine.lock();
            let project = engine.project();
            project.find_media_by_path(path).map(|media| {
                let bin = project.media(media).and_then(|source| source.bin);
                // The containing folder is what has to be open; its ancestors
                // are walked so a corrupt parent chain reports the depth it
                // could reach rather than opening nothing.
                let depth = bin.map_or(0, |bin| project.bin_ancestors(bin).count() + 1);
                (media.raw(), bin.map(BinId::raw), depth)
            })
        };
        let Some((media, bin, depth)) = found else {
            self.status_message = Some(format!("Not in the pool: {}", path.display()));
            return false;
        };
        self.open_bin(bin);
        self.pool_selection.replace(media);
        self.status_message = Some(match depth {
            0 => "Revealed at the pool root".to_string(),
            depth => format!("Revealed {depth} folder{} down", plural(depth)),
        });
        true
    }

    /// Ask for a file and reveal it in the pool.
    pub fn pick_and_reveal_media(&mut self) {
        self.request_pick(
            Ask::OpenFile(FileSpec::new("Reveal in pool")),
            |state, answer: Answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                state.reveal_media_in_pool(&path);
            },
        );
    }

    // --- describing a template before applying it ---------------------------

    /// What a template bundle will ask for: fill slots, retypeable texts, and
    /// whether its music is swappable.
    ///
    /// Read before committing to an apply. The editable-text count is the one
    /// the bundle tool prints and the editor never showed, which made "how much
    /// of this can I change" a question only the CLI could answer.
    pub fn describe_template(&self, path: &Path) -> Result<String, EngineError> {
        let template = Template::load_from_file(path)?;
        let slots = template.slots().len();
        let texts = template.editable_texts().len();
        let music = if template.music().is_some() {
            ", swappable music"
        } else {
            ""
        };
        Ok(format!(
            "{slots} slot{}, {texts} editable text{}{music}",
            plural(slots),
            plural(texts)
        ))
    }

    /// Ask for a bundle and report what it exposes.
    pub fn pick_and_describe_template(&mut self) {
        self.request_pick(
            Ask::OpenFile(
                FileSpec::new("Describe template").filter("Aria template", &[TEMPLATE_EXTENSION]),
            ),
            |state, answer: Answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                match state.describe_template(&path) {
                    Ok(summary) => state.status_message = Some(summary),
                    Err(err) => state.report_error(&err),
                }
            },
        );
    }
}

/// `"s"` unless there is exactly one of them.
fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
#[path = "project_reach_tests.rs"]
mod tests;
