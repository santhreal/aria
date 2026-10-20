//! AI transcript and composer.

use crate::components::Wraps as _;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::motion::glide;
use crate::theme::Theme;
use crate::types::{ActivePanel, AgentEntryKind, UiAgentEntry};

/// The permission card and the skill list, beside this file to keep it readable.
#[path = "ai_panel_blocks.rs"]
mod blocks;

/// The transcript's own rows, beside this file for the same reason.
#[path = "ai_panel_rows.rs"]
mod rows;
use rows::{bubble, empty};

/// Carry the transcript to its newest row when a beat asked for it.
///
/// A jump to the bottom loses the reading position: the rows on screen are
/// replaced in one frame by unread rows. So the view travels, on the
/// catalog's own scroll glide — the same motion the timeline uses to reach a
/// view the editor chose.
///
/// Bounded twice over. The store is asked for a value every frame, so when
/// nothing is following it tracks the real offset and a later follow starts from
/// where the view actually is; and the offset is written only inside the glide's
/// own duration, so once it has run the scrollbar belongs to the hand again.
fn follow_newest(
    state: &Entity<AppState>,
    scroll: &gpui::ScrollHandle,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let asked = state.read(cx).assistant.follow_at;
    let window = glide::SCROLL.total();
    let live = asked.is_some_and(|at| at.elapsed() < window);
    let here = f32::from(scroll.offset().y);
    // Down is negative, so the bottom is the scrollable extent negated.
    let bottom = -f32::from(scroll.max_offset().y);
    let eased = glide::glide(
        crate::motion::MotionKey::new("ai-transcript", ""),
        if live { bottom } else { here },
        glide::SCROLL,
        cx.reduce_motion() || !live,
    );
    if live {
        scroll.set_offset(gpui::point(scroll.offset().x, px(eased)));
    }
}

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    // A handle on the projection, not a copy of it.
    let store = state.read(cx).store.clone();
    let selected = state.read(cx).active_panel == ActivePanel::Ai;
    // The handle, not the conversation: a streamed answer must not cost a copy
    // of every entry drawn so far, once per frame.
    let entries = state.read(cx).transcript.clone();
    let scroll = state.read(cx).assistant.scroll.clone();
    follow_newest(state, &scroll, cx);
    // The handle again: a pending plan is drawn as a card every frame until the
    // operator applies or drops it.
    let plan = state.read(cx).assistant.plan.clone();
    let asking = state
        .read(cx)
        .assistant
        .pending_ask
        .as_ref()
        .map(|ask| (ask.name.clone(), ask.summary.clone()));
    let asked = asking.is_some();

    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    if entries.is_empty() {
        rows.push(empty(&theme, &store.project_name).into_any_element());
    } else {
        let streaming = state.read(cx).assistant.streaming_row();
        for (index, entry) in entries.iter().enumerate() {
            let live = streaming == Some(index);
            rows.push(bubble(&theme, entry, index, live, cx).into_any_element());
        }
    }
    if !plan.is_empty() {
        rows.push(plan_card(&theme, &plan, cx).into_any_element());
    }
    if let Some((name, summary)) = asking {
        rows.push(blocks::ask_card(&theme, &name, &summary, cx).into_any_element());
    }
    // A turn in flight is the panel's only wait, and the transcript is where
    // its answer will land, so the loader waits in the answer's place. A
    // parked ask is not a wait on the model but a wait for an Allow click,
    // and the card already shows that.
    if state.read(cx).assistant.running && !asked {
        rows.push(crate::pending::working(&theme, "ai.turn", "Thinking…", cx));
    }

    div()
        .id("ai-panel")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.surface)
        .border_l_1()
        .border_color(if selected { theme.accent } else { theme.border })
        .child(header(&theme, state, cx))
        .child(
            crate::edge_fade::edge_faded(
                24.0,
                div()
                    .id("ai-transcript")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .p(px(Theme::SPACE_MD))
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_SM))
                    .children(rows),
            )
            // The fade follows the real scroll offset, so the top edge is only
            // soft when there is something above it.
            .fade_overflow_y(&scroll),
        )
        .child(blocks::skills_block(&theme, state, cx))
        .child(composer(&theme, state, cx))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.active_panel = ActivePanel::Ai);
                cx.notify();
            }),
        )
}

/// The panel's name, and the one action that applies to the whole conversation.
fn header(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let has_transcript = !state.read(cx).transcript.is_empty();
    div()
        .h(px(Theme::HEADER_HEIGHT))
        .px(px(Theme::SPACE_MD))
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(Theme::TEXT_HEADING)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Assistant"),
        )
        .when(has_transcript, |el| {
            el.child(
                crate::components::ghost_button("ai-copy-all", "Copy", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        let text = this.state.read(cx).transcript_text();
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        this.state.update(cx, |state, _| {
                            state.status_message = Some("Conversation copied".into());
                        });
                        cx.notify();
                    }),
                ),
            )
        })
}

