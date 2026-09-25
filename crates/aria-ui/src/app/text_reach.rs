//! Title diagnostics and font preflight: the `aria-text` answers no surface asked for.
//!
//! Three capabilities lived in `aria-text` with no operator path:
//!
//! - [`aria_text::TextRenderer::load_font`] and
//!   [`aria_text::TextRenderer::font_count`]. `fontdb` drops unparseable bytes
//!   without complaint, so the face count is the only load-time failure signal
//!   a font file ever produces. Nothing read it, and a font that will not parse
//!   is indistinguishable from one the editor simply does not list.
//! - [`aria_text::ShapedText::has_ink`] and
//!   [`aria_text::paint_animated`]/[`aria_text::AnimatedText::has_ink`]. A title
//!   that draws nothing looks exactly like a title behind another layer, a title
//!   at zero opacity, and a title whose font produced no glyphs. Shaping it and
//!   saying which of those it is turns a guess into an answer.
//!
//! Neither loads a face into the session. The preview and the export share one
//! [`aria_render::Renderer`] inside the engine and it exposes no font hook, so a
//! face loaded into a UI-side renderer would make the inspector offer a family
//! the exported frame renders as the default. A preflight reports; it does not
//! install.

use std::path::Path;

use aria_render::LayerSource;
use aria_text::{TextRenderer, TextStyle};

use super::AppState;
use super::picker::{Answer, Ask, FileSpec};

/// Extensions a font preflight offers, and what the dialog calls the group.
///
/// Collections (`.ttc`, `.otc`) are listed because they are the case the face
/// count exists for: one file, several faces, and a count above one is the
/// only way to see that from outside.
const FONT_FILTER: (&str, &[&str]) = ("Font files", &["ttf", "otf", "ttc", "otc"]);

impl AppState {
    /// Read a font file and report what a shaper would make of it.
    ///
    /// Nothing is installed and nothing is written. The report is the face
    /// count the bytes added to a throwaway font set: `0` means `fontdb`
    /// refused them, which is the only failure a bad font file reports.
    pub fn check_font_file(&mut self) {
        let spec = FileSpec::new("Check a font file").filter(FONT_FILTER.0, FONT_FILTER.1);
        self.request_pick(Ask::OpenFile(spec), |state, answer: Answer| {
            let Some(path) = answer.one() else {
                return;
            };
            state.status_message = Some(font_file_report(&path));
        });
    }

    /// Shape the selected title and report whether it puts ink on the canvas.
    ///
    /// Resolved through [`aria_render::resolve`] at the playhead rather than
    /// read off the clip, so the content and style measured here are the ones
    /// the compositor would draw at this instant — including the animation slot
    /// the clip is inside.
    pub fn report_title_ink(&mut self) {
        self.status_message = Some(match self.title_ink_report() {
            Some(report) => report,
            None => "Select a title clip to measure its ink".to_string(),
        });
    }

    /// The selected title's shaping report, or `None` when the selection places
    /// no text layer at the playhead.
    fn title_ink_report(&self) -> Option<String> {
        let clip = self.selected_clip()?;
        let (content, style, animated) = {
            let engine = self.engine.lock();
            let project = engine.project();
            let rate = project.timeline().frame_rate;
            let at = aria_models::RationalTime::new(self.store.playhead_ticks, rate);
            let scene = aria_render::resolve(project, at).ok()?;
            let layer = scene
                .layers
                .into_iter()
                .find(|layer| layer.clip == Some(clip))?;
            // Only a text layer carries a string and a text style; the other
            // topologies have no answer to extract, not a different answer.
            let LayerSource::Text {
                content,
                style,
                animation,
            } = layer.source
            else {
                return None;
            };
            (content, style, animation)
        };
        Some(shaping_report(&content, &style, animated.is_some()))
    }
}

/// What a font file's bytes are worth to a shaper.
///
/// A fresh [`TextRenderer`] rather than a shared one: this must not change what
/// the session shapes with, and the before/after counts are only meaningful
/// against a set nothing else has touched.
fn font_file_report(path: &Path) -> String {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(err) => return format!("{}: {err}", path.display()),
    };
    let bytes = data.len();
    let mut renderer = TextRenderer::new();
    let installed = renderer.font_count();
    let added = renderer.load_font(data);
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    if added == 0 {
        return format!("{name}: {bytes} bytes, not a parseable font ({installed} system faces)");
    }
    format!("{name}: {added} face(s) parsed from {bytes} bytes, beside {installed} system faces")
}

/// Whether a shaped run produces coverage, and what its animated paint adds.
///
/// `animated` decides whether the per-cluster paint is measured at all, because
/// that is the pass the renderer runs only for an animated title: reporting an
/// animated extent for a static one would describe work nothing does.
fn shaping_report(content: &str, style: &TextStyle, animated: bool) -> String {
    let mut renderer = TextRenderer::new();
    let faces = renderer.font_count();
    let shaped = renderer.shape(content, style);
    let clusters = shaped.clusters.len();
    if !shaped.has_ink() {
        // Three different causes, and the counts separate them: no clusters is
        // an empty string, clusters with no extent is whitespace or a font that
        // produced no glyphs, and no faces at all is a host with no fonts.
        return format!(
            "Draws nothing: {clusters} cluster(s), no coverage, {faces} face(s) available"
        );
    }
    let (w, h) = shaped.extent;
    if !animated {
        return format!("{w}x{h} px of ink from {clusters} cluster(s), {faces} face(s) available");
    }
    let painted = aria_text::paint_animated(&shaped, style);
    let (pw, ph) = painted.extent;
    let card = match &painted.background {
        Some(image) => format!(", {}x{} background card", image.width, image.height),
        None => String::new(),
    };
    format!(
        "{w}x{h} px of ink; animated paint {pw}x{ph} over {} cluster(s){card}",
        painted.clusters.len()
    )
}

// The canvas pick lives beside this file: it is the same "a backend answer with
// no surface" work, on `aria-shapes` rather than `aria-text`, and keeping both
// here would put the file over the ceiling.
#[path = "text_reach_shapes.rs"]
mod shapes;

#[cfg(test)]
#[path = "text_reach_tests.rs"]
mod tests;
