//! Track headers: the name, the toggle row, and the reorder grab.
//!
//! The toggles are drawn from [`crate::surfaces::LANE_TOGGLES`] rather than
//! hand-built one by one. A control whose action lives inside a drawn closure is
//! invisible to the suite: nothing can enumerate it, so nothing can check that
//! the command it claims to send is a command something sends. A registry row
//! can be driven, and `app::drawn_census` drives every one of these, which is
//! what puts the lane header inside the forward reach census instead of beside
//! it.

use crate::fluent::FluentBuilder;
use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use aria_models::TrackId;

use crate::app::AppState;
use crate::motion;
use crate::surfaces::Glyph;
use crate::surfaces::lane_header::{LANE_TOGGLES, LaneFlags, LaneToggle};
use crate::theme::Theme;
use crate::types::{ContextTarget, UiTrack};

use super::glide;

/// A lane's header: its name, its four toggles, and the grab surface that reorders
/// the stack.
///
/// A drag is committed by the header the pointer is released over, so no geometry
/// has to be measured: the drop target is the row that saw the release.
pub(super) fn track_header(
    theme: &Theme,
    track: &UiTrack,
    badge: String,
    row: usize,
    y_top: f32,
    width: f32,
    height: f32,
    target_h: f32,
    dragging: bool,
    drop_row: Option<usize>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let track_id = track.id;
    let name = track.name.clone();
    let collapsed = track.collapsed;
    let is_drop = drop_row == Some(row);
    let badge_color = match track.kind {
        crate::types::UiTrackKind::Video => theme.track_video,
        crate::types::UiTrackKind::Audio | crate::types::UiTrackKind::Submix => theme.track_audio,
        crate::types::UiTrackKind::Text => theme.track_text,
        crate::types::UiTrackKind::Generator | crate::types::UiTrackKind::Adjustment => {
            theme.track_generator
        }
    };
    // Built before the element: each toggle takes the entity context to install
    // its listener, and a `children` closure cannot borrow it a second time.
    let flags = LaneFlags::of(track);
    let mut toggles = Vec::with_capacity(LANE_TOGGLES.len());
    for toggle in LANE_TOGGLES {
        if !(toggle.shown)(flags) {
            continue;
        }
        toggles.push(lane_toggle(toggle, track, theme, cx).into_any_element());
    }
    let debug_badge = badge.clone();
    div()
        .id(gpui::ElementId::named_usize(
            "track-header",
            track_id as usize,
        ))
        .debug_selector(move || format!("track-header-{debug_badge}"))
        .absolute()
        .top(px(y_top))
        .left_0()
        .w(px(width))
        .h(px(height))
        .px(px(Theme::SPACE_XS))
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .border_r_1()
        .border_b_1()
        .border_color(theme.hairline)
        .bg(theme.surface_header)
        .cursor(if dragging {
            CursorStyle::ClosedHand
        } else {
            CursorStyle::Arrow
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    state.begin_track_drag(TrackId::from_raw(track_id));
                });
            }),
        )
        .on_mouse_move(cx.listener(move |this, _: &gpui::MouseMoveEvent, _, cx| {
            let changed = this
                .state
                .update(cx, |state, _| state.hover_track_drop(row));
            if changed {
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseUpEvent, _, cx| {
                let dragging = this.state.read(cx).track_dragging();
                if !dragging {
                    return;
                }
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.drop_track_on_row(row) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    state.open_context(
                        ContextTarget::Track { track: track_id },
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    );
                });
                cx.notify();
            }),
        )
        .on_scroll_wheel(
            cx.listener(move |this, event: &gpui::ScrollWheelEvent, _, cx| {
                let ctrl_or_alt =
                    event.modifiers.control || event.modifiers.platform || event.modifiers.alt;
                if ctrl_or_alt || !event.modifiers.shift {
                    return;
                }
                cx.stop_propagation();
                let delta = event.delta.pixel_delta(px(24.0));
                let dx = f32::from(delta.x);
                let dy = f32::from(delta.y);
                let motion = if dy.abs() >= dx.abs() { dy } else { dx };
                if motion.abs() <= 0.01 {
                    return;
                }
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.step_lane_height_preset(track_id, motion > 0.0) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .child(
            div()
                .id(gpui::ElementId::named_usize(
                    "track-badge",
                    track_id as usize,
                ))
                .px(px(3.))
                .py(px(1.))
                .rounded(px(2.))
                .bg(badge_color.opacity(0.22))
                .font_family(theme.font_mono.clone())
                .text_size(Theme::TEXT_SMALL)
                .text_color(theme.text)
                .child(badge),
        )
        .child(
            div()
                .id(gpui::ElementId::named_usize(
                    "track-name",
                    track_id as usize,
                ))
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        if event.click_count >= 2 {
                            cx.stop_propagation();
                            this.state.update(cx, |state, _| {
                                state.begin_rename(crate::types::RenameTarget::Track(track_id));
                            });
                            cx.notify();
                        }
                    }),
                )
                .child(name),
        )
        .children(toggles)
        .when(!collapsed, |el| {
            let grip = motion::MotionKey::new("lane-grip", track_id);
            el.child(
                div()
                    .id(("lane-grip", track_id))
                    .absolute()
                    .bottom(px(-2.))
                    .left_0()
                    .right_0()
                    .h(px(5.))
                    .cursor(CursorStyle::ResizeUpDown)
                    .bg(motion::hover_blend(
                        grip,
                        theme.border_strong.opacity(0.0),
                        theme.border_strong,
                    ))
                    .on_hover(motion::hover_listener(grip))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.start_lane_resize(track_id, f32::from(event.position.y), target_h);
                            cx.notify();
                        }),
                    ),
            )
        })
        .when(is_drop, |el| {
            el.child(motion::fade_quick(
                motion::MotionKey::new("track-drop", track_id),
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(2.))
                    .bg(theme.accent),
            ))
        })
}

