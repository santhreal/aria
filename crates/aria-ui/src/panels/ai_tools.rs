//! What the assistant can actually do, listed rather than counted.
//!
//! The Settings AI section says "27 edit(s), 9 app tool(s)". A count answers
//! "is anything wired" and nothing else: it cannot tell an operator whether the
//! model can add a marker, or which of two similar phrasings names a tool that
//! exists. The vocabulary was a registry with no reader.
//!
//! Every row here is read from that registry at draw time
//! ([`AppState::ai_tool_groups`], which walks `aria_ai::wire::tool_groups` and
//! appends the two specs the agent adds per turn), so a tool added to `aria-ai`
//! appears here by existing and a tool removed disappears. Nothing in this file
//! holds a tool name.
//!
//! Docked in the inspector column beside the assistant, and only while the
//! assistant is the panel being looked at: it explains the surface next to it,
//! and a capability list over a timeline is a list of things the operator did not
//! ask about.

use gpui::{IntoElement, ParentElement, SharedString, Styled, div, px};

use crate::app::AppState;
use crate::theme::Theme;
use crate::types::ActivePanel;

/// Whether the panel is worth a column: only while the assistant is open.
///
/// The same shape `duck` uses — the panel decides its own relevance from the
/// state rather than the shell deciding for it.
pub fn visible(state: &AppState) -> bool {
    state.active_panel == ActivePanel::Ai
}

pub fn render(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let groups = state.read(cx).ai_tool_groups();
    let total: usize = groups.iter().map(|(_, tools)| tools.len()).sum();
    let flat = state.read(cx).ai_tool_names().len();

    let mut body =
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_SM))
            .p(px(Theme::SPACE_MD))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .child("What the assistant can do"),
            )
            .child(div().text_size(px(11.)).text_color(theme.text_faint).child(
                SharedString::from(format!(
                    "{total} tool(s) in {} group(s); {flat} of them edit the project",
                    groups.len()
                )),
            ));
    for (name, tools) in groups {
        body = body.child(group_block(&theme, &name, &tools));
    }
    body
}

/// One registry group: its name, then the tools it owns.
///
/// The group name is the registry's own (`markers`, `look`, …) rather than a
/// prettier one invented here, so a refusal that names a group and this list
/// name the same thing.
fn group_block(theme: &Theme, name: &SharedString, tools: &[SharedString]) -> impl IntoElement {
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
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_faint)
                .child(SharedString::from(format!("{name} · {}", tools.len()))),
        )
        .children(tools.iter().map(|tool| {
            div()
                .text_size(px(12.))
                .text_color(theme.text_muted)
                .child(tool.clone())
        }))
}
