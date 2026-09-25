//! The audio mixer: one strip per lane and per bus, in the order the mix folds
//! them.
//!
//! Everything a lane has in the mix is on its strip and nowhere else: the
//! fader, where the lane goes, and which buses get a copy of it. There is no
//! dialog and no dropdown — a send is a chip that cycles off → post → pre, and
//! a level is a drag on the fader whose level it is.
//!
//! The strips are drawn left to right in feed order, which the model hands over
//! ([`aria_models::Timeline::audio_buses`]), so a bus is always to the right of
//! everything that feeds it and the picture reads the way the mixdown runs. A
//! lane that cannot reach the master is bordered and says so: it is in the
//! project, and it is not in the mix.
//!
//! Motion is the shared vocabulary. The panel fades in on the pane switch and
//! every chip's background is `motion::hover_blend` on its own key, so the fade
//! is per chip. A fader's fill follows the pointer with no easing: on a drag an
//! animation is lag, not polish.

use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    div, px,
};

use crate::app::AppState;
use crate::app::mixer::{FaderTarget, MixerStrip, gain_label, gain_to_frac};
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::surfaces::mixer_strip::{self, Gesture};
use crate::theme::Theme;
use aria_models::TrackId;

/// How tall a fader's travel is, in pixels. Also the distance the pointer moves
/// to cross the whole range, since the drag is relative to where it was grabbed.
const FADER_HEIGHT: f32 = 132.0;
/// How wide one strip is. Wide enough for a bus name and a dB readout.
const STRIP_WIDTH: f32 = 124.0;

/// Whether the mixer is the pane in front.
pub fn visible(state: &AppState) -> bool {
    state.active_panel == crate::types::ActivePanel::Mixer
}

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let body = match state.read(cx).mixer_strips() {
        // A cyclic routing graph is the one thing this panel cannot draw, and
        // the model names the lane in the sentence. Shown as the refusal it is
        // rather than as an empty mixer.
        Err(refusal) => {
            let refusal = SharedString::from(refusal.to_string());
            div()
                .p(px(Theme::SPACE_MD))
                .text_size(px(13.))
                .text_color(theme.danger)
                .child(refusal)
                .into_any_element()
        }
        Ok(strips) if strips.is_empty() => div()
            .p(px(Theme::SPACE_MD))
            .text_size(px(13.))
            .text_color(theme.text_muted)
            .child("No audio lanes yet. Add one, or add a submix bus, to mix.")
            .into_any_element(),
        Ok(strips) => {
            let drawn: Vec<gpui::AnyElement> = strips
                .iter()
                .map(|strip| channel(state, &theme, strip, cx))
                .collect();
            div()
                .id("mixer-strips")
                .flex()
                .flex_row()
                .items_start()
                .gap(px(Theme::SPACE_SM))
                .p(px(Theme::SPACE_MD))
                .overflow_x_scroll()
                .children(drawn)
                .into_any_element()
        }
    };

    motion::fade_in(
        "mixer-panel",
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.surface)
            .child(header(&theme, cx))
            .child(body),
    )
    .into_any_element()
}

/// The panel's own row: what it is, and the one thing it can add.
fn header(theme: &Theme, cx: &mut gpui::Context<crate::shell::Shell>) -> gpui::AnyElement {
    let key = motion::MotionKey::new("mixer", "add-bus");
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(Theme::SPACE_MD))
        .py(px(Theme::SPACE_SM))
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme.text)
                .child("Mixer"),
        )
        .child(
            div()
                .id("mixer-add-bus")
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XXS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(motion::hover_blend(
                    key,
                    theme.input_bg,
                    theme.surface_raised_hover,
                ))
                .text_size(px(12.))
                .text_color(theme.text)
                .cursor(CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_hover(motion::hover_listener(key))
                .child("+ Submix bus")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = mixer_strip::apply(state, Gesture::AddBus) {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                ),
        )
        .into_any_element()
}

