//! Shared layout for the gallery: sections, specimens, captions, meters.
//!
//! Every section file draws through these so a reviewer reads one visual
//! grammar: a titled panel, a specimen, and the state name under it.

use aria_ui::motion;
use aria_ui::theme::Theme;
use gpui::{AnyElement, Hsla, IntoElement, ParentElement, Styled, div, px};

/// Section titles in scroll order.
///
/// The nav row and the scroll container index the same list, so a jump target
/// cannot drift from the section it names.
pub const SECTIONS: [&str; 8] = [
    "Controls",
    "States",
    "Motion",
    "Loaders",
    "Surfaces",
    "Typography",
    "Icons",
    "Theme",
];

/// A titled panel. `note` is the one line that says what the section proves.
pub fn section(theme: &Theme, title: &str, note: &str, body: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme.text_faint)
                        .child(note.to_string()),
                ),
        )
        .child(
            div()
                .p(px(Theme::SPACE_MD))
                .rounded(px(Theme::PANEL_RADIUS))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_MD))
                .child(body),
        )
        .into_any_element()
}

/// One specimen with its state name underneath. The name is the whole point:
/// a control drawn without it proves nothing a screenshot could not fake.
pub fn specimen(theme: &Theme, label: &str, body: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .items_center()
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .min_w(px(96.))
                .p(px(Theme::SPACE_SM))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_card)
                .border_1()
                .border_color(theme.border)
                .child(body),
        )
        .child(
            div()
                .text_size(px(10.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text_faint)
                .child(label.to_string()),
        )
        .into_any_element()
}

/// A wrapping specimen shelf.
pub fn shelf(children: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_end()
        .gap(px(Theme::SPACE_MD))
        .children(children)
        .into_any_element()
}

/// A titled sub-group inside a section.
pub fn group(theme: &Theme, title: &str, body: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .text_size(px(10.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text_muted)
                .child(title.to_string()),
        )
        .child(body)
        .into_any_element()
}

pub fn caption(theme: &Theme, text: impl Into<String>) -> AnyElement {
    div()
        .text_size(px(10.))
        .text_color(theme.text_dim)
        .child(text.into())
        .into_any_element()
}

pub fn swatch(theme: &Theme, label: &str, color: Hsla) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .w(px(64.))
                .h(px(36.))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(color)
                .border_1()
                .border_color(theme.border),
        )
        .child(
            div()
                .text_size(px(10.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text_faint)
                .child(label.to_string()),
        )
        .into_any_element()
}

/// A 0..1 readout: the bar and the number, so a curve can be compared against
/// the value it produced this frame.
pub fn meter(theme: &Theme, label: &str, t: f32) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .w(px(150.))
                .text_size(px(10.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text_muted)
                .child(label.to_string()),
        )
        .child(
            div()
                .w(px(180.))
                .h(px(6.))
                .rounded_full()
                .bg(theme.border)
                .child(
                    div()
                        .h_full()
                        .w(px((t.clamp(0.0, 1.0) * 180.0).max(1.0)))
                        .rounded_full()
                        .bg(theme.accent),
                ),
        )
        .child(
            div()
                .w(px(44.))
                .text_size(px(10.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text)
                .child(format!("{t:.3}")),
        )
        .into_any_element()
}

/// Pin a component's hover fade open so the hover state can be reviewed without
/// a pointer parked on it.
///
/// The key is rebuilt here from the same two parts the components hash, so a
/// specimen pins the same fade the real control drives. `reduced` is passed as
/// `true` deliberately: the fade should land instantly rather than restart
/// every frame.
pub fn pin_hover(kind: &str, id: &str) {
    motion::set_hover(motion::MotionKey::new(kind, id), true, true);
}

/// A checkerboard of theme tokens. A blur over a flat fill is invisible, so
/// every frost specimen sits on one of these.
pub fn backdrop(theme: &Theme) -> AnyElement {
    let tints = [
        theme.track_video,
        theme.track_audio,
        theme.track_text,
        theme.track_shape,
        theme.track_generator,
        theme.accent,
    ];
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .children((0..6).map(move |row| {
            div()
                .flex()
                .flex_row()
                .h(px(34.))
                .children((0..12).map(move |col| {
                    let tint = tints[(row + col) % tints.len()];
                    div().w(px(34.)).h(px(34.)).bg(tint)
                }))
        }))
        .into_any_element()
}
