//! Track headers: the source patch, the badge and name, the toggle row, and
//! the reorder grab.
//!
//! The row is three groups, vertically centred on the name: the source patch;
//! the badge, the clip twirl and the name, which truncates; and the toggles.
//! Every control is one size, a 16 px glyph in a 20 px hit box, so a 20 px
//! collapsed lane holds the row. Each toggle states where it draws and whether
//! it shows at rest ([`Rest`]); one hidden at rest keeps its place, so nothing
//! moves when the pointer reaches the header, and shows while lit.
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
    AnyElement, CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use aria_models::TrackId;

use crate::app::AppState;
use crate::motion;
use crate::surfaces::Glyph;
use crate::surfaces::lane_header::{LANE_TOGGLES, LaneFlags, LaneToggle, Rest};
use crate::theme::Theme;
use crate::types::{ContextTarget, UiTrack};

use super::glide;

/// The group a header registers, so a toggle hidden at rest shows while the
/// pointer is anywhere over its own header.
const LANE_HEADER_GROUP: &str = "lane-header";

/// Hit box of every control in a lane header.
pub(super) const LANE_CONTROL: f32 = Theme::ICON_HIT_ROW;

/// A lane header's side padding, the gap between its parts, and its badge's
/// side padding.
const LANE_INSET: f32 = Theme::SPACE_XS;

/// Left edge of the first text a lane header draws, the badge's label:
/// the header's padding, the patch slot, the gap after it, and the badge's own
/// padding. A property row's content starts on this edge, so the outline
/// under a lane reads in the lane's text column.
pub(super) const LANE_TEXT_EDGE: f32 = LANE_INSET + LANE_CONTROL + LANE_INSET + LANE_INSET;

/// A lane's header: its source patch, its badge and name, its toggles, and the
/// grab surface that reorders the stack. `twirl` is the lane's first clip and
/// whether its property rows are open, for a lane that has a clip.
///
/// A drag is committed by the header the pointer is released over, so no geometry
/// has to be measured: the drop target is the row that saw the release.
#[allow(clippy::too_many_arguments)]
pub(super) fn track_header(
    theme: &Theme,
    track: &UiTrack,
    badge: String,
    row: usize,
    y_top: f32,
    width: f32,
    height: f32,
    target_h: f32,
    twirl: Option<(u64, bool)>,
    dragging: bool,
    drop_row: Option<usize>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let track_id = track.id;
    let name = track.name.clone();
    let collapsed = track.collapsed;
    let is_drop = drop_row == Some(row);
    let audio = matches!(
        track.kind,
        crate::types::UiTrackKind::Audio | crate::types::UiTrackKind::Submix
    );
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
    let mut lead = Vec::new();
    let mut toggles = Vec::with_capacity(LANE_TOGGLES.len());
    for toggle in LANE_TOGGLES {
        if !(toggle.shown)(flags) {
            continue;
        }
        let at_rest = toggle.shows_at_rest(flags, audio);
        let drawn = lane_toggle(toggle, track, theme, cx);
        match toggle.rest {
            Rest::Lead => lead.push(drawn),
            // A toggle hidden at rest keeps its place, so nothing moves when
            // the pointer enters the header.
            _ => toggles.push(
                div()
                    .flex_none()
                    .when(!at_rest, |el| {
                        el.opacity(0.0)
                            .group_hover(LANE_HEADER_GROUP, |style| style.opacity(1.0))
                    })
                    .child(drawn)
                    .into_any_element(),
            ),
        }
    }
    let twirl = match twirl.filter(|_| !collapsed) {
        Some((clip_id, open)) => {
            super::property_rows::track_outline_twirl_button(theme, track_id, clip_id, open, cx)
                .into_any_element()
        }
        None => slot().into_any_element(),
    };
    if lead.is_empty() {
        lead.push(slot().into_any_element());
    }
    let debug_badge = badge.clone();
    div()
        .id(gpui::ElementId::named_usize(
            "track-header",
            track_id as usize,
        ))
        .debug_selector(move || format!("track-header-{debug_badge}"))
        .group(LANE_HEADER_GROUP)
        .absolute()
        .top(px(y_top))
        .left_0()
        .w(px(width))
        .h(px(height))
        .px(px(LANE_INSET))
        .flex()
        .items_center()
        .gap(px(LANE_INSET))
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
        .child(div().flex_none().flex().items_center().children(lead))
        .child(
            div()
                .id(gpui::ElementId::named_usize(
                    "track-badge",
                    track_id as usize,
                ))
                .debug_selector(move || format!("track-badge-{track_id}"))
                .flex_none()
                .h(px(Theme::ICON_SIZE))
                .px(px(LANE_INSET))
                .flex()
                .items_center()
                .rounded(px(Theme::SPACE_XXS))
                .bg(badge_color.opacity(0.22))
                .text_size(Theme::TEXT_SMALL)
                .text_color(theme.text)
                .child(badge),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .child(twirl)
                .child(
                    div()
                        .id(gpui::ElementId::named_usize(
                            "track-name",
                            track_id as usize,
                        ))
                        .debug_selector(move || format!("track-name-{track_id}"))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(Theme::TEXT_TITLE)
                        .text_color(theme.text_muted)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                if event.click_count >= 2 {
                                    cx.stop_propagation();
                                    this.state.update(cx, |state, _| {
                                        state.begin_rename(crate::types::RenameTarget::Track(
                                            track_id,
                                        ));
                                    });
                                    cx.notify();
                                }
                            }),
                        )
                        .child(name),
                ),
        )
        .child(div().flex_none().flex().items_center().children(toggles))
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

/// An empty control slot: keeps the badge and the name on one edge across
/// lanes that draw no patch or have no clip to twirl.
fn slot() -> impl IntoElement {
    div().flex_none().size(px(LANE_CONTROL))
}

/// A lane header toggle: the glyph in the text colour at rest and in the
/// accent while lit, with no plate. The pointer's fade is a wash under the
/// glyph.
pub(super) fn track_toggle(
    id: impl Into<motion::ControlId>,
    content: impl IntoElement,
    active: bool,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    on_click: impl Fn(&mut AppState) + 'static,
) -> gpui::Stateful<gpui::Div> {
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
        .flex_none()
        .size(px(LANE_CONTROL))
        .rounded(px(Theme::SPACE_XS))
        .flex()
        .items_center()
        .justify_center()
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .text_color(motion::mix(theme.text_faint, theme.accent, on))
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
) -> AnyElement {
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
    let lane_id = lane.id;
    track_toggle(id, content, lit, theme, cx, move |state| {
        if let Err(err) = (row.run)(state, flags) {
            state.report_error(&err);
        }
    })
    .debug_selector(move || format!("lane-{}-{lane_id}", row.id))
    .into_any_element()
}

#[cfg(test)]
#[path = "headers_indent_tests.rs"]
mod indent_tests;
