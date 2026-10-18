//! Home route panes: New, Open, Recents, Samples, and Learn views.
//!
//! Each pane corresponds to a rail selection. In addition, an active recovery
//! offer is presented across all panes as a floating frosted banner.

use crate::components::Wraps as _;
use gpui::{Entity, InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::app::AppState;
use crate::app::recents::RecentView;
use crate::fluent::FluentBuilder;
use crate::surfaces::home_pick::Pick;
use crate::theme::Theme;
use crate::types::HomeRail;

use super::cards::{card_action, hero_tile, learn_line, press, recents_grid, section_title};

/// Render the main content body of Home based on the selected rail tab.
pub(super) fn home_body(
    theme: &Theme,
    rail: HomeRail,
    recents: &[RecentView],
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let offer = state.read(cx).pending_recovery();
    div()
        .id("home-body")
        .flex_1()
        .h_full()
        .min_w_0()
        .px(px(Theme::SPACE_LG * 2.))
        .py(px(Theme::SPACE_LG * 2.))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .when_some(offer, |el, offer| {
            el.child(recovery_banner(theme, offer, cx))
        })
        .child(match rail {
            HomeRail::New => new_pane(theme, cx).into_any_element(),
            HomeRail::Open => open_pane(theme, cx).into_any_element(),
            HomeRail::Recents => recents_pane(theme, recents, cx).into_any_element(),
            HomeRail::Samples => samples_pane(theme, state, cx).into_any_element(),
            HomeRail::Learn => learn_pane(theme, state.read(cx)).into_any_element(),
        })
}

/// The offer to take back work a crash interrupted.
///
/// Shown on every Home rail rather than one of them: it is the most urgent
/// thing on the screen, and an operator looking for their lost afternoon should
/// not have to find the right tab first. Frosted, because it floats over the
/// route rather than belonging to the pane under it.
pub(super) fn recovery_banner(
    theme: &Theme,
    offer: crate::app::recovery::RecoveryOffer,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let line = format!(
        "{} has unsaved work from a session that ended early.",
        offer.name
    );
    crate::frost::frost(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(Theme::SPACE_MD))
            .p(px(Theme::SPACE_MD))
            .rounded(px(Theme::PANEL_RADIUS))
            .bg(theme.surface_raised)
            .border_1()
            .border_color(theme.accent)
            .child(div().flex_1().text_size(Theme::TEXT_UI).child(line))
            .child(card_action(
                crate::motion::MotionKey::new("recovery", "restore"),
                "Restore",
                theme,
                cx,
                |state| press(state, Pick::RestoreRecovery),
            ))
            .child(card_action(
                crate::motion::MotionKey::new("recovery", "discard"),
                "Discard",
                theme,
                cx,
                |state| press(state, Pick::DiscardRecovery),
            )),
    )
}

pub(super) fn new_pane(
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(section_title(theme, "New project"))
        .child(hero_tile(
            "home-new",
            crate::icons::PLUS,
            "New Project",
            "Empty timeline. Import media next.",
            0,
            theme,
            cx,
            |state| press(state, Pick::NewProject),
        ))
}

pub(super) fn open_pane(
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(section_title(theme, "Open"))
        .child(hero_tile(
            "home-open",
            crate::icons::FOLDER_OPEN,
            "Open",
            "Choose a .aria project.",
            0,
            theme,
            cx,
            |state| press(state, Pick::PickProject),
        ))
}

pub(super) fn recents_pane(
    theme: &Theme,
    recents: &[RecentView],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(section_title(theme, "Start"))
        .child(
            div()
                .flex()
                .flex_row()
                .wraps()
                .gap(px(Theme::SPACE_MD))
                .child(hero_tile(
                    "home-new",
                    crate::icons::PLUS,
                    "New Project",
                    "Empty timeline. Import media next.",
                    0,
                    theme,
                    cx,
                    |state| press(state, Pick::NewProject),
                ))
                .child(hero_tile(
                    "home-open",
                    crate::icons::FOLDER_OPEN,
                    "Open",
                    "Choose a .aria project.",
                    1,
                    theme,
                    cx,
                    |state| press(state, Pick::PickProject),
                ))
                .child(hero_tile(
                    "home-sample",
                    crate::icons::CLAPPERBOARD,
                    "Stills sequence",
                    "Opens a bundled still so the first run is not empty.",
                    2,
                    theme,
                    cx,
                    |state| press(state, Pick::OpenSample),
                )),
        )
        .child(section_title(theme, "Recents"))
        .child(if recents.is_empty() {
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child("Start a new project or open a .aria file.")
                .into_any_element()
        } else {
            crate::edge_fade::edge_faded(24.0, true, true, recents_grid(theme, recents, cx))
                .into_any_element()
        })
}

pub(super) fn samples_pane(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(section_title(theme, "Samples"))
        .child(hero_tile(
            "home-sample",
            crate::icons::CLAPPERBOARD,
            "Stills sequence",
            "Opens a bundled still so the first run is not empty.",
            0,
            theme,
            cx,
            |state| press(state, Pick::OpenSample),
        ))
        .child(hero_tile(
            "home-template",
            crate::icons::CLAPPERBOARD,
            "Use Template…",
            "Opens a .ariat bundle and fills its slots with your media.",
            1,
            theme,
            cx,
            |state| press(state, Pick::UseTemplate),
        ))
        .child(section_title(theme, "Cloud templates"))
        .child(crate::panels::cloud::cloud_catalog_block(
            theme,
            crate::cloud::CloudFeed::Templates,
            state,
            cx,
        ))
}

/// The Learn rail. Each chord is read from the key registry, so a rebinding
/// or a change to the shipped map shows here without a second edit.
pub(super) fn learn_pane(theme: &Theme, state: &AppState) -> impl IntoElement {
    let chord = |id: &str| {
        state
            .chord_label(id)
            .unwrap_or_else(|| "Unbound".to_string())
    };
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(section_title(theme, "Learn"))
        .child(learn_line(theme, chord("key-home-new"), "New project"))
        .child(learn_line(theme, chord("key-home-open"), "Open a project"))
        .child(learn_line(
            theme,
            chord("key-close"),
            "Close project, return Home",
        ))
        .child(learn_line(theme, "Esc", "Does nothing on Home"))
        .child(learn_line(
            theme,
            "Drop",
            ".aria opens. Media starts a project.",
        ))
}
