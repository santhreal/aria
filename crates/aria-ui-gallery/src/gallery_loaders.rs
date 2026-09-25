//! Loaders and Surfaces: every `aria_ui::loaders` entry running, and every
//! `aria_ui::frost` entry over something worth blurring.

use aria_ui::chrome::dialog;
use aria_ui::components;
use aria_ui::frost;
use aria_ui::loaders;
use aria_ui::motion;
use aria_ui::theme::Theme;
use gpui::{
    AnyElement, Context, EntityId, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Styled, div, px,
};

use crate::gallery_kit::{backdrop, caption, group, section, shelf, specimen};
use crate::{Gallery, GalleryState};

pub fn loaders_section(
    theme: &Theme,
    state: GalleryState,
    view: EntityId,
    cx: &mut Context<Gallery>,
) -> AnyElement {
    let splash_label = if state.show_splash {
        "Hide splash"
    } else {
        "Show splash"
    };
    section(
        theme,
        "Loaders",
        "every loaders:: entry, on the clock it ships with",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(group(
                theme,
                "spinners — pulse_loader / gradient_spinner / mini_gradient_spinner",
                shelf(vec![
                    specimen(
                        theme,
                        "pulse_loader 10px",
                        loaders::pulse_loader("ld-pulse", theme, 10., view, cx),
                    ),
                    specimen(
                        theme,
                        "pulse_loader 6px",
                        loaders::pulse_loader("ld-pulse-sm", theme, 6., view, cx),
                    ),
                    specimen(
                        theme,
                        "gradient_spinner 10px",
                        loaders::gradient_spinner("ld-spin", theme, 10., view, cx),
                    ),
                    specimen(
                        theme,
                        "gradient_spinner 5px",
                        loaders::gradient_spinner("ld-spin-sm", theme, 5., view, cx),
                    ),
                    specimen(
                        theme,
                        "mini_gradient_spinner 3px",
                        loaders::mini_gradient_spinner("ld-mini", 3., view, cx),
                    ),
                    specimen(
                        theme,
                        "mini_gradient_spinner 6px",
                        loaders::mini_gradient_spinner("ld-mini-lg", 6., view, cx),
                    ),
                ]),
            ))
            .child(group(
                theme,
                "pending_row(key, label, theme, view, cx)",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_XS))
                    .child(loaders::pending_row(
                        "ld-pending-0",
                        "Indexing media",
                        theme,
                        view,
                        cx,
                    ))
                    .child(loaders::pending_row(
                        "ld-pending-1",
                        "Rendering waveform",
                        theme,
                        view,
                        cx,
                    ))
                    .child(caption(
                        theme,
                        "The rows run off one clock and are held apart by their \
                         keys: mini_gradient_spinner offsets its phase by a hash \
                         of `key`, so two rows waiting at once do not read as one \
                         animation.",
                    )),
            ))
            .child(group(
                theme,
                "aria_mark_loader(id, theme, height_px, view, cx)",
                shelf(vec![
                    specimen(
                        theme,
                        "height 48",
                        loaders::aria_mark_loader("ld-mark-sm", theme, 48., view, cx),
                    ),
                    specimen(
                        theme,
                        "height 96",
                        loaders::aria_mark_loader("ld-mark", theme, 96., view, cx),
                    ),
                ]),
            ))
            .child(group(
                theme,
                "progress_bar(key, fraction, theme, view, cx)",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_SM))
                    .w(px(280.))
                    .child(specimen(
                        theme,
                        "determinate 35%",
                        loaders::progress_bar("ld-bar-35".into(), Some(0.35), theme, view, cx),
                    ))
                    .child(specimen(
                        theme,
                        "determinate 100%",
                        loaders::progress_bar("ld-bar-full".into(), Some(1.0), theme, view, cx),
                    ))
                    .child(specimen(
                        theme,
                        "indeterminate",
                        loaders::progress_bar("ld-bar-none".into(), None, theme, view, cx),
                    ))
                    .child(specimen(
                        theme,
                        "progress_row, determinate 60%",
                        loaders::progress_row(
                            "ld-bar-row",
                            "Rendering timeline",
                            Some(0.6),
                            theme,
                            view,
                            cx,
                        ),
                    ))
                    .child(caption(
                        theme,
                        "Every bar is empty for the first settle: a wait that resolves \
                         faster than that leaves no trace, which is why these fill in \
                         a moment rather than on the frame the tab opened. The \
                         determinate fill travels to a new report, and the \
                         indeterminate one shuttles on the loader clock.",
                    )),
            ))
            .child(group(
                theme,
                "loading_word(theme)",
                shelf(vec![specimen(
                    theme,
                    "loading_word",
                    loaders::loading_word(theme),
                )]),
            ))
            .child(splash(theme, state, splash_label, cx))
            .into_any_element(),
    )
    .into_any_element()
}

