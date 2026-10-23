//! Export dialog.

use aria_core::count::counted;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components;
use crate::motion;
use crate::theme::Theme;

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    // Where the button would write, from the one owner of that rule: the label
    // the sheet prints and the path the press sends cannot disagree.
    let dest = state.read(cx).export_destination();
    let dest_label = dest.display().to_string();
    let [canvas_w, canvas_h] = state.read(cx).canvas_size();
    let resolution = format!("{canvas_w} × {canvas_h}");
    let format_label = state.read(cx).export_format_label();
    let encoder_label = state.read(cx).export_encoder_label();
    let show_codec_bitrate = state.read(cx).export_shows_codec_and_bitrate();
    let show_audio = state.read(cx).export_shows_audio_toggle();
    let choose = components::ghost_button("export-dest", "Choose…", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.pick_export_destination());
            cx.notify();
        }),
    );
    // The codec-free sink, on the sheet that already holds the scale, the rate,
    // and the range it renders with. A build whose platform ships no native
    // encoder can still get frames out this way, and the sequence answers to the
    // same choices above it as the video does — minus the audio track a folder
    // of stills cannot carry.
    let frames = components::ghost_button("export-frames", "Frames", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.pick_and_export_image_sequence());
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
    let options = state.read(cx).export_options.clone();
    let (out_w, out_h) = state.read(cx).export_size();
    let range_label = state.read(cx).export_range_label();
    // The rows are built before the spinner: the spinner's element borrows the
    // context for as long as it lives, so anything else that needs `cx` has to
    // be finished first.
    let mut picks: Vec<gpui::AnyElement> = vec![
        pick_row(
            &theme,
            "export-format",
            "Format",
            format_label,
            cx,
            |state| state.cycle_export_format(),
        )
        .into_any_element(),
        pick_row(
            &theme,
            "export-scale",
            "Scale",
            format!("{}  ·  {out_w} × {out_h}", options.scale.label()),
            cx,
            |state| state.cycle_export_scale(),
        )
        .into_any_element(),
        pick_row(
            &theme,
            "export-rate",
            "Frame rate",
            options.rate.label().to_string(),
            cx,
            |state| state.cycle_export_rate(),
        )
        .into_any_element(),
    ];
    if show_codec_bitrate {
        picks.push(
            pick_row(
                &theme,
                "export-codec",
                "Codec",
                options.codec.label().to_string(),
                cx,
                |state| state.cycle_export_codec(),
            )
            .into_any_element(),
        );
        picks.push(
            pick_row(
                &theme,
                "export-bitrate",
                "Bitrate",
                options.bitrate.label(),
                cx,
                |state| state.cycle_export_bitrate(),
            )
            .into_any_element(),
        );
    }
    picks.push(
        pick_row(
            &theme,
            "export-gop",
            "Keyframes",
            options.gop.label(),
            cx,
            |state| state.cycle_export_gop(),
        )
        .into_any_element(),
    );
    if show_audio {
        picks.push(
            pick_row(
                &theme,
                "export-audio",
                "Audio",
                if options.audio {
                    "Included".to_string()
                } else {
                    "Video only".to_string()
                },
                cx,
                |state| state.toggle_export_audio(),
            )
            .into_any_element(),
        );
    }
    picks.push(
        pick_row(&theme, "export-range", "Range", range_label, cx, |state| {
            state.toggle_export_range()
        })
        .into_any_element(),
    );
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
    // Every row is one box, read-only or clickable, so the labels share one
    // left edge and the rows one pitch. The column steps out by the rows'
    // inset so the labels align with the sheet's title.
    let rows = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .mx(px(-Theme::SPACE_XS))
        .child(row(&theme, "Encoder", encoder_label))
        .child(row(&theme, "Canvas", resolution))
        .children(picks)
        .child(row(&theme, "Destination", dest_label));
    let body = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(rows)
        .children(in_flight);
    let body = DialogBody::new(body)
        .action(cancel)
        .action(choose)
        .action(frames)
        .action(go);
    dialog_surface("export", DialogKind::Export, "Export", state, cx, body)
}

/// A row whose value is a choice: clicking it steps to the next one.
fn pick_row(
    theme: &Theme,
    id: impl Into<motion::ControlId>,
    label: &'static str,
    value: String,
    cx: &mut gpui::Context<crate::shell::Shell>,
    step: fn(&mut AppState),
) -> impl IntoElement {
    let id = id.into();
    let fade = id.motion("export-pick");
    row_box(theme, label, value)
        .id(id.element("export-pick"))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        // The element id is already unique per row, so it doubles as the hover
        // key: a shared key would fade every row with whichever one is under
        // the pointer.
        .bg(motion::hover_blend(fade, theme.wash(0.0), theme.wash(0.06)))
        .on_hover(motion::hover_listener(fade))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| step(state));
                cx.notify();
            }),
        )
}

/// A label and its value.
fn row(theme: &Theme, label: &'static str, value: impl Into<String>) -> impl IntoElement {
    row_box(theme, label, value.into())
}

/// The box every export row shares: one row height, the label at the left,
/// the value at the right.
fn row_box(theme: &Theme, label: &'static str, value: String) -> gpui::Div {
    div()
        .debug_selector(move || format!("export-line-{label}"))
        .flex()
        .flex_row()
        .justify_between()
        .items_center()
        .gap(px(Theme::SPACE_MD))
        .h(px(Theme::ROW_HEIGHT))
        .px(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .child(label_cell(theme, label))
        .child(value_cell(theme, label, value))
}

fn label_cell(theme: &Theme, label: &'static str) -> impl IntoElement {
    div()
        .flex_none()
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_muted)
        .child(label)
}

/// A row's value, on one line. A value wider than the card gives up its start,
/// so a destination keeps its file name in view and an encoder its codec.
fn value_cell(theme: &Theme, label: &'static str, value: String) -> impl IntoElement {
    div()
        .debug_selector(move || format!("export-row-{label}"))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_start()
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text)
        .child(value)
}

#[cfg(test)]
#[path = "export_dialog_tests.rs"]
mod tests;
