//! The page a frame's panel is laid out on.
//!
//! A frame can be dragged down to its minimum height, 80 px for most panels,
//! which leaves about 54 px under the tab row. A panel whose fixed rows are
//! taller than that, a search field over a list over a footer, has nowhere to
//! put them, and the frame would clip them out of reach. Such a panel is laid
//! out on a page at least [`PAGE_FLOORS`] high, and the frame scrolls the page.
//! A frame taller than the floor draws the page at the frame's height, so
//! nothing scrolls and the panel's own lists scroll as before.
//!
//! A panel not in the table draws straight into the frame. The Viewer and the
//! Timeline fit their minimum frames and keep their own scrolling.

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _, div, px,
};

use crate::dock::{
    PANEL_ASSISTANT, PANEL_AUDIO_METERS, PANEL_EFFECTS, PANEL_INSPECTOR, PANEL_MIXER,
    PANEL_PROJECT, PanelId,
};

/// The height each panel is laid out at, at least, in px: its fixed rows plus
/// room for a few lines of its list.
const PAGE_FLOORS: &[(PanelId, f32)] = &[
    (PANEL_PROJECT, 240.0),
    (PANEL_INSPECTOR, 240.0),
    (PANEL_EFFECTS, 160.0),
    (PANEL_ASSISTANT, 240.0),
    (PANEL_MIXER, 280.0),
    (PANEL_AUDIO_METERS, 112.0),
];

/// The page floor of `panel`, if it is laid out on one.
fn page_floor(panel: PanelId) -> Option<f32> {
    PAGE_FLOORS
        .iter()
        .find(|(id, _)| *id == panel)
        .map(|&(_, floor)| floor)
}

/// `children` on a page at least the panel's floor high, in a scroller the
/// frame's body fills; `children` as they are for a panel with no floor.
pub(super) fn page(panel: PanelId, children: Vec<AnyElement>) -> Vec<AnyElement> {
    let Some(floor) = page_floor(panel) else {
        return children;
    };
    let page = div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .w_full()
        .h_full()
        .min_h(px(floor))
        .children(children);
    vec![
        crate::edge_fade::scroll_y(div().id(panel).flex().flex_col().size_full().child(page))
            .into_any_element(),
    ]
}