/// A validated plan waiting for a decision.
///
/// The plan is shown as the lines it would apply, not as a count: "3 edits"
/// is not something anyone can approve.
fn plan_card(
    theme: &Theme,
    plan: &[crate::app::assistant::PlanStep],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let mut steps: Vec<gpui::AnyElement> = Vec::new();
    for step in plan {
        steps.push(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(gpui::SharedString::from(format!("· {}", step.description)))
                .into_any_element(),
        );
        // The tool the step arrives under, from the command itself. The
        // description says what would happen; the tool name says which
        // capability the model called, which is what to compare against the
        // list beside the panel when a plan is not what the prompt asked
        // for.
        steps.push(
            div()
                .text_size(Theme::TEXT_SMALL)
                .text_color(theme.text_faint)
                .child(gpui::SharedString::from(format!(
                    "   {}",
                    step.command.tool_name()
                )))
                .into_any_element(),
        );
    }
    div()
        .p(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .border_1()
        .border_color(theme.accent)
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(gpui::SharedString::from(format!(
                    "planned · {} edit(s), nothing applied",
                    plan.len()
                ))),
        )
        .children(steps)
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(Theme::SPACE_SM))
                .child(
                    crate::components::button("ai-plan-apply", "Apply", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = state.apply_agent_plan() {
                                    state.report_error(err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    crate::components::ghost_button("ai-plan-discard", "Discard", theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state.update(cx, |state, _| state.discard_agent_plan());
                                cx.notify();
                            }),
                        ),
                ),
        )
}

/// The prompt box, where the prompt goes, and how to stop it.
///
/// The box is drawn by [`crate::components::text_box`] and typed into through the
/// key table (`aria.field.ai-prompt`), like every other box in the app; clicking
/// it only takes focus. Send becomes Stop for the duration of a turn, because a
/// model that runs for a minute needs one obvious way out and a second Send
/// during a turn has nothing to do.
fn composer(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let running = state.read(cx).assistant.running;
    let focused = state.read(cx).assistant.focused;
    let prompt = state.read(cx).assistant.prompt.clone();
    let target = state.read(cx).assistant_target();
    let scope = state.read(cx).assistant_scope();
    let plan_only = state.read(cx).assistant.plan_only;
    let has_transcript = !state.read(cx).transcript.is_empty();
    let slash = state.read(cx).assistant_slash_hint();

    div()
        .p(px(Theme::SPACE_MD))
        .border_t_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            // Narrower than the target and the switches, the switches take the
            // next line; a target wider than the panel truncates. The
            // switches' left auto margin puts them at the right end: the row
            // sets no justify of its own, because taffy hands the line's free
            // space to the auto margin and to the justify both, which drew the
            // switches past the panel's right edge.
            div()
                .flex()
                .flex_row()
                .wraps()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_faint)
                        .child(gpui::SharedString::from(target)),
                )
                .child(
                    div()
                        .ml_auto()
                        .flex()
                        .flex_row()
                        .gap(px(Theme::SPACE_XS))
                        .child(
                            // Plan only is a switch, not a menu: it changes what
                            // Send does, so it belongs next to Send.
                            crate::components::ghost_button(
                                "ai-plan-only",
                                if plan_only {
                                    "Plan only \u{2713}"
                                } else {
                                    "Plan only"
                                },
                                theme,
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.state.update(cx, |state, _| {
                                        state.set_assistant_plan_only(!plan_only);
                                    });
                                    cx.notify();
                                }),
                            ),
                        )
                        .when(has_transcript && !running, |el| {
                            el.child(
                                crate::components::ghost_button("ai-clear", "Clear", theme)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            this.state.update(cx, |state, _| {
                                                state.clear_agent_transcript()
                                            });
                                            cx.notify();
                                        }),
                                    ),
                            )
                        }),
                ),
        )
        .child(
            // What the next prompt is pointed at. An agent whose scope is
            // invisible produces edits that look arbitrary.
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(gpui::SharedString::from(scope)),
        )
        // Slash commands are prompt templates in the agent directory that expand
        // client side. Without this line they are files with no discoverable
        // effect: nothing else in the app names them, and a template nobody can
        // find is a template nobody types.
        .when_some(slash, |el, hint| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child(gpui::SharedString::from(hint)),
            )
        })
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(Theme::SPACE_SM))
                .child(
                    crate::components::text_box(
                        "ai-prompt",
                        &prompt,
                        "Message Aria…",
                        focused,
                        theme,
                    )
                    .flex_1()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| state.focus_assistant_prompt(true));
                            cx.notify();
                        }),
                    ),
                )
                .child(if running {
                    crate::components::danger_button("ai-stop", "Stop", theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state
                                    .update(cx, |state, _| state.cancel_agent_prompt());
                                cx.notify();
                            }),
                        )
                        .into_any_element()
                } else {
                    crate::components::button("ai-send", "Send", theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state
                                    .update(cx, |state, cx| state.send_composer_prompt(cx));
                                cx.notify();
                            }),
                        )
                        .into_any_element()
                }),
        )
}
