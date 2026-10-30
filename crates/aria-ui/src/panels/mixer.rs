//! The audio mixer: one strip per lane that holds sound and per bus, in the
//! order the mix folds them. A picture lane whose clips hold no sound in the
//! picture draws no strip ([`AppState::soundless_picture_lanes`]).
//!
//! Everything a lane has in the mix is on its strip and nowhere else: its
//! insert chain, the fader, where the lane goes, and which buses get a copy of
//! it. There is no dialog and no dropdown — a send is a chip that cycles off →
//! post → pre, and a level is a drag on the fader whose level it is.
//!
//! The strips are drawn left to right in feed order, which the model hands over
//! ([`aria_models::Timeline::audio_buses`]), so a bus is always to the right of
//! everything that feeds it and the picture reads the way the mixdown runs. The
//! master strip, last, holds the programme mix's inserts. A lane that cannot
//! reach the master is bordered and says so: it is in the project, and it is
//! not in the mix.
//!
//! Every strip's head, the rows above its pan, stands as tall as the tallest
//! head, and the feet under them fill the frame. Every fader so starts and
//! ends level with its neighbours' and runs as tall as the frame allows.
//!
//! Motion is the shared vocabulary. The panel fades in on the pane switch and
//! every chip's background is `motion::hover_blend` on its own key, so the fade
//! is per chip. A fader's fill follows the pointer with no easing: on a drag an
//! animation is lag, not polish.

use std::cell::Cell;
use std::rc::Rc;

use crate::components::Wraps as _;
use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, canvas, div, px, relative,
};

use crate::app::AppState;
use crate::app::meter::meter_fraction;
use crate::app::mixer::{
    FaderTarget, MixerStrip, gain_label, gain_to_frac, pan_label, pan_to_frac,
};
use crate::motion;
use crate::panels::insert_chain::{self, Chain};
use crate::panels::inspector::blocks::widgets::Columns;
use crate::surfaces::mixer_strip::{self, Gesture};
use crate::theme::Theme;
use aria_models::{AudioInsertTarget, AutomationMode, TrackId};
use gpui::prelude::FluentBuilder;

/// The shortest a fader runs. A fader runs the height its frame leaves under
/// the tallest strip head; a frame shorter than that scrolls.
const FADER_MIN_HEIGHT: f32 = 96.0;
const FADER_WIDTH: f32 = 28.0;
const PAN_WIDTH: f32 = 96.0;
const PAN_HEIGHT: f32 = 10.0;
/// A strip's width. Inside its border and padding it holds the insert chain's
/// outlined "+ Audio effect" button whole.
const STRIP_WIDTH: f32 = 140.0;

/// Whether the mixer is the pane in front.
pub fn visible(state: &AppState) -> bool {
    state.active_panel == crate::types::ActivePanel::Mixer
}

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let soundless = state.read(cx).soundless_picture_lanes();
    let body = match state.read(cx).mixer_strips() {
        // A cyclic routing graph is the one thing this panel cannot draw, and
        // the model names the lane in the sentence. Shown as the refusal it is
        // rather than as an empty mixer.
        Err(refusal) => {
            let refusal = SharedString::from(refusal.to_string());
            div()
                .p(px(Theme::SPACE_MD))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.danger)
                .child(refusal)
                .into_any_element()
        }
        Ok(strips) if strips.iter().all(|strip| soundless.contains(&strip.track)) => {
            crate::components::empty_state(
                "mixer",
                "No audio lanes yet. Add one, or add a submix bus, to mix.",
                &theme,
            )
            .into_any_element()
        }
        Ok(strips) => {
            let mut drawn: Vec<Strip> = strips
                .iter()
                .filter(|strip| !soundless.contains(&strip.track))
                .map(|strip| channel(state, &theme, strip, cx))
                .collect();
            drawn.push(master(state, &theme, cx));
            let (heads, feet): (Vec<_>, Vec<_>) = drawn
                .into_iter()
                .map(|strip| (strip.head, strip.foot))
                .unzip();
            // The strips scroll sideways, as a console does; the panel fit test
            // reads this region as one that scrolls. Strips taller than the
            // frame scroll down in the region around them.
            //
            // Each strip is drawn in two rows: the heads, which their row
            // stretches to the tallest, then the feet, which fill the height
            // the frame has left. Every fader so starts and ends level with
            // its neighbours', however many rows a head holds.
            crate::edge_fade::scroll_y(
                div()
                    .debug_selector(|| "mixer-strips-down".into())
                    .id("mixer-strips-down")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(crate::edge_fade::scroll_x(
                        div()
                            .id("mixer-strips")
                            .debug_selector(|| "mixer-strips".into())
                            .flex()
                            .flex_col()
                            .flex_grow_1()
                            .flex_shrink_0()
                            .items_start()
                            .p(px(Theme::SPACE_MD))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(Theme::SPACE_SM))
                                    .children(heads),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_grow_1()
                                    .gap(px(Theme::SPACE_SM))
                                    .children(feet),
                            ),
                    )),
            )
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
            .child(header(
                &theme,
                state.read(cx).dock_panel_title(crate::dock::PANEL_MIXER),
                cx,
            ))
            .child(body),
    )
    .into_any_element()
}

