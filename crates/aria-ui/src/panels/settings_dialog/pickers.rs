//! The Appearance, Canvas and Sequence pickers.

use std::sync::Arc;

use aria_models::{AudioLayout, CanvasAspect, Rational, TimeDisplay};
use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};

use crate::appearance::{self, AppearanceMode};
use crate::motion;
use crate::surfaces::settings_pick::{self, Pick};
use crate::theme::Theme;
use crate::theme::file::ThemeFile;

/// Page, card, and accent from a palette that is not installed.
///
/// A row that only names an appearance or an accent needs a click to find out.
/// Every palette here is built the way the real one is, so the chips cannot
/// drift from what the click installs.
fn swatch(preview: &Theme) -> impl IntoElement {
    let border = preview.border;
    div().flex().flex_row().gap(px(3.)).children(
        [preview.bg, preview.surface_raised, preview.accent].map(move |color| {
            div()
                .size(px(12.))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(color)
                .border_1()
                .border_color(border)
        }),
    )
}

/// The fill for a picker row, blended from rest to hover over `key`.
///
/// Every picker row shares this so a section fades at one rate. The key is the
/// row's own element id, which is already unique per row.
fn row_fill(theme: &Theme, key: motion::MotionKey, selected: bool) -> gpui::Hsla {
    let rest = if selected {
        theme.glass_selected_bg()
    } else {
        theme.wash(0.0)
    };
    motion::hover_blend(key, rest, theme.glass_hover())
}

/// The box every picker row shares: one row height, the label at the left and
/// any detail at the right, the fill from [`row_fill`], and the selected rows'
/// shadow. The row steps out by its inset so its label stands on the section
/// title's edge, as every other settings row does. A row adds its children and
/// its click.
fn row_shell(
    id: impl Into<motion::ControlId>,
    theme: &Theme,
    fade: motion::MotionKey,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into().element("picker-row"))
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .h(px(Theme::ROW_HEIGHT))
        .px(px(Theme::SPACE_XS))
        .mx(px(-Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(row_fill(theme, fade, selected))
        .when(selected, |el| {
            el.shadow(crate::theme::glass_selected_shadows())
        })
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(fade))
}

/// A picker row's label: the whole text, or its start and an ellipsis in a
/// row too narrow for it.
fn row_label(theme: &Theme, label: impl Into<SharedString>) -> impl IntoElement {
    let label = label.into();
    let selector = label.clone();
    div()
        .debug_selector(move || format!("picker-label-{selector}"))
        .min_w_0()
        .truncate()
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text)
        .child(label)
}

/// A picker row's detail, at the right.
fn row_detail(theme: &Theme, detail: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex_none()
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_muted)
        .child(detail.into())
}

pub(super) fn appearance_row(
    theme: &Theme,
    candidate: AppearanceMode,
    current: AppearanceMode,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let selected = candidate == current;
    let key = candidate.label();
    let fade = motion::MotionKey::new("appearance", key);
    // What this mode looks like right now, accent included: "System" names a
    // choice with an invisible result, and this is the result.
    let preview = Theme::for_choice(
        appearance::preview(candidate, cx),
        appearance::theme_choice(cx),
    );
    row_shell(key, theme, fade, selected)
        .child(row_label(theme, candidate.label()))
        .child(swatch(&preview))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                appearance::set_mode(candidate, cx);
                this.state.update(cx, |state, cx| {
                    state.ui_settings.appearance = candidate;
                    state.persist_settings_later(cx);
                    state.status_message = Some(format!("Appearance: {}", candidate.label()));
                });
                cx.notify();
            }),
        )
}

/// One motion speed.
///
/// The detail names the multiplier because that is the whole difference between
/// two of these rows, and the click installs the choice immediately: a motion
/// setting that waited for the next launch could not be judged against the app
/// it changes.
pub(super) fn motion_row(
    theme: &Theme,
    candidate: motion::MotionPreference,
    current: motion::MotionPreference,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let selected = candidate == current;
    let key = candidate.label();
    let fade = motion::MotionKey::new("motion-pref", key);
    row_shell(key, theme, fade, selected)
        .child(row_label(theme, candidate.label()))
        .child(row_detail(theme, candidate.detail()))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                candidate.apply(cx);
                this.state.update(cx, |state, cx| {
                    state.ui_settings.motion = candidate;
                    state.persist_settings_later(cx);
                    state.status_message = Some(format!("Motion: {}", candidate.detail()));
                });
                cx.notify();
            }),
        )
}

/// One selectable accent choice, labelled with the accent's name.
///
/// The click both persists the choice and repaints: writing it without
/// repainting would look like the setting did nothing until the next launch.
pub(super) fn theme_row(
    theme: &Theme,
    candidate: aria_settings::ThemeChoice,
    current: aria_settings::ThemeChoice,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let selected = candidate == current;
    let fade = motion::MotionKey::new("theme", candidate.key());
    // The accent this key installs, in the appearance that is on screen now.
    let preview = Theme::for_choice(theme.appearance, candidate);
    row_shell(fade, theme, fade, selected)
        .child(row_label(theme, format!("Accent: {}", candidate.label())))
        .child(swatch(&preview))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.set_theme_choice(candidate) {
                        state.report_error(err);
                    }
                });
                appearance::set_theme_choice(candidate, cx);
                cx.notify();
            }),
        )
}

