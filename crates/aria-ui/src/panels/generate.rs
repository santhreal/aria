//! The Generate tab: a prompt, a kind, and the generation jobs it produced.
//!
//! The panel starts nothing on its own and polls nothing. Submit hands the prompt to
//! [`crate::app::AppState::submit_generation`], which owns the request, the job, and the
//! import; the rows below are the same job projection the Jobs board draws, filtered to
//! the generations, so there is one view of a running job in the app rather than two.
//!
//! With no provider key the panel says so and Submit is absent. A Submit that queues a
//! request the provider will reject is how a missing key turns into a mystery failure.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::generate::{GenerateKind, missing_key_line};
use crate::components;
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

/// How many generations the panel shows before the rest stay on the Jobs
/// board, which is where the full history lives.
const JOB_LIMIT: usize = 6;

/// The panel's claim on the shared job list.
///
/// Only the generation rows: the full board is Window > Jobs, and repeating an
/// export here would say nothing about the prompt.
fn mine(job: &crate::app::jobs::UiJob) -> bool {
    job.label.starts_with("Generating ")
}

pub(crate) fn generate_panel(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let form = state.read(cx).generate.clone();
    let configured = state.read(cx).generation_configured();
    let rows = crate::pending::job_rows(theme, state, JOB_LIMIT, mine, cx);

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(
            components::text_box(
                "generate-prompt",
                &form.prompt,
                "Describe what to generate",
                form.focused,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, _| state.focus_generate_prompt(true));
                    cx.notify();
                }),
            ),
        )
        .child(
            div().flex().gap(px(Theme::SPACE_XS)).children(
                GenerateKind::ALL
                    .iter()
                    .map(|(kind, label, _)| kind_chip(theme, *kind, label, form.kind, cx)),
            ),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(SharedString::from(if !configured {
                    missing_key_line()
                } else if form.focused {
                    "Enter submits, Esc leaves the box".to_string()
                } else {
                    String::new()
                })),
        )
        .when(configured, |el| {
            el.child(
                components::ghost_button("generate-submit", "Generate", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, cx| {
                            if let Err(err) = state.submit_generation(cx) {
                                state.report_error(err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            )
        })
        .children(rows)
        // Lottie animations are generated clips too, and the endpoint the client
        // has always been able to call had no surface: a downloaded animation is
        // placed at the playhead like any other generator.
        .child(super::cloud::cloud_catalog_block(
            theme,
            crate::cloud::CloudFeed::Lottie,
            state,
            cx,
        ))
        .into_any_element()
}

/// One kind, as a chip. Drawn by [`crate::components::chip`], so it eases on
/// hover like every other chip instead of swapping colour.
fn kind_chip(
    theme: &Theme,
    target: GenerateKind,
    label: &'static str,
    current: GenerateKind,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    crate::components::chip(
        crate::motion::MotionKey::new("generate-kind", label),
        label,
        target == current,
        crate::components::ChipSize::Regular,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.set_generate_kind(target));
            cx.notify();
        }),
    )
    .into_any_element()
}