/// `splash_overlay` fills its parent, so it is boxed here rather than let loose
/// over the gallery. The fading variant runs `splash_out` once per mount, which
/// is why it is behind a toggle.
fn splash(
    theme: &Theme,
    state: GalleryState,
    label: &str,
    cx: &mut Context<Gallery>,
) -> AnyElement {
    // `Div` is not `Clone` (its interactivity owns boxed listeners), so each
    // stage is built fresh rather than copied.
    let boxed = |theme: &Theme| {
        div()
            .relative()
            .w(px(320.))
            .h(px(180.))
            .overflow_hidden()
            .rounded(px(Theme::PANEL_RADIUS))
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg)
    };
    group(
        theme,
        "splash_overlay(theme, fading)",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_SM))
            .child(
                components::ghost_button("ld-splash-toggle", label, theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this: &mut Gallery, _: &MouseDownEvent, _, cx| {
                        this.state.show_splash = !this.state.show_splash;
                        cx.notify();
                    }),
                ),
            )
            .child(shelf(vec![
                specimen(
                    theme,
                    "fading = false",
                    boxed(theme).child(loaders::splash_overlay(theme, false)),
                ),
                specimen(
                    theme,
                    if state.show_splash {
                        "fading = true (mounted)"
                    } else {
                        "fading = true (unmounted)"
                    },
                    if state.show_splash {
                        boxed(theme)
                            .child(loaders::splash_overlay(theme, true))
                            .into_any_element()
                    } else {
                        boxed(theme).into_any_element()
                    },
                ),
            ]))
            .child(caption(
                theme,
                format!(
                    "splash_out is {}ms after a {}ms delay and runs once per mount; \
                     toggle to replay it.",
                    motion::SPLASH_OUT.duration_ms(),
                    motion::SPLASH_OUT.delay_ms()
                ),
            )),
    )
}

pub fn surfaces(theme: &Theme, state: GalleryState, cx: &mut Context<Gallery>) -> AnyElement {
    let glassy = theme.is_glass();
    section(
        theme,
        "Surfaces",
        "frost / frosted / layered over a patterned backdrop, plus the scrim and the dialog",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_MD))
            .child(group(
                theme,
                "frost(child) — PANEL_RADIUS, MENU_BLUR",
                shelf(vec![
                    specimen(
                        theme,
                        "frost over tokens",
                        stage(theme, frost::frost(card(theme, "frost"))),
                    ),
                    specimen(
                        theme,
                        &format!("frosted(4, {:.0})", frost::MENU_BLUR / 2.0),
                        stage(
                            theme,
                            frost::frosted(4.0, frost::MENU_BLUR / 2.0, card(theme, "frosted 22")),
                        ),
                    ),
                    specimen(
                        theme,
                        "frosted(20, 6)",
                        stage(theme, frost::frosted(20.0, 6.0, card(theme, "frosted 6"))),
                    ),
                    specimen(
                        theme,
                        "layered(child)",
                        stage(theme, frost::layered(card(theme, "layered"))),
                    ),
                ]),
            ))
            .child(caption(
                theme,
                format!(
                    "Theme::is_glass() = {glassy} (GLASS_ALPHA = {:.2}). Frosted only \
                     paints a backdrop blur while that is true; off macOS it is a \
                     pass-through and these four specimens differ only in radius.",
                    Theme::GLASS_ALPHA
                ),
            ))
            .child(group(
                theme,
                "glass tokens",
                shelf(vec![
                    specimen(theme, "glass()", fill(theme, theme.glass())),
                    specimen(theme, "glass_hover()", fill(theme, theme.glass_hover())),
                    specimen(theme, "glass_overlay()", fill(theme, theme.glass_overlay())),
                    specimen(theme, "card_glass_bg()", fill(theme, theme.card_glass_bg())),
                    specimen(
                        theme,
                        "input_glass_bg()",
                        fill(theme, theme.input_glass_bg()),
                    ),
                    specimen(
                        theme,
                        "glass_selected_bg()",
                        fill(theme, theme.glass_selected_bg()),
                    ),
                    specimen(theme, "scrim()", fill(theme, theme.scrim())),
                ]),
            ))
            .child(scrim_and_dialog(theme, state, cx))
            .into_any_element(),
    )
    .into_any_element()
}

