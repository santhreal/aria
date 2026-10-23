//! The Info dock panel: the colour and position under the pointer in the
//! Viewer, the selected clip's times and source, the drag in progress, the
//! faults drivers and plugins reported, what the last rendered frame cost, and
//! the rate playback held.
//!
//! It draws [`AppState::info_readout`] and sends nothing.

use gpui::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, px,
};

use crate::app::AppState;
use crate::app::info::{InfoFault, InfoPlayback, InfoRow};
use crate::panels::inspector::blocks::widgets::{kv_row, kv_row_tinted, section_label, swatch};
use crate::theme::Theme;

/// The surface name of the panel.
const SURFACE: &str = "info-panel";

/// Window > Info.
pub fn render(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let theme = Theme::of(cx);
    let readout = state.read(cx).info_readout();
    let mut body: Vec<AnyElement> = vec![section_label(&theme, "Pointer").into_any_element()];
    match readout.pointer {
        Some([x, y]) => {
            body.push(kv_row(&theme, "X", x.to_string()).into_any_element());
            body.push(kv_row(&theme, "Y", y.to_string()).into_any_element());
            body.push(color_row(&theme, readout.color));
        }
        None => body.push(hint(&theme, "Move the pointer over the Viewer.")),
    }
    body.push(section_label(&theme, "Clip").into_any_element());
    if readout.clip.is_empty() {
        body.push(hint(&theme, "Select a clip."));
    } else {
        body.extend(rows(&theme, readout.clip));
    }
    if !readout.drag.is_empty() {
        body.push(section_label(&theme, "Drag").into_any_element());
        body.extend(rows(&theme, readout.drag));
    }
    body.push(section_label(&theme, "Errors").into_any_element());
    if readout.faults.is_empty() {
        body.push(hint(&theme, "No errors."));
    } else {
        body.extend(
            readout
                .faults
                .into_iter()
                .map(|fault| fault_row(&theme, fault)),
        );
    }
    body.push(section_label(&theme, "Last frame").into_any_element());
    if readout.frame.is_empty() {
        body.push(hint(&theme, "No frame rendered yet."));
    } else {
        body.extend(rows(&theme, readout.frame));
    }
    if let Some(playback) = readout.playback {
        body.push(section_label(&theme, "Playback").into_any_element());
        body.extend(playback_rows(&theme, playback));
    }
    div()
        .id(SURFACE)
        .debug_selector(|| SURFACE.into())
        .size_full()
        // Every row is a readout that changes with the pointer or the drag.
        .font_features(crate::theme::tabular_figures())
        .overflow_y_scroll()
        .p(px(Theme::SPACE_SM))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .children(body)
        .into_any_element()
}

/// One labelled value per row.
fn rows(theme: &Theme, rows: Vec<InfoRow>) -> impl Iterator<Item = AnyElement> + '_ {
    rows.into_iter()
        .map(move |row| kv_row(theme, row.label, row.value).into_any_element())
}

/// The colour under the pointer: a swatch, its channels and its hex, or a dash
/// until the frame on screen has been read there.
fn color_row(theme: &Theme, color: Option<[u8; 4]>) -> AnyElement {
    let Some([r, g, b, a]) = color else {
        return kv_row(theme, "Colour", "–").into_any_element();
    };
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .text_size(Theme::TEXT_UI)
                .child(div().text_color(theme.text_muted).child("Colour"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(Theme::SPACE_XS))
                        .child(
                            div()
                                .size(px(12.0))
                                .border_1()
                                .border_color(theme.border)
                                .bg(swatch([r, g, b, a])),
                        )
                        .child(
                            div()
                                .text_color(theme.text)
                                .child(format!("#{r:02X}{g:02X}{b:02X}")),
                        ),
                ),
        )
        .child(kv_row(theme, "RGB", format!("{r} {g} {b}")))
        .child(kv_row(
            theme,
            "Alpha",
            format!("{:.0} %", f32::from(a) * 100.0 / 255.0),
        ))
        .into_any_element()
}

/// A fault: where it came from, then why.
fn fault_row(theme: &Theme, fault: InfoFault) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .text_size(Theme::TEXT_UI)
        .child(div().text_color(theme.text_muted).child(fault.source))
        .child(div().text_color(theme.danger).child(fault.message))
        .into_any_element()
}

/// The playback rate in the colour of its verdict, green while the run holds
/// the sequence's rate and amber once it does not, then the late frames.
fn playback_rows(theme: &Theme, playback: InfoPlayback) -> [AnyElement; 2] {
    let verdict = if playback.holds_rate {
        theme.success
    } else {
        theme.warning
    };
    let [rate, late] = playback.rows;
    [
        kv_row_tinted(theme, rate.label, rate.value, verdict).into_any_element(),
        kv_row(theme, late.label, late.value).into_any_element(),
    ]
}

/// A faint line standing in for a section with nothing to show.
fn hint(theme: &Theme, text: &'static str) -> AnyElement {
    div()
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_faint)
        .child(text)
        .into_any_element()
}
