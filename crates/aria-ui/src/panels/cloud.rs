//! The cloud-feed widgets every surface shares: a card row, a catalog block,
//! and a feed list that fills a panel.
//!
//! The sidebar's Stock and SFX tabs fill their panel with a feed; the Look
//! tab's LUT section, the Text tab's preset packs, Home's templates and the
//! Assistant's skills hold one as a section among others. They differ only in
//! which feed they name, so the row, the block and the list are defined here
//! once. A copy per panel is how one of them ends up fetching from inside a
//! render, or showing an outage as an empty list.

use gpui::{
    AnyElement, CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::cloud::{CloudCard, CloudFeed, FeedState};
use crate::components;
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

/// A catalog feed: a Load button, then one row per card. Every line's text
/// starts one ghost inset in, where the Load button's label and each card's
/// name start, so a caller hangs the block to stand its text on a column.
///
/// The feed is never fetched from inside this function. A render that started network
/// traffic would refetch on every repaint, so loading is an explicit click.
pub(crate) fn cloud_catalog_block(
    theme: &Theme,
    feed: CloudFeed,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let load = state.read(cx).feed(feed);
    let loading = matches!(load, FeedState::Loading);
    let placeholder = load.placeholder();
    let rows: Vec<_> = match load {
        FeedState::Ready(cards) => cards
            .into_iter()
            .map(|card| cloud_card_row(theme, feed, card, cx))
            .collect(),
        _ => Vec::new(),
    };

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        // In a row of its own, so the button is as wide as its label rather
        // than the column.
        .child(div().flex().child(
            components::ghost_button("cloud-load", "Load catalog", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, cx| state.fetch_feed(feed, cx));
                    cx.notify();
                }),
            ),
        ))
        .when_some(placeholder, |el, line| {
            // A feed that is fetching gets a loader; a feed that failed or
            // holds nothing states it, faded up so the answer arrives rather
            // than replacing the wait in one frame.
            el.child(div().px(px(components::GHOST_INSET.0)).child(if loading {
                crate::pending::working(theme, &format!("cloud.feed.{}", feed.label()), line, cx)
            } else {
                crate::pending::settled(
                    crate::motion::MotionKey::new(feed.label(), &line),
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_faint)
                        .child(SharedString::from(line)),
                )
            }))
        })
        .children(rows)
}

/// A feed that fills its panel: the cards as a list that scrolls, or, while
/// there are none, one line in place of them. A search feed's list stands
/// under its search form, so the line stands where the first card would, on
/// the column's edge; a catalog's list is its whole tab, so the line is
/// centered in it.
///
/// `idle` is the line before the first fetch. `action` is the control under
/// the line before the first fetch and after a failed one, the click that
/// fetches. The caller lays the list out as a flex item of a column, so it
/// takes the height the column has left. The list hangs one ghost inset past
/// the column on each side, so each card's name starts on the column's edge.
pub(crate) fn cloud_feed_list(
    theme: &Theme,
    feed: CloudFeed,
    state: &Entity<AppState>,
    idle: impl Into<SharedString>,
    action: Option<AnyElement>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let id = feed.label();
    let search = feed.is_search();
    let empty = |text: SharedString| {
        if search {
            components::list_empty(id, text, theme)
        } else {
            components::empty_state(id, text, theme)
        }
    };
    match state.read(cx).feed(feed) {
        FeedState::Ready(cards) if !cards.is_empty() => crate::edge_fade::scroll_y(
            div()
                .id(crate::motion::MotionKey::new("cloud-feed-list", id).element_id("list"))
                .debug_selector(move || format!("cloud-feed-list-{id}"))
                .flex_1()
                .min_h_0()
                .mx(px(-components::GHOST_INSET.0))
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XXS))
                .children(
                    cards
                        .into_iter()
                        .map(|card| cloud_card_row(theme, feed, card, cx)),
                ),
        )
        .into_any_element(),
        FeedState::Loading => {
            let working =
                crate::pending::working(theme, &format!("cloud.feed.{id}"), "Loading…", cx);
            if search {
                div().child(working)
            } else {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(working)
            }
            .into_any_element()
        }
        FeedState::Idle => empty(idle.into()).children(action).into_any_element(),
        FeedState::Failed(err) => empty(err.into()).children(action).into_any_element(),
        FeedState::Ready(_) => empty("Nothing found".into()).into_any_element(),
    }
}

/// One cloud card. Clicking downloads it into the cache and adopts it, which is
/// whatever its feed means: an import, a LUT, a template, or a preset pack.
///
/// The element id carries the feed as well as the card id, because the same asset id
/// can appear under two feeds and two elements must not share one id.
pub(crate) fn cloud_card_row(
    theme: &Theme,
    feed: CloudFeed,
    card: CloudCard,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    // Keyed on the feed as well as the card because one asset can appear under
    // two feeds, and two rows must not share a fade any more than they may
    // share a hitbox. The element id is that same identity, so neither the key
    // nor the id costs a string per card per frame.
    let key = crate::motion::MotionKey::new(feed.label(), &card.id);
    let id = key.element_id("cloud-card");
    let lit = crate::motion::hover_t(key);
    let text = crate::motion::mix(theme.text_muted, theme.text, lit);
    let muted = crate::motion::mix(theme.text_faint, theme.text_muted, lit);
    let name = card.name.clone();
    let detail = card.detail.clone();
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .px(px(components::GHOST_INSET.0))
        .py(px(components::GHOST_INSET.1))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(crate::motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.wash(0.06),
        ))
        .on_hover(crate::motion::hover_listener(key))
        .child(div().text_size(Theme::TEXT_UI).text_color(text).child(name))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(muted)
                .child(detail),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                let card = card.clone();
                this.state.update(cx, |state, cx| {
                    if let Err(err) = state.import_cloud_card(feed, card, cx) {
                        state.report_error(err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
}
