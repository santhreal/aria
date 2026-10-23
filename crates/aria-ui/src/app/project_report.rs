//! What the project report says, held beside the store.
//!
//! Two tables, each built from a catalog the model owns:
//! [`aria_models::project_facts`] and [`aria_models::clip_facts`]. This module
//! owns none of the rows — it owns *when* they are measured, which is the only
//! thing a report needs a decision about. Drawing them is
//! [`crate::panels::project_report`].
//!
//! **The counts are measured when they are asked for.** Every project-wide
//! number is a walk of the project and every per-clip question is a walk of the
//! timeline, so answering fifteen of them on the render path meant fifteen
//! sessions locks and fifteen walks per repaint, behind whatever a preview
//! decode was doing with the same lock. They are measured instead when the
//! report is opened and again on any re-projection while it is open, both of
//! which are edit paths that already hold the session. With the report closed
//! the cost is zero.

use aria_engine::Engine;
use aria_models::{clip_facts, clips_with_fact, project_facts};

use crate::app::AppState;

/// One project-wide number, ready to draw.
///
/// The value is shared text: it is measured once, and every frame the report is
/// open draws it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFactRow {
    pub id: &'static str,
    pub label: &'static str,
    pub value: gpui::SharedString,
}

/// One per-clip question, with how many clips answer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipFactRow {
    pub id: &'static str,
    pub label: &'static str,
    pub count: usize,
}

/// Everything the report counts, as of the last time it was asked.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectReport {
    /// Every project-wide number, in catalog order. A handle, because the dialog
    /// that draws it cannot hold a borrow of the state across the frame and
    /// would otherwise copy every row.
    pub facts: std::sync::Arc<[ProjectFactRow]>,
    /// Every per-clip question with the number of clips it holds for, in
    /// catalog order. Rows nothing answers are kept: "no clip carries a speed
    /// ramp" is a useful answer.
    pub clip_facts: std::sync::Arc<[ClipFactRow]>,
    /// Tight size in canvas pixels of what the selected clip's generator
    /// actually draws, sampled at the playhead. `None` for a media clip and for
    /// a generator the compositor does not draw.
    pub content: Option<(u32, u32)>,
}

impl ProjectReport {
    /// Measure the report against `engine`.
    ///
    /// `&mut Engine` because the content size comes from the memoized
    /// rasterizers, so measuring a title warms the composite path that will
    /// draw it.
    pub fn of(engine: &mut Engine, selected: Option<aria_models::ClipId>, playhead: i64) -> Self {
        let facts = {
            let project = engine.project();
            project_facts()
                .iter()
                .map(|spec| ProjectFactRow {
                    id: spec.id,
                    label: spec.label,
                    value: (spec.read)(project).into(),
                })
                .collect()
        };
        let clip_facts = {
            let timeline = engine.project().timeline();
            clip_facts()
                .iter()
                .map(|spec| ClipFactRow {
                    id: spec.id,
                    label: spec.label,
                    count: clips_with_fact(timeline, spec.fact).len(),
                })
                .collect()
        };
        Self {
            facts,
            clip_facts,
            content: content_size(engine, selected, playhead),
        }
    }
}

/// The drawn size of `selected`'s generated content at `playhead`.
fn content_size(
    engine: &mut Engine,
    selected: Option<aria_models::ClipId>,
    playhead: i64,
) -> Option<(u32, u32)> {
    let clip = engine.project().timeline().clip(selected?)?;
    let aria_models::ClipSource::Generated(generator) = &clip.content else {
        return None;
    };
    let generator = generator.clone();
    let tick = clip.param_tick(playhead);
    engine.generator_content_size(&generator, tick)
}

impl AppState {
    /// Measure the report now.
    ///
    /// The one place the counts are taken: opening the report, re-projecting
    /// while it is open, and the two status-line commands that report a single
    /// row all come through here, so a row the dialog draws and a row the strip
    /// prints cannot disagree.
    pub(crate) fn measure_project_report(&mut self) {
        let selected = self.selected_clip();
        let playhead = self.playhead.value;
        let mut engine = self.engine.lock();
        self.report = ProjectReport::of(&mut engine, selected, playhead);
    }

    /// Every project-wide number, in catalog order, as last measured.
    ///
    /// A handle, as the undo list is: the dialog owns the rows for the frame
    /// without copying them.
    pub fn project_fact_rows(&self) -> std::sync::Arc<[ProjectFactRow]> {
        std::sync::Arc::clone(&self.report.facts)
    }

    /// Every per-clip question with the number of clips it holds for, as last
    /// measured.
    pub fn clip_fact_rows(&self) -> std::sync::Arc<[ClipFactRow]> {
        std::sync::Arc::clone(&self.report.clip_facts)
    }

    /// The drawn size of the selected clip's content, as last measured.
    pub fn selected_content_size(&self) -> Option<(u32, u32)> {
        self.report.content
    }
}