/// The panel's own row: its title, and the one thing it can add.
fn header(
    theme: &Theme,
    title: &'static str,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = motion::MotionKey::new("mixer", "add-bus");
    div()
        .flex()
        .flex_row()
        .wraps()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_MD))
        .py(px(Theme::SPACE_SM))
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(title),
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
                .text_size(Theme::TEXT_TITLE)
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

/// One strip as the mixer lays it out: the head, every row above the pan, and
/// the foot, the pan over the fader and its level.
struct Strip {
    head: gpui::AnyElement,
    foot: gpui::AnyElement,
}

/// One half of a strip's card. The head rounds and borders the top, the foot
/// the bottom, and nothing parts the two, so they read as one card.
fn half(
    id: impl Into<motion::ControlId>,
    head: bool,
    bg: gpui::Hsla,
    border: gpui::Hsla,
) -> gpui::Stateful<gpui::Div> {
    let kind = if head {
        "mixer-strip"
    } else {
        "mixer-strip-foot"
    };
    let half = div()
        .id(id.into().element(kind))
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap(px(Theme::SPACE_XS))
        .w(px(STRIP_WIDTH))
        .px(px(Theme::SPACE_SM))
        .bg(bg)
        .border_x_1()
        .border_color(border);
    if head {
        half.pt(px(Theme::SPACE_SM))
            .rounded_t(px(Theme::CONTROL_RADIUS))
            .border_t_1()
    } else {
        // The foot's top padding is the gap between the head's last row and
        // the pan, as between any two rows of a strip.
        half.pt(px(Theme::SPACE_XS))
            .pb(px(Theme::SPACE_SM))
            .rounded_b(px(Theme::CONTROL_RADIUS))
            .border_b_1()
    }
}

/// One channel strip: name, inserts, routing and sends in the head; pan,
/// fader and level in the foot.
fn channel(
    state: &Entity<AppState>,
    theme: &Theme,
    strip: &MixerStrip,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Strip {
    let track = TrackId::from_raw(strip.track);
    let raw_id = strip.track;
    let name = if strip.is_bus {
        format!("{} · bus", strip.name)
    } else {
        strip.name.to_string()
    };
    let bg = if strip.is_bus {
        theme.band
    } else {
        theme.surface_raised
    };
    let border = if strip.reaches_master {
        theme.border
    } else {
        theme.danger
    };
    let head = half(
        motion::MotionKey::new("mixer-strip", raw_id),
        true,
        bg,
        border,
    )
    .debug_selector(move || format!("mixer-head-{raw_id}"))
    .child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(Theme::SPACE_XXS))
            .child(
                div()
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(if strip.audible {
                        theme.text
                    } else {
                        theme.text_muted
                    })
                    .child(SharedString::from(name)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(2.))
                    .child(mute_button(
                        theme,
                        Some(track),
                        strip.track,
                        strip.muted,
                        cx,
                    ))
                    .child(solo_button(theme, track, strip.track, strip.solo, cx)),
            ),
    )
    .when(!strip.reaches_master, |el| {
        el.child(
            div()
                .text_size(Theme::TEXT_SMALL)
                .text_color(theme.danger)
                .child("Not in the mix"),
        )
    })
    .child(automation_row(
        theme,
        Some(track),
        strip.track,
        strip.automation,
        cx,
    ))
    .child(label_row(theme, "Inserts"))
    .child(insert_chain::render(
        theme,
        Chain {
            target: AudioInsertTarget::Track(track),
            inserts: &strip.inserts,
            keys: &strip.keys,
            expanded: false,
            columns: Columns::Free,
        },
        state,
        cx,
    ))
    .child(outputs(theme, strip, track, cx))
    .child(sends(state, theme, strip, track, cx))
    .into_any_element();
    let foot = half(
        motion::MotionKey::new("mixer-strip", raw_id),
        false,
        bg,
        border,
    )
    .debug_selector(move || format!("mixer-foot-{raw_id}"))
    .child(pan_control(
        state,
        theme,
        FaderTarget::LanePan(track),
        raw_id,
        strip.pan,
        strip.audible,
        cx,
    ))
    .child(fader(state, theme, strip, track, cx))
    .into_any_element();
    Strip { head, foot }
}

