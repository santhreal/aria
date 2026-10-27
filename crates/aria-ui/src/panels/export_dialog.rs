//! Export dialog.
//!
//! Every setting is one row: its label in a fixed column, then either a chip
//! for each choice, the one in force lit, or, where there is nothing to
//! choose, the setting as a fact. A chip row and a fact stand one chip tall,
//! so the rows keep one pitch.

use aria_core::count::counted;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::export_options::{
    AUDIO_CHOICES, ExportBitrate, ExportGop, ExportRate, ExportScale, WHOLE_TIMELINE,
};
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components::Wraps as _;
use crate::components::{self, ChipSize};
use crate::motion;
use crate::theme::Theme;

/// Width of the label column. The widest label, Destination, measures 58.5 px
/// at [`Theme::TEXT_UI`].
pub(crate) const LABEL_WIDTH: f32 = 72.0;

/// The chip every choice is drawn as.
const CHIP: ChipSize = ChipSize::Regular;

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let app = state.read(cx);
    // Where the button would write, from the one owner of that rule: the label
    // the sheet prints and the path the press sends cannot disagree.
    let dest_label = app.export_destination().display().to_string();
    let [canvas_w, canvas_h] = app.canvas_size();
    let encoder_label = app.export_encoder_label();
    let show_codec_bitrate = app.export_shows_codec_and_bitrate();
    let show_audio = app.export_shows_audio_toggle();
    let format_choices = app.export_format_choices();
    let options = app.export_options.clone();
    let (out_w, out_h) = app.export_size();
    let marked = app.marked_range().is_some();
    let marked_label = app.marked_range_label();
    let encoder = aria_encoder::capability();
    // The codecs this machine writes, once the probe started when the sheet
    // opened has answered; until then the chosen codec stands as a fact. The
    // choice in force is always one of the chips, so a codec the probe did not
    // find stays visible as the one that is asked for.
    let mut codecs: Vec<aria_core::VideoCodec> =
        aria_encoder::probed_codecs().map_or_else(Vec::new, <[_]>::to_vec);
    if !codecs.contains(&options.codec) {
        codecs.push(options.codec);
    }
    // The codec-free sink, on the sheet that already holds the scale, the rate,
    // and the range it renders with. A build whose platform ships no native
    // encoder can still get frames out this way, and the sequence answers to the
    // same choices above it as the video does — minus the audio track a folder
    // of stills cannot carry.
    let frames = components::ghost_button("export-frames", "Export Frames…", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.pick_and_export_image_sequence());
            cx.notify();
        }),
    );
    let save_as = components::ghost_button("export-dest", "Export As…", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.pick_export_destination());
            cx.notify();
        }),
    );
    let cancel = components::ghost_button("export-cancel", "Cancel", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    // The same press as Enter on this sheet: both run the state's own method, so
    // the pointer and the keyboard cannot queue two different exports.
    let go = components::button("export-go", "Export", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.export_to_destination());
            cx.notify();
        }),
    );
    // The rows are built before the spinner: the spinner's element borrows the
    // context for as long as it lives, so anything else that needs `cx` has to
    // be finished first.
    let mut rows: Vec<gpui::Div> = vec![
        fact(&theme, "Encoder", encoder_label),
        fact(&theme, "Canvas", format!("{canvas_w}×{canvas_h}")),
    ];
    rows.push(if format_choices.len() > 1 {
        line(
            &theme,
            "Format",
            chip_line(
                &theme,
                "export-format",
                format_choices
                    .into_iter()
                    .map(|(id, label)| (id, SharedString::from(label))),
                &options.plugin_encoder,
                AppState::select_export_plugin_encoder,
                cx,
            ),
        )
    } else {
        let (_, label) = format_choices.into_iter().next().unwrap_or_default();
        fact(&theme, "Format", label)
    });
    rows.push(line(
        &theme,
        "Scale",
        chip_line(
            &theme,
            "export-scale",
            ExportScale::ALL.map(|scale| (scale, SharedString::new_static(scale.label()))),
            &options.scale,
            AppState::set_export_scale,
            cx,
        )
        // The size the scale gives, read beside the choice that sets it.
        .child(
            div()
                .ml_auto()
                .line_height(px(CHIP.height()))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(format!("{out_w}×{out_h}")),
        ),
    ));
    rows.push(line(
        &theme,
        "Frame rate",
        chip_line(
            &theme,
            "export-rate",
            ExportRate::ALL.map(|rate| (rate, SharedString::new_static(rate.label()))),
            &options.rate,
            AppState::set_export_rate,
            cx,
        ),
    ));
    if show_codec_bitrate {
        rows.push(if codecs.len() > 1 {
            line(
                &theme,
                "Codec",
                chip_line(
                    &theme,
                    "export-codec",
                    codecs
                        .into_iter()
                        .map(|codec| (codec, SharedString::new_static(codec.label()))),
                    &options.codec,
                    AppState::set_export_codec,
                    cx,
                ),
            )
        } else {
            fact(&theme, "Codec", options.codec.label().to_string())
        });
        rows.push(if encoder.bitrate {
            line(
                &theme,
                "Bitrate",
                chip_line(
                    &theme,
                    "export-bitrate",
                    ExportBitrate::ALL.map(|bitrate| (bitrate, bitrate.label().into())),
                    &options.bitrate,
                    AppState::set_export_bitrate,
                    cx,
                ),
            )
        } else {
            fact(&theme, "Bitrate", format!("Set by {}", encoder.backend))
        });
    }
    rows.push(line(
        &theme,
        "Keyframes",
        chip_line(
            &theme,
            "export-gop",
            ExportGop::ALL.map(|gop| (gop, gop.label().into())),
            &options.gop,
            AppState::set_export_gop,
            cx,
        ),
    ));
    if show_audio {
        rows.push(line(
            &theme,
            "Audio",
            chip_line(
                &theme,
                "export-audio",
                AUDIO_CHOICES.map(|(audio, label)| (audio, SharedString::new_static(label))),
                &options.audio,
                AppState::set_export_audio,
                cx,
            ),
        ));
    }
    // The marked choice names the span it renders; with nothing marked, its
    // press says how to mark one.
    rows.push(line(
        &theme,
        "Range",
        chip_line(
            &theme,
            "export-range",
            [
                (false, SharedString::new_static(WHOLE_TIMELINE)),
                (true, marked_label.into()),
            ],
            &(options.marked_range && marked),
            AppState::set_export_marked_range,
            cx,
        ),
    ));
    rows.push(fact(&theme, "Destination", dest_label));
    // What is already rendering. Pressing Export twice is otherwise invisible
    // from this sheet until the Jobs board is opened, and the loader is what
    // separates a render that is still working from one that wedged.
    // The handle: the board is projected when a job reports, not when the sheet
    // draws, so counting what is in flight costs a filter over shared rows.
    let board = state.read(cx).job_view.clone();
    let running: Vec<&crate::app::jobs::UiJob> = board.iter().filter(|job| !job.terminal).collect();
    let mut in_flight: Vec<gpui::AnyElement> = Vec::new();
    if !running.is_empty() {
        // The same row every other wait draws. It was the big gradient spinner
        // with a line beside it, which is a third loader shape for a wait the
        // rows underneath already describe.
        in_flight.push(
            crate::loaders::pending_row(
                "export-inflight",
                SharedString::from(format!(
                    "{} in flight",
                    counted(running.len(), "render", "renders")
                )),
                &theme,
                cx.entity_id(),
                cx,
            )
            .into_any_element(),
        );
    }
    for job in &running {
        // The percentage stays in the label: a bar says roughly how far, and a
        // render long enough to need one is also long enough to need the
        // number.
        let line: SharedString = match job.progress {
            Some(fraction) => format!("{} · {:.0}%", job.label, fraction * 100.0).into(),
            None => job.label.clone(),
        };
        in_flight.push(
            crate::loaders::progress_row(
                motion::MotionKey::new("export-job", job.id),
                line,
                job.progress,
                &theme,
                cx.entity_id(),
                cx,
            )
            .into_any_element(),
        );
        // A render is the longest thing this sheet starts, and the sheet is
        // where a change of mind happens. The same
        // control as the board's, from the same owner.
        in_flight.extend(crate::pending::stop_button("export", job, &theme, cx));
    }
    let rows = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .children(rows);
    let body = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(rows)
        .children(in_flight);
    let body = DialogBody::new(body)
        .action(cancel)
        .action(frames)
        .action(save_as)
        .action(go);
    dialog_surface("export", DialogKind::Export, "Export", state, cx, body)
}

