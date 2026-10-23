//! The Home cards: the hero tiles, the recents grid, and the small actions on
//! them.
//!
//! Home is the first surface of a session, so it is the one that has to look
//! designed rather than assembled. Three things do that here and nowhere else in
//! the route: the cards arrive one after another instead of all at once, they
//! lift under the pointer instead of switching colour, and they sit on frosted
//! glass instead of a flat fill.

use crate::components::Wraps as _;
use gpui::motion::{loaders, presets};
use gpui::{
    AnimationExt, CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::motion;
use crate::surfaces::home_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::ContextTarget;

use crate::app::recents::RecentView;

/// Press one Home row, reporting a refusal rather than dropping it.
///
/// What a tile or a card action does is a row in
/// [`crate::surfaces::home_pick`], and this is the only place the route runs
/// one: the closures below carry a `Pick` and nothing else.
pub(super) fn press(state: &mut AppState, pick: Pick) {
    if let Err(err) = home_pick::apply(state, pick) {
        state.report_error(&err);
    }
}

/// `ms` as a share of the entry, as [`presets::FADE_IN`] measures it.
///
/// The grid's timings are fractions of one animation rather than durations of
/// their own, because `with_animation` hands out a phase from 0 to 1. This is
/// how a length in milliseconds becomes that fraction.
const fn share(ms: u64) -> f32 {
    ms as f32 / (presets::FADE_IN.delay_ms + presets::FADE_IN.duration_ms) as f32
}

/// How much of the entry each card waits out before its own fade starts: two
/// row staggers per card.
const CARD_STAGGER: f32 = share(2 * motion::ROW_STAGGER_MS);
/// The share of the entry one card's own fade takes: 200 ms.
const CARD_FADE: f32 = share(200);
/// How far a card rises under the pointer.
const HOVER_LIFT: f32 = 3.0;

/// The last stagger slot. Cards past it share the slot, so the tail of a long
/// list still completes its fade inside the animation instead of freezing part
/// way in. Derived from the two shares rather than written down beside them.
fn last_slot() -> usize {
    ((1.0 - CARD_FADE) / CARD_STAGGER).floor() as usize
}

/// Where card `index` is in the grid's entry: 0 before its turn, 1 once it has
/// arrived.
///
/// [`loaders::staggered_phase`] is the offset-and-wrap the loaders run on. The
/// clamps here are what turn a looping phase into a one-shot entry, because a
/// card that has arrived has to stay arrived. `raw` is only ever handed to the
/// helper strictly past this card's start, so the wrap cannot fire and the
/// phase is the card's own share of the fade.
pub(super) fn card_entry(raw: f32, index: usize) -> f32 {
    let slot = index.min(last_slot());
    let start = slot as f32 * CARD_STAGGER;
    if raw <= start {
        return 0.0;
    }
    (loaders::staggered_phase(raw.min(start + CARD_FADE), slot, CARD_STAGGER) / CARD_FADE)
        .clamp(0.0, 1.0)
}

/// Wrap a card in its staggered entry and its hover lift.
///
/// Both go on the same element because they are the same property: the entry
/// slides it up into place, the hover holds it a little higher.
fn arriving<E>(
    id: impl Into<motion::ControlId>,
    index: usize,
    key: motion::MotionKey,
    card: E,
) -> impl IntoElement
where
    E: Styled + IntoElement + 'static,
{
    let lift = HOVER_LIFT * motion::hover_t(key);
    card.with_animation(
        id.into().element("card-in"),
        motion::animation(presets::FADE_IN),
        move |el, raw| {
            let t = card_entry(raw, index);
            el.relative().opacity(t).top(px(6.0 * (1.0 - t) - lift))
        },
    )
}

pub(super) fn section_title(theme: &Theme, label: &'static str) -> impl IntoElement {
    div()
        .text_size(Theme::TEXT_HEADING)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme.text)
        .child(label)
}

pub(super) fn learn_line(
    theme: &Theme,
    key: impl Into<gpui::SharedString>,
    copy: &'static str,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .gap(px(Theme::SPACE_MD))
        .child(
            div()
                .w(px(96.))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(key.into()),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(copy),
        )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn hero_tile(
    id: impl Into<motion::ControlId>,
    icon: &'static str,
    title: &'static str,
    detail: &'static str,
    index: usize,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> impl IntoElement {
    let id = id.into();
    let fade = id.motion("hero-tile");
    let card = div()
        .id(id.element("hero-tile"))
        .w(px(320.))
        .px(px(Theme::SPACE_LG))
        .py(px(Theme::SPACE_LG))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(motion::hover_blend(
            fade,
            theme.surface,
            theme.glass_hover(),
        ))
        .border_1()
        .border_color(theme.border)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(fade))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(crate::icons::icon(icon).size(px(Theme::SPACE_LG)))
        .child(
            div()
                .text_size(Theme::TEXT_HEADING)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(detail),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| action(state));
                cx.notify();
            }),
        );
    crate::frost::frost(arriving(
        motion::MotionKey::new("home-tile", index),
        index,
        fade,
        card,
    ))
}

