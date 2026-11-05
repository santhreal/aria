//! The Viewer's zoom chip: the zoom on screen, and the list of levels it opens
//! above itself.

use gpui::{AnyElement, Entity, IntoElement};

use crate::app::AppState;
use crate::app::preview_view::{ViewerMenu, ZOOM_STEPS, zoom_label};
use crate::pick_list::{ListId, Opens, is_open, picker as list_picker, row};
use crate::theme::Theme;

/// The widest label the chip states. Figures are tabular, so it is as wide
/// as every four-figure level.
const WIDEST: &str = "000%";

/// The chip, and the list above it while it is open.
pub(super) fn picker(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let id = ListId::Viewer(ViewerMenu::Zoom);
    let open = is_open(state, id, cx);
    let zoom = state.read(cx).preview_view.zoom;
    let chip =
        crate::components::sized_list_chip("preview-zoom", zoom_label(zoom), WIDEST, open, theme);
    let rows = open.then(|| levels(zoom, theme, cx));
    list_picker(
        "preview-zoom",
        id,
        chip,
        Theme::ICON_HIT,
        Opens::AboveLeft,
        rows,
        theme,
        cx,
    )
}

/// Fit, then every level from the smallest.
fn levels(
    zoom: Option<f32>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
    std::iter::once(None)
        .chain(ZOOM_STEPS.into_iter().map(Some))
        .enumerate()
        .map(|(index, level)| {
            row(
                ("preview-zoom-level", index),
                zoom_label(level),
                level == zoom,
                theme,
                cx,
                move |state, _| state.set_preview_zoom(level),
            )
        })
        .collect()
}
