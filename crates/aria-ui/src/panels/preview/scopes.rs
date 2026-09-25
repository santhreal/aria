//! The scope strip beside the preview: waveform, vectorscope, and histogram.
//!
//! The measurement and the bitmaps live in [`crate::app::scope_view`] and
//! [`crate::scopes`]; this module places them and owns the toggle. Each scope
//! is one texture, so the strip costs three images per frame rather than a
//! grid of elements.
//!
//! Width is fixed. A scope read at a varying size is a scope whose graticule
//! moves, and an operator judging exposure against the 50% line needs that
//! line to stay where it was.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement,
    StatefulInteractiveElement, Styled, StyledImage, div, img, px,
};

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::theme::Theme;

/// Strip width. Holds a 192px scope plus the panel's own padding.
const STRIP_W: f32 = 216.0;

/// The strip: three scopes stacked, newest measurement first.
pub(super) fn strip(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let summary = state.read(cx).scope_summary();
    let textures = state.read(cx).scope_textures.as_ref().map(|t| {
        (
            t.waveform.clone(),
            t.vectorscope.clone(),
            t.histogram.clone(),
        )
    });
    let mut column = div()
        .id("preview-scopes")
        .w(px(STRIP_W))
        .h_full()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .p(px(Theme::SPACE_SM))
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .overflow_y_scroll()
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.text_muted)
                .font_family(theme.font_mono.clone())
                .child(summary),
        );

    match textures {
        Some((waveform, vectorscope, histogram)) => {
            // The id holds nothing that changes per measurement on purpose:
            // the strip fades up once, when the first frame is read. Keyed on
            // the reading instead, the fade would restart on every frame of
            // playback and the scopes would never finish arriving.
            column = column.child(crate::motion::fade_in(
                "preview-scope-body",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_SM))
                    .child(labelled("Waveform", theme))
                    .child(scope_image("scope-waveform", waveform, 192., theme))
                    .child(labelled("Vectorscope", theme))
                    .child(scope_image("scope-vector", vectorscope, 192., theme))
                    .child(labelled("Histogram", theme))
                    .child(scope_image("scope-histogram", histogram, 96., theme)),
            ));
        }
        None => {
            // Open with nothing measured is a wait, not an empty state: the
            // next frame the operator scrubs to fills the strip.
            column = column.child(crate::pending::working(
                theme,
                "preview.scopes",
                "Scrub or play to measure a frame.",
                cx,
            ));
        }
    }
    column.into_any_element()
}

/// One scope caption.
fn labelled(text: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .text_size(px(11.))
        .text_color(theme.text_faint)
        .child(text)
}

/// One scope texture, drawn at the strip's width and a fixed height so the
/// graticule keeps its position between frames.
fn scope_image(
    id: impl Into<crate::motion::ControlId>,
    image: std::sync::Arc<gpui::RenderImage>,
    height: f32,
    theme: &Theme,
) -> impl IntoElement {
    let id = id.into();
    div()
        .w_full()
        .h(px(height))
        .rounded(px(Theme::CONTROL_RADIUS))
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .child(
            img(image)
                .id(id.element("scope-image"))
                .object_fit(ObjectFit::Fill)
                .w_full()
                .h(px(height)),
        )
}

/// The toolbar toggle. Reads as on or off, because a scope strip that is
/// closed is indistinguishable from one that has nothing to show.
pub(super) fn toggle(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let open = state.read(cx).scopes_open;
    div()
        .id("preview-scopes-toggle")
        .px(px(Theme::SPACE_XS))
        .text_size(px(11.))
        .text_color(if open { theme.accent } else { theme.text_muted })
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .when_some(
            crate::tooltip::builder("preview-scopes-toggle"),
            |el, tip| el.tooltip(tip),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.toggle_scopes());
                cx.notify();
            }),
        )
        .child("Scopes")
}
