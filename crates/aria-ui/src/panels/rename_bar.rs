//! The visible surface for an in-progress inline rename.
//!
//! A single centred bar serves every [`RenameTarget`], because the rename state
//! is one field rather than one field per surface. Keys are routed by the shell,
//! so this draws the draft and nothing else.

use std::cell::RefCell;

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::motion::{self, CARET_BLINK};
use crate::plugins::overlay_host::{self as overlays, OverlayWhen};
use crate::theme::Theme;
use crate::types::RenameTarget;

thread_local! {
    /// The rename the bar drew last, so the exit has something to draw.
    ///
    /// `AppState::rename` is cleared the instant the rename is applied or
    /// cancelled, which is correct: the draft must not survive one frame longer
    /// than the edit. The picture still owes a fade, and it takes it from here.
    static LAST_DRAFT: RefCell<Option<(RenameTarget, String)>> = RefCell::new(None);
}

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let open = {
        let app = state.read(cx);
        app.rename.map(|target| (target, app.rename_draft.clone()))
    };
    let exiting = match open {
        Some(_) => None,
        None => overlays::exit_t(OverlayWhen::Rename, state, cx),
    };
    let Some((target, draft)) = drawn_rename(open, exiting, cx) else {
        return div().into_any_element();
    };
    let theme = Theme::of(cx);
    // The caret is drawn rather than focused, so it has to be told to blink.
    // Held solid while the bar leaves: a caret that keeps ticking through the
    // fade reads as a field that is still taking keys.
    let blink = match exiting {
        Some(_) => 1.0,
        None => {
            let phase = motion::pulse_delta(&CARET_BLINK, cx.entity_id(), cx);
            if phase < 0.55 { 1.0 } else { 0.0 }
        }
    };

    let card = div()
        .id("rename-bar-card")
        .min_w(px(320.0))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.accent)
        .p(px(Theme::SPACE_SM))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, _| {})
        .child(
            div()
                .text_size(px(10.0))
                .text_color(theme.text_muted)
                .child(SharedString::from(target.label())),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme.text)
                        .child(SharedString::from(draft)),
                )
                .child(
                    div()
                        .w(px(1.5))
                        .h(px(15.0))
                        .bg(theme.caret.opacity(blink))
                        .into_any_element(),
                ),
        )
        .child(
            div()
                .text_size(px(9.0))
                .text_color(theme.text_muted.opacity(0.7))
                .child(SharedString::from("Enter to apply, Esc to cancel")),
        );
    // Entry and exit take different element ids. GPUI keys animation state by
    // id, so a shared one would hand the exit the finished entry's clock and the
    // exit would never draw a frame.
    let card = match exiting {
        Some(t) => motion::menu_out("rename-bar", t, card).into_any_element(),
        None => motion::menu_in("rename-bar", card).into_any_element(),
    };

    div()
        .id("rename-scrim")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        // A bar on its way out takes no cancel: the rename it belonged to is
        // already applied or already dropped.
        .when(exiting.is_none(), |el| {
            el.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.cancel_rename());
                    cx.notify();
                }),
            )
        })
        // The frost wraps the animation rather than the other way round:
        // `Frosted` is not `Styled`, so it cannot be faded.
        .child(crate::frost::frost(card))
        .into_any_element()
}

/// The rename to draw: the live one, or the one still fading out.
fn drawn_rename(
    open: Option<(RenameTarget, String)>,
    exiting: Option<f32>,
    cx: &gpui::App,
) -> Option<(RenameTarget, String)> {
    if let Some(rename) = open {
        LAST_DRAFT.with(|slot| *slot.borrow_mut() = Some(rename.clone()));
        return Some(rename);
    }
    // An exit needs frames to spend and reduced motion schedules none, so the
    // fade would hang on the screen at full opacity instead of finishing.
    if exiting.is_none() || motion::reduced_motion(cx) {
        LAST_DRAFT.with(|slot| slot.borrow_mut().take());
        return None;
    }
    LAST_DRAFT.with(|slot| slot.borrow().clone())
}
