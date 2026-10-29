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
//!
//! The timeline's draw hands the store the cells it drew without their exact
//! frame, once for all its lane groups, and the store renders them one at a
//! time, starting the next as one lands, so a strip fills without waiting a
//! frame between cells. The next cell is the first at or after the one
//! started last, so the renderer's decoders roll forward from one cell to the
//! next and decode no frame twice; after the last, the strip wraps to its
//! first cell still owed. More renderers do not fill a strip faster: their
//! decoders share the host's decode capacity, and each seeks to a sync frame
//! of its own.
//!
//! A cell the draw finds blank, with neither a picture of its own nor a
//! stand-in, renders first snapped: its media decodes the sync frame at or
//! before the cell's tick, one seek and one decode, where the exact frame
//! costs every frame from that sync frame on. Every blank cell snaps before
//! any cell renders exactly, so an empty strip fills at one decode per cell.
//! A snapped cell draws until the exact frame replaces it, and the draw asks
//! for it again as it asks for a missing cell. A snap that lands on the frame
//! at the tick is stored as exact.
//!
//! Cells render on a [`CellRenderer`] of their own, so a cell neither holds the
//! engine through its decode nor moves the decoders the Viewer's frames are
//! read from. A cell the store drops leaves the window atlas with the Viewer's
//! replaced frames, through `AppState::release_retired_images`, and at the
//! latest when the next cell lands.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use aria_engine::{CellRenderer, CellSource, SeekPolicy};
use aria_models::RationalTime;
use aria_plugin::Reverted;
use gpui::{App, Context, Entity, RenderImage};
use parking_lot::Mutex;

use crate::app::AppState;

/// Height of a filmstrip cell, and of the strip. A 16:9 cell is then 60px wide,
/// which is about one cell per second at the default zoom.
pub const CELL_H: u32 = 34;

/// Widest cell that is ever requested, so a very wide canvas cannot make a
/// thumbnail that costs more than the strip can show.
const CELL_MAX_W: u32 = 96;

/// Cells kept across all clips. A 16:9 cell is about 8 KB, so the store holds
/// at most about 8 MB. A cell landing in a full store drops the cell farthest
/// from it, which is the one farthest from the view.
const MAX_CELLS: usize = 1024;

/// Edits the store remembers for a rollback to walk back. A gesture's
/// rollback walks back the few edits of one pointer move; a rollback that
/// reaches past the oldest one drops every cell.
const JOURNAL: usize = 64;

/// One rendered cell.
#[derive(Clone)]
pub struct Cell {
    pub image: Arc<RenderImage>,
    /// The picture is the sync frame at or before the cell's tick, drawn until
    /// the exact frame lands.
    pub snapped: bool,
}

/// One edit the store followed: the revision moved from `from` to `to`, and
/// the picture changed only inside `spans`.
struct Step {
    from: u64,
    to: u64,
    spans: Vec<(i64, i64)>,
}

/// The render in flight: the tick and the revision it renders.
#[derive(Clone, Copy)]
struct Flight {
    tick: i64,
    revision: u64,
}

/// Cells by the tick they were rendered at.
#[derive(Default)]
pub struct FilmstripStore {
    revision: u64,
    cells: BTreeMap<i64, Cell>,
    /// The render in flight. A render started before an edit holds the
    /// renderer until it lands, and lands nothing.
    flight: Option<Flight>,
    /// Ticks that produced no frame, so a gap in the sequence is not retried
    /// every frame for as long as it stays on screen.
    empty: BTreeSet<i64>,
    /// The cells the timeline's last draw drew without their exact frame, in
    /// tick order.
    queue: Vec<i64>,
    /// The cells of `queue` the last draw drew with no picture at all, in tick
    /// order. They render snapped before any cell renders exactly.
    blank: Vec<i64>,
    /// The latest edits followed by span, oldest first. The newest ends at
    /// `revision`; the store forgets them all when it drops every cell.
    journal: VecDeque<Step>,
    /// The renderer cells are drawn on.
    renderer: Arc<Mutex<CellRenderer>>,
    /// The tick of the cell started last exactly, which is where the
    /// renderer's decoders stand. `None` after a snap, which leaves them at a
    /// sync frame rather than at a cell.
    at: Option<i64>,
    /// Images of dropped cells, for the window atlas to release. Cells drop
    /// on an edit or a landing, and both redraw the timeline without them.
    retired: Vec<Arc<RenderImage>>,
}

