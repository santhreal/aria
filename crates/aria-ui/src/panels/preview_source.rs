//! The Source tab's stage and bar, the Program frame, and the safe-margin
//! guides.
//!
//! Each item is `pub(super)` so the parent and its other children reach it by
//! name.

use super::*;
use crate::surfaces::source_pick::{self, Pick};
use gpui::AppContext as _;

/// Press one row of the registry, reporting a refusal in the status line rather
/// than swallowing it.
fn press(state: &mut AppState, pick: Pick) {
    if let Err(err) = source_pick::apply(state, pick) {
        state.report_error(&err);
    }
}

/// Lay a picture out in its stage: the zoomed frame's own rectangle, which
/// the handles and overlays are placed by too, or while the view fits, a box
/// `1 / width` by `1 / height` times the stage, where `ObjectFit::Contain`
/// letterboxes it. A stage cut down to `width` by `height` of the pane by a
/// divider still lines its fitted picture up with the pane's.
pub(super) fn place<E: Styled>(
    picture: E,
    zoomed: Option<crate::app::canvas_overlay::Rect>,
    [width, height]: [f32; 2],
) -> E {
    let picture = picture.absolute();
    match zoomed {
        Some(frame) => picture
            .left(px(frame.x))
            .top(px(frame.y))
            .w(px(frame.w))
            .h(px(frame.h)),
        None => picture
            .left_0()
            .top_0()
            .w(relative(1.0 / width))
            .h(relative(1.0 / height)),
    }
}

pub(super) fn frame_view(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let zoomed = state.read(cx).preview_zoomed_frame();
    let texture = state.read(cx).preview_worker.shown_texture();
    let image = state.read(cx).last_render.clone();
    let gpu_probe = gpui::canvas(
        {
            let state = state.clone();
            move |_, window, cx| {
                if state
                    .read(cx)
                    .preview_worker
                    .set_gpu_device(window.gpu_device())
                {
                    cx.defer(move |app| {
                        state.update(app, |state, _| {
                            state.picture_changed();
                        });
                        crate::app::spawn_preview(state, app);
                    });
                }
            }
        },
        |_, _, _, _| {},
    )
    .size_0();
    if let Some(texture) = texture {
        div()
            .id("preview-stage")
            .size_full()
            .overflow_hidden()
            .relative()
            .child(gpu_probe)
            .child(crate::components::picture(place(
                gpui::surface(texture).object_fit(ObjectFit::Contain),
                zoomed,
                [1.0, 1.0],
            )))
            .into_any_element()
    } else if let Some(image) = image {
        div()
            .id("preview-stage")
            .size_full()
            .overflow_hidden()
            .relative()
            .child(gpu_probe)
            .child(crate::components::picture(place(
                img(image)
                    .id("preview-frame")
                    .object_fit(ObjectFit::Contain),
                zoomed,
                [1.0, 1.0],
            )))
            .into_any_element()
    } else {
        let stage = div().flex().size_full().child(gpu_probe);
        match program_line(state.read(cx)) {
            Some((id, line)) => stage
                .child(crate::components::empty_state(id, line, theme))
                .into_any_element(),
            None => stage.into_any_element(),
        }
    }
}

/// The line the Program stage draws while it has no picture: why the frame it
/// holds is not shown, or in a sequence with no clip, the way to a first one.
/// `None` while the first frame of a sequence with clips renders, which an
/// instruction to import media would misdescribe.
fn program_line(state: &AppState) -> Option<(&'static str, &'static str)> {
    if state.last_frame.is_some() {
        Some((
            "viewer-unshown",
            "Couldn't convert this frame for display. Try scrubbing again.",
        ))
    } else if state.store.sequence.clip_count() == 0 {
        Some((
            "viewer",
            "Import media, then click a tile to place it on the timeline.",
        ))
    } else {
        None
    }
}

