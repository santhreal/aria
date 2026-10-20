//! The pool grid: the wall of cards, one card, and a card's picture.
//!
//! Split out of `sidebar_media.rs` to stay under the file ceiling. A pure move.
//! Each item is `pub(super)` so the parent and its other children can reach it
//! by name; nothing else changed.

use super::*;
use crate::app::pool_rows::PoolRows;
use crate::components::Wraps as _;
use crate::motion::RiseOnHover as _;
use crate::surfaces::pool_pick;

pub(super) fn media_grid(
    theme: &Theme,
    state: &Entity<AppState>,
    tab: SidebarTab,
    rows: &PoolRows,
    view: PoolView,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    if rows.is_empty() {
        return div()
            .text_size(Theme::TEXT_UI)
            .text_color(theme.text_faint)
            .child("Import files or drag from your file manager.")
            .into_any_element();
    }
    let missing_any = rows.iter().any(|item| item.missing);
    let items: gpui::AnyElement = match view {
        PoolView::Freeform => {
            let mut tiles: Vec<gpui::AnyElement> = Vec::with_capacity(rows.len());
            let mut max_x: f32 = 280.0;
            let mut max_y: f32 = 320.0;
            for (index, item) in rows.iter().enumerate() {
                let (x, y) = item
                    .position
                    .unwrap_or_else(|| freeform_default_position(index));
                max_x = max_x.max(x + POOL_CELL_WIDTH + 24.0);
                max_y = max_y.max(y + 90.0 + 24.0);
                let tile = media_tile(theme, state, tab, item, index, view, cx);
                tiles.push(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(POOL_CELL_WIDTH))
                        .child(tile)
                        .into_any_element(),
                );
            }
            div()
                .id("pool-freeform-canvas")
                .relative()
                .w(px(max_x))
                .min_h(px(max_y))
                .on_drag_move(cx.listener(
                    move |this, event: &gpui::DragMoveEvent<crate::types::MediaDrag>, _, _| {
                        let pos = freeform_drop_to_position(
                            (
                                f32::from(event.event.position.x),
                                f32::from(event.event.position.y),
                            ),
                            (
                                f32::from(event.bounds.origin.x),
                                f32::from(event.bounds.origin.y),
                            ),
                        );
                        this.set_freeform_drop(pos.0, pos.1);
                    },
                ))
                // The colour, not the theme: a drag-over closure outlives this
                // call, and a borrowed theme cannot escape into it.
                .drag_over::<crate::types::MediaDrag>({
                    let wash = theme.wash(0.04);
                    move |style, _, _, _| style.bg(wash)
                })
                .on_drop(
                    cx.listener(move |this, drag: &crate::types::MediaDrag, _, cx| {
                        let media = drag.media;
                        if let Some((x, y)) = this.take_freeform_drop() {
                            this.state.update(cx, |state, _| {
                                let _ = crate::surfaces::pool_drop::apply(
                                    state,
                                    crate::surfaces::pool_drop::Dropped::MediaAtSpot {
                                        media,
                                        x,
                                        y,
                                    },
                                );
                            });
                            cx.notify();
                        }
                    }),
                )
                .children(tiles)
                .into_any_element()
        }
        PoolView::Icons => {
            let mut tiles: Vec<gpui::AnyElement> = Vec::with_capacity(rows.len());
            for (index, item) in rows.iter().enumerate() {
                tiles.push(media_tile(theme, state, tab, item, index, view, cx));
            }
            div()
                .flex()
                .gap(px(Theme::SPACE_XS))
                .flex_row()
                .wraps()
                .children(tiles)
                .into_any_element()
        }
        PoolView::Tiles | PoolView::List => {
            let mut tiles: Vec<gpui::AnyElement> = Vec::with_capacity(rows.len());
            for (index, item) in rows.iter().enumerate() {
                tiles.push(media_tile(theme, state, tab, item, index, view, cx));
            }
            div()
                .flex()
                .gap(px(Theme::SPACE_XS))
                .flex_col()
                .children(tiles)
                .into_any_element()
        }
    };
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .when(missing_any, |el| {
            el.child(
                components::ghost_button("relink-all", "Relink All", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| state.relink_all());
                        cx.notify();
                    }),
                ),
            )
        })
        .child(items)
        .into_any_element()
}

