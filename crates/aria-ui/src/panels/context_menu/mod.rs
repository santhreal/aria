//! Floating context menu over clip, track, and preview targets.

use crate::app::AppState;
use crate::motion;
use crate::plugins::overlay_host::{self as overlays, OverlayWhen};
use crate::surfaces::context_menu::{
    BadgeMenu, ClipMenu, EffectMenu, KeyframeMenu, MaskPathKeyMenu, PoolMenu, TrackMenu,
};
use crate::surfaces::labels::LabelTarget;
use crate::theme::Theme;
use crate::types::ContextTarget;
use gpui::prelude::FluentBuilder;
use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Styled, anchored, div, point, px,
};
use std::cell::RefCell;
thread_local! {
    /// The target the menu drew last.
    ///
    /// A dismissal clears `AppState::context_menu` on the frame the pointer goes
    /// down. That is the right moment for the state and the wrong one for the
    /// picture, which still owes a fade out. Remembering the target here keeps
    /// the two apart, so the close stays instant and no write has to be
    /// postponed in order to animate it.
    static LAST_TARGET: RefCell<Option<ContextTarget>> = const { RefCell::new(None) };
}
pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let open = state.read(cx).context_menu.clone();
    let exiting = match open {
        Some(_) => None,
        None => overlays::exit_t(OverlayWhen::ContextMenu, state, cx),
    };
    let Some(target) = drawn_target(open, exiting, cx) else {
        return div().into_any_element();
    };
    let theme = Theme::of(cx);
    // Every fact the rows read, gathered once, through the registry that declares
    // them: the panel that draws a row and the census that presses it must be
    // looking at the same lane.
    let rows = state.read(cx);
    let clip = match target {
        ContextTarget::Clip { clip, .. } => Some(ClipMenu::of(rows, clip)),
        ContextTarget::Transition { clip } => Some(ClipMenu::of(rows, clip)),
        _ => None,
    };
    let track = match target {
        ContextTarget::Track { track } => TrackMenu::of(rows, track),
        _ => None,
    };
    let pool = match target {
        ContextTarget::PoolItem { media } => Some(PoolMenu::of(rows, media)),
        _ => None,
    };
    let effect = match target {
        ContextTarget::Effect { clip, index } => Some(EffectMenu::of(rows, clip, index)),
        _ => None,
    };
    let badge = match target {
        ContextTarget::ClipBadge { clip, .. } => BadgeMenu::of(rows, clip),
        _ => None,
    };
    let keyframe = match target {
        ContextTarget::Keyframe { clip, param, tick } => KeyframeMenu::of(rows, clip, param, tick),
        _ => None,
    };
    let path_key = match target {
        ContextTarget::MaskPathKey { clip, mask, tick } => {
            MaskPathKeyMenu::of(rows, clip, mask, tick)
        }
        _ => None,
    };
    let selected_label = rows.selected_label();
    // Only a clip menu draws the groups, so an unrelated menu never walks the
    // sequence to count labels it will not show.
    let labels_in_use = if matches!(target, ContextTarget::Clip { .. }) {
        rows.label_counts()
    } else {
        Vec::new()
    };
    let pool_label = match target {
        ContextTarget::PoolItem { media } => rows.media_label(media),
        _ => None,
    };
    // Which blend the junction already carries, so the catalog rows can mark it
    // rather than offering fifteen that all look unapplied, and which kind it
    // takes, so a cut between sounds lists the crossfades.
    let (transition_id, transition_media) = match target {
        ContextTarget::Transition { clip } => {
            let clip = aria_models::ClipId::from_raw(clip);
            (
                rows.clip_transition_id(clip),
                rows.clip_transition_media(clip),
            )
        }
        _ => (None, None),
    };
    let plugin_rows = std::sync::Arc::clone(&rows.context_plugin_rows);
    let (at_x, at_y) = rows.context_at;
    // A menu generated from data (a clip's properties, every blend) can be
    // taller than the window, so its rows scroll past that.
    let room = (rows.last_window_h - 2.0 * Theme::SPACE_SM).max(Theme::ROW_HEIGHT * 4.0);

    // The rows scroll inside the card, so the card's ground and edge stay
    // whole under the fade over the rows' scrolled edges.
    let rows_frame = crate::edge_fade::scroll_y(
        div()
            .id("context-menu-rows")
            .p(px(Theme::SPACE_XS))
            .flex()
            .flex_col()
            .children(items(
                target,
                MenuFacts {
                    clip,
                    track,
                    pool,
                    effect,
                    badge,
                    keyframe,
                    path_key,
                    label: selected_label,
                    label_groups: labels_in_use,
                    pool_label,
                    transition_id,
                    transition_media,
                },
                &theme,
                cx,
            ))
            .children(plugin_items(&plugin_rows, &theme, cx)),
    );
    let card = div()
        .id("context-menu-card")
        .debug_selector(|| "context-menu-card".into())
        .min_w(px(180.))
        .max_h(px(room))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, _| {})
        .child(rows_frame);
    // Entry and exit take different element ids. GPUI keys animation state by
    // id, so a shared one would hand the exit the finished entry's clock and the
    // exit would never draw a frame.
    let card = match exiting {
        Some(t) => motion::menu_out("context-menu", t, card).into_any_element(),
        None => motion::menu_in("context-menu", card).into_any_element(),
    };

    div()
        .id("context-menu-scrim")
        .absolute()
        .inset_0()
        // A menu on its way out takes no dismissal: the press that closed it has
        // already landed, and a second one has nothing left to close.
        .when(exiting.is_none(), |el| {
            el.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.close_context());
                    cx.notify();
                }),
            )
        })
        .child(
            // The frost is what says the menu floats over the timeline rather
            // than belonging to it. It wraps the animation, not the other way
            // round: `Frosted` is not `Styled`, so it cannot be faded. On each
            // axis the card opens after the pointer where it fits there, before
            // it where it fits only there, and against the window's edge where
            // it fits on neither side, read from the card's measured size.
            anchored()
                .position(point(px(at_x), px(at_y)))
                .child(crate::frost::frost(card)),
        )
        .into_any_element()
}
/// Everything the menu's rows and blocks read, gathered when the menu opens so
/// nothing re-reads the session while it is on screen.
///
/// The row facts come from the registry's own constructors; what is left here is
/// what the panel draws itself: the label swatches and the mark on the blend a
/// junction already carries.
struct MenuFacts {
    /// The clip under the pointer, on a clip or junction menu.
    clip: Option<ClipMenu>,
    /// The lane the header was clicked on.
    track: Option<TrackMenu>,
    /// The pool row the menu was opened on.
    pool: Option<PoolMenu>,
    /// The effect header the menu was opened on.
    effect: Option<EffectMenu>,
    /// The clip whose fx badge the menu was opened on.
    badge: Option<BadgeMenu>,
    /// The keyframe the menu was opened on.
    keyframe: Option<KeyframeMenu>,
    /// The Path keyframe the menu was opened on.
    path_key: Option<MaskPathKeyMenu>,
    /// The label on the selection, for the swatch row.
    label: Option<aria_models::LabelColor>,
    /// Every label colour in use with its clip count, so the menu can offer any
    /// group rather than only the one the clip under the pointer carries.
    label_groups: Vec<(aria_models::LabelColor, usize)>,
    /// The label on the pool row the menu was opened on, when it was one.
    pool_label: Option<aria_models::LabelColor>,
    /// The catalog id of the blend already at the junction the menu was opened
    /// on, so the catalog rows can mark it.
    transition_id: Option<String>,
    /// Which kind of transition that junction takes.
    transition_media: Option<aria_models::TransitionMedia>,
}
// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
mod drawn;
mod label;
mod row;
use drawn::{drawn_target, items, plugin_items};

#[cfg(test)]
mod grid_tests;
#[cfg(test)]
mod place_tests;
