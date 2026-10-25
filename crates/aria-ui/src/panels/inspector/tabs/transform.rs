//! Transform tab: position, scale, rotation, anchor, opacity, and the Parent
//! row.
//!
//! Every row comes from the property catalog, so the block keeps no list of the
//! properties it owns and every one of them is written the keyframe-aware way. A
//! hand-listed block wrote the whole transform on each nudge, which replaced an
//! animated property with the constant the pointer left it at, and it silently
//! lacked the anchor point the catalog has always carried.

use crate::components::Wraps as _;
use crate::tooltip::WithTip as _;
use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    Styled, div, px,
};

use aria_models::ClipId;

use crate::app::clip_parent::keep_for;
use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use crate::panels::inspector::blocks::widgets::{Columns, chips_under_labels, param_rows};

use crate::components::section_label;

pub fn transform_tab(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let rows = param_rows(theme, "transform", state, cx);
    let reset = components::icon_button("inspector-reset", crate::icons::ROTATE_CCW, theme)
        .debug_selector(|| "inspector-reset".into())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.reset_selected_transform() {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        );
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        // The block's reset ends its title's line, as an effect's ends its
        // header in the Effects tab, over the edge the rows' time areas end on.
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .pt(px(Theme::SPACE_SM))
                .pr(px(Theme::SPACE_XS))
                .child(section_label(theme, "Transform").pt_0())
                .child(reset),
        )
        .children(rows)
        .children(parent_row(theme, state, cx))
        .child(crate::panels::inspector::blocks::framing::framing_block(
            theme, clip, state, cx,
        ))
        .child(crate::panels::inspector::blocks::keyframes::keyframes_block(theme, clip, state, cx))
        .child(
            crate::panels::inspector::blocks::curve_editor::curve_editor_block(
                theme, clip, state, cx,
            ),
        )
}

/// One clip the Parent row draws.
struct ParentChip {
    clip: ClipId,
    name: SharedString,
    /// The clip's parent now.
    current: bool,
    /// The model accepts it as the parent. A parent the clip no longer overlaps,
    /// or one an edit took away, is still drawn so it reads as the parent, and
    /// None releases it.
    eligible: bool,
}

/// The Parent row: None, then every clip the model accepts as this clip's
/// parent, and the pick whip that takes the next clip pressed on the timeline.
/// A press keeps the clip where it is drawn at the playhead; Shift snaps it
/// onto the parent's anchor.
fn parent_row(
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<impl IntoElement> {
    let app = state.read(cx);
    let choices = app.selected_parent_choices()?;
    let current = choices.current;
    let name_of = |clip: ClipId| -> SharedString {
        app.ui_clip_ref(clip).map_or_else(
            || SharedString::new_static("Missing clip"),
            |ui| ui.name.clone(),
        )
    };
    let stale = current.filter(|clip| !choices.eligible.contains(clip));
    let chips: Vec<ParentChip> = stale
        .into_iter()
        .map(|clip| (clip, false))
        .chain(choices.eligible.iter().map(|clip| (*clip, true)))
        .map(|(clip, eligible)| ParentChip {
            clip,
            name: name_of(clip),
            current: current == Some(clip),
            eligible,
        })
        .collect();
    let whip_armed = app.parent_whip().is_some();

    let none = components::chip(
        "parent-none",
        "None",
        current.is_none(),
        ChipSize::Compact,
        theme,
    )
    .when(current.is_some(), |el| {
        el.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                pick_parent(this, Pick::ReleaseParent, cx);
            }),
        )
    });
    let chips: Vec<_> = chips
        .into_iter()
        .map(|chip| {
            let ParentChip {
                clip,
                name,
                current,
                eligible,
            } = chip;
            components::chip(
                MotionKey::new("parent-choice", clip.raw()),
                name,
                current,
                ChipSize::Compact,
                theme,
            )
            .when(eligible, |el| {
                el.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        // The parent again keeps the clip where it is; only a
                        // snap onto it is an edit.
                        if current && !event.modifiers.shift {
                            return;
                        }
                        let keep = keep_for(event.modifiers.shift);
                        pick_parent(this, Pick::Parent { parent: clip, keep }, cx);
                    }),
                )
            })
            .into_any_element()
        })
        .collect();
    let whip = components::ghost_button("parent-whip", "@", theme)
        .when(whip_armed, |el| el.bg(theme.accent).text_color(theme.text))
        .when_some(
            crate::tooltip::builder("inspector-parent-whip"),
            |el, tip| el.with_tip(tip),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.toggle_parent_whip());
                cx.notify();
            }),
        );
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .pt(px(Theme::SPACE_SM))
            .child(section_label(theme, "Parent"))
            .child(chips_under_labels(
                Columns::Keyed,
                ChipSize::Compact,
                [div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .wraps()
                    .items_center()
                    .gap(px(Theme::SPACE_XS))
                    .child(none)
                    .children(chips)
                    .child(whip)
                    .into_any_element()],
            )),
    )
}

fn pick_parent(
    this: &mut crate::shell::Shell,
    pick: Pick,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    this.state.update(cx, |state, _| {
        if let Err(err) = clip_pick::apply(state, pick) {
            state.report_error(&err);
        }
    });
    cx.notify();
}
