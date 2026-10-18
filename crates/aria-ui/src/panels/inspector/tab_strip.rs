//! The inspector's tab strip: the row of tabs, and the underline under the
//! active one.
//!
//! Everything the strip draws comes from [`TabHost`] — no tab is named here,
//! and the underline is placed by the active tab's position in the visible
//! list rather than by a table of offsets. A tab contributed at runtime is
//! underlined, hover-fades, and slides like any other.
//!
//! Each tab's share of the strip is its label's width plus its padding, so a
//! long label gets the room it needs before a short one gets spare room. The
//! shares are flex weights on a zero basis: tab `i` starts at the weights
//! before it over their sum and is its own weight over the sum wide, whatever
//! the strip's width turns out to be, so the underline is placed by those
//! fractions rather than in pixels.

use gpui::motion::presets;
use gpui::{
    CursorStyle, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, px, relative,
};

use crate::motion::{self, glide};
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::types::UiClipKind;

use super::tab_host::TabHost;

/// Thickness of the underline.
const INDICATOR_H: f32 = 2.0;

/// What the underline's travel is filed under. One inspector means one strip,
/// so the key is a constant rather than something derived from the tabs.
const SLIDE_KEY: &str = "inspector.tab-underline";

/// The strip.
pub(super) fn tab_row(
    theme: &Theme,
    host: &TabHost,
    kind: UiClipKind,
    active: gpui::SharedString,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let text = theme.text;
    let muted = theme.text_muted;
    let accent = theme.accent;
    let active_bg = theme.element_hover;
    let idle_bg = theme.wash(0.0);
    let hover_bg = theme.glass_hover();

    // Separate iterator passes avoid retaining a per-frame vector. The host is
    // immutable for the process, so the sums and the drawing read one table,
    // and each label's advance is a hit in the text system's cache after the
    // first frame.
    let font = cx
        .text_system()
        .resolve_font(&theme.ui_font(FontWeight::NORMAL));
    let weight = |label: &str| {
        f32::from(
            cx.text_system()
                .advance_for_text_with_font_id(label, font, Theme::TEXT_UI),
        ) + 2.0 * Theme::SPACE_SM
    };
    let total: f32 = host
        .visible(kind)
        .map(|tab| weight(tab.label.as_ref()))
        .sum();
    let mut before = 0.0;
    let mut front = None;
    for tab in host.visible(kind) {
        let share = weight(tab.label.as_ref());
        if tab.id.as_ref() == active.as_ref() {
            front = Some((before, share));
            break;
        }
        before += share;
    }
    let mut row = div().relative().flex().flex_row().w_full();
    for tab in host.visible(kind) {
        let selected = tab.id.as_ref() == active.as_ref();
        // Keyed per tab, so two tabs fading at once do not share a curve.
        let key = motion::MotionKey::new("insp.tab", &tab.id);
        let pick = tab.id.clone();
        row = row.child(
            div()
                .id(motion::MotionKey::new("insp-tab", &tab.id).element_id("insp-tab"))
                .debug_selector({
                    let id = tab.id.clone();
                    move || format!("insp-tab-{id}")
                })
                // No minimum from the label: every tab keeps its weighted
                // share, which is where the underline is drawn, and a label
                // longer than its share ends in an ellipsis.
                .flex_basis(relative(0.))
                .flex_grow(weight(tab.label.as_ref()))
                .flex_shrink(1.)
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .py(px(Theme::SPACE_XS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .text_size(Theme::TEXT_UI)
                .text_color(if selected {
                    text
                } else {
                    motion::mix(muted, text, motion::hover_t(key))
                })
                .bg(if selected {
                    active_bg
                } else {
                    motion::hover_blend(key, idle_bg, hover_bg)
                })
                .cursor(CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_hover(motion::hover_listener(key))
                .with_tip(crate::tooltip::text_builder(tab.label.clone()))
                .child(div().min_w_0().truncate().child(tab.label.clone()))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            state.inspector_tab = pick.to_string();
                        });
                        // The redraw retargets the underline, and the glide
                        // store requests frames until TAB_SLIDE is spent.
                        cx.notify();
                    }),
                ),
        );
    }

    if let Some((before, share)) = front.filter(|_| total > 0.0) {
        let left = glide::glide(
            motion::MotionKey::new(SLIDE_KEY, "left"),
            before / total,
            presets::TAB_SLIDE,
            false,
        );
        let width = glide::glide(
            motion::MotionKey::new(SLIDE_KEY, "width"),
            share / total,
            presets::TAB_SLIDE,
            false,
        );
        row = row.child(
            div()
                .debug_selector(|| "insp-tab-underline".to_owned())
                .absolute()
                .bottom_0()
                .left(relative(left))
                .w(relative(width))
                .h(px(INDICATOR_H))
                .rounded(px(INDICATOR_H / 2.0))
                .bg(accent),
        );
    }

    div()
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface_header)
        .child(row)
}
