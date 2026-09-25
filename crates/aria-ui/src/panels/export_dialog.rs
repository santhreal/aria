//! Export dialog.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::picker::{Ask, FileSpec};
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::dialog_surface;
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
    // The file picker needs only the suggested name: where the Export button
    // writes is read again when it is pressed, from the one owner of that rule.
    let suggested_name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export.mp4")
        .to_owned();
    let choose = components::ghost_button("export-dest", "Choose…", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                state.request_pick(
                    Ask::SaveFile(
                        FileSpec::new("Export video")
                            .filter("MPEG-4", &["mp4"])
                            .file_name(suggested_name.as_str()),
                    ),
                    |state, answer| {
                        let Some(path) = answer.one() else {
                            return;
                        };
                        match state.export_job(path) {
                            Ok(_) => state.open_dialog(DialogKind::Jobs),
                            Err(err) => state.report_error(err),
                        }
                    },
                );
            });
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
    let options = state.read(cx).export_options;
    let (out_w, out_h) = state.read(cx).export_size();
    let range_label = state.read(cx).export_range_label();
    // The rows are built before the spinner: the spinner's element borrows the
    // context for as long as it lives, so anything else that needs `cx` has to
    // be finished first.
    let picks: Vec<gpui::AnyElement> = vec![
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
        pick_row(
            &theme,
            "export-codec",
            "Codec",
            options.codec.label().to_string(),
            cx,
            |state| state.cycle_export_codec(),
        )
        .into_any_element(),
        pick_row(
            &theme,
            "export-bitrate",
            "Bitrate",
            options.bitrate.label(),
            cx,
            |state| state.cycle_export_bitrate(),
        )
        .into_any_element(),
        pick_row(
            &theme,
            "export-gop",
            "Keyframes",
            options.gop.label(),
            cx,
            |state| state.cycle_export_gop(),
        )
        .into_any_element(),
        pick_row(
            &theme,
            "export-audio",
            "Audio",
            if options.audio {
                "included".to_string()
            } else {
                "video only".to_string()
            },
            cx,
            |state| state.toggle_export_audio(),
        )
        .into_any_element(),
        pick_row(&theme, "export-range", "Range", range_label, cx, |state| {
            state.toggle_export_range()
        })
        .into_any_element(),
    ];
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
                SharedString::from(format!("{} render(s) in flight", running.len())),
                &theme,
                cx.entity_id(),
                cx,
            )
            .into_any_element(),
        );
    }
    for job in &running {
        // The percentage stays in the label: a bar says roughly how far, and a
        // render long enough to need one is also long enough that the operator
        // wants the number.
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
        // where the operator is standing when they change their mind. The same
        // control as the board's, from the same owner.
        in_flight.extend(crate::pending::stop_button("export", job, &theme, cx));
    }
    let body = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(row(
            &theme,
            "Format",
            crate::app::export_options::format_label(options.codec),
        ))
        .child(row(&theme, "Encoder", aria_encoder::capability().backend))
        .child(row(&theme, "Canvas", resolution))
        .children(picks)
        .child(row(&theme, "Destination", dest_label))
        .children(in_flight)
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap(px(Theme::SPACE_SM))
                .child(cancel)
                .child(choose)
                .child(frames)
                .child(go),
        );
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
    div()
        .id(id.element("export-pick"))
        .flex()
        .flex_row()
        .justify_between()
        .items_center()
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
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
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme.text_muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(13.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text)
                .child(value),
        )
}

fn row(theme: &Theme, label: &str, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme.text_muted)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(px(13.))
                .font_family(theme.font_mono.clone())
                .text_color(theme.text)
                .child(value.into()),
        )
}
