//! GPUI gallery for Aria's design system.
//!
//! This is the only place a human can look at every shipped component, state,
//! motion primitive, loader and token at once, in both appearances. Run it with
//! `cargo run -p aria-ui-gallery`.
//!
//! Sections live in sibling files rather than one wall of code. They are
//! declared with `#[path]` because this crate keeps every source file beside
//! `main.rs`.

// Does the gallery still show every component and every motion spec? Both
// surfaces are read out of `aria-ui` source at run time.
#[cfg(test)]
#[path = "gallery_coverage_tests.rs"]
mod gallery_coverage_tests;

#[path = "gallery_controls.rs"]
mod gallery_controls;
#[path = "gallery_icons.rs"]
mod gallery_icons;
#[path = "gallery_kit.rs"]
mod gallery_kit;
#[path = "gallery_loaders.rs"]
mod gallery_loaders;
#[path = "gallery_motion.rs"]
mod gallery_motion;

use aria_ui::appearance::{self, AppearanceMode};
use aria_ui::components;
use aria_ui::icons;
use aria_ui::motion;
use aria_ui::settings;
use aria_ui::theme::Theme;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Render, ScrollHandle, StatefulInteractiveElement, Styled,
    Window, WindowBounds, WindowOptions, div, px, size,
};

use gallery_kit::SECTIONS;

/// No text field holds focus.
pub const NO_FIELD: usize = usize::MAX;

fn main() {
    // Icons are SVG assets served through `aria_ui::icons::Assets`. Without this
    // the icon controls lay out and paint nothing, which reads as a missing
    // glyph rather than a missing asset source.
    let app = gpui_platform::application().with_assets(icons::Assets);
    app.run(|cx: &mut App| {
        aria_ui::register_fonts(cx);
        appearance::init(
            AppearanceMode::System,
            None,
            settings::default_data_dir(),
            cx,
        );
        let bounds = gpui::Bounds::centered(None, size(px(1100.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(800.), px(560.))),
                app_id: Some("aria-gallery".into()),
                ..Default::default()
            },
            |_, cx| cx.new(|_cx| Gallery::new()),
        )
        .expect("open gallery window");
        cx.activate(true);
    });
}

/// Everything a section needs to know about the gallery's own state. Copied into
/// each section rather than borrowed, so a section can still take `cx` mutably.
#[derive(Clone, Copy)]
pub struct GalleryState {
    /// Bumped by the Replay button. Spliced into every entry-animation element
    /// id, because gpui runs an animation once per id.
    pub replay: usize,
    /// Which live text field holds focus, or [`NO_FIELD`].
    pub focused_field: usize,
    pub show_dialog: bool,
    pub show_splash: bool,
}

pub struct Gallery {
    pub state: GalleryState,
    /// Owned so the nav row can jump to a section by index.
    scroll: ScrollHandle,
}

impl Gallery {
    fn new() -> Self {
        Self {
            state: GalleryState {
                replay: 0,
                focused_field: NO_FIELD,
                // The dialog is up on first paint: an overlay nobody can see is
                // not reviewed.
                show_dialog: true,
                show_splash: false,
            },
            scroll: ScrollHandle::new(),
        }
    }
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Every hover blend and entrance this draw builds samples the frame
        // opened here, so it comes before any section is built.
        motion::begin_frame(cx);
        let theme = Theme::of(cx);
        let view = cx.entity_id();
        let state = self.state;
        let scroll = self.scroll.clone();

        let head = header(&theme, cx);
        let index = nav(&theme, cx);
        // One child per entry in SECTIONS, in the same order: the nav row scrolls
        // by index, so a section inserted here must be named there too.
        let sections = vec![
            gallery_controls::controls(&theme, cx),
            gallery_controls::states(&theme, state, cx),
            gallery_motion::motion_section(&theme, state, view, cx),
            gallery_loaders::loaders_section(&theme, state, view, cx),
            gallery_loaders::surfaces(&theme, state, cx),
            gallery_icons::typography(&theme),
            gallery_icons::icons_section(&theme),
            gallery_icons::theme_tokens(&theme),
        ];
        debug_assert_eq!(sections.len(), SECTIONS.len(), "nav row is index-aligned");

        div()
            .id("gallery")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.bg)
            .text_color(theme.text)
            .font_family(theme.font_sans.clone())
            .child(head)
            .child(index)
            .child(
                div()
                    .id("gallery-scroll")
                    .track_scroll(&scroll)
                    .overflow_y_scroll()
                    .flex_1()
                    .min_h_0()
                    .p(px(Theme::SPACE_LG))
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_LG))
                    .children(sections),
            )
            // Ends the motion frame after every element that read the stores,
            // and requests the next frame while a value is still moving. Hover
            // fades are functions of the clock, so nothing else repaints them:
            // without this a fade freezes at whatever value the last mouse
            // event left it on.
            .child(motion_driver())
    }
}

/// A zero-size element that ends the motion frame during prepaint. Exactly one
/// per window: a second would advance the stores' frame counter twice and drop
/// the values the first had just drawn.
fn motion_driver() -> impl IntoElement {
    gpui::canvas(
        |_, window, _| {
            motion::end_frame(window);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .w(px(0.))
    .h(px(0.))
}

/// Appearance switcher and the reduced-motion switch. Both are the real
/// controls: the gallery is reviewed in both modes, and reduced motion is a
/// state every primitive here has to survive.
fn header(theme: &Theme, cx: &mut Context<Gallery>) -> AnyElement {
    let mode = appearance::mode(cx);
    let reduced = cx.reduce_motion();
    div()
        .h(px(Theme::HEADER_HEIGHT))
        .px(px(Theme::SPACE_LG))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.glass())
        .child(
            div()
                .flex_1()
                .text_size(px(14.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Aria gallery"),
        )
        .child(
            components::ghost_button(
                "hdr-reduce",
                if reduced {
                    "reduce motion: on"
                } else {
                    "reduce motion: off"
                },
                theme,
            )
            .when(reduced, |el| el.bg(theme.wash(0.10)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                    cx.set_reduce_motion(!reduced);
                    cx.notify();
                }),
            ),
        )
        .children(AppearanceMode::ALL.into_iter().map(|candidate| {
            let selected = candidate == mode;
            components::ghost_button(candidate.label(), candidate.label(), theme)
                .when(selected, |el| el.bg(theme.wash(0.10)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        appearance::set_mode(candidate, cx);
                        cx.notify();
                    }),
                )
        }))
        .into_any_element()
}

/// The section index, parked under the header so a section label is always on
/// screen. gpui has no sticky positioning; a fixed row that scrolls the body is
/// the cheap equivalent, and it jumps as well as labels.
fn nav(theme: &Theme, cx: &mut Context<Gallery>) -> AnyElement {
    div()
        .px(px(Theme::SPACE_LG))
        .py(px(Theme::SPACE_XS))
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .children(SECTIONS.into_iter().enumerate().map(|(index, title)| {
            components::ghost_button(motion::MotionKey::new("gallery-nav", title), title, theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Gallery, _: &MouseDownEvent, _, cx| {
                        this.scroll.scroll_to_top_of_item(index);
                        cx.notify();
                    }),
                )
        }))
        .into_any_element()
}