/// The monitor showing a pool source: the frame it is scrubbed to with the
/// corner chip over it, and the source bar under it.
///
/// While a newly scrubbed tick renders, the frame already held for that source
/// stays on screen — a scrub that blanked the pane between frames would be
/// unusable.
pub(super) fn source_stage(
    view: SourceView,
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    pool_preview::request_source(state, cx, view.media, view.tick);
    let image = state
        .read(cx)
        .pool_preview
        .source(view.media, view.tick)
        .or_else(|| state.read(cx).pool_preview.source_stale(view.media))
        .cloned();
    let still = state
        .read(cx)
        .store
        .media
        .iter()
        .any(|item| item.id == view.media && item.still);
    let picture = div()
        .debug_selector(|| "source-picture".into())
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .when(image.is_none(), |el| {
            // A source render is a wait like any other: the decode is already
            // requested above, and nothing else on screen says so.
            el.child(crate::pending::working(
                theme,
                "preview.source",
                "Rendering this source…",
                cx,
            ))
        })
        .when_some(image, |el, image| {
            // The frame is the handle for a drag onto the timeline, which
            // drops the marked range with the same patching as the buttons.
            el.child(
                div()
                    .id("source-frame-drag")
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::components::picture(
                        img(image)
                            .id("source-frame")
                            .object_fit(ObjectFit::Contain)
                            .size_full(),
                    ))
                    .on_drag(
                        crate::types::MediaDrag {
                            media: view.media,
                            label: view.name.clone(),
                            from_source: true,
                        },
                        |drag, _offset, _window, cx| {
                            let label = drag.label.clone();
                            cx.new(|_| crate::panels::sidebar::DragGhost { label })
                        },
                    ),
            )
        })
        .child(name_chip(&view, still, state, theme, cx));
    div()
        .id("source-stage")
        .size_full()
        .flex()
        .flex_col()
        .child(picture)
        .child(source_bar(&view, still, theme, cx))
        .into_any_element()
}

/// The corner chip's position: the source and the frame on screen out of the
/// take's frames. A still is one picture whose length is counted in
/// milliseconds, not frames, so it reads as a still.
fn position_text(view: &SourceView, still: bool) -> String {
    if still {
        return format!("{} · still image", view.name);
    }
    let frames = view.duration_ticks.max(1);
    format!("{} · frame {} of {}", view.name, view.tick + 1, frames)
}

/// The source bar's marked range: In, Out, and the marked length out of the
/// take's. A moving picture counts frames; a still's marks set the length it is
/// placed for, which reads in seconds.
fn marked_text(view: &SourceView, still: bool) -> String {
    let frames = view.duration_ticks.max(1);
    let start = view.mark_in.unwrap_or(0);
    let end = view.mark_out.unwrap_or(frames);
    let unset = || "—".to_string();
    if still {
        let seconds = |ticks: i64| {
            let unit = aria_models::STILL_TICK_RATE.seconds_per_unit();
            format!("{:.2} s", ticks as f64 * unit)
        };
        return format!(
            "In {} · Out {} · {} of {}",
            view.mark_in.map(seconds).unwrap_or_else(unset),
            view.mark_out.map(seconds).unwrap_or_else(unset),
            seconds((end - start).max(0)),
            seconds(frames),
        );
    }
    format!(
        "In {} · Out {} · {} of {} frames",
        view.mark_in
            .map(|t| (t + 1).to_string())
            .unwrap_or_else(unset),
        view.mark_out.map(|t| t.to_string()).unwrap_or_else(unset),
        (end - start).max(0),
        frames,
    )
}

/// The chip in the picture's top left corner: which source it is, the frame it
/// shows, and the way back to the sequence.
///
/// As wide as its text, up to the stage's width less the inset; a longer name
/// ends in an ellipsis before the Sequence button.
fn name_chip(
    view: &SourceView,
    still: bool,
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let mut position = position_text(view, still);
    // A shuttle speed is the one thing about the source clock the frame number
    // does not show.
    let rate = state.read(cx).sources.play_rate().filter(|rate| *rate != 1);
    if let Some(rate) = rate {
        position.push_str(&format!(" · {rate}x"));
    }
    div()
        .absolute()
        .top(px(Theme::SPACE_SM))
        .left(px(Theme::SPACE_SM))
        .right(px(Theme::SPACE_SM))
        .flex()
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XXS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_overlay)
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(gpui::SharedString::from(position)),
                )
                .child(
                    crate::components::ghost_button("source-close", "Sequence", theme)
                        .flex_none()
                        .when_some(crate::tooltip::builder("source-close"), |el, tip| {
                            el.with_tip(tip)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, _| state.show_program());
                                cx.notify();
                            }),
                        ),
                ),
        )
}