/// One channel strip: name, routing, sends, fader.
fn channel(
    state: &Entity<AppState>,
    theme: &Theme,
    strip: &MixerStrip,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let track = TrackId::from_raw(strip.track);
    let name = if strip.is_bus {
        format!("{} · bus", strip.name)
    } else {
        strip.name.to_string()
    };
    div()
        .id(motion::MotionKey::new("mixer-strip", strip.track).element_id("mixer-strip"))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .w(px(STRIP_WIDTH))
        .p(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(if strip.is_bus {
            theme.band
        } else {
            theme.surface_raised
        })
        .border_1()
        .border_color(if strip.reaches_master {
            theme.border
        } else {
            theme.danger
        })
        .child(
            div()
                .text_size(px(12.))
                .text_color(if strip.audible {
                    theme.text
                } else {
                    theme.text_muted
                })
                .child(SharedString::from(name)),
        )
        .when(!strip.reaches_master, |el| {
            el.child(
                div()
                    .text_size(px(10.))
                    .text_color(theme.danger)
                    .child("not in the mix"),
            )
        })
        .child(outputs(theme, strip, track, cx))
        .child(sends(state, theme, strip, track, cx))
        .child(fader(state, theme, strip, track, cx))
        .into_any_element()
}

/// Where this lane goes: the master, or a bus. One chip each, the live one lit.
fn outputs(
    theme: &Theme,
    strip: &MixerStrip,
    track: TrackId,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let chips: Vec<gpui::AnyElement> = strip
        .output_targets
        .iter()
        .map(|target| {
            let to = target.to.map(TrackId::from_raw);
            let label = target.name.to_string();
            let live = target.current;
            chip(
                theme,
                motion::MotionKey::new("mixer-out", strip.track)
                    .with(&label)
                    .into(),
                label,
                live,
                false,
                move |state| {
                    if let Err(err) = mixer_strip::apply(state, Gesture::PointOutput { track, to })
                    {
                        state.report_error(&err);
                    }
                },
                cx,
            )
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .child(label_row(theme, "Out"))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(Theme::SPACE_XXS))
                .children(chips),
        )
        .into_any_element()
}

/// Which buses get a copy of this lane. A chip per bus: off, post, or pre.
fn sends(
    state: &Entity<AppState>,
    theme: &Theme,
    strip: &MixerStrip,
    track: TrackId,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    if strip.send_targets.is_empty() {
        return div().into_any_element();
    }
    let chips: Vec<gpui::AnyElement> = strip
        .send_targets
        .iter()
        .map(|target| {
            let to = TrackId::from_raw(target.to);
            let level = target.index.and_then(|at| {
                strip
                    .sends
                    .iter()
                    .find(|send| send.index == at)
                    .map(|send| {
                        let value = state.read(cx).fader_value(
                            FaderTarget::Send {
                                track,
                                index: send.index,
                            },
                            send.level,
                        );
                        gain_label(value)
                    })
            });
            let text = match (&level, target.pre_fader) {
                (None, _) => target.name.to_string(),
                (Some(level), true) => format!("{} pre {level}", target.name),
                (Some(level), false) => format!("{} {level}", target.name),
            };
            chip(
                theme,
                motion::MotionKey::new("mixer-send", strip.track)
                    .with(target.to)
                    .into(),
                text,
                target.index.is_some(),
                target.pre_fader,
                move |state| {
                    if let Err(err) = mixer_strip::apply(state, Gesture::CycleSend { track, to }) {
                        state.report_error(&err);
                    }
                },
                cx,
            )
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .child(label_row(theme, "Sends"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XXS))
                .children(chips),
        )
        .into_any_element()
}

/// The lane's fader: a press grabs it, a move rides it, a release writes one
/// command. The travel is relative to the grab, so the value never jumps to
/// wherever the pointer happened to land.
fn fader(
    state: &Entity<AppState>,
    theme: &Theme,
    strip: &MixerStrip,
    track: TrackId,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let target = FaderTarget::Lane(track);
    let gain = state.read(cx).fader_value(target, strip.gain);
    let frac = gain_to_frac(gain);
    let level = state.read(cx).strip_meter(track);
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .items_center()
        .child(
            div()
                .flex()
                .gap(px(3.))
                .items_end()
                .child(fader_track(theme, strip, target, frac, cx))
                .child(meter(theme, strip, level)),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.text_muted)
                .child(SharedString::from(gain_label(gain))),
        )
        .into_any_element()
}

/// This lane's own level, post-fader: two bars beside the fader it belongs to.
///
/// Full scale is the top of the fader's travel, so a bar level with the fader's
/// unity mark is not a coincidence to be read off — the two are the same height.
/// Silence draws the trough alone, which is what a lane the mix is not carrying
/// reads.
fn meter(theme: &Theme, strip: &MixerStrip, level: [f32; 2]) -> gpui::AnyElement {
    let bar = |peak: f32, side: &'static str| {
        div()
            .id(motion::MotionKey::new("mixer-meter", side)
                .with(strip.track)
                .element_id("mixer-meter"))
            .relative()
            .w(px(4.))
            .h(px(FADER_HEIGHT))
            .rounded(px(1.))
            .bg(theme.wash(0.12))
            .child(
                div()
                    .absolute()
                    .bottom(px(0.))
                    .left(px(0.))
                    .w_full()
                    .h(px((FADER_HEIGHT * peak.clamp(0.0, 1.0)).round()))
                    .rounded(px(1.))
                    .bg(if peak >= 1.0 {
                        theme.danger
                    } else if strip.audible {
                        theme.success
                    } else {
                        theme.text_muted
                    }),
            )
    };
    div()
        .flex()
        .gap(px(1.))
        .child(bar(level[0], "l"))
        .child(bar(level[1], "r"))
        .into_any_element()
}