impl FilmstripStore {
    pub fn get(&self, tick: i64) -> Option<&Cell> {
        self.cells.get(&tick)
    }

    /// The rendered cell nearest `tick` inside `[within.0, within.1)`: the
    /// last at or before it, or else the first after it. A cell not rendered
    /// yet draws this one in its place, which after a zoom in is the frame
    /// the strip showed there before.
    pub fn stand_in(&self, tick: i64, within: (i64, i64)) -> Option<&Cell> {
        let (from, until) = within;
        if from >= until {
            return None;
        }
        let tick = tick.clamp(from, until - 1);
        self.cells
            .range(from..=tick)
            .next_back()
            .or_else(|| self.cells.range(tick + 1..until).next())
            .map(|(_, cell)| cell)
    }

    /// How many cells are being rendered now.
    pub fn in_flight(&self) -> usize {
        usize::from(self.flight.is_some())
    }

    /// How many cells the timeline's last draw drew with no picture, less
    /// those landed since.
    pub fn blanks(&self) -> usize {
        self.blank.len()
    }

    /// Whether `tick` should be rendered exactly: there is no retained cell
    /// for it or only a snapped one, and it did not already come back empty.
    fn wants(&self, tick: i64) -> bool {
        !self.empty.contains(&tick) && self.cells.get(&tick).is_none_or(|cell| cell.snapped)
    }

    /// Whether `tick` should be rendered snapped: there is no retained cell
    /// for it, and it did not already come back empty.
    fn wants_snap(&self, tick: i64) -> bool {
        !self.cells.contains_key(&tick) && !self.empty.contains(&tick)
    }

