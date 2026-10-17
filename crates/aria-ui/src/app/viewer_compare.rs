//! Viewer comparison: the Program frame split with a reference picture.
//!
//! View > Compare holds a reference beside the Program frame: the sequence
//! frame at the playhead when the row was chosen, a still read from disk, or
//! the source in the Source monitor at its own playhead. A wipe draws the
//! reference over the Program frame on one side of the divider, split
//! vertically or horizontally; Side by Side draws it in a pane of its own left
//! of Program. The Program frame keeps playing and scrubbing beside it, and the
//! divider is dragged in the Viewer.
//!
//! A frame or a still is rendered off the UI thread. A reference replaced
//! before its picture arrives drops the late picture, so the picture drawn is
//! always the one the last row asked for.

use std::path::PathBuf;
use std::sync::Arc;

use aria_core::RationalTime;
use gpui::{Context, RenderImage};

use super::AppState;
use super::picker::{Answer, Ask, FileSpec};

/// What the Program frame is compared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    /// The sequence frame at this time.
    Frame { at: RationalTime },
    /// A still image file.
    Still { path: PathBuf },
    /// The source in the Source monitor, at its own playhead.
    Source,
}

/// How the reference and the Program frame share the Viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompareLayout {
    /// The reference left of a vertical divider, Program right of it.
    #[default]
    Vertical,
    /// The reference above a horizontal divider, Program below it.
    Horizontal,
    /// The reference and Program in panes of their own, side by side.
    SideBySide,
}

impl CompareLayout {
    /// Whether the divider moves along y.
    pub fn horizontal(self) -> bool {
        self == Self::Horizontal
    }
}

/// The comparison the Viewer draws, if any.
#[derive(Debug)]
pub struct ViewerCompare {
    reference: Option<Reference>,
    layout: CompareLayout,
    /// The divider, from 0 at the left (or the top) of the stage to 1 at the
    /// right (or the bottom).
    divider: f32,
    /// The reference's picture once rendered or decoded. A Source reference
    /// is read from its Source tab instead.
    picture: Option<Arc<RenderImage>>,
    /// Counts references, so a picture for one already replaced is dropped.
    generation: u64,
    /// The stage the divider spans, `[x, y, width, height]` in window pixels,
    /// from the last paint.
    stage: Option<[f32; 4]>,
    /// The divider is held.
    dragging: bool,
}

impl Default for ViewerCompare {
    fn default() -> Self {
        Self {
            reference: None,
            layout: CompareLayout::default(),
            divider: 0.5,
            picture: None,
            generation: 0,
            stage: None,
            dragging: false,
        }
    }
}

impl ViewerCompare {
    pub fn reference(&self) -> Option<&Reference> {
        self.reference.as_ref()
    }

    pub fn comparing(&self) -> bool {
        self.reference.is_some()
    }

    pub fn layout(&self) -> CompareLayout {
        self.layout
    }

    pub fn divider(&self) -> f32 {
        self.divider
    }

    /// The held frame or still, once it has arrived.
    pub fn picture(&self) -> Option<&Arc<RenderImage>> {
        self.picture.as_ref()
    }

    pub fn dragging(&self) -> bool {
        self.dragging
    }

    /// The stage painted at `stage`. Returns whether it moved.
    pub fn record_stage(&mut self, stage: [f32; 4]) -> bool {
        let moved = self.stage != Some(stage);
        self.stage = Some(stage);
        moved
    }

    /// Replace the reference, dropping the old picture. Returns the
    /// generation a picture for this reference lands with.
    fn begin(&mut self, reference: Reference) -> u64 {
        self.generation += 1;
        self.reference = Some(reference);
        self.picture = None;
        self.dragging = false;
        self.generation
    }

    /// A picture arrived for generation `generation`. Returns whether it is
    /// the current reference's and was kept.
    fn land(&mut self, generation: u64, picture: Arc<RenderImage>) -> bool {
        let current = generation == self.generation && self.reference.is_some();
        if current {
            self.picture = Some(picture);
        }
        current
    }
}

/// Where the divider sits for a pointer at `at` over `stage`, clamped to the
/// stage. `None` for a stage with no extent along the divider's axis.
pub fn divider_at(layout: CompareLayout, stage: [f32; 4], at: [f32; 2]) -> Option<f32> {
    let (origin, span, pointer) = if layout.horizontal() {
        (stage[1], stage[3], at[1])
    } else {
        (stage[0], stage[2], at[0])
    };
    (span > 0.0).then(|| ((pointer - origin) / span).clamp(0.0, 1.0))
}