pub(super) fn recents_grid(
    theme: &Theme,
    recents: &[RecentView],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .wraps()
        .gap(px(Theme::SPACE_MD))
        .children(
            recents
                .iter()
                .enumerate()
                .map(|(index, recent)| recent_card(theme, index, recent, cx)),
        )
}

fn recent_card(
    theme: &Theme,
    index: usize,
    recent: &RecentView,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let missing = recent.missing;
    // Which row a moved project offers is the card's own answer: the label
    // and the row change together, so a Relink can never run an Open.
    let open = move |path: &std::sync::Arc<std::path::PathBuf>| {
        if missing {
            Pick::RelinkRecent((**path).clone())
        } else {
            Pick::OpenRecent((**path).clone())
        }
    };
    let path = recent.path.clone();
    let card_path = path.clone();
    let open_path = path.clone();
    let reveal_path = path.clone();
    let menu_path = path;
    let status = if missing {
        SharedString::new_static("Missing")
    } else {
        recent.opened.clone()
    };
    // The hover key carries the index for the same reason the element id does:
    // one fade per card, or the whole grid lifts with whichever card is under
    // the pointer.
    let key = gpui::ElementId::named_usize("recent", index);
    let fade = motion::MotionKey::new("recent-card", index);
    let card = div()
        .id(key)
        .debug_selector(move || format!("recent-card-{index}"))
        .w(px(280.))
        .px(px(Theme::SPACE_MD))
        .py(px(Theme::SPACE_MD))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(motion::hover_blend(
            fade,
            if missing {
                theme.warning_muted
            } else {
                theme.surface
            },
            theme.glass_hover(),
        ))
        .border_1()
        .border_color(if missing { theme.danger } else { theme.border })
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(fade))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(recent.label.clone()),
        )
        .child(
            div()
                // The dimmest tier: the folder distinguishes two projects with
                // the same name apart, and nothing else. One
                // line, giving up its start, so the folder's own name stays.
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_dim)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_start()
                .child(recent.detail.clone()),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(status),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(Theme::SPACE_SM))
                .mt(px(Theme::SPACE_XS))
                .child(card_action(
                    motion::MotionKey::new("recent-open", index),
                    if missing { "Relink" } else { "Open" },
                    theme,
                    cx,
                    move |state| press(state, open(&open_path)),
                ))
                .child(
                    card_action(
                        motion::MotionKey::new("recent-reveal", index),
                        "Reveal",
                        theme,
                        cx,
                        move |state| press(state, Pick::Reveal((*reveal_path).clone())),
                    )
                    .debug_selector(move || format!("recent-reveal-{index}")),
                ),
        )
        // The whole card opens the project, as a hero tile runs its row; the
        // actions on it stop the press, so Reveal does not open it too.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| press(state, open(&card_path)));
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let target = (*menu_path).clone();
                this.state.update(cx, |state, _| {
                    state.open_context(
                        ContextTarget::Recent { path: target },
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    );
                });
                cx.notify();
            }),
        );
    crate::frost::frost(arriving(
        motion::MotionKey::new("recent-card", index),
        index,
        fade,
        card,
    ))
    .into_any_element()
}

/// A small action on a card: a ghost button whose press stops at it, so the
/// card under it does not run its own row as well.
pub(super) fn card_action(
    key: motion::MotionKey,
    label: &'static str,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> gpui::Stateful<gpui::Div> {
    crate::components::ghost_button(key, label, theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.state.update(cx, |state, _| action(state));
            cx.notify();
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry is one-shot and complete for every card, however long the list
    /// is. A looping phase would send a card that had already arrived back to
    /// transparent, and an uncapped offset would leave the tail of a long list
    /// frozen part way in.
    #[test]
    fn every_card_starts_hidden_and_finishes_arrived() {
        for index in 0..24 {
            assert_eq!(card_entry(0.0, index), 0.0, "card {index} at rest");
            let arrived = card_entry(1.0, index);
            assert!(
                (arrived - 1.0).abs() < 1e-5,
                "card {index} ended at {arrived}"
            );
        }
    }

    /// Later cards arrive later. Without this the stagger is decoration that
    /// happens to compile.
    #[test]
    fn a_later_card_is_behind_an_earlier_one() {
        let raw = 0.2;
        let mut previous = card_entry(raw, 0);
        for index in 1..8 {
            let current = card_entry(raw, index);
            assert!(
                current <= previous,
                "card {index} at {current} overtook {previous}"
            );
            previous = current;
        }
    }

    /// Progress only ever moves forward, which is what makes it an entry rather
    /// than a pulse.
    #[test]
    fn progress_never_goes_backwards() {
        for index in 0..12 {
            let mut previous = 0.0;
            for step in 0..=100 {
                let current = card_entry(step as f32 / 100.0, index);
                assert!(
                    current >= previous - 1e-6,
                    "card {index} fell from {previous} to {current}"
                );
                previous = current;
            }
        }
    }
}
