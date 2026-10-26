//! Filmstrip cells for the timeline.
//!
//! A cell is a rendered frame of the sequence at one tick, scaled down to the
//! strip's height. The cell is rendered rather than decoded from the source:
//! the renderer applies color conversion, transforms, and effects, so a cell
//! shows the frame that plays at that tick instead of a raw source frame.
//!
//! Cells are keyed by tick. An edit invalidates only cells whose ticks
//! intersect the picture spans it changed, and a rollback only the cells the
//! edits it reverted changed; audio-only and organizational edits keep every
//! rendered cell. A change the store did not follow drops every cell. The
//! session revision still rejects a render that finishes after a newer edit.
//! Only one render is in flight at a time: a filmstrip is worth idle
//! capacity, never a stall in front of a scrub.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use aria_models::RationalTime;
use aria_plugin::Reverted;
use gpui::{App, Context, Entity, RenderImage};

use crate::app::AppState;

/// Height of a filmstrip cell, and of the strip. A 16:9 cell is then 60px wide,
/// which is about one cell per second at the default zoom.
pub const CELL_H: u32 = 34;

/// Widest cell that is ever requested, so a very wide canvas cannot make a
/// thumbnail that costs more than the strip can show.
const CELL_MAX_W: u32 = 96;

/// Cells kept across all clips. A cell is a few KB; this bounds the strip's
/// memory at a few megabytes on a project with many lanes.
const MAX_CELLS: usize = 512;

/// Edits the store remembers for a rollback to walk back. A gesture's
/// rollback walks back the few edits of one pointer move; a rollback that
/// reaches past the oldest one drops every cell.
const JOURNAL: usize = 64;

/// One rendered cell.
#[derive(Clone)]
pub struct Cell {
    pub image: Arc<RenderImage>,
}

/// One edit the store followed: the revision moved from `from` to `to`, and
/// the picture changed only inside `spans`.
struct Step {
    from: u64,
    to: u64,
    spans: Vec<(i64, i64)>,
}

/// Cells by the tick they were rendered at.
#[derive(Default)]
pub struct FilmstripStore {
    revision: u64,
    cells: HashMap<i64, Cell>,
    /// The tick currently being rendered, if any. One at a time.
    in_flight: Option<i64>,
    /// Ticks that produced no frame, so a gap in the sequence is not retried
    /// every frame for as long as it stays on screen.
    empty: Vec<i64>,
    /// The latest edits followed by span, oldest first. The newest ends at
    /// `revision`; the store forgets them all when it drops every cell.
    journal: VecDeque<Step>,
}

impl FilmstripStore {
    pub fn get(&self, tick: i64) -> Option<&Cell> {
        self.cells.get(&tick)
    }

    /// Whether `tick` should be rendered now: there is no retained cell for it,
    /// it did not already come back empty, and nothing else is in flight.
    pub fn wants(&self, tick: i64) -> bool {
        self.in_flight.is_none() && !self.cells.contains_key(&tick) && !self.empty.contains(&tick)
    }

    /// Drop everything rendered against an older revision. The fallback for a
    /// change that was not followed by span: a mutation that reports no
    /// picture spans, an undo, or an edit made outside `apply_command`.
    pub fn sync_revision(&mut self, revision: u64) {
        if self.revision != revision {
            self.revision = revision;
            self.cells.clear();
            self.empty.clear();
            self.journal.clear();
            // A render started against the old project is no longer wanted; it
            // is dropped on arrival by the revision check.
            self.in_flight = None;
        }
    }

    /// Follow one edit from revision `from` to `to` that changed the picture
    /// only inside `spans`, retaining every cell outside them.
    ///
    /// A cell is one frame at one exact timeline tick, so half-open span
    /// intersection reduces to membership. Empty results follow the same rule:
    /// an edit inside a gap may put a frame there, while an edit elsewhere
    /// cannot. The cells hold for the store's revision only, so an edit from
    /// any other revision follows a change the store never saw, and every cell
    /// is dropped.
    pub fn advance(&mut self, from: u64, to: u64, spans: Vec<(i64, i64)>) {
        if to == self.revision {
            return;
        }
        if from != self.revision {
            self.sync_revision(to);
            return;
        }
        self.drop_spans(&spans);
        self.follow(Step { from, to, spans });
    }

    /// Follow a rollback: drop the cells the edits between `opened` and
    /// `found` changed, which the rollback changed back, and retain the rest.
    /// When the journal does not link every one of those edits, or the store
    /// is not at `found`, every cell is dropped.
    pub fn revert(&mut self, reverted: Reverted) {
        let Reverted {
            opened,
            found,
            left,
        } = reverted;
        if found != self.revision {
            self.sync_revision(left);
            return;
        }
        if left == found {
            return;
        }
        let Some(spans) = self.spans_since(opened) else {
            self.sync_revision(left);
            return;
        };
        self.drop_spans(&spans);
        // Recorded as one edit, so a rollback of an enclosing group walks
        // back across this one.
        self.follow(Step {
            from: found,
            to: left,
            spans,
        });
    }

    /// Every span the journal recorded from `opened` up to the store's
    /// revision, or `None` when an edit between them is missing.
    fn spans_since(&self, opened: u64) -> Option<Vec<(i64, i64)>> {
        let mut cursor = self.revision;
        let mut spans = Vec::new();
        for step in self.journal.iter().rev() {
            if cursor == opened {
                break;
            }
            if step.to != cursor {
                return None;
            }
            spans.extend_from_slice(&step.spans);
            cursor = step.from;
        }
        (cursor == opened).then_some(spans)
    }