impl AppState {
    /// View > Compare > Current Frame: hold the sequence frame at the playhead
    /// as the reference.
    pub fn compare_with_frame(&mut self, cx: &mut Context<Self>) {
        let at = self.playhead;
        let generation = self.viewer_compare.begin(Reference::Frame { at });
        self.sources.focus_program();
        let project = self.host.snapshot();
        let fit = self.preview_fit();
        let gpu = super::render_device::shared_device(&self.ctx);
        self.load_reference(generation, cx, move || {
            let mut renderer = super::render_device::renderer_on(gpu, "the comparison frame")?;
            let frame = match fit {
                Some((width, height)) => renderer.render_frame_fit(&project, at, width, height),
                None => renderer.render_frame(&project, at),
            };
            frame.map_err(|err| format!("rendering the comparison frame: {err}"))
        });
        self.status_message = Some(format!(
            "Comparing with {}",
            crate::timecode::format_ticks(at.value, at.rate.num, at.rate.den)
        ));
    }

    /// View > Compare > Still: ask for an image, then compare with it.
    pub fn pick_compare_still(&mut self, cx: &mut Context<Self>) {
        let extensions = aria_decoder::image_extensions();
        let (wake, wakes) = crate::handoff::wakes::<PathBuf>();
        self.request_pick(
            Ask::OpenFile(FileSpec::new("Compare with Still").filter("Image", &extensions)),
            move |_, answer: Answer| {
                if let Some(path) = answer.one() {
                    wake.wake(path);
                }
            },
        );
        cx.spawn(async move |this, cx| {
            let Some(path) = wakes.next().await else {
                return;
            };
            let _ = this.update(cx, |state, cx| state.compare_with_still(path, cx));
        })
        .detach();
    }

    /// Hold the still at `path` as the reference.
    pub fn compare_with_still(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.status_message = Some(format!("Comparing with {}", path.display()));
        let generation = self
            .viewer_compare
            .begin(Reference::Still { path: path.clone() });
        self.sources.focus_program();
        self.load_reference(generation, cx, move || {
            aria_decoder::decode_image(&path)
                .map_err(|err| format!("reading the still {}: {err}", path.display()))
        });
    }

    /// View > Compare > Source Tab: compare with the source in the Source
    /// monitor, at its own playhead.
    pub fn compare_with_source(&mut self) {
        let Some(name) = self.monitor_source().map(|view| view.name.clone()) else {
            self.status_message = Some("Open a source in the Source monitor first.".into());
            return;
        };
        self.viewer_compare.begin(Reference::Source);
        self.sources.focus_program();
        self.status_message = Some(format!("Comparing with {name}"));
    }

    /// Draw the comparison as `layout`. Kept for the next comparison when
    /// nothing is compared.
    pub fn set_compare_layout(&mut self, layout: CompareLayout) {
        self.viewer_compare.layout = layout;
    }

    /// View > Compare > Stop Comparing. The layout and the divider stay for
    /// the next comparison.
    pub fn stop_comparing(&mut self) {
        let compare = &mut self.viewer_compare;
        compare.generation += 1;
        compare.reference = None;
        compare.picture = None;
        compare.dragging = false;
    }

    /// A press on the divider.
    pub fn grab_compare_divider(&mut self) {
        self.viewer_compare.dragging = self.viewer_compare.comparing();
    }

    /// The pointer moved to `at` in window pixels. Returns whether the held
    /// divider moved.
    pub fn drag_compare_divider(&mut self, at: [f32; 2]) -> bool {
        let compare = &mut self.viewer_compare;
        if !compare.dragging {
            return false;
        }
        let Some(divider) = compare
            .stage
            .and_then(|stage| divider_at(compare.layout, stage, at))
        else {
            return false;
        };
        let moved = divider != compare.divider;
        compare.divider = divider;
        moved
    }

    /// Let go of the divider. Returns whether it was held.
    pub fn release_compare_divider(&mut self) -> bool {
        std::mem::take(&mut self.viewer_compare.dragging)
    }

    /// Run `load` off the UI thread and hold its picture for reference
    /// `generation`. A failure drops the comparison and reports why.
    fn load_reference(
        &mut self,
        generation: u64,
        cx: &mut Context<Self>,
        load: impl FnOnce() -> Result<aria_core::RgbaImage, String> + Send + 'static,
    ) {
        let task = cx.background_executor().spawn(async move {
            load().and_then(|image| {
                crate::frame::rgba_to_render(&image)
                    .ok_or_else(|| "the reference picture has no pixels".to_string())
            })
        });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            let _ = this.update(cx, |state, cx| {
                match outcome {
                    Ok(picture) => {
                        state.viewer_compare.land(generation, picture);
                    }
                    Err(reason) if state.viewer_compare.generation == generation => {
                        state.stop_comparing();
                        state.report_error(reason);
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "viewer_compare_tests.rs"]
mod tests;