pub(super) fn media_tile(
    theme: &Theme,
    state: &Entity<AppState>,
    tab: SidebarTab,
    item: &UiMediaItem,
    index: usize,
    view: PoolView,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = item.id;
    let missing = item.missing;
    let picked = state.read(cx).pool_selection.contains(id);
    let key = motion::MotionKey::new("sidebar.media", id);
    let lit = motion::hover_t(key);
    let caption = crate::app::media::media_caption(item);
    let caption_color = if missing {
        theme.warning
    } else {
        theme.text_muted
    };
    // Why the last Relink All left this one missing. Drawn, not tooltipped: a
    // refusal that needs a hover to find is a silent skip with extra steps.
    let note = if missing {
        state.read(cx).relink_notes.get(&id).cloned()
    } else {
        None
    };
    let relink = missing.then(|| {
        components::ghost_button("relink-media", "Relink", theme).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    state.request_pick(
                        Ask::OpenFile(
                            FileSpec::new("Relink media")
                                .filter("Media", &media_import_extensions()),
                        ),
                        move |state, answer| {
                            let Some(path) = answer.one() else {
                                return;
                            };
                            if let Err(err) = state.relink_media(MediaId::from_raw(id), path) {
                                state.report_error(&err);
                            }
                        },
                    );
                });
                cx.notify();
            }),
        )
    });
    let tile = div()
        .id(gpui::ElementId::named_usize("media", id as usize))
        .debug_selector(move || format!("pool-item-{index}"))
        .flex()
        // The compact view reads left to right, so the name and the facts share
        // one line instead of stacking into two.
        .either(
            view.inline(),
            |el| el.flex_row().items_center().justify_between(),
            |el| el.flex_col(),
        )
        .when_some(view.width(), |el, width| el.w(px(width)))
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(key, theme.wash(0.0), theme.wash(0.06)))
        // A picked card is drawn picked, which is what makes a
        // selection of several visible at all. The border is always drawn and
        // only its colour changes, so picking a card cannot shift the row it
        // sits in by a pixel.
        .border_1()
        .border_color(if picked {
            theme.border_strong
        } else {
            theme.wash(0.0)
        })
        .when(picked, |el| el.bg(theme.glass_selected_bg()))
        // The tile rises under the pointer. A pool of near-identical
        // thumbnails needs the one that is about to be placed to stand out
        // from the ones beside it, and colour alone does not do it.
        .rise_on_hover(2.0, lit, motion::hover_listener(key))
        // Drawing the picture is what asks for a decode, so a view that does not
        // show one costs nothing per item beyond its text.
        .when(
            view.picture() && !missing && item.kind != UiClipKind::Audio,
            |el| el.child(thumbnail(theme, state, item.id, item.duration_ticks, cx)),
        )
        .child(
            // A name with no space to wrap at ends in an ellipsis at the
            // tile's edge rather than drawing past the panel.
            div()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .when_some(item.label, |el, label| {
                    el.child(
                        div()
                            .flex_none()
                            .w(px(8.))
                            .h(px(8.))
                            .rounded(px(4.))
                            .bg(theme.label(label, false)),
                    )
                })
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(item.name.clone()),
                ),
        )
        .when(view.facts(), |el| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(caption_color)
                    .child(caption),
            )
        })
        .when_some(note, |el, note| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.warning)
                    .child(note),
            )
        })
        .when_some(relink, |el, btn| el.child(btn))
        .when(!missing, |el| {
            let label = item.name.clone();
            el.on_drag(
                crate::types::MediaDrag {
                    media: id,
                    label,
                    from_source: false,
                },
                move |drag, _offset, _window, cx| {
                    let label = drag.label.clone();
                    cx.new(|_| DragGhost { label })
                },
            )
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                // Which physical key is the platform's multi-select modifier is
                // the panel's business; what the two of them mean is the
                // registry's.
                let reach = pool_pick::Reach::of(
                    event.modifiers.secondary(),
                    event.modifiers.shift,
                    event.click_count,
                );
                this.state
                    .update(cx, |state, _| pool_pick::apply(state, reach, id, tab));
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    state.select_pool_item(id);
                    state.open_context(
                        crate::types::ContextTarget::PoolItem { media: id },
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    );
                });
                cx.notify();
            }),
        );
    crate::pending::arriving(crate::motion::MotionKey::new("media-in", id), index, tile)
}

/// A tile's picture: the first frame of the source, or the frame under the
/// pointer while the row is being scrubbed.
///
/// The pointer position is turned into a source tick against the rectangle this
/// element was last painted in, which a mouse move does not carry — hence the
/// measuring canvas. A glyph stands in until the first frame arrives, so a tile
/// never collapses while a decode is in flight.
pub(super) fn thumbnail(
    theme: &Theme,
    state: &Entity<AppState>,
    media: u64,
    duration_ticks: i64,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let tick = state.read(cx).pool_tile_tick(media);
    pool_preview::request_thumb(state, cx, media, tick);
    let image = state.read(cx).pool_preview.get(media, tick).cloned();
    let measure = {
        let state = state.clone();
        canvas(
            move |bounds, _, cx| {
                let left = f32::from(bounds.origin.x);
                let width = f32::from(bounds.size.width);
                state.update(cx, |state, _| {
                    state.pool_preview.record_rect(media, left, width)
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    };
    div()
        .id(gpui::ElementId::named_usize("media-thumb", media as usize))
        .relative()
        .w_full()
        .h(px(pool_preview::THUMB_H as f32))
        .rounded(px(Theme::CONTROL_RADIUS))
        .overflow_hidden()
        .bg(theme.surface_raised)
        .flex()
        .items_center()
        .justify_center()
        .child(measure)
        .when(image.is_none(), |el| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child("▣"),
            )
        })
        .when_some(image, |el, image| {
            el.child(crate::components::picture(
                img(image)
                    .id(gpui::ElementId::named_usize(
                        "media-thumb-img",
                        media as usize,
                    ))
                    .object_fit(ObjectFit::Contain)
                    .size_full(),
            ))
        })
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            let Some((left, width)) = this.state.read(cx).pool_preview.rect(media) else {
                return;
            };
            let tick =
                pool_preview::scrub_tick(f32::from(event.position.x) - left, width, duration_ticks);
            if this.state.read(cx).pool_hover == Some((media, tick)) {
                return;
            }
            this.state.update(cx, |state, cx| {
                state.hover_pool_tile(Some((media, tick)), cx)
            });
        }))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                return;
            }
            let ours = matches!(this.state.read(cx).pool_hover, Some((held, _)) if held == media);
            if !ours {
                return;
            }
            this.state
                .update(cx, |state, cx| state.hover_pool_tile(None, cx));
        }))
        .into_any_element()
}
