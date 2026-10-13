//! Home route: New, Open, Recents, Samples, Learn. Owns the window.
//!
//! The route is composed of domain-named children:
//! - `rail`: The sidebar navigation rail, brand mark, and route switching.
//! - `panes`: The individual route panes (New, Open, Recents, Samples, Learn) and recovery banner.
//! - `cards`: The action hero tiles, recents grid, card animations, and interaction dispatch.
//!
//! The recents list itself is not Home's: it outlives the route, is seeded before
//! any panel is drawn, and is written from the menu bar, so it lives in
//! [`crate::app::recents`]. Home draws what the state hands it.

mod cards;
mod panes;
mod rail;

use gpui::{Entity, InteractiveElement, IntoElement, ParentElement, Styled, div};

use crate::app::AppState;
use crate::theme::Theme;

use panes::home_body;
use rail::home_rail;

/// Render the complete Home surface (sidebar rail + active pane body).
pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let rail = state.read(cx).ui_settings.home_rail;
    // A handle on the list the state holds, not a read of the file behind it:
    // this route is animated, and the list only changes when a project does.
    let recents = state.read(cx).recents().clone();

    // No entrance of its own: the shell fades the route it is drawing, at one
    // rate for both ends and cancellable mid-flight. A one-shot here would
    // replay from zero every time Home was entered.
    div()
        .id("home")
        .size_full()
        .flex()
        .flex_row()
        .bg(theme.bg)
        .text_color(theme.text)
        .child(home_rail(&theme, rail, cx))
        .child(home_body(&theme, rail, &recents, state, cx))
}

#[cfg(test)]
mod tests {
    use crate::types::HomeRail;

    /// Every Home rail variant maps to a distinct, non-empty label and has a
    /// corresponding pane in the conventional module structure.
    #[test]
    fn every_home_rail_variant_has_distinct_label() {
        let rails = [
            HomeRail::New,
            HomeRail::Open,
            HomeRail::Recents,
            HomeRail::Samples,
            HomeRail::Learn,
        ];
        let mut labels = std::collections::BTreeSet::new();
        for rail in rails {
            let label = rail.label();
            assert!(!label.is_empty(), "rail label must not be empty");
            assert!(
                labels.insert(label),
                "duplicate rail label detected: {label}"
            );
        }
        assert_eq!(labels.len(), 5);
    }
}
