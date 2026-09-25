//! Clip cards on the timeline.
//!
//! The card, its edge handles, the gestures those handles commit and the
//! waveform canvas drawn inside an audio card.

use crate::fluent::FluentBuilder;
use crate::motion::{self, glide};
use aria_models::{ClipId, TrackId};
use gpui::{
    AnimationExt, CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, StatefulInteractiveElement, Styled, div, px,
};
use std::cell::Cell;
use std::rc::Rc;
// Geometry belongs to the panel rather than the card: one conversion serves the
// ruler, the lanes and the clips.
use crate::panels::timeline::{ticks_to_px, ticks_to_px_f};
use crate::snap::threshold_ticks;
use crate::theme::{self, Theme};
use crate::types::{CatalogDrag, ContextTarget, UiClip, UiClipKind};
const TRIM: f32 = 8.0;
const EXPANDED_H: f32 = 56.0;
/// Height of the waveform strip inside an audio clip card.
const WAVE_H: f32 = 22.0;
/// Height of the filmstrip along the bottom of a media clip card. Matches
/// `filmstrips::CELL_H`, which is the height each cell was rendered at.
const FILM_H: f32 = crate::filmstrips::CELL_H as f32;
/// Side of a fade handle square, drawn in the card's top corner.
const FADE_GRIP: f32 = 9.0;
#[derive(Clone, Copy)]
enum DragKind {
    Move,
    TrimStart,
    TrimEnd,
    FadeIn,
    FadeOut,
}
#[derive(Clone, Copy)]
struct DragOrigin {
    kind: DragKind,
    x: f32,
}
/// The colour a clip is drawn in.
///
/// A label colour is the operator's own grouping, so it outranks the tint the
/// clip's kind would give it. One owner, because a collapsed lane's summary bar
/// has to agree with the card it replaces.
pub(crate) fn tint(theme: &Theme, clip: &UiClip) -> gpui::Hsla {
    match clip.label {
        Some(label) => theme.marker(label),
        None => match clip.kind {
            UiClipKind::Media => theme.track_video,
            UiClipKind::Audio => theme.track_audio,
            UiClipKind::Text => theme.track_text,
            UiClipKind::Shape => theme.track_shape,
            UiClipKind::Generator => theme.track_generator,
        },
    }
}
pub fn render(
    theme: &Theme,
    clip: &UiClip,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    expanded: bool,
    reduced: bool,
    // Whether this card is the clip an edit has just made.
    settling: bool,
    bars: Vec<f32>,
    cells: Vec<(f32, std::sync::Arc<gpui::RenderImage>)>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    // Built before the builder chain: a closure in the chain cannot borrow `cx`.
    let strip: Vec<gpui::AnyElement> = cells
        .into_iter()
        .map(|(x, image)| {
            div()
                .absolute()
                .left(px(x))
                .top_0()
                .h_full()
                .w(px(60.))
                .child(gpui::img(image).h_full())
                .into_any_element()
        })
        .collect();
    let start_ticks = clip.timeline_start_ticks;
    let end_ticks = clip.timeline_end_ticks;
    // Position and length travel to their new values. A ripple, a roll or a
    // move is one command over many clips, and this is what makes the cards it
    // displaced slide rather than reappear somewhere else. Ticks rather than
    // pixels, so scrolling and zooming stay exact.
    let start_at = glide::glide(
        motion::MotionKey::new("clip-start", clip.id),
        start_ticks as f32,
        glide::EDIT,
        reduced,
    );
    let span_at = glide::glide(
        motion::MotionKey::new("clip-span", clip.id),
        (end_ticks - start_ticks).max(1) as f32,
        glide::EDIT,
        reduced,
    );
    let start = ticks_to_px_f(start_at, fps, pps) - scroll_x;
    let width = ticks_to_px_f(span_at, fps, pps).max(8.0);
    let selected = clip.selected;
    let id = clip.id;
    let track_id = clip.track_id;
    let name = clip.name.clone();
    let locked = clip.locked;
    let fade_in_ticks = clip.fade_in_ticks.max(0);
    let fade_out_ticks = clip.fade_out_ticks.max(0);
    let duration = clip.duration_label.clone();
    let slot_badge = clip.replaceable.as_ref().map(|slot| match &slot.label {
        Some(label) => format!("slot {} · {label}", slot.order),
        None => format!("slot {}", slot.order),
    });
    // The strip is drawn inside the card, so diamonds are placed relative to the
    // clip's own left edge and never need the scroll offset.
    let key_marks: Vec<_> = clip
        .keyframe_ticks
        .iter()
        .map(|tick| {
            div()
                .absolute()
                .left(px(ticks_to_px(*tick, fps, pps)))
                .top(px(1.))
                .w(px(6.))
                .h(px(6.))
                .rounded(px(1.))
                .bg(theme.accent)
                .into_any_element()
        })
        .collect();
    let tint = tint(theme, clip);
    let drag = Rc::new(Cell::new(None::<DragOrigin>));
    let drag_down = drag.clone();
    let drag_up = drag.clone();
    let drop_accent = theme.accent;
    let card_key = motion::MotionKey::new("clip-card", id);
    // Selection and lock are chased values, not one-shots. Both are states the
    // next click can take away, and a one-shot cannot leave: gpui drops the
    // element on the frame the flag flips, so a ring played that way appears
    // over a beat and then vanishes in one frame. See `clip::state`.
    let selected_t = state::selection_t(id, selected, reduced);
    let locked_t = state::lock_t(id, locked, reduced);
    let card = div()
        .id(gpui::ElementId::named_usize("clip", id as usize))
        .absolute()
        .left(px(start))
        .top(px(6.))
        .when(expanded, |el| el.h(px(EXPANDED_H)).bottom(px(2.)))
        .when(!expanded, |el| el.bottom(px(6.)))
        .w(px(width))
        .rounded(px(Theme::CONTROL_RADIUS))
        .border_l_2()
        .border_color(tint)
        .overflow_hidden()
        .cursor(if locked {
            CursorStyle::OperationNotAllowed
        } else {
            CursorStyle::OpenHand
        })
        // The inset hairline fades with the ring below it: two parts of one
        // response to one click, so they cannot arrive at different times.
        .when(state::showing(selected_t), |el| {
            el.shadow(state::selected_shadows(selected_t))
        })
        // The fill follows the pointer rather than switching under it.
        .bg(motion::hover_blend(
            card_key,
            theme.surface_card,
            theme.surface_raised_hover,
        ))
        .on_hover(motion::hover_listener(card_key))
        // Becoming selected is worth watching: the ring arrives over the card
        // instead of appearing around it, and — because it is a chased value
        // rather than a one-shot — it leaves the same way.
        .when(state::showing(selected_t), |el| {
            el.child(state::ring(theme, selected_t))
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let tool = this.state.read(cx).tool;
                if tool.pans() {
                    this.start_pan(f32::from(event.position.x), cx);
                    cx.notify();
                    return;
                }
                if tool.zooms() {
                    let factor = if event.modifiers.alt {
                        1.0 / 1.15
                    } else {
                        1.15
                    };
                    this.zoom_at(f32::from(event.position.x), factor, cx);
                    cx.notify();
                    return;
                }
                if !locked {
                    drag_down.set(Some(DragOrigin {
                        kind: DragKind::Move,
                        x: f32::from(event.position.x),
                    }));
                }
                let clip_id = ClipId::from_raw(id);
                if event.click_count >= 2 {
                    this.state.update(cx, |state, _| {
                        state.toggle_expanded(id);
                    });
                    cx.notify();
                    return;
                }
                if this.state.read(cx).tool.cuts() {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = state.split_at_playhead() {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                    return;
                }
                this.state.update(cx, |state, _| {
                    if event.modifiers.shift {
                        state.add_clip_to_selection(clip_id);
                    } else if event.modifiers.control || event.modifiers.platform {
                        state.toggle_clip_in_selection(clip_id);
                    } else {
                        state.select_clip_linked(clip_id, !event.modifiers.alt);
                    }
                });
                cx.notify();
            }),
        )
        // A drag only commits on mouse-up, so the guide is computed here: this is
        // the only moment the magnet's decision can be shown before it is taken.
        .on_mouse_move({
            let drag = drag.clone();
            cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let Some(origin) = drag.get() else {
                    return;
                };
                let delta_px = f32::from(event.position.x) - origin.x;
                let delta_ticks = ((delta_px / pps.max(1.0)) * fps.max(1.0)).round() as i64;
                let desired = match origin.kind {
                    DragKind::Move | DragKind::TrimStart => (start_ticks + delta_ticks).max(0),
                    DragKind::TrimEnd => (end_ticks + delta_ticks).max(0),
                    DragKind::FadeIn | DragKind::FadeOut => return,
                };
                let (_, guide) = snap_desired(this, desired, id, pps, fps, event.modifiers.alt, cx);
                let changed = this.state.read(cx).snap_guide != guide;
                if changed {
                    this.state.update(cx, |state, _| state.snap_guide = guide);
                    cx.notify();
                }
            })
        })
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                let Some(origin) = drag_up.take() else {
                    return;
                };
                if !matches!(origin.kind, DragKind::Move) {
                    return;
                }
                finish_body_drag(
                    this,
                    event,
                    origin.x,
                    id,
                    track_id,
                    start_ticks,
                    end_ticks,
                    pps,
                    fps,
                    cx,
                );
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    state.open_context(
                        ContextTarget::Clip {
                            clip: id,
                            track: track_id,
                        },
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    );
                });
                cx.notify();
            }),
        )
        // Animation, mask, and transition entries land on a clip. The card owns the
        // clip id, so the drop needs no pointer maths; `drop_catalog_on_clip` selects
        // the clip first, which is why the applied preset is then visible in the
        // inspector.
        .drag_over::<CatalogDrag>(move |style, _, _, _| style.border_color(drop_accent))
        .on_drop(cx.listener(move |this, drag: &CatalogDrag, _, cx| {
            let drag = *drag;
            this.state.update(cx, |state, _| {
                if let Err(err) = state.drop_catalog_on_clip(drag, ClipId::from_raw(id)) {
                    state.report_error(&err);
                }
            });
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }))
        // Double-clicking the NAME renames; double-clicking the card body still
        // expands the keyframe strip, so the two gestures do not fight.
        .child(
            div()
                .id(("clip-name", id))
                .px(px(Theme::SPACE_SM))
                .pt(px(Theme::SPACE_XS))
                .text_size(px(11.))
                .text_color(theme.text)
                .overflow_hidden()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        if event.click_count < 2 {
                            return;
                        }
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.begin_rename(crate::types::RenameTarget::Clip(id));
                        });
                        cx.notify();
                    }),
                )
                .child(name),
        )
        .child(
            div()
                .absolute()
                .right(px(Theme::SPACE_SM))
                .bottom(px(Theme::SPACE_XS))
                .text_size(px(11.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text_faint)
                .child(duration),
        )
        // A template slot reads as a badge in the corner, so an author can see
        // fill order without opening the inspector.
        .when_some(slot_badge, |el, badge| {
            el.child(
                div()
                    .absolute()
                    .left(px(Theme::SPACE_SM))
                    .bottom(px(Theme::SPACE_XS))
                    .px(px(3.))
                    .rounded(px(2.))
                    .bg(theme.accent.opacity(0.35))
                    .text_size(px(9.))
                    .text_color(theme.text)
                    .child(badge),
            )
        })
        // A locked clip says so. The cursor and the missing trim handles are
        // only discoverable by trying, and the badge takes the corner the fade
        // handles use — which is free, because a locked clip has none.
        .when(state::showing(locked_t), |el| {
            el.child(state::lock_badge(theme, locked_t))
        })
        // The wave is drawn behind the badges and above the card fill, so a
        // clip still reads its name and length while showing its audio.
        .when(!bars.is_empty(), |el| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(2.))
                    .h(px(WAVE_H))
                    .child(crate::waveforms::wave(bars, theme)),
            )
        })
        // The filmstrip sits along the bottom of the card, clipped by it, so a
        // trimmed clip shows only the frames it plays.
        .when(!strip.is_empty(), |el| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(FILM_H))
                    .overflow_hidden()
                    .children(strip),
            )
        })
        .when(expanded, |el| {
            el.child(
                div()
                    .absolute()
                    .left(px(Theme::SPACE_SM))
                    .right(px(6.))
                    .bottom(px(16.))
                    .h(px(10.))
                    .rounded(px(2.))
                    .bg(theme.wash(0.10))
                    .children(key_marks),
            )
        })
        .when(!locked, |el| {
            el.child(trim_handle(
                true,
                theme,
                id,
                start_ticks,
                end_ticks,
                pps,
                fps,
                drag.clone(),
                cx,
            ))
            .child(trim_handle(
                false,
                theme,
                id,
                start_ticks,
                end_ticks,
                pps,
                fps,
                drag.clone(),
                cx,
            ))
            .child(fade_handle(
                true,
                theme,
                id,
                fade_in_ticks,
                pps,
                fps,
                drag.clone(),
                cx,
            ))
            .child(fade_handle(
                false,
                theme,
                id,
                fade_out_ticks,
                pps,
                fps,
                drag,
                cx,
            ))
        });

    // A clip the operator has just made lands: it arrives a few pixels high and
    // thin, and settles onto the lane over the same beat an edit takes anywhere
    // else. This outranks the selection fade below — a dropped clip is selected
    // too, and two animations on one card is one animation and a loser.
    if settling && !reduced {
        return card
            .with_animation(
                motion::MotionKey::new("clip-land", id).element_id("clip-land"),
                glide::EDIT.animation(),
                |el, t| el.opacity(0.35 + 0.65 * t).top(px(6.0 - 5.0 * (1.0 - t))),
            )
            .into_any_element();
    }
    card.into_any_element()
}

// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
mod finish;
use finish::{fade_handle, finish_body_drag, snap_desired, trim_handle};
mod state;