/// The Source tab's marks and cut-in controls, a row under the picture: the
/// marked range, a subclip of it in the pool, and the two edits that put that
/// range in the sequence. The mini ruler under the control row shows the range
/// across the take and scrubs it.
pub(super) fn source_bar(
    view: &SourceView,
    still: bool,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let marked = marked_text(view, still);
    // A row of the Viewer's own, as the control row under it, so no control
    // covers the picture.
    div()
        .debug_selector(|| "source-bar".into())
        .flex_none()
        .px(px(Theme::SPACE_MD))
        .py(px(Theme::SPACE_XS))
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            // Narrower than the label and the buttons, the buttons take the
            // next line at the right.
            div()
                .flex()
                .wraps()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(gpui::SharedString::from(marked)),
                )
                .child(
                    div()
                        .flex()
                        .wraps()
                        .items_center()
                        .justify_end()
                        .gap(px(Theme::SPACE_SM))
                        .ml_auto()
                        .child(
                            crate::components::ghost_button("source-mark-in", "In", theme)
                                .when_some(crate::tooltip::builder("source-mark-in"), |el, tip| {
                                    el.with_tip(tip)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state
                                            .update(cx, |state, _| press(state, Pick::MarkIn));
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            crate::components::ghost_button("source-mark-out", "Out", theme)
                                .when_some(crate::tooltip::builder("source-mark-out"), |el, tip| {
                                    el.with_tip(tip)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state
                                            .update(cx, |state, _| press(state, Pick::MarkOut));
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            crate::components::ghost_button("source-mark-clear", "Clear", theme)
                                .when_some(
                                    crate::tooltip::builder("source-mark-clear"),
                                    |el, tip| el.with_tip(tip),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state
                                            .update(cx, |state, _| press(state, Pick::ClearMarks));
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            crate::components::ghost_button("source-subclip", "Subclip", theme)
                                .when_some(crate::tooltip::builder("source-subclip"), |el, tip| {
                                    el.with_tip(tip)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state
                                            .update(cx, |state, _| press(state, Pick::Subclip));
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            crate::components::button("source-insert", "Insert", theme)
                                .when_some(crate::tooltip::builder("source-insert"), |el, tip| {
                                    el.with_tip(tip)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state
                                            .update(cx, |state, _| press(state, Pick::Insert));
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            crate::components::button("source-overwrite", "Overwrite", theme)
                                .when_some(
                                    crate::tooltip::builder("source-overwrite"),
                                    |el, tip| el.with_tip(tip),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state
                                            .update(cx, |state, _| press(state, Pick::Overwrite));
                                        cx.notify();
                                    }),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

/// A mark the Viewer draws over the picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum GuideMark {
    /// A safe-area outline.
    SafeArea,
    /// A line of the thirds grid.
    Third,
}

/// Title-safe, action-safe, and the thirds grid.
pub(super) fn guides(
    view: crate::app::preview_view::PreviewView,
    frame: crate::app::canvas_overlay::FrameBox,
    theme: &Theme,
) -> Vec<gpui::AnyElement> {
    guide_rects(view, frame)
        .into_iter()
        .map(|(mark, rect)| {
            let mark_div = div()
                .absolute()
                .left(px(rect.x))
                .top(px(rect.y))
                .w(px(rect.w))
                .h(px(rect.h));
            match mark {
                GuideMark::SafeArea => mark_div.border_1().border_color(theme.border_strong),
                GuideMark::Third => mark_div.bg(theme.border_strong),
            }
            .into_any_element()
        })
        .collect()
}

/// Each guide `view` turns on and its rectangle in pane pixels, placed on the
/// picture rectangle the last paint recorded in `frame`, so a guide follows
/// the fit, zoom, and pan of the picture rather than the pane around it.
/// Nothing is placed before the first paint.
///
/// The percentages are SMPTE ST 2046-1's: action safe is the inner 93% of the
/// frame and title safe the inner 90%.
pub(super) fn guide_rects(
    view: crate::app::preview_view::PreviewView,
    frame: crate::app::canvas_overlay::FrameBox,
) -> Vec<(GuideMark, crate::app::canvas_overlay::Rect)> {
    use crate::app::canvas_overlay::Rect;
    let (Some(pane), Some(picture)) = (frame.container, frame.canvas) else {
        return Vec::new();
    };
    let (x, y, w, h) = (picture.x - pane.x, picture.y - pane.y, picture.w, picture.h);
    let mut out = Vec::new();
    for (on, inset) in [(view.action_safe, 0.035), (view.title_safe, 0.05)] {
        if on {
            let (dx, dy) = (w * inset, h * inset);
            out.push((
                GuideMark::SafeArea,
                Rect::new(x + dx, y + dy, w - 2.0 * dx, h - 2.0 * dy),
            ));
        }
    }
    if view.grid {
        for third in [1.0 / 3.0, 2.0 / 3.0] {
            out.push((GuideMark::Third, Rect::new(x + w * third, y, 1.0, h)));
            out.push((GuideMark::Third, Rect::new(x, y + h * third, w, 1.0)));
        }
    }
    out
}

/// "Media Offline" and the missing files, over a Program frame that shows a
/// clip whose file is gone. The preview draws such a clip as a plain red
/// slate, which reads as footage without the words.
pub(super) fn offline_notice(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &gpui::Context<crate::shell::Shell>,
) -> Option<gpui::AnyElement> {
    let read = state.read(cx);
    let tick = read.shown_tick.unwrap_or(read.playhead.value);
    let names = read.store.sequence.offline_at(tick);
    if names.is_empty() {
        return None;
    }
    let files = names
        .iter()
        .map(|name| name.as_ref())
        .collect::<Vec<_>>()
        .join(", ");
    Some(
        div()
            .debug_selector(|| "viewer-offline".into())
            .absolute()
            .top(px(Theme::SPACE_MD))
            .left(px(Theme::SPACE_MD))
            .max_w(relative(0.9))
            .px(px(Theme::SPACE_SM))
            .py(px(Theme::SPACE_XS))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(theme.danger)
            .text_color(Theme::ink_on(theme.danger))
            .text_size(Theme::TEXT_UI)
            .truncate()
            .child(gpui::SharedString::from(format!("Media Offline: {files}")))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use aria_core::RgbaImage;
    use aria_plugin::PluginContext;

    use super::{marked_text, position_text, program_line};
    use crate::app::AppState;
    use crate::app::pool_preview::SourceView;
    use crate::settings::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
    use crate::test_support::panel_bounds;

    /// WHY: the Program stage told the editor to import media and place a
    /// tile whenever no frame had arrived, including while a sequence full of
    /// clips rendered its first frame. Each state the stage can be in without
    /// a picture draws its own line, or none.
    #[test]
    fn the_program_stage_offers_an_import_only_to_an_empty_sequence() {
        let mut empty = AppState::new(
            PluginContext::new(),
            crate::scratch_pool::scratch("program-line"),
        )
        .expect("state");
        empty.new_project().expect("project");
        assert_eq!(program_line(&empty).map(|line| line.0), Some("viewer"));

        let (mut placed, _) = crate::app::premiere_tests::with_clip();
        assert_eq!(
            program_line(&placed),
            None,
            "a sequence with a clip is rendering its first frame"
        );

        placed.last_frame = Some(Arc::new(RgbaImage::new(1, 1, vec![0; 4])));
        assert_eq!(
            program_line(&placed).map(|line| line.0),
            Some("viewer-unshown")
        );
    }

    /// WHY: a still in a Source tab read `texture.png · frame 1 of 5000` and
    /// `In — · Out — · 5000 of 5000 frames`: a still counts its placement
    /// length in milliseconds, and the chip and the bar printed those as
    /// frames. Closes "a tick that is not a frame read as one" for both lines
    /// of the Source tab, a still and a moving picture each, marked and not.
    /// Not covered: the step keys, which move a still by one tick.
    #[test]
    fn a_still_source_reads_as_a_still_and_its_marks_in_seconds() {
        let view = |duration_ticks, mark_in, mark_out| SourceView {
            media: 1,
            name: "take".into(),
            duration_ticks,
            tick: 9,
            mark_in,
            mark_out,
            clip: None,
        };
        for (still, source, position, marked) in [
            (
                false,
                view(48, None, None),
                "take · frame 10 of 48",
                "In — · Out — · 48 of 48 frames",
            ),
            (
                false,
                view(48, Some(12), Some(36)),
                "take · frame 10 of 48",
                "In 13 · Out 36 · 24 of 48 frames",
            ),
            (
                true,
                view(5_000, None, None),
                "take · still image",
                "In — · Out — · 5.00 s of 5.00 s",
            ),
            (
                true,
                view(5_000, Some(500), Some(3_000)),
                "take · still image",
                "In 0.50 s · Out 3.00 s · 2.50 s of 5.00 s",
            ),
        ] {
            assert_eq!(position_text(&source, still), position, "{source:?}");
            assert_eq!(marked_text(&source, still), marked, "{source:?}");
        }
    }

    /// WHY: the Source tab drew its bar of marks and edits over the foot of
    /// the picture, so its buttons covered the take's bottom lines. Closes "a
    /// Source bar laid over the picture": with a Source tab in front, the bar
    /// is drawn under the picture's box and the box keeps a height, at the
    /// smallest window, where the bar wraps, and at a wide one. Does not cover
    /// the corner chip, which stands over the picture's top left corner.
    #[gpui::test]
    fn a_wrapped_source_bar_stands_under_the_picture(cx: &mut gpui::TestAppContext) {
        bar_under_picture(
            cx,
            "source-bar-smallest",
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
    }

    #[gpui::test]
    fn a_wide_source_bar_stands_under_the_picture(cx: &mut gpui::TestAppContext) {
        bar_under_picture(cx, "source-bar-wide", 1920.0, 1080.0);
    }

    /// Open a take in a Source tab in a `width` by `height` window and check
    /// the bar against the picture's box.
    fn bar_under_picture(cx: &mut gpui::TestAppContext, tag: &str, width: f32, height: f32) {
        cx.executor().allow_parking();
        let at = gpui::Bounds::new(
            gpui::point(gpui::px(0.), gpui::px(0.)),
            gpui::size(gpui::px(width), gpui::px(height)),
        );
        let (state, mut visual) = crate::test_support::drawn_shell_at(cx, tag, at);
        let take = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4");
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                let media = state.import_media(take).expect("import the take");
                assert!(state.open_source(media.raw()), "the Source tab opens");
                cx.notify();
            })
        });
        let picture = panel_bounds(&mut visual, "source-picture")
            .expect("the Source tab draws its picture box");
        let bar = panel_bounds(&mut visual, "source-bar").expect("the Source tab draws its bar");
        assert!(
            picture.size.height > gpui::px(0.),
            "{tag}: the bar leaves the picture no height: {picture:?}"
        );
        assert!(
            bar.top() + gpui::px(0.5) >= picture.bottom(),
            "{tag}: the bar {bar:?} is drawn over the picture {picture:?}"
        );
    }

    /// The safe areas and the thirds grid stand on the picture, not on the
    /// pane around it.
    ///
    /// The class closed here: guides placed as insets of the pane, so a
    /// picture pillarboxed, letterboxed, zoomed or panned in the pane drew its
    /// safe areas and thirds off the picture. Driven over a pillarboxed
    /// portrait picture, a letterboxed wide one and a zoomed, panned one. Not
    /// caught: a guide drawn on an element other than the one the pane
    /// positions from.
    #[test]
    fn the_guides_stand_on_the_picture_whatever_the_pane_around_it() {
        use super::{GuideMark, guide_rects};
        use crate::app::canvas_overlay::{FrameBox, Rect, view_frame};
        use crate::app::preview_view::PreviewView;

        let pane = Rect::new(100.0, 50.0, 800.0, 450.0);
        let all = PreviewView {
            title_safe: true,
            action_safe: true,
            grid: true,
            ..PreviewView::default()
        };
        assert!(
            guide_rects(all, FrameBox::default()).is_empty(),
            "a guide was placed before the first paint"
        );
        let zoomed = PreviewView {
            zoom: Some(2.0),
            pan: (40.0, -20.0),
            ..all
        };
        for (name, view, canvas) in [
            ("portrait", all, [1080, 1920]),
            ("wide", all, [2560, 1080]),
            ("zoomed", zoomed, [1920, 1080]),
        ] {
            let picture = view_frame(pane, view, canvas);
            let frame = FrameBox {
                container: Some(pane),
                canvas: Some(picture),
                layer: None,
            };
            let (x, y, w, h) = (picture.x - pane.x, picture.y - pane.y, picture.w, picture.h);
            let want = [
                (
                    GuideMark::SafeArea,
                    [x + w * 0.035, y + h * 0.035, w * 0.93, h * 0.93],
                ),
                (
                    GuideMark::SafeArea,
                    [x + w * 0.05, y + h * 0.05, w * 0.9, h * 0.9],
                ),
                (GuideMark::Third, [x + w / 3.0, y, 1.0, h]),
                (GuideMark::Third, [x, y + h / 3.0, w, 1.0]),
                (GuideMark::Third, [x + w * 2.0 / 3.0, y, 1.0, h]),
                (GuideMark::Third, [x, y + h * 2.0 / 3.0, w, 1.0]),
            ];
            let got = guide_rects(view, frame);
            assert_eq!(got.len(), want.len(), "{name}: {got:?}");
            for ((mark, rect), (want_mark, [wx, wy, ww, wh])) in got.iter().zip(want) {
                let near = |a: f32, b: f32| (a - b).abs() < 0.01;
                assert!(
                    *mark == want_mark
                        && near(rect.x, wx)
                        && near(rect.y, wy)
                        && near(rect.w, ww)
                        && near(rect.h, wh),
                    "{name}: {mark:?} {rect:?}, want {want_mark:?} at {wx} {wy} {ww}x{wh}"
                );
            }
        }
    }
}
