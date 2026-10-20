//! Embedded Lucide icons, the brand mark, and the gpui [`AssetSource`] that
//! serves them.
//!
//! Art lives in repo-root `assets/icon/`. An icon paints its glyph as a mask
//! in the text colour of the control around it, so one glyph serves both
//! appearances; the brand mark is raster art with its own colours and is drawn
//! through [`brand_mark`]. A plugin command's icon is served under
//! `plugin-icon/` once `plugin_icon_path` has named it.

use std::borrow::Cow;
use std::collections::HashMap;
use std::panic::Location;
use std::sync::{Arc, LazyLock};

use parking_lot::RwLock;

use gpui::{
    App, AssetSource, Bounds, Element, ElementId, GlobalElementId, ImageSource, Img,
    InspectorElementId, IntoElement, LayoutId, Pixels, Result, SharedString, StyleRefinement,
    Styled, Svg, Transformation, Window, img, px, svg,
};

macro_rules! icon_assets {
    ($(($const_name:ident, $file:literal, $path:literal)),+ $(,)?) => {
        $(pub const $const_name: &str = $path;)+

        /// Every registered icon as its constant's name and its file under
        /// `assets/icon/`, in registry order.
        #[cfg(test)]
        const REGISTRY: &[(&str, &str)] = &[$((stringify!($const_name), $file)),+];

        /// Serves the embedded icons to gpui's SVG renderer.
        pub struct Assets;

        impl AssetSource for Assets {
            fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
                Ok(match path {
                    $($path => Some(Cow::Borrowed(
                        include_bytes!(concat!("../../../assets/icon/", $file)).as_slice(),
                    )),)+
                    _ => plugin_icon(path),
                })
            }

            fn list(&self, path: &str) -> Result<Vec<SharedString>> {
                let all = [$($path),+];
                Ok(all
                    .iter()
                    .filter(|p| p.starts_with(path))
                    .map(|p| SharedString::from(*p))
                    .collect())
            }
        }
    };
}