/// The master strip: the programme mix's insert chain, which runs after every
/// lane and bus has been summed.
fn master(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Strip {
    let chain = state.read(cx).master_chain();
    let head = half("mixer-master", true, theme.band, theme.border)
        .debug_selector(|| "mixer-head-0".into())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(Theme::SPACE_XXS))
                .child(
                    div()
                        .text_size(Theme::TEXT_TITLE)
                        .text_color(if chain.muted {
                            theme.text_muted
                        } else {
                            theme.text
                        })
                        .child("Master"),
                )
                .child(mute_button(theme, None, 0, chain.muted, cx)),
        )
        .child(automation_row(theme, None, 0, chain.automation, cx))
        .child(label_row(theme, "Inserts"))
        .child(insert_chain::render(
            theme,
            Chain {
                target: AudioInsertTarget::Master,
                inserts: &chain.inserts,
                keys: &chain.keys,
                expanded: false,
                columns: Columns::Free,
            },
            state,
            cx,
        ))
        .into_any_element();
    let foot = half("mixer-master-foot", false, theme.band, theme.border)
        .debug_selector(|| "mixer-foot-0".into())
        .child(pan_control(
            state,
            theme,
            FaderTarget::MasterPan,
            0,
            chain.pan,
            !chain.muted,
            cx,
        ))
        .child(master_fader(state, theme, &chain, cx))
        .into_any_element();
    Strip { head, foot }
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
                .wraps()
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

/// The strip's mute chip. `track` is `None` on the master strip.
fn mute_button(
    theme: &Theme,
    track: Option<TrackId>,
    raw_id: u64,
    muted: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    chip(
        theme,
        motion::MotionKey::new("mixer-mute", raw_id).into(),
        "M".to_string(),
        muted,
        true,
        move |state| {
            if let Err(err) = mixer_strip::apply(state, Gesture::ToggleMute { track }) {
                state.report_error(&err);
            }
        },
        cx,
    )
}

fn solo_button(
    theme: &Theme,
    track: TrackId,
    raw_id: u64,
    solo: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    chip(
        theme,
        motion::MotionKey::new("mixer-solo", raw_id).into(),
        "S".to_string(),
        solo,
        false,
        move |state| {
            if let Err(err) = mixer_strip::apply(state, Gesture::ToggleSolo { track }) {
                state.report_error(&err);
            }
        },
        cx,
    )
}