    /// Drop the cells and empty results inside `spans`.
    fn drop_spans(&mut self, spans: &[(i64, i64)]) {
        let inside = |tick: i64| {
            spans
                .iter()
                .any(|&(start, end)| tick >= start && tick < end)
        };
        self.cells.retain(|tick, _| !inside(*tick));
        self.empty.retain(|tick| !inside(*tick));
        // Any in-flight result describes the old revision. It is rejected by
        // `finish`, even when its tick sits outside the dirty spans: retaining
        // the already-rendered cell is safe; adopting work started under an
        // unknown renderer state is not.
        self.in_flight = None;
    }

    /// Move to `step.to` and remember the step for a later rollback.
    fn follow(&mut self, step: Step) {
        self.revision = step.to;
        if self.journal.len() == JOURNAL {
            self.journal.pop_front();
        }
        self.journal.push_back(step);
    }

    fn start(&mut self, tick: i64) {
        self.in_flight = Some(tick);
    }

    fn finish(&mut self, revision: u64, tick: i64, cell: Option<Cell>) {
        if self.in_flight == Some(tick) {
            self.in_flight = None;
        }
        if revision != self.revision {
            return;
        }
        match cell {
            Some(cell) => {
                if self.cells.len() >= MAX_CELLS {
                    self.cells.clear();
                }
                self.cells.insert(tick, cell);
            }
            None => self.empty.push(tick),
        }
    }
}

/// Ticks between filmstrip cells at a given zoom, rounded to a power of two.
///
/// A step derived from one clip's own span would put every clip on a different
/// grid, so nothing would ever be shared and a split would double the renders.
/// Rounding to a power of two also means a small zoom change reuses the cells
/// already rendered instead of asking for a whole new set.
pub fn cell_step(span_ticks: i64, width_px: f32, cell_w: f32) -> i64 {
    if span_ticks <= 0 || width_px <= 0.0 || cell_w <= 0.0 {
        return 0;
    }
    let per_cell = (span_ticks as f64 / f64::from(width_px) * f64::from(cell_w)).max(1.0);
    let exponent = per_cell.log2().round().max(0.0) as u32;
    1i64 << exponent.min(30)
}

/// The ticks a clip's filmstrip is built from: the cells of the global grid
/// that fall inside the clip, so two clips cut from one region share them.
pub fn cell_ticks(start_ticks: i64, end_ticks: i64, width_px: f32, cell_w: f32) -> Vec<i64> {
    let span = end_ticks - start_ticks;
    let step = cell_step(span, width_px, cell_w);
    if step == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut tick = start_ticks - start_ticks.rem_euclid(step);
    if tick < start_ticks {
        tick += step;
    }
    while tick < end_ticks {
        out.push(tick);
        tick += step;
    }
    if out.is_empty() {
        out.push(start_ticks);
    }
    out
}

/// Render one cell if the store wants it. Safe to call from a render: the tick
/// is marked in flight before the thread starts, and nothing here takes the
/// engine lock, which the cell render holds for as long as its decode runs.
pub fn request(state: &Entity<AppState>, cx: &mut App, tick: i64) {
    let wanted = state.update(cx, |state, _| {
        // The revision the drawn timeline was projected from. An engine edit
        // not yet projected reaches the store through `sync_from_engine`,
        // which drops what was rendered before it.
        let revision = state.revision;
        state.filmstrips.sync_revision(revision);
        if !state.filmstrips.wants(tick) {
            return None;
        }
        state.filmstrips.start(tick);
        Some((revision, state.engine().clone(), state.playhead.rate))
    });
    let Some((revision, engine, rate)) = wanted else {
        return;
    };
    let state = state.clone();
    cx.spawn(async move |cx| {
        let at = RationalTime::new(tick, rate);
        // Its own thread, so a codec that panics cannot take the window with it,
        // and the wait is as long as the render: waiting on the foreground
        // executor would hold every animation still for it, and a lane of cells
        // asks for one after another.
        let rendered =
            crate::handoff::on_a_thread(cx.background_executor(), "aria-filmstrip", move || {
                engine
                    .lock()
                    .get_frame_fit(at, CELL_MAX_W, CELL_H)
                    .ok()
                    .and_then(|frame| crate::frame::rgba_to_render(&frame))
            })
            .await
            .flatten();
        state.update(cx, |state, cx| {
            land(
                state,
                revision,
                tick,
                rendered.map(|image| Cell { image }),
                cx,
            );
        });
    })
    .detach();
}

/// Store the cell rendered for `tick` at `revision` and redraw the panel
/// bodies that draw filmstrips, through [`crate::app::FilmstripChanged`].
pub(crate) fn land(
    state: &mut AppState,
    revision: u64,
    tick: i64,
    cell: Option<Cell>,
    cx: &mut Context<AppState>,
) {
    state.filmstrips.finish(revision, tick, cell);
    cx.emit(crate::app::FilmstripChanged);
}

/// A one-pixel cell, for tests that land a rendered cell.
#[cfg(test)]
pub(crate) fn test_cell() -> Cell {
    let buffer = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(1, 1, vec![0u8, 0, 0, 255])
        .expect("a one-pixel buffer");
    Cell {
        image: Arc::new(RenderImage::new(smallvec::SmallVec::from_const([
            image::Frame::new(buffer),
        ]))),
    }
}

#[cfg(test)]
#[path = "filmstrip_tests.rs"]
mod filmstrip_tests;

#[cfg(test)]
#[path = "filmstrip_rollback_tests.rs"]
mod filmstrip_rollback_tests;