icon_assets![
    // The brand mark, and the only raster row: the same art the packagers turn
    // into the app icon, so the mark in the window and the icon in the dock
    // cannot drift apart.
    (BRAND_MARK, "aria-icon.png", "icon/aria-icon.png"),
    (PLAY, "play.svg", "icon/play.svg"),
    (PAUSE, "pause.svg", "icon/pause.svg"),
    (PLUS, "plus.svg", "icon/plus.svg"),
    (FOLDER_OPEN, "folder-open.svg", "icon/folder-open.svg"),
    (CLAPPERBOARD, "clapperboard.svg", "icon/clapperboard.svg"),
    (IMAGE, "library/image.svg", "icon/image.svg"),
    (SPARKLES, "shell/sparkles.svg", "icon/sparkles.svg"),
    (EYE, "timeline/eye.svg", "icon/eye.svg"),
    (EYE_OFF, "timeline/eye-off.svg", "icon/eye-off.svg"),
    (LOCK, "timeline/lock.svg", "icon/lock.svg"),
    (LOCK_OPEN, "timeline/lock-open.svg", "icon/lock-open.svg"),
    (VOLUME, "timeline/volume-2.svg", "icon/volume.svg"),
    (VOLUME_X, "timeline/volume-x.svg", "icon/volume-x.svg"),
    (HEADPHONES, "timeline/headphones.svg", "icon/headphones.svg"),
    (CROSSHAIR, "timeline/crosshair.svg", "icon/crosshair.svg"),
    (LINK, "timeline/link.svg", "icon/link.svg"),
    (UNLINK, "timeline/unlink.svg", "icon/unlink.svg"),
    (
        AUDIO_LINES,
        "timeline/audio-lines.svg",
        "icon/audio-lines.svg"
    ),
    (
        ROTATE_CCW,
        "inspector/rotate-ccw.svg",
        "icon/rotate-ccw.svg"
    ),
    (UPLOAD, "shell/upload.svg", "icon/upload.svg"),
    (SETTINGS, "shell/settings.svg", "icon/settings.svg"),
    (TRASH, "inspector/trash-2.svg", "icon/trash.svg"),
    (ELLIPSIS, "inspector/ellipsis.svg", "icon/ellipsis.svg"),
    (FIT, "fullscreen.svg", "icon/fit.svg"),
    (SCISSORS, "timeline/scissors.svg", "icon/scissors.svg"),
    (ZOOM_IN, "timeline/zoom-in.svg", "icon/zoom-in.svg"),
    (ZOOM_OUT, "timeline/zoom-out.svg", "icon/zoom-out.svg"),
    (MAGNET, "timeline/magnet.svg", "icon/magnet.svg"),
    (WRENCH, "timeline/wrench.svg", "icon/wrench.svg"),
    // The timeline header: a marker at the playhead, and the graph editor.
    (FLAG, "timeline/flag.svg", "icon/flag.svg"),
    (GRAPH, "timeline/chart-spline.svg", "icon/graph.svg"),
    (MINUS, "shell/minus.svg", "icon/minus.svg"),
    (SQUARE, "shell/square.svg", "icon/square.svg"),
    (CLOSE, "shell/x.svg", "icon/close.svg"),
    // The five trim tools. Each is a Lucide glyph chosen for the gesture: the
    // take moving inside a fixed clip, a clip between two neighbours, the shared
    // cut being dragged, playback speed, and a gap closing.
    (SLIP, "timeline/move-horizontal.svg", "icon/slip.svg"),
    (SLIDE, "timeline/columns-3.svg", "icon/slide.svg"),
    (ROLL, "timeline/grip-vertical.svg", "icon/roll.svg"),
    (RATE, "timeline/gauge.svg", "icon/rate.svg"),
    (RIPPLE, "timeline/fold-horizontal.svg", "icon/ripple.svg"),
    (PEN, "inspector/spline.svg", "icon/pen.svg"),
    (TYPE, "library/text.svg", "icon/type.svg"),
    // The selection, track select and hand tools.
    (POINTER, "timeline/mouse-pointer-2.svg", "icon/pointer.svg"),
    (HAND, "timeline/hand.svg", "icon/hand.svg"),
    (
        CHEVRONS_RIGHT,
        "timeline/chevrons-right.svg",
        "icon/chevrons-right.svg"
    ),
    (
        CHEVRONS_LEFT,
        "timeline/chevrons-left.svg",
        "icon/chevrons-left.svg"
    ),
    // Transport. The strip previously drew the two zoom magnifiers for
    // "go to start/end" and for frame stepping, which read as zoom controls
    // in the one place playback controls are expected.
    (SKIP_BACK, "skip-back.svg", "icon/skip-back.svg"),
    (SKIP_FORWARD, "skip-forward.svg", "icon/skip-forward.svg"),
    (CHEVRON_LEFT, "chevron-left.svg", "icon/chevron-left.svg"),
    (CHEVRON_RIGHT, "chevron-right.svg", "icon/chevron-right.svg"),
    // One glyph for every disclosure in the app: a row that opens turns this
    // one, rather than swapping to a second asset mid-transition.
    (CHEVRON_DOWN, "ui/chevron-down.svg", "icon/chevron-down.svg"),
    // A favourite: lit when the entry is starred, faint when it is not.
    (STAR, "ui/star.svg", "icon/star.svg"),
    // The Project panel's pool: Import, the views, and the folder and
    // multicam makers.
    (IMPORT, "library/import.svg", "icon/import.svg"),
    (LIST, "library/list.svg", "icon/list.svg"),
    (
        LAYOUT_GRID,
        "library/layout-grid.svg",
        "icon/layout-grid.svg"
    ),
    (MOVE, "library/move.svg", "icon/move.svg"),
    (
        FOLDER_PLUS,
        "library/folder-plus.svg",
        "icon/folder-plus.svg"
    ),
    (LAYERS, "library/layers.svg", "icon/layers.svg"),
    // The Viewer's control row: the three handle sets, the scope strip, loop
    // playback and the proxy switch.
    (TRANSFORM, "preview/scaling.svg", "icon/transform.svg"),
    (CROP, "preview/crop.svg", "icon/crop.svg"),
    (MASK, "preview/square-dashed.svg", "icon/mask.svg"),
    (SCOPES, "preview/activity.svg", "icon/scopes.svg"),
    (LOOP, "timeline/repeat.svg", "icon/loop.svg"),
    (PROXY, "preview/replace.svg", "icon/proxy.svg"),
];

/// Where plugin command icons are served.
const PLUGIN_ICON_DIR: &str = "plugin-icon/";

/// Plugin command icons by content key: the asset path each is served under,
/// and the SVG served there. An icon stays for the session once named, so a
/// path handed to a drawn row never stops resolving.
static PLUGIN_ICONS: LazyLock<RwLock<HashMap<u64, (&'static str, Arc<str>)>>> =
    LazyLock::new(RwLock::default);

/// The asset path `icon` is served under, for [`icon`]. Equal icons share one
/// path, so gpui rasterizes each shape once.
pub(crate) fn plugin_icon_path(icon: &aria_plugin::CommandIcon) -> &'static str {
    served_icon_path(icon.key(), || Arc::from(icon.svg()))
}

