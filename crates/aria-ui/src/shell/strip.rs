//! The status bar, the timeline splitter, and the session tickers.
//!
//! Split out of `shell.rs` to stay under the file ceiling. Each item is
//! `pub(super)` so the parent and its other children can reach it by name.

use super::*;
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use gpui::StatefulInteractiveElement as _;

pub(super) fn status_strip(
    theme: &Theme,
    state: &Entity<AppState>,
    status: Option<String>,
    err: Option<String>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    // Which timeline the edits are landing on. Built before the store is
    // borrowed below, because a chip carries a listener and a listener needs
    // `cx` mutably: the projection read has to be over by then.
    //
    // Nothing else in the editor says a nest is open, and an edit inside one
    // does not reach the root timeline, so without this the operator can trim
    // for a minute in a sequence they no longer remember opening.
    let breadcrumb = state.read(cx).open_sequence_name().map(str::to_string);
    let breadcrumb = breadcrumb.map(|name| {
        crate::components::chip(
            "status-nest-breadcrumb",
            &format!("Nest · {name}  ✕"),
            true,
            crate::components::ChipSize::Compact,
            theme,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.leave_sequence() {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
    });
    let job = live_job(theme, state, cx);
    // A dropped frame is news until it is acknowledged with a click; none
    // dropped is the normal state and draws nothing.
    let dropped = state.read(cx).dropped_frames().total();
    let drops = (dropped > 0).then(|| dropped_indicator(theme, dropped, cx));
    let read = state.read(cx);
    // During playback: the rate achieved against the sequence's, the
    // resolution it plays at, and the device drawing it. At rest the rate is
    // in the Viewer and the timeline and the resolution in the Viewer's
    // control row, so the strip's right side is empty.
    let report = read.playback_report();
    let playing = read.playing;
    let achieved = (playing && report.presented > 0).then(|| {
        let rate = read.store.sequence.frame_rate;
        let fps = if rate.den == 0 {
            0.0
        } else {
            rate.num as f64 / rate.den as f64
        };
        format!("{:.1}/{fps:.3} fps", report.achieved_fps)
    });
    let resolution = playing.then(|| format!("Playback {}", read.playback_resolution_text()));
    let gpu = read.preview_worker.device_name().filter(|_| playing);
    let has_err = err.is_some();
    let note = err.or(status).unwrap_or_default();
    // A status line that replaced another one has to be seen to have changed:
    // two refusals in a row can differ by their text alone. The element id is
    // the message's own identity, so a new message is a new element and fades
    // in, while a repaint holding the same message does not restart the fade.
    let note_id = motion::MotionKey::new("status", &note);
    div()
        .h(px(Theme::STATUS_STRIP_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .px(px(Theme::SPACE_MD))
        .gap(px(Theme::SPACE_MD))
        // The strip is washed for an error rather than only coloured: a
        // one-line message in a 22px strip is easy to miss.
        .bg(if has_err {
            theme.danger_muted
        } else {
            theme.surface
        })
        .border_t_1()
        .border_color(theme.border)
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_muted)
        // Left: where edits land, and the last thing that happened.
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_MD))
                .when_some(breadcrumb, |el, chip| el.child(chip))
                .child(
                    div().min_w_0().truncate().child(motion::fade_quick(
                        note_id,
                        div()
                            .text_color(if has_err {
                                theme.danger
                            } else {
                                theme.text_faint
                            })
                            .child(note),
                    )),
                ),
        )
        // Centre: the work in flight.
        .when_some(job, |el, job| el.child(job))
        // Right: how playback is keeping up, and on what.
        .child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(Theme::SPACE_MD))
                // The rate and the count change while playback runs.
                .when_some(achieved, |el, achieved| {
                    el.child(
                        div()
                            .font_features(crate::theme::tabular_figures())
                            .child(achieved),
                    )
                })
                .when_some(drops, |el, drops| el.child(drops))
                .when_some(resolution, |el, resolution| {
                    el.child(
                        div()
                            .id("status-playback-resolution")
                            .debug_selector(|| "status-playback-resolution".into())
                            .when_some(crate::tooltip::builder("preview-resolution"), |el, tip| {
                                el.with_tip(tip)
                            })
                            .child(resolution),
                    )
                })
                .when_some(gpu, |el, gpu| {
                    el.child(
                        div()
                            .debug_selector(|| "status-gpu".into())
                            .text_color(theme.text_faint)
                            .child(gpu),
                    )
                }),
        )
}

