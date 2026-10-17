//! The scope readings, drawn two ways: as the strip beside the preview, and as
//! the Scopes dock panel. Both show the waveform, vectorscope, and histogram,
//! then every plugin scope with its readout.
//!
//! The measurement and the bitmaps are in [`crate::app::scope_view`] and
//! [`crate::scopes`]; this module places them and draws the strip's toggle.
//! Each scope is one texture, so a layout costs three images per frame rather
//! than a grid of elements.
//!
//! Sizes are fixed per layout. A scope read at a varying size is a scope whose
//! graticule moves, and exposure is judged against a 50% line that stays where
//! it was.

use crate::tooltip::WithTip as _;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, StyledImage, div, img, px,
};

use crate::app::AppState;
use crate::app::scope_view::{PluginScopeTexture, ScopeTextures};
use crate::components::ChipSize;
use crate::fluent::FluentBuilder;
use crate::motion::MotionKey;
use crate::scopes::WaveformMode;
use crate::theme::Theme;

/// Strip width. Holds a 192px scope plus the panel's own padding.
const STRIP_W: f32 = 216.0;

/// Width every scope is drawn at.
const SCOPE_W: f32 = 192.0;

/// Tallest a plugin scope is drawn, so one tall scope cannot push the rest
/// out of the strip.
const PLUGIN_SCOPE_MAX_H: f32 = 384.0;

/// Height of a scope in the Scopes panel, which has a frame of its own.
const PANEL_SCOPE_H: f32 = 256.0;

/// Narrowest a waveform or histogram is laid out in the panel before the row
/// wraps.
const PANEL_SCOPE_MIN_W: f32 = 240.0;

/// The surface name of the Scopes panel.
const PANEL_SURFACE: &str = "scopes-panel";

/// The reading on screen, or `None` before the first frame is measured.
///
/// Borrowed rather than copied out: a layout draws from the reading, so a
/// frame costs no clone of the plugin readings.
fn shown<'a>(state: &Entity<AppState>, cx: &'a gpui::App) -> Option<&'a ScopeTextures> {
    state.read(cx).scope_textures.as_ref()
}

/// The strip: three scopes stacked, newest measurement first.
pub(super) fn strip(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let summary = state.read(cx).scope_summary();
    let modes = waveform_modes(state, "preview-scopes", theme, cx);
    let textures = shown(state, cx);
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
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .font_family(theme.font_mono.clone())
                .child(summary),
        );

    match textures {
        Some(ScopeTextures {
            waveform,
            vectorscope,
            histogram,
            plugins,
            ..
        }) => {
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
                    .children(modes)
                    .child(scope_image("scope-waveform", waveform.clone(), 192., theme))
                    .child(labelled("Vectorscope", theme))
                    .child(scope_image(
                        "scope-vector",
                        vectorscope.clone(),
                        192.,
                        theme,
                    ))
                    .child(labelled("Histogram", theme))
                    .child(scope_image(
                        "scope-histogram",
                        histogram.clone(),
                        96.,
                        theme,
                    ))
                    .children(plugins.iter().map(|reading| plugin_scope(reading, theme))),
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

/// The Scopes dock panel: the same readings laid across the panel in one row
/// that wraps as the frame narrows. The waveform and histogram take the width;
/// the vectorscope stays square, because a stretched vectorscope moves every
/// hue off its target.
pub(crate) fn panel(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let summary = state.read(cx).scope_summary();
    let modes = waveform_modes(state, PANEL_SURFACE, &theme, cx);
    let body = match shown(state, cx) {
        Some(ScopeTextures {
            waveform,
            vectorscope,
            histogram,
            plugins,
            ..
        }) => crate::motion::fade_in(
            "scopes-panel-body",
            div()
                .flex()
                .flex_wrap()
                .gap(px(Theme::SPACE_MD))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(Theme::SPACE_XS))
                        .children(modes)
                        .child(scope_image(
                            "scopes-panel-waveform",
                            waveform.clone(),
                            PANEL_SCOPE_H,
                            &theme,
                        ))
                        .flex_1()
                        .min_w(px(PANEL_SCOPE_MIN_W)),
                )
                .child(
                    card(
                        "Vectorscope",
                        scope_image(
                            "scopes-panel-vector",
                            vectorscope.clone(),
                            PANEL_SCOPE_H,
                            &theme,
                        ),
                        &theme,
                    )
                    .flex_none()
                    .w(px(PANEL_SCOPE_H)),
                )
                .child(
                    card(
                        "Histogram",
                        scope_image(
                            "scopes-panel-histogram",
                            histogram.clone(),
                            PANEL_SCOPE_H,
                            &theme,
                        ),
                        &theme,
                    )
                    .flex_1()
                    .min_w(px(PANEL_SCOPE_MIN_W)),
                )
                .children(plugins.iter().map(|reading| {
                    div()
                        .flex_none()
                        .w(px(SCOPE_W))
                        .child(plugin_scope(reading, &theme))
                })),
        )
        .into_any_element(),
        None => crate::pending::working(
            &theme,
            "scopes.panel",
            "Scrub or play to measure a frame.",
            cx,
        ),
    };
    div()
        .id(PANEL_SURFACE)
        .debug_selector(|| PANEL_SURFACE.into())
        .size_full()
        .overflow_y_scroll()
        .p(px(Theme::SPACE_SM))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .bg(theme.surface)
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .font_family(theme.font_mono.clone())
                .child(summary),
        )
        .child(body)
        .into_any_element()
}

