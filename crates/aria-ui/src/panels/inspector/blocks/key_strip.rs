//! The time area right of a property's value in Effect Controls.
//!
//! A strip paints the selected clip's range as a lane, the playhead across it,
//! and the property's keyframes at their places in it, in the icons the
//! timeline's property rows paint. What a press there does, and the drag that
//! follows, is `crate::app::key_strip`: the press is held on the state, and the
//! shell routes the pointer until the button comes up.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Bounds, CursorStyle, HitboxBehavior, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Styled, Window, canvas, div, fill, point, px, size,
};

use super::widgets::LABEL_YIELD;
use crate::app::key_strip::KeyStrip;
use crate::app::spawn_preview;
use crate::motion::MotionKey;
use crate::panels::keyframe_icon::{KeyInk, paint_keyframe_icon};
use crate::theme::Theme;

/// Width of a strip, in pixels, where its row has the room.
pub const STRIP_W: f32 = 112.0;

/// The narrowest a strip is drawn. A frame too narrow for every control on a
/// row narrows the strip first; below this width the strip is left blank and
/// takes no press, and the row's label gives up none of its reserve until the
/// strip has no width left.
pub const STRIP_MIN: f32 = 32.0;

/// A strip's flex shrink factor, a thousand times the row label's
/// `LABEL_YIELD`. Shrink is shared in proportion to factor times width, so
/// while a strip narrows to nothing its label loses under a tenth of a pixel,
/// and the label ends in an ellipsis only once the strip has no width left.
const STRIP_YIELD: f32 = 1000.0 * LABEL_YIELD;

/// Height of a strip: a keyframe icon and two pixels above and below it.
const STRIP_H: f32 = 12.0;

/// Distance from either end at which a keyframe icon is held, so a key on the
/// clip's first or last frame is painted whole.
const ICON_INSET: f32 = 4.0;

/// One property's strip. `key` is the row's motion key, so each row's strip is
/// an element of its own.
pub fn key_strip(
    theme: &Theme,
    key: MotionKey,
    strip: KeyStrip,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let param = strip.param;
    // Where the strip painted, in window pixels, for the press to read the
    // pointer against: its left edge and its width.
    let frame = Rc::new(Cell::new([0.0_f32; 2]));
    let pressed = Rc::clone(&frame);
    let ink = Ink {
        key: KeyInk::of(theme),
        card: theme.surface_card,
        lane: theme.border,
        playhead: theme.playhead,
    };
    let painted = canvas(
        move |bounds: Bounds<Pixels>, window, _| {
            let width = f32::from(bounds.size.width);
            frame.set([f32::from(bounds.origin.x), width]);
            drawn(width).then(|| window.insert_hitbox(bounds, HitboxBehavior::Normal))
        },
        move |bounds: Bounds<Pixels>, hitbox, window, _| {
            if let Some(hitbox) = hitbox {
                window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
                paint_strip(bounds, &strip, ink, window);
            }
        },
    )
    .absolute()
    .size_full();

    blank_strip()
        .id(key.element_id("insp-strip"))
        .debug_selector(move || format!("insp-strip-{param:?}"))
        .child(painted)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let [left, width] = pressed.get();
                // A blank strip leaves the press to the row under it.
                if !drawn(width) {
                    return;
                }
                // The row underneath takes a press as the start of a scrub of
                // its value; a press here is about time.
                cx.stop_propagation();
                let x = f32::from(event.position.x);
                let additive = event.modifiers.shift;
                let taken = this.state.update(cx, |state, _| {
                    state.press_key_strip(param, x, left, width, additive)
                });
                if taken {
                    spawn_preview(this.state.clone(), cx);
                    cx.notify();
                }
            }),
        )
}

/// A strip's box with nothing in it: `STRIP_W` wide where its row has the
/// room, narrowing at `STRIP_YIELD` before the label beside it. A row that
/// cannot be keyed among rows that can draws it bare, so its label and value
/// narrow with theirs.
pub fn blank_strip() -> gpui::Div {
    div()
        .relative()
        .flex_grow_0()
        .flex_shrink(STRIP_YIELD)
        .w(px(STRIP_W))
        .min_w_0()
        .h(px(STRIP_H))
}

/// Whether a strip `width` pixels wide is drawn and takes a press.
fn drawn(width: f32) -> bool {
    width >= STRIP_MIN
}

/// The colours a strip paints in, read off the theme before the paint.
#[derive(Clone, Copy)]
struct Ink {
    key: KeyInk,
    card: Hsla,
    lane: Hsla,
    playhead: Hsla,
}

/// The card under the strip, the lane across its middle, the playhead where
/// it sits on the clip, and every keyframe over them.
fn paint_strip(bounds: Bounds<Pixels>, strip: &KeyStrip, ink: Ink, window: &mut Window) {
    let left = f32::from(bounds.origin.x);
    let top = f32::from(bounds.origin.y);
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let middle = top + height * 0.5;
    window.paint_quad(fill(bounds, ink.card).corner_radii(px(Theme::CONTROL_RADIUS)));
    window.paint_quad(fill(
        Bounds {
            origin: point(px(left), px(middle - 0.5)),
            size: size(px(width), px(1.0)),
        },
        ink.lane,
    ));
    if let Some(at) = strip.playhead_fraction() {
        window.paint_quad(fill(
            Bounds {
                origin: point(px(left + at * width - 0.5), px(top)),
                size: size(px(1.0), px(height)),
            },
            ink.playhead,
        ));
    }
    let (first, last) = (
        left + ICON_INSET,
        (left + width - ICON_INSET).max(left + ICON_INSET),
    );
    let mut painted = 0usize;
    for mark in &strip.marks {
        let x = (left + strip.fraction(mark.tick) * width).clamp(first, last);
        paint_keyframe_icon(
            window,
            &mut painted,
            ink.key,
            x,
            middle,
            mark.shape,
            mark.selected,
        );
    }
}
