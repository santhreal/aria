//! Media bin.

use crate::app::media::import::MEDIA_IMPORT_EXTENSIONS;
use crate::app::picker::{Ask, FileSpec};
use crate::app::pool_preview;
use crate::app::{AppState, spawn_preview};
use crate::cloud::CloudFeed;
use crate::components;
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::theme::Theme;
use crate::timecode;
use crate::types::{
    ActivePanel, CatalogDrag, CatalogDragKind, MediaSort, POOL_CELL_WIDTH, PoolView, SidebarTab,
    UiClipKind, UiMediaItem, freeform_default_position, freeform_drop_to_position,
};
use aria_cloud::dto::StockKind;
use aria_models::{Generator, MediaId, Shape};
use gpui::{
    AppContext, CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, ObjectFit, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    StyledImage, canvas, div, img, px,
};

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    // A handle on the projection, not a copy of it.
    let store = state.read(cx).store.clone();
    let selected = state.read(cx).active_panel == ActivePanel::Media;
    let tab = state.read(cx).sidebar_tab;
    let rate = store.sequence.frame_rate;
    // The rows the pool tabs draw: a view over the projection above, computed
    // only for the two tabs that draw one.
    let rows =
        matches!(tab, SidebarTab::Media | SidebarTab::Audio).then(|| state.read(cx).pool_rows(tab));
    // What the selection already carries, so the browsable catalogs can say so.
    // Read once, next to the other reads, rather than per row. Animations occupy
    // three slots at once, which is why each of these is a set.
    let (animations, mask, transition) = {
        let state = state.read(cx);
        let animations = crate::app::clip_anim::SLOTS.map(|slot| {
            state
                .selected_animation(slot)
                .map(|animation| animation.id.clone())
        });
        let selected = state.selected_ui_clip_ref();
        (
            animations,
            selected
                .and_then(|clip| clip.mask)
                .map(|mask| mask.kind.id()),
            selected
                .and_then(|clip| clip.transition.as_ref())
                .map(|transition| transition.id.clone()),
        )
    };

    div()
        .id("sidebar")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.surface)
        .border_r_1()
        .border_color(if selected { theme.accent } else { theme.border })
        .child(header(&theme, cx))
        .child(filter_chips(&theme, tab, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .p(px(Theme::SPACE_MD))
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_SM))
                .child(match tab {
                    SidebarTab::Titles => {
                        generator_grid(&theme, &[("Title", Generator::text("Title"))], cx)
                            .into_any_element()
                    }
                    SidebarTab::Shapes => generator_grid(
                        &theme,
                        &[
                            (
                                "Rectangle",
                                Generator::shape(Shape::Rectangle, [255, 255, 255, 255]),
                            ),
                            (
                                "Ellipse",
                                Generator::shape(Shape::Ellipse, [255, 255, 255, 255]),
                            ),
                        ],
                        cx,
                    )
                    .into_any_element(),
                    SidebarTab::Stickers => catalog_grid(
                        &theme,
                        crate::catalog_text::stickers(),
                        CatalogDragKind::Sticker,
                        "Click to place at the playhead, or drag onto a lane",
                        |state, id| state.add_generated(Generator::sticker(id)),
                        |_| false,
                        cx,
                    )
                    .into_any_element(),
                    SidebarTab::Animations => catalog_grid(
                        &theme,
                        crate::catalog_text::animations(),
                        CatalogDragKind::Animation,
                        "Click for the selected clip, or drag onto a clip",
                        |state, id| state.set_selected_animation(id),
                        |id| animations.iter().flatten().any(|animation| animation == id),
                        cx,
                    )
                    .into_any_element(),
                    SidebarTab::Masks => catalog_grid(
                        &theme,
                        crate::catalog_text::masks(),
                        CatalogDragKind::Mask,
                        "Click for the selected clip, or drag onto a clip",
                        |state, id| match aria_models::MaskKind::from_id(id) {
                            Some(kind) => state.set_selected_mask_kind(kind),
                            None => Ok(()),
                        },
                        |id| mask == Some(id),
                        cx,
                    )
                    .into_any_element(),
                    SidebarTab::Transitions => catalog_grid(
                        &theme,
                        crate::catalog_text::transitions(),
                        CatalogDragKind::Transition,
                        "Drag onto a cut, or click for the selected clip's right cut",
                        |state, id| state.set_selected_transition(id),
                        |id| transition.as_deref() == Some(id),
                        cx,
                    )
                    .into_any_element(),
                    SidebarTab::Stock => {
                        cloud_grid(&theme, CloudFeed::Stock, state, cx).into_any_element()
                    }
                    SidebarTab::CloudSfx => {
                        cloud_grid(&theme, CloudFeed::Sfx, state, cx).into_any_element()
                    }
                    SidebarTab::Generate => super::generate::generate_panel(&theme, state, cx),
                    SidebarTab::Captions => super::captions::captions_panel(&theme, state, cx),
                    SidebarTab::Analysis => super::analysis::analysis_panel(&theme, state, cx),
                    SidebarTab::Markers => {
                        super::markers::markers_panel(&theme, state, cx).into_any_element()
                    }
                    SidebarTab::Media | SidebarTab::Audio => {
                        let rows = rows.as_ref().expect("the pool tabs read their rows above");
                        bin(&theme, state, tab, rows, rate.num, rate.den, cx)
                    }
                }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.active_panel = ActivePanel::Media);
                cx.notify();
            }),
        )
}
/// The stock media kinds, as chips. A data table so a new [`StockKind`] shows up as a
/// missing row here rather than as a kind nobody can ask for.
const STOCK_KINDS: [(&str, &str, StockKind); 3] = [
    ("stock-kind-video", "Video", StockKind::Video),
    ("stock-kind-photo", "Photo", StockKind::Photo),
    ("stock-kind-audio", "Audio", StockKind::Audio),
];
#[path = "sidebar_bins.rs"]
pub(crate) mod bins;
/// What follows the pointer during a catalog drag: the entry's own name, so the
/// operator can see which preset they are carrying over a dense timeline.
///
/// It carries no theme. A ghost is built inside the drag closure, where the
/// caller has only a borrow, and it renders with a context of its own — so it
/// reads the installed theme like every other surface instead of being handed
/// a copy of one at the moment the drag started.
struct DragGhost {
    label: gpui::SharedString,
}
impl gpui::Render for DragGhost {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx);
        // The ghost rides over the timeline, not the sidebar, so it is frosted
        // rather than filled: the clips it is about to land on stay readable
        // underneath it, which is the only reason to watch where it is. It
        // fades up rather than snapping to the pointer, so the drag reads as
        // having been picked up.
        motion::fade_quick(
            "catalog-drag-ghost",
            div().child(crate::frost::frost(
                div()
                    .px(px(Theme::SPACE_SM))
                    .py(px(Theme::SPACE_XS))
                    .rounded(px(Theme::CONTROL_RADIUS))
                    .border_1()
                    .border_color(theme.accent)
                    .text_size(px(12.))
                    .text_color(theme.text)
                    .child(self.label.clone()),
            )),
        )
    }
}

// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
#[path = "sidebar_grid.rs"]
mod grid;
#[path = "sidebar_media.rs"]
mod media;
#[path = "sidebar_multicam.rs"]
pub(crate) mod multicam;
#[path = "sidebar_subclip.rs"]
mod subclip;
use grid::{catalog_grid, cloud_grid, filter_chips, generator_grid, header};
use media::bin;