/// The waveform's caption: one chip per mode, the one shown lit, so the trace
/// in use is read off the scope and changed where it is read. `place` keeps
/// the strip's chips and the panel's apart when both are drawn.
///
/// `None` before the first reading, when no waveform is drawn to caption. Built
/// before the reading is borrowed for the layout, because a chip's click needs
/// the context the borrow holds.
fn waveform_modes(
    state: &Entity<AppState>,
    place: &'static str,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<gpui::AnyElement> {
    let state = state.read(cx);
    if state.scope_textures.is_none() {
        return None;
    }
    let shown = state.waveform_mode();
    let row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(Theme::SPACE_XXS))
        .child(labelled("Waveform", theme))
        .children(WaveformMode::ALL.into_iter().map(|mode| {
            crate::components::chip(
                MotionKey::new(place, mode.id()),
                mode.short_label(),
                mode == shown,
                ChipSize::Compact,
                theme,
            )
            .debug_selector(move || format!("{place}-{}", mode.id()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state
                        .update(cx, |state, cx| state.set_waveform_mode(mode, cx));
                    cx.notify();
                }),
            )
        }))
        .into_any_element();
    Some(row)
}

/// One captioned scope in the panel's row.
fn card(caption: &'static str, image: impl IntoElement, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(labelled(caption, theme))
        .child(image)
}

/// One scope caption.
fn labelled(text: impl Into<SharedString>, theme: &Theme) -> impl IntoElement {
    div()
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_faint)
        .child(text.into())
}

/// One plugin scope: its caption and plugin, the image at the scope's aspect
/// ratio, and its readout. A failed measurement shows its reason where the
/// image would be.
fn plugin_scope(reading: &PluginScopeTexture, theme: &Theme) -> gpui::AnyElement {
    let spec = &reading.scope.spec;
    let caption = format!("{} \u{b7} {}", spec.name, reading.scope.plugin_name);
    let height = (SCOPE_W * spec.height as f32 / spec.width as f32).clamp(1.0, PLUGIN_SCOPE_MAX_H);
    let line = |text: SharedString, color| {
        div()
            .text_size(Theme::TEXT_UI)
            .text_color(color)
            .font_family(theme.font_mono.clone())
            .child(text)
    };
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(labelled(caption, theme))
        .when_some(reading.drawn.as_ref().ok(), |column, image| {
            column.child(scope_image(
                MotionKey::new("plugin-scope", &spec.id),
                image.clone(),
                height,
                theme,
            ))
        })
        .when_some(reading.drawn.as_ref().err(), |column, reason| {
            column.child(line(reason.clone().into(), theme.danger))
        })
        .when(!reading.readout.is_empty(), |column| {
            column.child(line(reading.readout.clone().into(), theme.text_muted))
        })
        .into_any_element()
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

/// The Viewer row's Scopes toggle. Reads as on or off, because a scope strip that is
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
        .text_size(Theme::TEXT_UI)
        .text_color(if open { theme.accent } else { theme.text_muted })
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .when_some(
            crate::tooltip::builder("preview-scopes-toggle"),
            |el, tip| el.with_tip(tip),
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
