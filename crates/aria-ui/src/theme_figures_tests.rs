//! Which text is shaped with tabular figures.
//!
//! WHY: with `tnum` on every run, Inter's tabular `one` and hyphen, each as
//! wide as a zero, set a clip or file name such as `shot-1080p60-01` like a
//! monospace face. Tabular figures belong to readouts, whose digits change in
//! place. The platform text system is the choke point: every line an element or
//! a hand-built run draws is laid out through it, so a recording text system
//! reads the features each line was shaped with, whatever drew it.
//!
//! Covers the features the window root hands down, the timeline's hand-built
//! clip labels and ruler, the pool's names, and both playhead fields. Does not
//! cover readouts the default layout does not draw (Info, the drop counter, an
//! open value field), or whether the face has the feature.

use std::borrow::Cow;
use std::sync::Arc;

use gpui::{
    Bounds, DevicePixels, Font, FontId, FontMetrics, FontRun, GlyphId, LineLayout, NoopTextSystem,
    Pixels, PlatformTextSystem, RenderGlyphParams, Size, TextRenderingMode,
};
use parking_lot::Mutex;

const PROPORTIONAL: FontId = FontId(1);
const TABULAR: FontId = FontId(2);

/// [`NoopTextSystem`] with one font id per figure style, recording each line it
/// lays out and whether any run of it asked for tabular figures.
#[derive(Default)]
struct Figures {
    lines: Mutex<Vec<(String, bool)>>,
}

impl PlatformTextSystem for Figures {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui::Result<()> {
        NoopTextSystem.add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        NoopTextSystem.all_font_names()
    }

    fn font_id(&self, descriptor: &Font) -> gpui::Result<FontId> {
        let tabular = descriptor
            .features
            .0
            .iter()
            .any(|(tag, on)| tag == "tnum" && *on != 0);
        Ok(if tabular { TABULAR } else { PROPORTIONAL })
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        NoopTextSystem.font_metrics(font_id)
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> gpui::Result<Bounds<f32>> {
        NoopTextSystem.typographic_bounds(font_id, glyph_id)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> gpui::Result<Size<f32>> {
        NoopTextSystem.advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        NoopTextSystem.glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(
        &self,
        params: &RenderGlyphParams,
    ) -> gpui::Result<Bounds<DevicePixels>> {
        NoopTextSystem.glyph_raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> gpui::Result<(Size<DevicePixels>, Vec<u8>)> {
        NoopTextSystem.rasterize_glyph(params, raster_bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        let tabular = runs.iter().any(|run| run.font_id == TABULAR);
        self.lines.lock().push((text.to_owned(), tabular));
        NoopTextSystem.layout_line(text, font_size, runs)
    }

    fn recommended_rendering_mode(&self, font_id: FontId, font_size: Pixels) -> TextRenderingMode {
        NoopTextSystem.recommended_rendering_mode(font_id, font_size)
    }
}

/// A whole line that reads as a non-drop or drop-frame timecode, `00:00:00:00`.
fn is_timecode(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 11
        && bytes.iter().enumerate().all(|(ix, byte)| match ix {
            2 | 5 => *byte == b':',
            8 => matches!(*byte, b':' | b';'),
            _ => byte.is_ascii_digit(),
        })
}

#[test]
fn names_set_in_proportional_figures_and_playhead_readouts_in_tabular() {
    // The media's file stem, drawn by the pool, and the clip's own name, drawn
    // by the clip bar. Both carry the digits and hyphens tnum widens.
    const STEM: &str = "shot-1080p60-01";
    const CLIP: &str = "B-cam-07";
    let figures = Arc::new(Figures::default());
    let mut app =
        gpui::TestApp::with_text_system_and_assets(figures.clone(), Arc::new(crate::icons::Assets));
    let state = app.update(|app| {
        use gpui::AppContext as _;
        crate::theme::Theme::install(
            crate::theme::Appearance::Dark,
            Default::default(),
            None,
            app,
        );
        let ctx = aria_plugin::PluginContext::new();
        ctx.mount(&crate::plugins::UiPlugin)
            .unwrap_or_else(|err| panic!("the ui plugin needs nothing: {err:?}"));
        let dir = crate::scratch_pool::scratch("figures");
        let still = dir.join(format!("{STEM}.png"));
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/texture.png"),
            &still,
        )
        .unwrap_or_else(|err| panic!("copy the still to {}: {err}", still.display()));
        let mut state = crate::app::AppState::new(ctx, dir)
            .unwrap_or_else(|err| panic!("editor state: {err:?}"));
        state
            .new_project()
            .unwrap_or_else(|err| panic!("a project to edit: {err:?}"));
        let media = state
            .import_media(still)
            .unwrap_or_else(|err| panic!("import the still: {err:?}"));
        state
            .drop_media_on_timeline(media, None, 0)
            .unwrap_or_else(|err| panic!("place the still: {err:?}"));
        // A name of its own, so the clip bar's label is a line of its own: the
        // pool tile's name, at the same size and face, would share its layout.
        let clip = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|lane| lane.clips.iter())
            .map(|clip| clip.id)
            .next()
            .unwrap_or_else(|| panic!("the still was placed"));
        state
            .apply_command(aria_commands::Command::Edit(
                aria_commands::EditCommand::SetClipName {
                    clip: aria_models::ClipId::from_raw(clip),
                    name: Some(CLIP.to_owned()),
                },
            ))
            .unwrap_or_else(|err| panic!("name the clip: {err:?}"));
        app.new(|_| state)
    });
    let owned = state.clone();
    let mut window = app.open_window(|_, cx| crate::shell::Shell::new(owned, cx));
    window.draw();

    let lines = figures.lines.lock().clone();
    // The pool's tile and the clip bar's label.
    for name in [STEM, CLIP] {
        let drawn: Vec<&(String, bool)> = lines
            .iter()
            .filter(|(text, _)| text.contains(name))
            .collect();
        assert!(
            !drawn.is_empty(),
            "the name {name:?} was not drawn: {lines:?}"
        );
        for (text, tabular) in drawn {
            assert!(
                !tabular,
                "the name {text:?} was shaped with tabular figures"
            );
        }
    }

    let clocks: Vec<&(String, bool)> = lines.iter().filter(|(text, _)| is_timecode(text)).collect();
    // The timeline's and the Viewer's playhead fields, and the ruler's labels.
    assert!(
        clocks.len() >= 3,
        "{} timecode line(s) drawn, short of the two playhead fields and a ruler label: {lines:?}",
        clocks.len()
    );
    for (text, tabular) in clocks {
        assert!(
            tabular,
            "the readout {text:?} was shaped with proportional figures"
        );
    }

    // gpui's leak detector fails the case for any entity alive after shutdown.
    drop(state);
    drop(window);
    app.update(|app| app.shutdown());
}