/// The asset path a clip type's icon is served under. The catalog holds the
/// icon [`aria_plugin::CommandIcon`] rewrote, keyed the same way, so a clip
/// type and a command with one icon share one path.
pub(crate) fn clip_type_icon_path(icon: &aria_models::ClipTypeIcon) -> &'static str {
    served_icon_path(icon.key, || Arc::clone(&icon.svg))
}

/// The path the icon keyed `key` is served under, serving `svg` there the
/// first time the key is named.
fn served_icon_path(key: u64, svg: impl FnOnce() -> Arc<str>) -> &'static str {
    if let Some((path, _)) = PLUGIN_ICONS.read().get(&key) {
        return path;
    }
    PLUGIN_ICONS
        .write()
        .entry(key)
        .or_insert_with(|| {
            let path = crate::interned::intern(&format!("{PLUGIN_ICON_DIR}{key:016x}.svg"));
            (path, svg())
        })
        .0
}

/// The SVG of a plugin icon [`plugin_icon_path`] named, or `None` for any other
/// path.
fn plugin_icon(path: &str) -> Option<Cow<'static, [u8]>> {
    let key = path.strip_prefix(PLUGIN_ICON_DIR)?.strip_suffix(".svg")?;
    let key = u64::from_str_radix(key, 16).ok()?;
    let icons = PLUGIN_ICONS.read();
    let (served, svg) = icons.get(&key)?;
    (*served == path).then(|| Cow::Owned(svg.as_bytes().to_vec()))
}

/// An icon element for an embedded asset path, `Theme::ICON_SIZE` (16 px)
/// unless the caller sets `.size(..)`.
///
/// The glyph paints in the colour set with `.text_color(..)` on the icon, and
/// otherwise in the text colour inherited from its parent.
pub fn icon(path: &'static str) -> Icon {
    Icon(
        svg()
            .path(path)
            .flex_none()
            .size(px(crate::theme::Theme::ICON_SIZE)),
    )
}

/// An embedded glyph, painted in its own text colour or its parent's.
///
/// gpui's [`Svg`] paints only in a colour set on the SVG element itself and
/// paints nothing without one. Every icon in the app sits inside a control
/// that sets the text colour for its label and hover state, so [`Icon`] reads
/// that inherited colour at paint time when the icon sets none.
pub struct Icon(Svg);

impl Icon {
    /// Rotate or scale the glyph when painted. Layout and hit testing keep the
    /// untransformed bounds.
    pub fn with_transformation(self, transformation: Transformation) -> Self {
        Self(self.0.with_transformation(transformation))
    }
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        self.0.style()
    }
}

impl IntoElement for Icon {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Icon {
    type RequestLayoutState = <Svg as Element>::RequestLayoutState;
    type PrepaintState = <Svg as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.0.id()
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        self.0.source_location()
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.0.request_layout(global_id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        crate::components::drawn::record(crate::components::drawn::Drawn::Image, bounds);
        self.0
            .prepaint(global_id, inspector_id, bounds, request_layout, window, cx)
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let text = &mut self.0.style().text;
        if text.color.is_none() {
            text.color = Some(window.text_style().color);
        }
        self.0.paint(
            global_id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
    }
}

/// The brand mark at a given height. Raster art, so the caller states the size
/// it wants drawn rather than tinting it: this is the one asset the theme does
/// not colour.
///
/// Decoded once per process into a shared [`gpui::RenderImage`] so opening a
/// window or a test app does not re-decode the embedded PNG on GPUI's
/// background asset loader.
pub fn brand_mark_image() -> std::sync::Arc<gpui::RenderImage> {
    static BRAND_MARK_IMAGE: std::sync::LazyLock<std::sync::Arc<gpui::RenderImage>> =
        std::sync::LazyLock::new(|| {
            let Some(bytes) = Assets.load(BRAND_MARK).expect("brand mark asset") else {
                panic!("missing embedded brand mark {BRAND_MARK}");
            };
            let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                .expect("embedded brand mark PNG decodes")
                .into_rgba8();
            crate::frame::rgba_bytes_to_render(decoded.width(), decoded.height(), decoded.as_raw())
                .expect("embedded brand mark has non-zero dimensions")
        });
    std::sync::Arc::clone(&BRAND_MARK_IMAGE)
}

pub fn brand_mark(size_px: f32) -> Img {
    img(ImageSource::Render(brand_mark_image()))
        .flex_none()
        .size(px(size_px))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every asset path read, in order, and serves [`Assets`].
    #[derive(Clone, Default)]
    struct Loads(std::sync::Arc<parking_lot::Mutex<Vec<String>>>);

    impl AssetSource for Loads {
        fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
            self.0.lock().push(path.to_owned());
            Assets.load(path)
        }

        fn list(&self, path: &str) -> Result<Vec<SharedString>> {
            Assets.list(path)
        }
    }