/// The scrim and the dialog card fill their parent, so both are staged in a
/// fixed box. The dialog is behind a toggle because `dialog_in` runs once per
/// mount.
fn scrim_and_dialog(theme: &Theme, state: GalleryState, cx: &mut Context<Gallery>) -> AnyElement {
    let label = if state.show_dialog {
        "Hide dialog"
    } else {
        "Show dialog"
    };
    let scrim_stage = div()
        .relative()
        .w(px(300.))
        .h(px(200.))
        .overflow_hidden()
        .rounded(px(Theme::PANEL_RADIUS))
        .child(backdrop(theme))
        .child(div().absolute().inset_0().bg(theme.scrim()));
    let dialog_stage = div()
        .relative()
        .w(px(560.))
        .h(px(320.))
        .overflow_hidden()
        .rounded(px(Theme::PANEL_RADIUS))
        .border_1()
        .border_color(theme.border)
        .child(backdrop(theme));
    group(
        theme,
        "scrim + dialog_layer(id, title, exit, cx, body)",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_SM))
            .child(
                components::ghost_button("sf-dialog-toggle", label, theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this: &mut Gallery, _: &MouseDownEvent, _, cx| {
                        this.state.show_dialog = !this.state.show_dialog;
                        cx.notify();
                    }),
                ),
            )
            .child(shelf(vec![
                specimen(theme, "scrim() over tokens", scrim_stage),
                specimen(
                    theme,
                    if state.show_dialog {
                        "dialog_layer (mounted)"
                    } else {
                        "dialog_layer (unmounted)"
                    },
                    if state.show_dialog {
                        dialog_stage
                            .child(dialog::dialog_layer(
                                "gallery-dialog",
                                "Delete 3 clips",
                                None,
                                cx,
                                dialog_body(theme),
                            ))
                            .into_any_element()
                    } else {
                        dialog_stage.into_any_element()
                    },
                ),
            ]))
            .child(caption(
                theme,
                format!(
                    "dialog_layer draws its own scrim and runs dialog_in ({}ms) on \
                     mount; toggle to replay it.",
                    motion::DIALOG_IN.duration_ms()
                ),
            )),
    )
}

fn dialog_body(theme: &Theme) -> dialog::DialogBody {
    dialog::DialogBody::new(
        div()
            .text_size(px(12.))
            .text_color(theme.text_muted)
            .child("This cannot be undone."),
    )
    .action(components::ghost_button("dlg-cancel", "Cancel", theme))
    .action(components::danger_button("dlg-delete", "Delete", theme))
}

/// A patterned box for a blur to have something to work on.
fn stage(theme: &Theme, child: impl IntoElement + 'static) -> AnyElement {
    div()
        .relative()
        .w(px(200.))
        .h(px(140.))
        .overflow_hidden()
        .rounded(px(Theme::PANEL_RADIUS))
        .child(backdrop(theme))
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(child),
        )
        .into_any_element()
}

fn card(theme: &Theme, label: &str) -> gpui::Div {
    div()
        .w(px(140.))
        .h(px(64.))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.glass_overlay())
        .border_1()
        .border_color(theme.border_strong)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .text_color(theme.text)
        .child(label.to_string())
}

fn fill(theme: &Theme, color: gpui::Hsla) -> AnyElement {
    div()
        .relative()
        .w(px(72.))
        .h(px(44.))
        .overflow_hidden()
        .rounded(px(Theme::CONTROL_RADIUS))
        .child(backdrop(theme))
        .child(div().absolute().inset_0().bg(color))
        .into_any_element()
}
