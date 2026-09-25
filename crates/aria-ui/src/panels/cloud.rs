//! The two cloud-feed widgets every surface shares: a card row, and a catalog block.
//!
//! Three panels list cloud feeds — the sidebar SFX tab, the Look tab's LUT section,
//! and the Text tab's preset packs. They differ only in which feed they name, so the
//! row and the load-and-list block live here once. A copy per panel is how one of them
//! ends up fetching from inside a render, or showing an outage as an empty list.

use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::cloud::{CloudCard, CloudFeed, FeedState};
use crate::components;
use crate::fluent::FluentBuilder;
use crate::theme::Theme;

/// A catalog feed: a Load button, then one row per card.
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
    let rows: Vec<_> = load
        .cards()
        .iter()
        .cloned()
        .map(|card| cloud_card_row(theme, feed, card, cx))
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            components::ghost_button("cloud-load", "Load catalog", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, cx| state.fetch_feed(feed, cx));
                    cx.notify();
                }),
            ),
        )
        .when_some(load.placeholder(), |el, line| {
            // A feed that is fetching gets a loader; a feed that is idle or
            // out of reach keeps the line it already had, faded up so the
            // answer arrives rather than replacing the wait in one frame.
            el.child(if matches!(load, FeedState::Loading) {
                crate::pending::working(theme, &format!("cloud.feed.{}", feed.label()), line, cx)
            } else {
                crate::pending::settled(
                    crate::motion::MotionKey::new(feed.label(), &line),
                    div()
                        .text_size(px(11.))
                        .text_color(theme.text_faint)
                        .child(SharedString::from(line)),
                )
            })
        })
        .children(rows)
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
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(crate::motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.wash(0.06),
        ))
        .on_hover(crate::motion::hover_listener(key))
        .child(div().text_size(px(13.)).text_color(text).child(name))
        .child(div().text_size(px(11.)).text_color(muted).child(detail))
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
