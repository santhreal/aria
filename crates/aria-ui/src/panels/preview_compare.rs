//! The comparison drawn over, or beside, the Program frame. What is compared
//! and where the divider sits are set in [`crate::app::viewer_compare`].

use std::sync::Arc;

use gpui::{AnyElement, DispatchPhase, MouseMoveEvent, MouseUpEvent, RenderImage, SharedString};

use super::*;
use crate::app::viewer_compare::{CompareLayout, Reference};

/// The divider's grab strip, in pixels across.
const GRAB_PX: f32 = 8.0;

/// The reference picture to draw now: the held frame or still, or the frame
/// the Source monitor shows.
fn picture(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<Arc<RenderImage>> {
    let (media, tick) = {
        let read = state.read(cx);
        match read.viewer_compare.reference()? {
            Reference::Source => read.monitor_source().map(|view| (view.media, view.tick))?,
            Reference::Frame { .. } | Reference::Still { .. } => {
                return read.viewer_compare.picture().cloned();
            }
        }
    };
    pool_preview::request_source(state, cx, media, tick);
    let pool = &state.read(cx).pool_preview;
    pool.source(media, tick)
        .or_else(|| pool.source_stale(media))
        .cloned()
}

/// The reference's caption: the frame's timecode, the still's file name, or
/// the source's name.
fn caption(state: &AppState) -> Option<SharedString> {
    Some(match state.viewer_compare.reference()? {
        Reference::Frame { at } => {
            timecode::format_ticks(at.value, at.rate.num, at.rate.den).into()
        }
        Reference::Still { path } => path
            .file_name()
            .map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
            .into(),
        Reference::Source => state.monitor_source()?.name.clone(),
    })
}

/// The reference drawn as the Program frame is: the same fit, zoom, and pan
/// inside a box `width` by `height` times the stage, so a box cut down to one
/// side of the divider still lines up with the frame beside it.
fn reference_area(
    state: &Entity<AppState>,
    theme: &Theme,
    width: f32,
    height: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::Div {
    let view = state.read(cx).preview_view;
    let scale = view.scale();
    let (dx, dy) = view.pan;
    let label = caption(state.read(cx));
    let area = div().relative().overflow_hidden().bg(theme.bg);
    let area = match picture(state, cx) {
        Some(picture) => area.child(
            img(picture)
                .object_fit(ObjectFit::Contain)
                .absolute()
                .left(px(dx))
                .top(px(dy))
                .w(relative(scale / width))
                .h(relative(scale / height)),
        ),
        None => area
            .flex()
            .items_center()
            .justify_center()
            .child(crate::pending::working(
                theme,
                "preview.compare",
                "Rendering the reference…",
                cx,
            )),
    };
    area.when_some(label, |el, label| {
        el.child(
            div()
                .absolute()
                .top(px(Theme::SPACE_SM))
                .left(px(Theme::SPACE_SM))
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XXS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_overlay)
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(label),
        )
    })
}

/// Records where the stage is painted, and follows a held divider anywhere in
/// the window until the button comes up.
fn stage_probe(state: &Entity<AppState>) -> impl IntoElement {
    let measure = state.clone();
    let follow = state.clone();
    canvas(
        move |bounds, _, cx| {
            let stage = [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ];
            measure.update(cx, |state, _| state.viewer_compare.record_stage(stage));
        },
        move |_, _, window, _| {
            let moved = follow.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let at = [f32::from(event.position.x), f32::from(event.position.y)];
                if moved.update(cx, |state, _| state.drag_compare_divider(at)) {
                    window.refresh();
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Left
                    && follow.update(cx, |state, _| state.release_compare_divider())
                {
                    window.refresh();
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// The divider: a 1 px accent line in a strip a press grabs.
fn divider(
    layout: CompareLayout,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::Stateful<gpui::Div> {
    let horizontal = layout.horizontal();
    let line = div().bg(theme.accent);
    let line = if horizontal {
        line.h(px(1.)).w_full()
    } else {
        line.w(px(1.)).h_full()
    };
    let strip = div()
        .id("compare-divider")
        .debug_selector(|| "compare-divider".into())
        .flex()
        .items_center()
        .justify_center()
        .flex_none();
    let strip = if horizontal {
        strip
            .w_full()
            .h(px(GRAB_PX))
            .cursor(gpui::CursorStyle::ResizeUpDown)
    } else {
        strip
            .h_full()
            .w(px(GRAB_PX))
            .cursor(gpui::CursorStyle::ResizeLeftRight)
    };
    strip.child(line).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            // The press is the divider's: the frame under it neither pans nor
            // picks a layer.
            cx.stop_propagation();
            this.state
                .update(cx, |state, _| state.grab_compare_divider());
            cx.notify();
        }),
    )
}

/// A wipe over the Program frame: the reference on the near side of the
/// divider, Program past it. `None` unless a wipe is drawn.
pub(super) fn wipe(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<AnyElement> {
    let compare = &state.read(cx).viewer_compare;
    let layout = compare.layout();
    if !compare.comparing() || layout == CompareLayout::SideBySide {
        return None;
    }
    let split = compare.divider();
    let horizontal = layout.horizontal();
    let mut stage = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(stage_probe(state));
    if split > 0.0 {
        let (width, height) = if horizontal {
            (1.0, split)
        } else {
            (split, 1.0)
        };
        let area = reference_area(state, theme, width, height, cx)
            .id("compare-reference")
            .debug_selector(|| "compare-reference".into())
            .absolute()
            .top_0()
            .left_0()
            .w(relative(width))
            .h(relative(height));
        stage = stage.child(area);
    }
    let strip = divider(layout, theme, cx).absolute();
    let strip = if horizontal {
        strip.left_0().top(relative(split)).mt(px(-GRAB_PX / 2.))
    } else {
        strip.top_0().left(relative(split)).ml(px(-GRAB_PX / 2.))
    };
    Some(stage.child(strip).into_any_element())
}

/// The Program pane, beside the reference's own pane while Side by Side
/// compares, and alone otherwise.
pub(super) fn beside(
    pane: gpui::Stateful<gpui::Div>,
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let compare = &state.read(cx).viewer_compare;
    if !compare.comparing() || compare.layout() != CompareLayout::SideBySide {
        return pane.into_any_element();
    }
    let split = compare.divider();
    div()
        .id("compare-stage")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .relative()
        .child(stage_probe(state))
        .child(
            reference_area(state, theme, 1.0, 1.0, cx)
                .id("compare-reference")
                .debug_selector(|| "compare-reference".into())
                .flex_none()
                .h_full()
                .w(relative(split)),
        )
        .child(divider(CompareLayout::SideBySide, theme, cx))
        .child(pane)
        .into_any_element()
}