    /// Drop everything rendered against an older revision. The fallback for a
    /// change that was not followed by span: a mutation that reports no
    /// picture spans, an undo, or an edit made outside `apply_command`.
    pub fn sync_revision(&mut self, revision: u64) {
        if self.revision != revision {
            self.revision = revision;
            let cells = std::mem::take(&mut self.cells);
            self.retired
                .extend(cells.into_values().map(|cell| cell.image));
            self.empty.clear();
            self.journal.clear();
            // A render started against the old project lands nothing: its
            // revision no longer matches.
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

    /// Drop the cells and empty results inside `spans`. A render in flight
    /// lands nothing: the step that follows moves the revision past it.
    fn drop_spans(&mut self, spans: &[(i64, i64)]) {
        let inside = |tick: i64| {
            spans
                .iter()
                .any(|&(start, end)| tick >= start && tick < end)
        };
        let retired = &mut self.retired;
        self.cells.retain(|tick, cell| {
            let keep = !inside(*tick);
            if !keep {
                retired.push(cell.image.clone());
            }
            keep
        });
        self.empty.retain(|tick| !inside(*tick));
    }

    /// Move to `step.to` and remember the step for a later rollback.
    fn follow(&mut self, step: Step) {
        self.revision = step.to;
        if self.journal.len() == JOURNAL {
            self.journal.pop_front();
        }
        self.journal.push_back(step);
    }

    /// Mark `tick` in flight at the store's revision, rendered under `policy`.
    fn start(&mut self, tick: i64, policy: SeekPolicy) {
        self.flight = Some(Flight {
            tick,
            revision: self.revision,
        });
        self.at = (policy == SeekPolicy::Exact).then_some(tick);
    }

    /// The next tick to render and how, marked in flight, while the renderer
    /// is free: the first blank cell snapped while one is left, else the first
    /// queued cell at or after the one started last, or else the first,
    /// exactly. A snap costs one decode wherever the decoders stand, so blank
    /// cells snap left to right, and the exact walk after them starts again
    /// at the first cell. Ticks no longer wanted leave their list.
    fn next_flight(&mut self) -> Option<(i64, SeekPolicy)> {
        if self.flight.is_some() {
            return None;
        }
        let mut blank = std::mem::take(&mut self.blank);
        blank.retain(|&tick| self.wants_snap(tick));
        self.blank = blank;
        let mut queue = std::mem::take(&mut self.queue);
        queue.retain(|&tick| self.wants(tick));
        self.queue = queue;
        let (tick, policy) = match self.blank.first() {
            Some(&tick) => (tick, SeekPolicy::NearestSync),
            None => {
                let ahead = self
                    .at
                    .map_or(0, |at| self.queue.partition_point(|&tick| tick < at));
                let tick = *self.queue.get(ahead).or_else(|| self.queue.first())?;
                (tick, SeekPolicy::Exact)
            }
        };
        self.start(tick, policy);
        Some((tick, policy))
    }

    fn finish(&mut self, revision: u64, tick: i64, cell: Option<Cell>) {
        if self
            .flight
            .is_some_and(|flight| flight.tick == tick && flight.revision == revision)
        {
            self.flight = None;
        }
        if revision != self.revision {
            return;
        }
        match cell {
            Some(cell) => {
                if !self.cells.contains_key(&tick) && self.cells.len() >= MAX_CELLS {
                    self.drop_farthest_from(tick);
                }
                if let Some(replaced) = self.cells.insert(tick, cell) {
                    self.retired.push(replaced.image);
                }
            }
            None => {
                self.empty.insert(tick);
            }
        }
    }

    /// Drop the retained cell farthest from `tick`, which is the first or the
    /// last.
    fn drop_farthest_from(&mut self, tick: i64) {
        let (Some((&first, _)), Some((&last, _))) =
            (self.cells.first_key_value(), self.cells.last_key_value())
        else {
            return;
        };
        let far = if tick.abs_diff(first) >= tick.abs_diff(last) {
            first
        } else {
            last
        };
        if let Some(cell) = self.cells.remove(&far) {
            self.retired.push(cell.image);
        }
    }

    /// Hand the images of dropped cells to `out`, the window atlas's release
    /// list.
    pub(crate) fn retire_into(&mut self, out: &mut Vec<Arc<RenderImage>>) {
        out.append(&mut self.retired);
    }

    /// Store `cell` at `tick` as rendered at the store's revision and landed.
    #[cfg(test)]
    pub(crate) fn hold(&mut self, tick: i64, cell: Cell) {
        self.start(tick, SeekPolicy::Exact);
        self.finish(self.revision, tick, Some(cell));
    }

    /// The cells the store was asked for and has not landed, and the ones of
    /// them asked for snapped.
    #[cfg(test)]
    pub(crate) fn asked(&self) -> (&[i64], &[i64]) {
        (&self.queue, &self.blank)
    }
}

/// Ticks between filmstrip cells at a given zoom: the largest power of two no
/// longer than one cell, so neighboring cells abut or overlap and the strip
/// leaves no gap, while no more than two cells are asked for per cell width.
///
/// A step derived from one clip's own span would put every clip on a different
/// grid, so nothing would ever be shared and a split would double the renders.
/// A power of two also means a small zoom change reuses the cells already
/// rendered instead of asking for a whole new set. The step is never under one
/// tick: where one frame is wider than a cell, its cell spans the frame.
pub fn cell_step(span_ticks: i64, width_px: f32, cell_w: f32) -> i64 {
    if span_ticks <= 0 || width_px <= 0.0 || cell_w <= 0.0 {
        return 0;
    }
    let per_cell = (span_ticks as f64 / f64::from(width_px) * f64::from(cell_w)).max(1.0);
    let exponent = per_cell.log2().floor() as u32;
    1i64 << exponent.min(30)
}

/// The cells a clip's filmstrip is built from, as `[tick, until)` spans: the
/// clip's first tick up to the global grid, then the cells of the grid inside
/// the clip, so two clips cut from one region share every cell but their
/// heads. The spans cover the clip end to end; only those that overlap the
/// visible ticks `view` are returned, so a long clip asks for the cells on
/// screen and no others.
pub fn cell_spans(
    start_ticks: i64,
    end_ticks: i64,
    width_px: f32,
    cell_w: f32,
    view: (i64, i64),
) -> Vec<(i64, i64)> {
    let step = cell_step(end_ticks - start_ticks, width_px, cell_w);
    let (from, to) = (view.0.max(start_ticks), view.1.min(end_ticks));
    if step == 0 || from >= to {
        return Vec::new();
    }
    let grid = |tick: i64| tick - tick.rem_euclid(step);
    let mut out = Vec::new();
    let mut tick = grid(from);
    if tick <= start_ticks {
        tick = grid(start_ticks) + step;
        out.push((start_ticks, tick.min(end_ticks)));
    }
    while tick < to {
        out.push((tick, (tick + step).min(end_ticks)));
        tick += step;
    }
    out
}

/// Render the cells at `missing`, the ticks the timeline's draw drew without
/// their exact frame, snapping first the ones of them at `blank`, the ticks it
/// drew with no picture at all. They replace the ones an earlier draw asked
/// for. Safe to call from a render: nothing here takes the engine lock, and
/// each render thread holds it only to take the cell's source.
pub fn ask(state: &Entity<AppState>, cx: &mut App, mut missing: Vec<i64>, mut blank: Vec<i64>) {
    for ticks in [&mut missing, &mut blank] {
        ticks.sort_unstable();
        ticks.dedup();
    }
    state.update(cx, |state, cx| {
        state.filmstrips.queue = missing;
        state.filmstrips.blank = blank;
        pump(state, cx);
    });
}

/// Start the next queued cell when the renderer is free.
fn pump(state: &mut AppState, cx: &mut Context<AppState>) {
    // The revision the drawn timeline was projected from. An engine edit not
    // yet projected reaches the store through `sync_from_engine`, which drops
    // what was rendered before it.
    let revision = state.revision;
    state.filmstrips.sync_revision(revision);
    let Some((tick, policy)) = state.filmstrips.next_flight() else {
        return;
    };
    let engine = state.engine().clone();
    let renderer = Arc::clone(&state.filmstrips.renderer);
    let at = RationalTime::new(tick, state.playhead.rate);
    // Its own thread, so a codec that panics cannot take the window with it,
    // started here so the render starts now rather than when the task that
    // waits for it first runs. The wait is as long as the render: waiting on
    // the foreground executor would hold every animation still for it.
    let rendering =
        crate::handoff::on_a_thread(cx.background_executor(), "aria-filmstrip", move || {
            let source = engine.lock().cell_source();
            render_cell(&mut renderer.lock(), &source, at, policy)
        });
    cx.spawn(async move |this, cx| {
        let rendered = rendering.await.flatten();
        let _ = this.update(cx, |state, cx| {
            land(state, revision, tick, rendered, cx);
        });
    })
    .detach();
}

/// The cell at `at`: the frame `source` plays there, its media decoded under
/// `policy`, fit to a cell and drawn on `renderer`. `None` where nothing plays
/// or the render fails.
pub fn render_cell(
    renderer: &mut CellRenderer,
    source: &CellSource,
    at: RationalTime,
    policy: SeekPolicy,
) -> Option<Cell> {
    let frame = renderer
        .render_fit(source, at, CELL_MAX_W, CELL_H, policy)
        .ok()?;
    Some(Cell {
        image: crate::frame::rgba_to_render(&frame.image)?,
        snapped: frame.snapped,
    })
}

/// Store the cell rendered for `tick` at `revision`, start the next queued
/// cell, release the cells the store dropped from the window atlas, and
/// redraw the panel bodies that draw filmstrips, through
/// [`crate::app::FilmstripChanged`]. Called where no window is being updated.
pub(crate) fn land(
    state: &mut AppState,
    revision: u64,
    tick: i64,
    cell: Option<Cell>,
    cx: &mut Context<AppState>,
) {
    state.filmstrips.finish(revision, tick, cell);
    pump(state, cx);
    state.release_retired_images(None, cx);
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
        snapped: false,
    }
}

#[cfg(test)]
#[path = "filmstrip_tests.rs"]
mod filmstrip_tests;

#[cfg(test)]
#[path = "filmstrip_rollback_tests.rs"]
mod filmstrip_rollback_tests;

#[cfg(test)]
#[path = "filmstrip_fill_tests.rs"]
mod filmstrip_fill_tests;
