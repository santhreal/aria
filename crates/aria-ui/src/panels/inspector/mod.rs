//! Inspector: properties for the selected clip.

pub mod blocks;
mod clip_type;
#[cfg(test)]
mod columns_tests;
#[cfg(test)]
mod keyframes_tests;
#[cfg(test)]
mod reorder_tests;
#[cfg(test)]
mod strip_tests;
pub mod tab_host;
mod tab_strip;
pub mod tabs;

use aria_plugin::PanelLocation;
use gpui::{
    AnyElement, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Styled, div, px,
};

use crate::app::AppState;
use crate::motion;
use crate::theme::Theme;
use crate::types::{ActivePanel, UiClip};

pub use tab_host::{TabBody, TabEntry, TabHost, host_of};

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    // The lines a test records are the ones built from here to the end.
    #[cfg(test)]
    let _building = blocks::widgets::BuildingInspector::start();
    let theme = Theme::of(cx);
    let (selected, tab, rate, duration_ticks, canvas) = {
        let app = state.read(cx);
        (
            app.selected_ui_clip(),
            app.inspector_tab.clone(),
            app.store.sequence.frame_rate,
            app.store.sequence.duration_ticks,
            app.canvas_size(),
        )
    };

    // The stacked sub panels (look picker, reports) scroll with the body, so a
    // short frame reaches them.
    let subs: Vec<AnyElement> = {
        let read = state.read(cx);
        crate::plugins::panel_host::host_of(&read.ctx)
            .map(|host| host.sub_entries_at(PanelLocation::Inspector, read))
            .unwrap_or_default()
    }
    .into_iter()
    .map(|sub| sub(state, cx))
    .collect();

    div()
        .id("inspector")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.surface)
        .child(match selected {
            Some(clip) => {
                clip_body(&theme, clip, tab, rate.num, rate.den, subs, state, cx).into_any_element()
            }
            None => crate::edge_fade::scroll_y(
                scroller()
                    .child(sequence_body(
                        &theme,
                        duration_ticks,
                        rate.num,
                        rate.den,
                        canvas,
                    ))
                    .children(subs),
            )
            .into_any_element(),
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.active_panel = ActivePanel::Inspector);
                cx.notify();
            }),
        )
}

const SEQUENCE_PROMPT: &str = "Select a clip, or click a gap to edit the sequence.";

fn rate_label(num: i32, den: i32) -> String {
    if den == 1 {
        format!("{num} fps")
    } else {
        format!("{num}/{den} fps")
    }
}

fn sequence_body(
    theme: &Theme,
    duration_ticks: i64,
    fps_num: i32,
    fps_den: i32,
    canvas: [u32; 2],
) -> impl IntoElement {
    use crate::components::{readout, section_label};
    use crate::timecode::format_ticks;

    div()
        .flex()
        .flex_col()
        .p(px(Theme::SPACE_MD))
        .gap(px(Theme::SPACE_MD))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(SEQUENCE_PROMPT),
        )
        .child(section_label(theme, "Sequence"))
        .child(readout(theme, "Rate", rate_label(fps_num, fps_den)))
        .child(readout(
            theme,
            "Frame",
            format!("{}×{}", canvas[0], canvas[1]),
        ))
        .child(readout(
            theme,
            "Duration",
            format_ticks(duration_ticks, fps_num, fps_den),
        ))
}

/// The region under the tab row that scrolls, empty. Each caller fills it with
/// the tab's body and then the stacked sub panels, and makes it a scroll frame
/// with [`crate::edge_fade::scroll_y`].
fn scroller() -> gpui::Stateful<gpui::Div> {
    div()
        .id("inspector-body")
        .debug_selector(|| "inspector-body".into())
        .flex_1()
        .min_h_0()
}

#[allow(clippy::too_many_arguments)]
fn clip_body(
    theme: &Theme,
    clip: UiClip,
    requested: String,
    fps_num: i32,
    fps_den: i32,
    subs: Vec<AnyElement>,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let host = host_of();
    // The strip and the body resolve the same tab once: a strip underlining one
    // tab while the body draws another is the bug this shape rules out.
    let (active_id, active_body) = match host.resolve(&requested, clip.kind) {
        Some(tab) => (tab.id.clone(), Some(tab.body)),
        None => (gpui::SharedString::new_static(""), None),
    };

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .children(
            clip.clip_type
                .as_ref()
                .map(|clip_type| clip_type::clip_type_row(theme, clip_type, cx)),
        )
        .child(tab_strip::tab_row(
            theme,
            host,
            clip.kind,
            active_id.clone(),
            cx,
        ))
        .child(
            // A tab taller than its frame scrolls under the tab row.
            crate::edge_fade::scroll_y(
                scroller()
                    .child(
                        div().p(px(Theme::SPACE_MD)).child(match active_body {
                            // The tab's id is the animation's id, so switching tabs is a
                            // new element and the incoming body fades up instead of
                            // replacing the outgoing one between two frames.
                            Some(body) => motion::fade_quick(
                                motion::MotionKey::new("insp-body", &active_id),
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(Theme::SPACE_MD))
                                    .child(body(theme, &clip, fps_num, fps_den, state, cx)),
                            )
                            .into_any_element(),
                            None => crate::components::empty_state(
                                "inspector",
                                "This clip has no properties to edit.",
                                theme,
                            )
                            .into_any_element(),
                        }),
                    )
                    .children(subs),
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::rate_label;

    #[test]
    fn a_rate_is_written_whole_or_as_its_fraction() {
        assert_eq!(rate_label(24, 1), "24 fps");
        assert_eq!(rate_label(30000, 1001), "30000/1001 fps");
    }
}
