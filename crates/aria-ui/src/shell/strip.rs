//! Strip: 8 helpers from [`super`].
//!
//! Split out of `shell.rs` to stay under the file ceiling. A pure
//! move. Each item is `pub(super)` so the parent and its other children can
//! reach it by name; nothing else changed.

use super::*;

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
    let store = &state.read(cx).store;
    let rate = store.sequence.frame_rate;
    let fps = if rate.den == 0 {
        0.0
    } else {
        rate.num as f32 / rate.den as f32
    };
    let clock = format!(
        "{}  ·  {:.3} fps  ·  {} tracks",
        crate::timecode::format_ticks(store.playhead_ticks, rate.num, rate.den),
        fps,
        store.sequence.tracks.len(),
    );
    let has_err = err.is_some();
    let note = err.or(status).unwrap_or_default();
    // A status line that replaced another one has to be seen to have changed:
    // two refusals in a row can differ by their text alone. The element id is
    // the message's own identity, so a new message is a new element and fades
    // in, while a repaint carrying the same message does not restart the fade.
    let note_id = motion::MotionKey::new("status", &note);
    // A dot beside the clock while any job is in flight, breathing on the
    // catalog's own pulse so it reads as live rather than as a decoration. The
    // jobs panel says what the work is; this says only that there is some.
    let live_work = crate::pending::any_live(state, |_| true, cx);
    let dot = if live_work {
        let phase = motion::pulse_delta(&motion::ZERON_PULSE, cx.entity_id(), cx);
        motion::pulse_opacity(phase)
    } else {
        0.0
    };
    div()
        .h(px(Theme::STATUS_STRIP_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .px(px(Theme::SPACE_MD))
        .gap(px(Theme::SPACE_MD))
        // The strip carries the error rather than only its text: a one-line
        // message in a 24px strip is easy to miss, and the wash is quiet enough
        // to sit under a timecode that is read constantly.
        .bg(if has_err {
            theme.danger_muted
        } else {
            theme.surface
        })
        .border_t_1()
        .border_color(theme.border)
        .text_size(px(11.))
        .font_family(theme.font_mono.clone())
        .text_color(theme.text_muted)
        .when(live_work, |el| {
            el.child(
                div()
                    .size(px(6.))
                    .rounded(px(3.))
                    .bg(theme.recording_dot.opacity(dot)),
            )
        })
        .when_some(breadcrumb, |el, chip| el.child(chip))
        .child(clock)
        .child(
            div().flex_1().child(motion::fade_quick(
                note_id,
                div()
                    .text_color(if has_err {
                        theme.danger
                    } else {
                        theme.text_faint
                    })
                    .child(note),
            )),
        )
}

pub(super) fn pane(width: f32, child: impl IntoElement) -> impl IntoElement {
    motion::fade_in(
        "pane-in",
        div()
            .w(px(width))
            .h_full()
            .min_h_0()
            .overflow_hidden()
            .child(child),
    )
}

/// Concrete `Div` for the same reason [`crate::chrome::editor_bar`] is: the
/// timeline is the last stage of the editor opening, and a stage is animated.
pub(super) fn pane_h(height: f32, child: impl IntoElement) -> gpui::Div {
    div()
        .h(px(height))
        .w_full()
        .min_h_0()
        .overflow_hidden()
        .child(child)
}

pub(super) fn collapsed_strip(
    label: &'static str,
    sidebar: bool,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    motion::fade_quick(
        label,
        div()
            .id(label)
            .w(px(28.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.surface)
            .text_size(px(11.))
            .text_color(theme.text_faint)
            .cursor(CursorStyle::PointingHand)
            .active(|style| style.opacity(0.92))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if sidebar {
                            state.ui_settings.sidebar_collapsed = false;
                        } else {
                            state.ui_settings.inspector_collapsed = false;
                        }
                    });
                    this.persist(cx);
                    cx.notify();
                }),
            )
            .child(label.chars().next().unwrap_or('·').to_string()),
    )
}

pub(super) fn split_col(cx: &mut Context<Shell>, kind: DragKind) -> impl IntoElement {
    let theme = Theme::of(cx);
    let id = match kind {
        DragKind::Sidebar => "split-sidebar",
        DragKind::Inspector => "split-inspector",
        DragKind::Timeline | DragKind::Pan | DragKind::Lane(_) => "split-timeline",
    };
    div()
        .id(id)
        .w(px(SPLIT))
        .h_full()
        .cursor(CursorStyle::ResizeLeftRight)
        .bg(theme.bg)
        .hover(|el| el.bg(theme.border_strong))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if event.click_count >= 2 {
                    this.state.update(cx, |state, _| {
                        let which = match kind {
                            DragKind::Sidebar => "sidebar",
                            DragKind::Inspector => "inspector",
                            DragKind::Timeline | DragKind::Pan | DragKind::Lane(_) => "timeline",
                        };
                        state.reset_split(which);
                    });
                    this.persist(cx);
                    cx.notify();
                    return;
                }
                let panes = this.state.read(cx).ui_settings.panes();
                let start = match kind {
                    DragKind::Sidebar => panes.sidebar_width,
                    DragKind::Inspector => panes.inspector_width,
                    DragKind::Timeline | DragKind::Pan | DragKind::Lane(_) => panes.timeline_height,
                };
                this.drag = Some(Drag::new(kind, f32::from(event.position.x), start));
            }),
        )
}

pub(super) fn split_row(cx: &mut Context<Shell>, kind: DragKind) -> impl IntoElement {
    let theme = Theme::of(cx);
    div()
        .id("split-timeline")
        .h(px(SPLIT))
        .w_full()
        .cursor(CursorStyle::ResizeUpDown)
        .bg(theme.bg)
        .hover(|el| el.bg(theme.border_strong))
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

/// Age the status line and write recovery copies for the life of the session.
///
/// The ticker asks the state whether the message is spent rather than every
/// writer arming a timer, and repaints only when something actually cleared.
/// Autosave rides the same half-second wake rather than arming a second timer:
/// it decides for itself whether a copy is due.
pub(super) fn spawn_status_ageing(state: Entity<AppState>, cx: &mut Context<Shell>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(STATUS_TICK).await;
            // A dropped shell is shutdown: stop rather than touch the state.
            let running = this
                .update(cx, |_, cx| {
                    state.update(cx, |state, cx| {
                        let now = Instant::now();
                        state.autosave_tick(now);
                        if state.age_status(now) {
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
/// picture of the sound happening now.
pub(super) fn spawn_meter(state: Entity<AppState>, cx: &mut Context<Shell>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(METER_TICK).await;
            let running = this
                .update(cx, |_, cx| {
                    state.update(cx, |state, cx| {
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
