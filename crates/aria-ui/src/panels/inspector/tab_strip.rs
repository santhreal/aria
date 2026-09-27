//! The inspector's tab strip: the row of tabs, and the underline under the
//! active one.
//!
//! Everything the strip draws comes from [`TabHost`] — no tab is named here,
//! and the underline is placed by the active tab's position in the visible
//! list rather than by a table of offsets. A tab contributed at runtime is
//! underlined, hover-fades, and slides like any other.
//!
//! The underline is positioned as a fraction of the row instead of in pixels,
//! because a tab's width is its share of the strip and nothing measures it:
//! every tab is `flex_1`, so tab `i` of `n` starts at `i/n` and is `1/n` wide,
//! whatever the strip's width turns out to be.

use gpui::motion::presets;
use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px, relative,
};

use crate::motion::{self, glide};
use crate::theme::Theme;
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
    // immutable for the process, so count, position, and drawing read one table.
    let count = host.visible(kind).count();
    let target = host
        .visible(kind)
        .position(|tab| tab.id.as_ref() == active.as_ref());
    let mut row = div().relative().flex().flex_row().w_full();
    for tab in host.visible(kind) {
        let selected = tab.id.as_ref() == active.as_ref();
        // Keyed per tab, so two tabs fading at once do not share a curve.
        let key = motion::MotionKey::new("insp.tab", &tab.id);
        let pick = tab.id.clone();
        row = row.child(
            div()
                .id(motion::MotionKey::new("insp-tab", &tab.id).element_id("insp-tab"))
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .py(px(Theme::SPACE_XS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .text_size(px(11.))
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
                .child(tab.label.clone())
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

    if let Some(index) = target {
        let slot = 1.0 / count as f32;
        let at = glide::glide(
            motion::MotionKey::new(SLIDE_KEY, ""),
            index as f32,
            presets::TAB_SLIDE,
            false,
        );
        row = row.child(
            div()
                .absolute()
                .bottom_0()
                .left(relative(at * slot))
                .w(relative(slot))
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
        .child(row)
}