/// The fader itself: a press grabs it, a move rides it, a release writes one
/// command.
fn fader_track(
    theme: &Theme,
    strip: &MixerStrip,
    target: FaderTarget,
    frac: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    div()
        .id(motion::MotionKey::new("mixer-fader", strip.track).element_id("mixer-fader"))
        .relative()
        .w(px(28.))
        .h(px(FADER_HEIGHT))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.wash(0.12))
        .cursor(CursorStyle::ResizeUpDown)
        .child(
            div()
                .absolute()
                .bottom(px(0.))
                .left(px(0.))
                .w_full()
                .h(px((FADER_HEIGHT * frac).round()))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(if strip.audible {
                    theme.accent
                } else {
                    theme.text_muted
                }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let at = f32::from(event.position.y);
                this.state.update(cx, |state, _| {
                    if let Err(err) = mixer_strip::apply(
                        state,
                        Gesture::GrabFader {
                            target,
                            at,
                            travel: FADER_HEIGHT,
                        },
                    ) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            let at = f32::from(event.position.y);
            let moved = this.state.update(cx, |state, _| {
                match mixer_strip::apply(state, Gesture::RideFader { at }) {
                    Ok(moved) => moved,
                    Err(err) => {
                        state.report_error(&err);
                        false
                    }
                }
            });
            if moved {
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| {
                let wrote = this.state.update(cx, |state, _| {
                    match mixer_strip::apply(state, Gesture::ReleaseFader) {
                        Ok(wrote) => wrote,
                        Err(err) => {
                            state.report_error(&err);
                            false
                        }
                    }
                });
                if wrote {
                    cx.notify();
                }
            }),
        )
        .into_any_element()
}

fn label_row(theme: &Theme, text: &'static str) -> gpui::AnyElement {
    div()
        .text_size(px(10.))
        .text_color(theme.text_muted)
        .child(text)
        .into_any_element()
}

/// One clickable chip. `live` lights it; `alt` marks the second state a
/// three-state chip can be in, which is a pre-fader send.
fn chip(
    theme: &Theme,
    id: motion::ControlId,
    text: String,
    live: bool,
    alt: bool,
    click: impl Fn(&mut AppState) + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = id.motion("mixer.chip");
    let resting = match (live, alt) {
        (false, _) => theme.input_bg,
        (true, false) => theme.selection,
        (true, true) => theme.warning_muted,
    };
    div()
        .id(id.element("mixer.chip"))
        .px(px(Theme::SPACE_XS))
        .py(px(1.))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(
            key,
            resting,
            theme.surface_raised_hover,
        ))
        .text_size(px(11.))
        .text_color(if live { theme.text } else { theme.text_muted })
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .child(SharedString::from(text))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| click(state));
                cx.notify();
            }),
        )
        .into_any_element()
}