    /// A control that sets the text colour and holds an icon that sets none,
    /// which is the shape of every icon control in the app.
    struct Tinted;

    impl gpui::Render for Tinted {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            use gpui::ParentElement as _;
            gpui::div().text_color(gpui::red()).child(icon(PLAY))
        }
    }

    /// An icon with no colour of its own paints in its parent's text colour.
    ///
    /// Closes the class of icons drawn with no colour: gpui's `Svg` rasterises
    /// a glyph only when the SVG element has a colour of its own, so every
    /// icon inside a control that coloured itself painted nothing. The
    /// rasteriser reads the glyph through the asset source only when it
    /// paints, so a read of the path is the observable proof the glyph reached
    /// the scene. Does not check the colour value: the test platform keeps no
    /// scene to read it from.
    #[test]
    fn an_icon_without_a_colour_paints_in_its_parents_text_colour() {
        let loads = Loads::default();
        let mut app = gpui::TestApp::with_text_system_and_assets(
            std::sync::Arc::new(gpui::NoopTextSystem),
            std::sync::Arc::new(loads.clone()),
        );
        let mut window = app.open_window(|_, _| Tinted);
        window.draw();
        assert_eq!(*loads.0.lock(), [PLAY]);
    }

    /// Every registered path loads, and what loads is the format its extension
    /// promises. Driven off the registry rather than a list of names, so a row
    /// added with a typo'd file, or a raster asset registered where a glyph is
    /// expected, fails here rather than at the first frame that draws it.
    #[test]
    fn every_registered_asset_loads_as_the_format_its_name_promises() {
        let assets = Assets;
        let listed = assets.list("icon/").unwrap();
        assert!(!listed.is_empty());
        for path in listed {
            let bytes = assets
                .load(&path)
                .unwrap()
                .unwrap_or_else(|| panic!("missing asset {path}"));
            if path.ends_with(".png") {
                assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{path} is not a png");
                continue;
            }
            assert!(path.ends_with(".svg"), "{path} has no known format");
            let text = std::str::from_utf8(&bytes).expect("icon svg is utf-8");
            assert!(text.contains("<svg"), "{path} is not an svg");
            assert!(text.contains("viewBox"), "{path} lacks a viewBox");
        }
    }

