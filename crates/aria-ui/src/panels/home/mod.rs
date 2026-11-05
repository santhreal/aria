//! Home route: the New and Open actions, and the Recents, Samples and Learn
//! panes. Owns the window.
//!
//! The route is composed of domain-named children:
//! - `rail`: The sidebar rail: brand mark, the two actions, and pane switching.
//! - `panes`: The Recents, Samples and Learn panes and the recovery banner.
//! - `cards`: The action hero tiles, recents grid, card animations, and interaction dispatch.
//!
//! The recents list itself is not Home's: it outlives the route, is seeded before
//! any panel is drawn, and is written from the menu bar, so it lives in
//! [`crate::app::recents`]. Home draws what the state hands it.

mod cards;
#[cfg(test)]
mod layout_tests;
mod panes;
#[cfg(test)]
mod press_tests;
mod rail;
#[cfg(test)]
mod rail_tests;
#[cfg(test)]
mod scroll_tests;

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

    /// WHY: `HomeRail::ALL` is the one list of panes the rail draws and the
    /// tests and the panels reference walk. The match stops the build on a new
    /// variant until it is placed here, and the placement must agree with
    /// `ALL`, so a pane drawn out of order or two panes under one label fail.
    /// Does not catch a new variant placed here but left out of `ALL`.
    #[test]
    fn every_home_pane_stands_once_in_all_under_its_own_label() {
        let placed = |rail: HomeRail| match rail {
            HomeRail::Recents => 0,
            HomeRail::Samples => 1,
            HomeRail::Learn => 2,
        };
        let mut labels = std::collections::BTreeSet::new();
        for (index, rail) in HomeRail::ALL.into_iter().enumerate() {
            assert_eq!(placed(rail), index, "{rail:?} stands out of rail order");
            let label = rail.label();
            assert!(!label.is_empty(), "{rail:?} has no label");
            assert!(labels.insert(label), "two panes are labeled {label}");
        }
    }

    /// WHY: New and Open were panes, so a settings file can hold `"new"` or
    /// `"open"`. Closes "a stale rail fails the whole settings file": each
    /// loads on Recents, whose Start row holds both tiles, and every current
    /// pane still round-trips. Does not cover a value no version wrote.
    #[test]
    fn a_stored_new_or_open_rail_loads_on_recents() {
        for stale in ["\"new\"", "\"open\""] {
            let rail: HomeRail = serde_json::from_str(stale)
                .unwrap_or_else(|err| panic!("{stale} does not load: {err}"));
            assert_eq!(rail, HomeRail::Recents, "{stale}");
        }
        for rail in HomeRail::ALL {
            let stored = serde_json::to_string(&rail).expect("a rail serializes");
            let loaded: HomeRail = serde_json::from_str(&stored).expect("a rail loads");
            assert_eq!(loaded, rail, "{stored}");
        }
    }
}