pub(super) fn track_toggle(
    id: impl Into<motion::ControlId>,
    content: impl IntoElement,
    active: bool,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    on_click: impl Fn(&mut AppState) + 'static,
) -> impl IntoElement {
    let id = id.into();
    // The lit state eases in, and the pointer's own fade rides on top of it, so a
    // toggle reads as one control changing rather than two colours swapping.
    let on = glide::glide(
        id.motion("toggle"),
        if active { 1.0 } else { 0.0 },
        glide::EDIT,
        cx.reduce_motion(),
    );
    let key = id.motion("toggle-hover");
    div()
        .id(id.element("toggle"))
        .w(px(Theme::SPACE_LG))
        .h(px(Theme::SPACE_LG))
        .rounded(px(Theme::SPACE_XS))
        .flex()
        .items_center()
        .justify_center()
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.16 * on),
            theme.wash(0.16 * on + 0.10),
        ))
        .text_color(motion::mix(theme.text_faint, theme.text, on))
        .on_hover(motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, _| on_click(state));
                cx.notify();
            }),
        )
        .child(content)
}

/// Draw one registered toggle for `lane`.
fn lane_toggle(
    row: &'static LaneToggle,
    lane: &UiTrack,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let flags = LaneFlags::of(lane);
    let lit = (row.lit)(flags);
    let id = motion::MotionKey::new(row.id, lane.id);
    let content = match row.glyph {
        Glyph::Icons { on, off } => {
            crate::icons::icon(if lit { on } else { off }).into_any_element()
        }
        Glyph::Text { on, off } => div()
            .text_size(Theme::TEXT_SMALL)
            .child(if lit { on } else { off })
            .into_any_element(),
        Glyph::Chevron => crate::components::chevron(
            motion::MotionKey::new("lane-chev", lane.id),
            lit,
            theme.text_muted,
            cx.reduce_motion(),
        )
        .into_any_element(),
        Glyph::Channel => div()
            .text_size(Theme::TEXT_SMALL)
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(match flags.channel {
                Some(aria_models::SourceChannel::Video) => "V",
                Some(aria_models::SourceChannel::Audio) => "A",
                None => "",
            })
            .into_any_element(),
    };
    track_toggle(id, content, lit, theme, cx, move |state| {
        if let Err(err) = (row.run)(state, flags) {
            state.report_error(&err);
        }
    })
}