    std::thread_local! {
        static USVG_RECORDS: std::cell::RefCell<Vec<String>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Records every warning and error logged on the thread that logs it.
    struct Recorder;

    impl log::Log for Recorder {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Warn
        }

        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                let line = format!("{} {}: {}", record.level(), record.target(), record.args());
                USVG_RECORDS.with_borrow_mut(|records| records.push(line));
            }
        }

        fn flush(&self) {}
    }

    /// The warnings and errors this thread logs while gpui's renderer parses
    /// `bytes`.
    fn parse_warnings(renderer: &gpui::SvgRenderer, name: &str, bytes: &[u8]) -> Vec<String> {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            log::set_logger(&Recorder).expect("aria-ui's tests install no other logger");
            log::set_max_level(log::LevelFilter::Warn);
        });
        USVG_RECORDS.with_borrow_mut(Vec::clear);
        if let Err(error) = renderer.parse_svg(bytes) {
            panic!("{name} does not parse: {error}");
        }
        USVG_RECORDS.take()
    }

    /// Every registered glyph parses through gpui's own SVG renderer without
    /// a warning.
    ///
    /// Closes the class of icon files holding a value usvg cannot read: usvg
    /// drops the attribute and logs a warning whenever gpui rasterises the
    /// glyph at a new size, so a `color="currentColor"` on the play, pause and
    /// fullscreen roots logged one at startup and two when playback first
    /// started. Driven off the registry, so a new glyph is checked on the row
    /// that adds it. The first parse, the pause glyph's root and one of its
    /// strokes as they shipped, is the control: it must warn, or the recorder
    /// is not receiving usvg. Does not check what a glyph looks like.
    #[test]
    fn every_registered_glyph_parses_without_a_warning() {
        let renderer = gpui::SvgRenderer::new(std::sync::Arc::new(Assets));
        let control = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" color="currentColor"><path d="M8.5 5.5V18.5" stroke="currentColor" stroke-width="2"/></svg>"#;
        assert_eq!(
            parse_warnings(&renderer, "the control", control),
            ["WARN usvg::parser::svgtree: Failed to parse color value: 'currentColor'."]
        );
        let glyphs = Assets.list("icon/").unwrap();
        let glyphs: Vec<_> = glyphs
            .iter()
            .filter(|path| path.ends_with(".svg"))
            .collect();
        assert!(glyphs.contains(&&SharedString::from(PLAY)));
        for path in glyphs {
            let bytes = Assets.load(path).unwrap().expect("a listed path loads");
            let warnings = parse_warnings(&renderer, path, &bytes);
            assert!(warnings.is_empty(), "{path}: {warnings:?}");
        }
    }

    #[test]
    fn unknown_paths_are_none() {
        assert!(Assets.load("icon/nope.svg").unwrap().is_none());
    }

    #[test]
    fn list_filters_by_prefix() {
        assert!(Assets.list("fonts/").unwrap().is_empty());
        assert!(
            Assets
                .list("icon/play")
                .unwrap()
                .iter()
                .all(|p| p.as_ref() == PLAY)
        );
    }

    /// `docs/icons.md` has one table row per registered icon, and the row
    /// states the file the registry embeds.
    ///
    /// Closes the class of an icon added, renamed, pointed at another file, or
    /// removed without its reference row changing with it. The rows are read
    /// from the page and compared with [`REGISTRY`], which the macro expands
    /// from the same rows that serve the assets, so a new icon fails here until
    /// the page lists it. Does not check the Use column against the controls
    /// that draw the icon.
    #[test]
    fn every_registered_icon_has_one_row_in_icons_md() {
        let page = include_str!("../../../docs/icons.md");
        let mut rows: HashMap<&str, Vec<&str>> = HashMap::new();
        for line in page.lines() {
            let Some(rest) = line.strip_prefix('|') else {
                continue;
            };
            let mut cells = rest.split('|').map(str::trim);
            let (Some(first), Some(file)) = (cells.next(), cells.next()) else {
                continue;
            };
            let Some(constant) = first.strip_prefix('`').and_then(|c| c.strip_suffix('`')) else {
                continue;
            };
            let is_constant = !constant.is_empty()
                && constant
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
            if is_constant {
                rows.entry(constant).or_default().push(file);
            }
        }
        for (constant, file) in REGISTRY {
            let expected = format!("`assets/icon/{file}`");
            match rows.get(constant).map(Vec::as_slice) {
                Some([only]) => assert_eq!(
                    *only, expected,
                    "docs/icons.md: the row of {constant} states {only}, the registry embeds {expected}"
                ),
                Some(many) => panic!("docs/icons.md: {constant} has {} rows", many.len()),
                None => panic!("docs/icons.md: {constant} ({expected}) has no row"),
            }
        }
        for constant in rows.keys() {
            assert!(
                REGISTRY.iter().any(|(name, _)| name == constant),
                "docs/icons.md: a row names {constant}, which icon_assets! does not register"
            );
        }
    }

    struct BrandView;

    impl gpui::Render for BrandView {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            brand_mark(18.0)
        }
    }

    /// WHY: `brand_mark` previously returned `ImageSource::Resource(Embedded(BRAND_MARK))`,
    /// causing every new `App` / `TestApp` to reload and PNG-decode `icon/aria-icon.png`
    /// on GPUI's background executor (~5% of startup CPU cycles). It now shares one
    /// process-wide `RenderImage` decoded once from the embedded PNG.
    ///
    /// Does NOT catch: GPU texture upload of the `RenderImage` on a real window surface.
    #[test]
    fn brand_mark_reuses_one_decoded_render_image_without_per_app_asset_load() {
        let first = brand_mark_image();
        let second = brand_mark_image();
        assert!(std::sync::Arc::ptr_eq(&first, &second));

        let loads = Loads::default();
        let mut app = gpui::TestApp::with_text_system_and_assets(
            std::sync::Arc::new(gpui::NoopTextSystem),
            std::sync::Arc::new(loads.clone()),
        );
        let mut window = app.open_window(|_, _| BrandView);
        window.draw();
        app.run_until_parked();
        assert!(
            loads.0.lock().is_empty(),
            "brand_mark must use the shared RenderImage instead of reloading through AssetSource"
        );
    }
}
