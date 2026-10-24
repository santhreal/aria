//! Home route panes: New, Open, Recents, Samples, and Learn views.
//!
//! Each pane corresponds to a rail selection. In addition, an active recovery
//! offer is presented across all panes as a floating frosted banner.

use crate::components::Wraps as _;
use gpui::{Entity, InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::app::AppState;
use crate::app::recents::RecentView;
use crate::surfaces::home_pick::Pick;
use crate::theme::Theme;
use crate::types::HomeRail;
use gpui::prelude::FluentBuilder;

use super::cards::{card_action, hero_tile, learn_line, press, recents_grid, section_title};

/// Render the main content body of Home based on the selected rail tab.
///
/// The body scrolls, so every pane reads whole in a short window, as twelve
/// recents do in the smallest one. Its top and bottom fade
/// only while the pane continues past them.
pub(super) fn home_body(
    theme: &Theme,
    rail: HomeRail,
    recents: &[RecentView],
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let offer = state.read(cx).pending_recovery();
    let scroll = state.read(cx).home_scroll.clone();
    crate::edge_fade::scroll_y(
        div()
            .id("home-body")
            .debug_selector(|| "home-body".into())
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
            }),
    )
    .track_scroll(&scroll)
}

/// The offer to take back work a crash interrupted.
///
/// Shown on every Home rail rather than one of them: it is the most urgent
/// thing on the screen, and lost work should not hide behind the wrong tab.
/// Frosted, because it floats over the route rather than belonging to the pane
/// under it.
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
                    "Sample Project",
                    "A new project with a bundled still in Media.",
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
            recents_grid(theme, recents, cx).into_any_element()
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
            "Sample Project",
            "A new project with a bundled still in Media.",
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

/// The Home chords the Learn pane lists: key id, then what the chord does.
const HOME_CHORDS: [(&str, &str); 2] = [
    ("key-home-new", "New project"),
    ("key-home-open", "Open a project"),
];

/// The editor chords the Learn pane lists: key id, then what the chord does.
const EDITOR_CHORDS: [(&str, &str); 9] = [
    ("key-import", "Import media"),
    ("key-play", "Play or pause"),
    ("key-mark-in", "Mark in"),
    ("key-mark-out", "Mark out"),
    ("key-split", "Split at the playhead"),
    ("key-undo", "Undo"),
    ("key-palette", "Find any command"),
    ("key-export", "Export"),
    ("key-close", "Close the project and return Home"),
];

/// The Learn rail. Each chord is read from the key registry, so a rebinding
/// or a change to the shipped map shows here without a second edit.
pub(super) fn learn_pane(theme: &Theme, state: &AppState) -> impl IntoElement {
    let line = |(id, what): (&str, &'static str)| {
        let chord = state
            .chord_label(id)
            .unwrap_or_else(|| "Unbound".to_string());
        learn_line(theme, chord, what)
    };
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(section_title(theme, "Learn"))
        .children(HOME_CHORDS.map(line))
        .child(learn_line(
            theme,
            "Drop",
            "A .aria file opens; media starts a project",
        ))
        .child(
            div()
                .pt(px(Theme::SPACE_MD))
                .child(section_title(theme, "In the editor")),
        )
        .children(EDITOR_CHORDS.map(line))
}

#[cfg(test)]
mod learn_tests {
    /// Every chord the Learn pane lists is bound in the shipped key map.
    ///
    /// WHY: the pane reads each chord by key id and draws "Unbound" for an id
    /// the registry does not have, so a mistyped or renamed id ships a Learn
    /// row that teaches nothing, and no other test draws the pane. Covers both
    /// tables the pane draws from. Does not cover the wording of each row.
    #[test]
    fn every_learn_chord_is_bound() {
        let ctx = aria_plugin::PluginContext::new();
        ctx.mount(&crate::plugins::UiPlugin)
            .unwrap_or_else(|err| panic!("the ui plugin needs nothing: {err:?}"));
        let state = crate::app::AppState::new(ctx, crate::scratch_pool::scratch("learn"))
            .unwrap_or_else(|err| panic!("editor state: {err:?}"));
        let unbound: Vec<&str> = super::HOME_CHORDS
            .iter()
            .chain(super::EDITOR_CHORDS.iter())
            .map(|(id, _)| *id)
            .filter(|id| state.chord_label(id).is_none())
            .collect();
        assert!(
            unbound.is_empty(),
            "the Learn pane lists key ids the registry does not bind: {unbound:?}"
        );
    }
}
