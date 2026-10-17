//! The Audio Meters panel: one vertical bar per output channel on a dBFS scale.
//!
//! Each bar draws the root mean square full width and the peak as a narrower
//! bar in front of it, so both stay readable where they differ. A one pixel
//! line marks the highest peak of the last [`PEAK_HOLD_SECONDS`]. The scale runs
//! from [`METER_FLOOR_DBFS`] at the bottom to 0 dBFS at the top. The clip light
//! above the bars stays lit after a block reaches full scale until it is
//! clicked.
//!
//! The panel reads [`AppState::meter`] and never advances it: the meter ticker
//! in `shell::strip` moves the ballistics and asks for the repaint.
//!
//! [`PEAK_HOLD_SECONDS`]: crate::app::meter::PEAK_HOLD_SECONDS

use gpui::{
    AnyElement, CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, px, relative,
};

use crate::app::AppState;
use crate::app::meter::{METER_FLOOR_DBFS, MeterReading, meter_fraction};
use crate::fluent::FluentBuilder;
use crate::shell::Shell;
use crate::theme::Theme;

/// Scale marks, in dBFS, with the text drawn beside each.
const MARKS: [(f32, &str); 8] = [
    (0.0, "0"),
    (-6.0, "-6"),
    (-12.0, "-12"),
    (-18.0, "-18"),
    (-24.0, "-24"),
    (-36.0, "-36"),
    (-48.0, "-48"),
    (METER_FLOOR_DBFS, "-60"),
];

/// The channel names under the bars, in reading order.
const CHANNELS: [&str; 2] = ["L", "R"];

const SCALE_WIDTH: f32 = 26.0;
const BAR_MAX_WIDTH: f32 = 22.0;
const MARK_TEXT: gpui::Pixels = Theme::TEXT_SMALL;

/// Where a scale mark sits on the bar, from the bottom.
fn mark_fraction(dbfs: f32) -> f32 {
    ((dbfs - METER_FLOOR_DBFS) / -METER_FLOOR_DBFS).clamp(0.0, 1.0)
}

pub fn render(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    let theme = Theme::of(cx);
    let reading = state.read(cx).meter();
    let bars = (0..CHANNELS.len()).map(|side| bar(&theme, &reading, side));
    let names = CHANNELS.map(|name| {
        div()
            .flex_1()
            .max_w(px(BAR_MAX_WIDTH))
            .text_center()
            .text_size(MARK_TEXT)
            .text_color(theme.text_muted)
            .child(name)
    });
    div()
        .id("audio-meters")
        .flex()
        .flex_col()
        .size_full()
        .min_h(px(80.))
        .gap(px(Theme::SPACE_XS))
        .p(px(Theme::SPACE_SM))
        .bg(theme.surface)
        .child(clip_light(&theme, reading.clipped, cx))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_1()
                .min_h(px(40.))
                .gap(px(Theme::SPACE_XS))
                .child(scale(&theme))
                .children(bars),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(Theme::SPACE_XS))
                .child(div().w(px(SCALE_WIDTH)))
                .children(names),
        )
        .into_any_element()
}

/// The clip light: lit from the first full-scale block until clicked.
fn clip_light(theme: &Theme, clipped: bool, cx: &mut gpui::Context<Shell>) -> impl IntoElement {
    div()
        .id("audio-meters-clip")
        .h(px(14.))
        .ml(px(SCALE_WIDTH + Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .flex()
        .items_center()
        .justify_center()
        .text_size(MARK_TEXT)
        .bg(if clipped {
            theme.danger
        } else {
            theme.wash(0.08)
        })
        .text_color(if clipped {
            theme.on_accent
        } else {
            theme.text_faint
        })
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.clear_clip());
                cx.notify();
            }),
        )
        .child("CLIP")
}

/// The dBFS marks down the left edge, each beside the level it names.
fn scale(theme: &Theme) -> impl IntoElement {
    div()
        .relative()
        .w(px(SCALE_WIDTH))
        .h_full()
        .children(MARKS.map(|(dbfs, text)| {
            div()
                .absolute()
                .right_0()
                .bottom(relative(mark_fraction(dbfs)))
                .mb(-MARK_TEXT / 2.0)
                .h(MARK_TEXT)
                .text_size(MARK_TEXT)
                .line_height(MARK_TEXT)
                .text_color(theme.text_faint)
                .child(text)
        }))
}

/// One channel: RMS full width behind, the peak in front, the hold line on top.
fn bar(theme: &Theme, reading: &MeterReading, side: usize) -> impl IntoElement {
    let rms = meter_fraction(reading.rms[side]);
    let peak = meter_fraction(reading.peaks[side]);
    let hold = meter_fraction(reading.hold[side]);
    let fill = |fraction: f32| div().absolute().bottom_0().h(relative(fraction));
    div()
        .id(("audio-meter-bar", side))
        .relative()
        .flex_1()
        .max_w(px(BAR_MAX_WIDTH))
        .h_full()
        .rounded(px(1.))
        .overflow_hidden()
        .bg(theme.wash(0.08))
        .children(MARKS.map(|(dbfs, _)| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(relative(mark_fraction(dbfs)))
                .h(px(1.))
                .bg(theme.wash(0.06))
        }))
        .child(fill(rms).left_0().right_0().bg(theme.waveform_rms))
        .child(
            fill(peak)
                .left(relative(0.25))
                .right(relative(0.25))
                .bg(theme.waveform_peak),
        )
        .when(hold > 0.0, |el| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(relative(hold))
                    .h(px(1.))
                    .bg(if hold >= 1.0 {
                        theme.danger
                    } else {
                        theme.text
                    }),
            )
        })
}
