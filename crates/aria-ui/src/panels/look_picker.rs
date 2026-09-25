//! The shipped looks, as a strip you can click.
//!
//! `aria_render::starter_lut_pack` bakes one `.cube` per filter-catalog recipe
//! and the only caller was the authoring binary that uploads the pack to a CDN.
//! In the editor a LUT could be loaded from a file or downloaded from the cloud
//! feed, so the looks the build already contained were the one source the
//! operator could not reach. This is the view that was missing; the grade itself
//! is the same `set_selected_lut` the LUT field has always used.
//!
//! The list is `AppState::starter_looks`, which is the filter catalog: a preset
//! added to `filters.toml` appears here with no edit to this file, and nothing
//! here holds a copy of the names, the labels, or the count.
//!
//! Each row previews itself. The swatch is what the recipe does to
//! `aria_render::SWATCH_REFERENCE`, computed on the CPU through the same
//! `ColorGrade` the shader runs, so a look shows what it does before it is
//! clicked and costs no texture upload to draw.
//!
//! It shows itself, like the duck tuner: registered with a selected clip as its
//! visibility, because grading is a per-clip property and a picker with nothing
//! to grade is a row of buttons that all refuse.

use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::app::render_reach::UiLook;
use crate::motion;
use crate::theme::Theme;

/// One swatch colour, as the compositor's 8-bit output.
fn colour(rgb: [u8; 3]) -> gpui::Hsla {
    gpui::Hsla::from(gpui::Rgba {
        r: f32::from(rgb[0]) / 255.0,
        g: f32::from(rgb[1]) / 255.0,
        b: f32::from(rgb[2]) / 255.0,
        a: 1.0,
    })
}

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let looks = state.read(cx).starter_looks();
    let rows: Vec<gpui::AnyElement> = looks
        .into_iter()
        .map(|look| row(&theme, look, cx))
        .collect();

    motion::fade_in(
        "look-picker",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .p(px(Theme::SPACE_SM))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(theme.surface_raised)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.text_muted)
                    .child("Looks"),
            )
            .children(rows),
    )
    .into_any_element()
}

fn row(
    theme: &Theme,
    look: UiLook,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    // One fade key per row. The pack id is unique and the panel is drawn once,
    // so no two rows share a fade.
    let key = motion::MotionKey::new("look", &look.id);
    let background = motion::hover_blend(key, theme.input_bg, theme.surface_raised_hover);
    let id = look.id.clone();
    div()
        .id(SharedString::from(look.id))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(background)
        .border_1()
        .border_color(if look.applied {
            theme.accent
        } else {
            theme.border
        })
        .text_size(px(13.))
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                let id = id.clone();
                this.state
                    .update(cx, |state, _| state.apply_starter_look(&id));
                cx.notify();
            }),
        )
        .child(div().child(SharedString::from(look.label)))
        .child(strip(look.swatch))
        .into_any_element()
}

/// The graded reference, one cell per colour. Built from the swatch the look
/// reported, so a wider reference strip shows up here with no edit.
fn strip(swatch: Vec<[u8; 3]>) -> gpui::AnyElement {
    let mut out = div()
        .flex()
        .items_center()
        .rounded(px(2.))
        .overflow_hidden();
    for cell in swatch {
        out = out.child(div().w(px(10.)).h(px(12.)).bg(colour(cell)));
    }
    out.into_any_element()
}