/// One theme file, or the shipped theme for `None`. The swatch is the theme in
/// the appearance on screen now. The click repaints and saves the choice.
pub(super) fn theme_file_row(
    theme: &Theme,
    candidate: Option<Arc<ThemeFile>>,
    current: Option<&str>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let stem = candidate.as_ref().map(|file| file.stem.clone());
    let selected = stem.as_deref() == current;
    let label = candidate
        .as_ref()
        .map_or_else(|| "Shipped".to_owned(), |file| file.label());
    let fade = motion::MotionKey::new("theme-file", stem.as_deref().unwrap_or(""));
    let base = Theme::for_choice(theme.appearance, appearance::theme_choice(cx));
    let preview = match &candidate {
        Some(file) => base.with_file(file),
        None => base,
    };
    let detail = match &candidate {
        Some(file) if !file.covers(theme.appearance) => "Shipped colors in this appearance",
        Some(_) => "",
        None => "The built-in palette",
    };
    row_shell(fade, theme, fade, selected)
        .child(row_label(theme, format!("Theme: {label}")))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .child(row_detail(theme, detail))
                .child(swatch(&preview)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                appearance::set_theme_file(candidate.clone(), cx);
                let stem = stem.clone();
                let label = label.clone();
                this.state.update(cx, |state, cx| {
                    state.ui_settings.theme_file = stem;
                    state.persist_settings_later(cx);
                    state.status_message = Some(format!("Theme: {label}"));
                });
                cx.notify();
            }),
        )
}

/// A theme file that could not be read, and why. Not selectable, so it has no
/// row inset: its label stands on the section title's edge.
pub(super) fn theme_file_failure_row(theme: &Theme, file: &str, reason: &str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .py(px(Theme::SPACE_XXS))
        .child(row_label(theme, format!("Theme file {file}")))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.danger)
                .child(reason.to_owned()),
        )
}

/// One selectable canvas aspect. The label is the model's own, so a new preset
/// appears here without a second list.
pub(super) fn canvas_row(
    theme: &Theme,
    candidate: CanvasAspect,
    current: CanvasAspect,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let selected = candidate == current;
    let fade = motion::MotionKey::new("canvas", candidate.name());
    row_shell(fade, theme, fade, selected)
        .child(row_label(theme, SharedString::from(candidate.label())))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = settings_pick::apply(state, Pick::Aspect(candidate)) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
}

/// One selectable canvas size: a delivery baseline, or "Derived" (`size` `None`)
/// which hands the size back to the aspect or the footage. The label carries the
/// pixels, because "2160p" and "3840×2160" answer different questions and a
/// delivery spec is written in the second one.
pub(super) fn resolution_row(
    theme: &Theme,
    id: motion::ControlId,
    label: SharedString,
    size: Option<[u32; 2]>,
    selected: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let fade = id.motion("resolution");
    row_shell(id, theme, fade, selected)
        .child(row_label(theme, label))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = settings_pick::apply(state, Pick::Resolution(size)) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
}

/// One selectable sequence frame rate.
pub(super) fn sequence_rate_row(
    theme: &Theme,
    candidate: Rational,
    current: Rational,
    label: SharedString,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    pick_row(
        theme,
        motion::MotionKey::new("sequence-rate", candidate.num).with(candidate.den),
        aria_core::rate_eq(candidate, current),
        label,
        Pick::Rate(candidate),
        cx,
    )
}

/// One selectable audio layout.
pub(super) fn audio_layout_row(
    theme: &Theme,
    candidate: AudioLayout,
    current: AudioLayout,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    pick_row(
        theme,
        motion::MotionKey::new("audio-layout", candidate.name()),
        candidate == current,
        SharedString::from(candidate.label()),
        Pick::Layout(candidate),
        cx,
    )
}

/// One selectable time display: how the ruler and the clocks count time.
pub(super) fn time_display_row(
    theme: &Theme,
    candidate: TimeDisplay,
    current: TimeDisplay,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    pick_row(
        theme,
        motion::MotionKey::new("time-display", candidate.name()),
        candidate == current,
        SharedString::from(candidate.label()),
        Pick::Display(candidate),
        cx,
    )
}

/// A labelled row that installs `pick` on press, lit while it is the current
/// value.
fn pick_row(
    theme: &Theme,
    fade: motion::MotionKey,
    selected: bool,
    label: SharedString,
    pick: Pick,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    row_shell(fade, theme, fade, selected)
        .child(row_label(theme, label))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = settings_pick::apply(state, pick) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
}
