//! Label: the swatch palette and the group chips.
//!
//! Two blocks the panel draws itself, because a colour is recognized by sight
//! and a row of colour *names* is what a label is meant to replace. What each
//! press asks for is declared in [`crate::surfaces::labels`], so the palette can
//! be driven by `app::drawn_census` even though its controls are not text rows.

use super::*;
use crate::components::Wraps as _;
use crate::surfaces::labels::{self, LabelTarget};
use gpui::StatefulInteractiveElement;

/// The sixteen label colours plus a clear, as rows of swatches.
///
/// The current label is ringed, so the palette says what is set as well as what
/// can be set.
pub(super) fn label_row(
    target: LabelTarget,
    current: Option<aria_models::LabelColor>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let prefix = target.id_prefix();
    let mut swatches: Vec<gpui::AnyElement> = Vec::new();
    for (index, label) in labels::swatches().enumerate() {
        let chosen = current == label && label.is_some();
        let swatch = div()
            .id((prefix, index as u64))
            .w(px(16.))
            .h(px(16.))
            .rounded(px(Theme::CONTROL_RADIUS))
            .cursor(CursorStyle::PointingHand)
            .active(|style| style.opacity(0.92))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = target.write(state, label) {
                            state.report_error(&err);
                        }
                        state.close_context();
                    });
                    cx.notify();
                }),
            );
        // The clear reads as an absence: an outline with a cross in it, rather
        // than a tenth colour that would look like one more label to pick.
        swatches.push(match label {
            Some(color) => swatch
                .bg(theme.label(color, false))
                .when(chosen, |el| el.border_2().border_color(theme.text))
                .into_any_element(),
            None => swatch
                .border_1()
                .border_color(theme.border)
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child("×")
                .into_any_element(),
        });
    }
    div()
        .flex()
        .flex_row()
        .wraps()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .children(swatches)
        .into_any_element()
}

/// Every label group in use, as one row of counted swatches. Clicking one
/// selects that whole group.
///
/// Offered instead of a single "same label" row because the reach worth having
/// is any group, not only the one the clip under the pointer happens to carry,
/// and the count says how large a selection the click is about to make. A clip
/// with no label of its own counts under the label it is drawn in. `None` when
/// the sequence has no clips.
pub(super) fn label_groups(
    groups: &[(aria_models::LabelColor, usize)],
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<gpui::AnyElement> {
    if groups.is_empty() {
        return None;
    }
    let mut chips: Vec<gpui::AnyElement> = Vec::with_capacity(groups.len());
    for (index, (color, count)) in groups.iter().enumerate() {
        let color = *color;
        chips.push(
            div()
                .id(("ctx-label-group", index as u64))
                .px(px(Theme::SPACE_XS))
                .h(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.label(color, false))
                .text_size(Theme::TEXT_SMALL)
                .text_color(theme.label_ink(color, false))
                .cursor(CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            labels::select_group(state, color);
                            state.close_context();
                        });
                        cx.notify();
                    }),
                )
                .child(count.to_string())
                .into_any_element(),
        );
    }
    Some(
        div()
            .flex()
            .flex_row()
            .wraps()
            .items_center()
            .gap(px(Theme::SPACE_XS))
            .px(px(Theme::SPACE_SM))
            .pb(px(Theme::SPACE_XS))
            .child(
                div()
                    .text_size(Theme::TEXT_SMALL)
                    .text_color(theme.text_faint)
                    .child("Select"),
            )
            .children(chips)
            .into_any_element(),
    )
}