/// The first job still running, its bar, and how many more are running beside
/// it, or nothing while no work is in flight. A click brings the Jobs panel
/// forward, where every job has its own row and its Stop.
fn live_job(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> Option<gpui::AnyElement> {
    // The handle: the projection is rebuilt when a job reports, not when a
    // frame draws.
    let jobs = state.read(cx).job_view.clone();
    let mut live = jobs.iter().filter(|job| !job.terminal);
    let first = live.next()?;
    let more = live.count();
    // The same key as the job's row on the board, so the two bars draw one
    // width for one job.
    let bar = crate::loaders::progress_bar(
        motion::ControlId::Key(motion::MotionKey::new("job-bar", first.id)),
        first.progress,
        theme,
        cx.entity_id(),
        cx,
    )
    .into_any_element();
    // Breathing on the catalog's pulse, so the dot reads as live rather than
    // as a decoration.
    let phase = motion::pulse_delta(&gpui::motion::presets::ZERON_PULSE, cx.entity_id(), cx);
    let dot = gpui::motion::loaders::pulse_opacity(phase);
    Some(
        div()
            .id("status-job")
            .debug_selector(|| "status-job".into())
            .flex()
            .items_center()
            .gap(px(Theme::SPACE_SM))
            .cursor_pointer()
            .child(
                div()
                    .size(px(6.))
                    .rounded(px(3.))
                    .bg(theme.recording_dot.opacity(dot)),
            )
            .child(
                div()
                    .max_w(px(240.))
                    .truncate()
                    .text_color(theme.text)
                    .child(first.label.clone()),
            )
            .child(div().w(px(72.)).child(bar))
            .when(more > 0, |el| {
                el.child(
                    div()
                        .text_color(theme.text_faint)
                        .child(gpui::SharedString::from(format!("+{more}"))),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        state.focus_dock_panel(crate::dock::PANEL_JOBS)
                    });
                    cx.notify();
                }),
            )
            .into_any_element(),
    )
}

/// The dropped-frame indicator, drawn once a playback frame has dropped since
/// the last reset: the count in `warning`. A click resets it.
fn dropped_indicator(
    theme: &Theme,
    dropped: u32,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    crate::components::chip(
        "status-dropped-frames",
        gpui::SharedString::from(format!("{dropped} dropped")),
        false,
        crate::components::ChipSize::Compact,
        theme,
    )
    .debug_selector(|| "status-dropped-frames".into())
    .text_color(theme.warning)
    .font_features(crate::theme::tabular_figures())
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.reset_dropped_frames());
            cx.notify();
        }),
    )
}

pub(super) fn split_row(cx: &mut Context<Shell>, kind: DragKind) -> impl IntoElement {
    let theme = Theme::of(cx);
    let key = motion::MotionKey::new("split-timeline", "row");
    div()
        .id("split-timeline")
        .debug_selector(|| "split-timeline".into())
        .h(px(SPLIT))
        .w_full()
        .cursor(CursorStyle::ResizeUpDown)
        // A fade redraws the window chrome alone; a gpui hover style would
        // notify the Shell, which rebuilds every panel body.
        .bg(motion::hover_blend(key, theme.bg, theme.border_strong))
        .on_hover(motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if event.click_count >= 2 {
                    this.state
                        .update(cx, |state, _| state.reset_split("timeline"));
                    this.persist(cx);
                    cx.notify();
                    return;
                }
                let start = this.state.read(cx).ui_settings.timeline_height;
                this.drag = Some(Drag::new(kind, f32::from(event.position.y), start));
            }),
        )
}

/// Age the status line, write recovery copies and reload plugins whose files
/// changed, for the life of the session.
///
/// The ticker asks the state whether the message is spent rather than every
/// writer arming a timer, and repaints only when something cleared or a
/// plugin reloaded. Autosave and the plugin watch ride the same half-second
/// wake rather than arming timers of their own: each checks whether it has
/// work.
///
/// The task holds a weak shell handle and nothing else. The state is reached
/// through the shell on each wake, so a timer still pending after
/// `App::shutdown` retains no entity. A strong state handle in the task would
/// outlive the app and fail gpui's leak detector when the app drops.
pub(super) fn spawn_status_ageing(cx: &mut Context<Shell>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(STATUS_TICK).await;
            // A dropped shell is shutdown: stop rather than touch the state.
            let running = this
                .update(cx, |shell, cx| {
                    shell.state.update(cx, |state, cx| {
                        let now = Instant::now();
                        state.autosave_tick(now);
                        let reloaded = state.reload_changed_plugins();
                        if state.age_status(now) || reloaded {
                            cx.notify();
                        }
                    });
                })
                .is_ok();
            if !running {
                break;
            }
        }
    })
    .detach();
}

/// Drive the output meter's ballistics.
///
/// A separate wake from the status ticker because the two clocks are different
/// kinds: a status line is a receipt half a second either way, a meter is a
/// picture of the sound happening now. Like the status ticker, the task holds a
/// weak shell handle only.
pub(super) fn spawn_meter(cx: &mut Context<Shell>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(METER_TICK).await;
            let running = this
                .update(cx, |shell, cx| {
                    shell.state.update(cx, |state, cx| {
                        if state.meter_tick(Instant::now()) {
                            cx.notify();
                        }
                    });
                })
                .is_ok();
            if !running {
                break;
            }
        }
    })
    .detach();
}