/// One row: `label` in the label column, on one line, then `value`.
fn line(theme: &Theme, label: &'static str, value: impl IntoElement) -> gpui::Div {
    div()
        .debug_selector(move || format!("export-line-{label}"))
        .flex()
        .flex_row()
        .items_start()
        .gap(px(Theme::SPACE_MD))
        .child(
            div()
                .flex_none()
                .w(px(LABEL_WIDTH))
                .line_height(px(CHIP.height()))
                .whitespace_nowrap()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(label),
        )
        .child(value)
}

/// A setting with nothing to choose here, as one line of text a chip tall, so
/// its text stands on the chips' line. A value wider than the card gives up
/// its start, so a destination keeps its file name in view and an encoder its
/// codec.
fn fact(theme: &Theme, label: &'static str, value: String) -> gpui::Div {
    line(
        theme,
        label,
        div()
            .debug_selector(move || format!("export-row-{label}"))
            .flex_1()
            .min_w_0()
            .line_height(px(CHIP.height()))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis_start()
            .text_size(Theme::TEXT_UI)
            .text_color(theme.text)
            .child(value),
    )
}

/// A chip for each of `choices`, the one equal to `current` lit; a press sets
/// that choice through `set`.
///
/// The row steps out by a chip's side padding, so the first chip's label
/// stands on the value column's text edge, where a fact's text starts.
fn chip_line<T: Clone + PartialEq + 'static>(
    theme: &Theme,
    id: &'static str,
    choices: impl IntoIterator<Item = (T, SharedString)>,
    current: &T,
    set: fn(&mut AppState, T),
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::Div {
    let chips = choices
        .into_iter()
        .enumerate()
        .map(|(index, (choice, label))| {
            let active = choice == *current;
            let selector = label.clone();
            components::chip((id, index), label, active, CHIP, theme)
                .debug_selector(move || format!("{id}-{selector}"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        let choice = choice.clone();
                        this.state.update(cx, |state, _| set(state, choice));
                        cx.notify();
                    }),
                )
        })
        .collect::<Vec<_>>();
    div()
        .flex_1()
        .min_w_0()
        .ml(px(-CHIP.pad().0))
        .wraps()
        .gap(px(Theme::SPACE_XS))
        .children(chips)
}

#[cfg(test)]
#[path = "export_dialog_tests.rs"]
mod tests;