fn automation_row(
    theme: &Theme,
    track: Option<TrackId>,
    raw_id: u64,
    current: AutomationMode,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let chips: Vec<gpui::AnyElement> = AutomationMode::ALL
        .iter()
        .copied()
        .map(|mode| {
            let text = mode.label().to_string();
            chip(
                theme,
                motion::MotionKey::new("mixer-auto", raw_id)
                    .with(mode.label())
                    .into(),
                text,
                mode == current,
                mode.writes_envelope() && mode == current,
                move |state| {
                    if let Err(err) =
                        mixer_strip::apply(state, Gesture::SetAutomation { track, mode })
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
        .child(label_row(theme, "Auto"))
        .child(div().flex().flex_row().wraps().gap(px(2.)).children(chips))
        .into_any_element()
}

fn pan_control(
    state: &Entity<AppState>,
    theme: &Theme,
    target: FaderTarget,
    raw_id: u64,
    stored_pan: f32,
    audible: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let pan = state.read(cx).fader_value(target, stored_pan);
    let frac = pan_to_frac(pan);
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .flex()
                .justify_between()
                .child(label_row(theme, "Pan"))
                .child(
                    div()
                        .text_size(Theme::TEXT_SMALL)
                        .text_color(theme.text_muted)
                        .child(SharedString::from(pan_label(pan))),
                ),
        )
        .child(drag_track(
            theme,
            ("mixer-pan", raw_id),
            target,
            frac,
            audible,
            true,
            cx,
        ))
        .into_any_element()
}

fn fader(
    state: &Entity<AppState>,
    theme: &Theme,
    strip: &MixerStrip,
    track: TrackId,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let target = FaderTarget::Lane(track);
    let gain = state.read(cx).fader_value(target, strip.gain);
    let level = state.read(cx).strip_meter(track);
    fader_and_meter(theme, strip.track, target, gain, strip.audible, level, cx)
}

fn master_fader(
    state: &Entity<AppState>,
    theme: &Theme,
    chain: &crate::app::mixer::MasterChain,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let target = FaderTarget::Master;
    let gain = state.read(cx).fader_value(target, chain.gain);
    let level = state.read(cx).meter().peaks;
    fader_and_meter(theme, 0, target, gain, !chain.muted, level, cx)
}

fn fader_and_meter(
    theme: &Theme,
    raw_id: u64,
    target: FaderTarget,
    gain: f32,
    audible: bool,
    level: [f32; 2],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let frac = gain_to_frac(gain);
    div()
        .flex()
        .flex_col()
        .flex_1()
        .gap(px(Theme::SPACE_XXS))
        .items_center()
        .child(
            div()
                .flex()
                .flex_1()
                .min_h(px(FADER_MIN_HEIGHT))
                .gap(px(3.))
                .child(drag_track(
                    theme,
                    ("mixer-fader", raw_id),
                    target,
                    frac,
                    audible,
                    false,
                    cx,
                ))
                .child(meter(theme, raw_id, audible, level)),
        )
        .child(
            div()
                .debug_selector(move || format!("mixer-gain-{raw_id}"))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(SharedString::from(gain_label(gain))),
        )
        .into_any_element()
}

/// A strip's two peak bars on the meter scale the Audio Meters panel draws:
/// the foot of a bar is -60 dBFS and its head 0 dBFS. A bar at full scale
/// draws red.
fn meter(theme: &Theme, raw_id: u64, audible: bool, level: [f32; 2]) -> gpui::AnyElement {
    let bar = |peak: f32, side: &'static str| {
        div()
            .id(motion::MotionKey::new("mixer-meter", side)
                .with(raw_id)
                .element_id("mixer-meter"))
            .debug_selector(move || format!("mixer-meter-{raw_id}-{side}"))
            .relative()
            .w(px(4.))
            .rounded(px(1.))
            .bg(theme.wash(0.12))
            .child(
                div()
                    .debug_selector(move || format!("mixer-meter-{raw_id}-{side}-fill"))
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .h(relative(meter_fraction(peak)))
                    .rounded(px(1.))
                    .bg(if peak >= 1.0 {
                        theme.danger
                    } else if audible {
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

/// A fader or a pan track. `(name, raw_id)` names it, for its motion key and
/// its debug selector.
///
/// A press rides the track as drawn: a fader runs the height its strip leaves
/// it, which only layout settles, so the track's length is read where it is
/// painted and a press takes the length last painted as the travel. A drag
/// across the whole track so moves the value across its whole range.
///
/// A press grabs the value, and the shell (`Shell::carry_gesture`) rides it
/// wherever the pointer goes and writes one command at the release. The
/// travel is relative to the grab, so the value never jumps to wherever the
/// pointer happened to land.
fn drag_track(
    theme: &Theme,
    (name, raw_id): (&'static str, u64),
    target: FaderTarget,
    frac: f32,
    audible: bool,
    horizontal: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let (fill, cursor) = if horizontal {
        (
            div().absolute().top_0().left_0().h_full().w(relative(frac)),
            CursorStyle::ResizeLeftRight,
        )
    } else {
        (
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .w_full()
                .h(relative(frac)),
            CursorStyle::ResizeUpDown,
        )
    };
    let length = Rc::new(Cell::new(0.0_f32));
    let painted = Rc::clone(&length);
    div()
        .id(motion::MotionKey::new(name, raw_id).element_id("mixer-drag"))
        .debug_selector(move || format!("{name}-{raw_id}"))
        .relative()
        .when(horizontal, |track| track.w(px(PAN_WIDTH)).h(px(PAN_HEIGHT)))
        .when(!horizontal, |track| track.w(px(FADER_WIDTH)))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.wash(0.12))
        .cursor(cursor)
        .child(fill.rounded(px(Theme::CONTROL_RADIUS)).bg(if audible {
            theme.accent
        } else {
            theme.text_muted
        }))
        .child(
            canvas(
                move |bounds, _, _| {
                    painted.set(f32::from(if horizontal {
                        bounds.size.width
                    } else {
                        bounds.size.height
                    }));
                },
                |_, (), _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let at = if horizontal {
                    f32::from(event.position.x)
                } else {
                    f32::from(event.position.y)
                };
                let travel = length.get();
                this.state.update(cx, |state, _| {
                    if let Err(err) =
                        mixer_strip::apply(state, Gesture::GrabFader { target, at, travel })
                    {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
}

fn label_row(theme: &Theme, text: &'static str) -> gpui::AnyElement {
    div()
        .text_size(Theme::TEXT_SMALL)
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
        .text_size(Theme::TEXT_UI)
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

#[cfg(test)]
#[path = "mixer_tests.rs"]
mod tests;
