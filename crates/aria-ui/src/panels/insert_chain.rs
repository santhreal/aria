//! An audio insert chain, as drawn in the inspector's Audio block and on a
//! mixer strip.
//!
//! Both places draw the same projected rows and press the same gestures through
//! [`insert_rack`]. Each insert is a header (its bypass switch, its name, the
//! chevrons that move it, and a remove button) with its parameters as steppers under
//! it: under every insert in the inspector, and under the open one on a strip.
//! An insert whose effect reads a key lists the lanes that may key it.

use crate::components::Wraps as _;
use crate::tooltip::WithTip as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Div, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Stateful, Styled, div, px,
};

use aria_models::{AudioInsertTarget, TrackId};

use crate::app::AppState;
use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::panels::inspector::blocks::widgets::{Columns, Readout, under_labels, unkeyed_row};
use crate::surfaces::insert_rack::{self, Gesture};
use crate::theme::Theme;
use crate::types::{UiInsert, UiInsertParam, UiKeyChoice};

/// One chain to draw.
#[derive(Debug)]
pub struct Chain<'a> {
    pub target: AudioInsertTarget,
    pub inserts: &'a [UiInsert],
    /// The lanes that may key an insert on this chain.
    pub keys: &'a [UiKeyChoice],
    /// Whether every insert's parameters are drawn, or only the open one's.
    pub expanded: bool,
    /// The columns the parameter rows stand in: the keyed rows' on an Effect
    /// Controls tab, free on a mixer strip.
    pub columns: Columns,
}

/// The chain's rows, then its Add button and, when open, its Add list.
pub fn render(
    theme: &Theme,
    chain: Chain<'_>,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let (picker, open) = {
        let read = state.read(cx);
        (read.insert_picker, read.open_insert)
    };
    let target = chain.target;
    let columns = chain.columns;
    let base = chain_key(target);
    let mut rows: Vec<gpui::AnyElement> = Vec::with_capacity(chain.inserts.len() + 2);
    for (index, insert) in chain.inserts.iter().enumerate() {
        let at = Place {
            target,
            index,
            key: base.with(index),
            last: index + 1 == chain.inserts.len(),
            expanded: chain.expanded,
        };
        rows.push(header(theme, at, insert, cx));
        if !chain.expanded && open != Some((target, index)) {
            continue;
        }
        if !insert.installed {
            rows.push(note(
                theme,
                "Not installed. Its stored values are kept and the sound passes through.",
            ));
            continue;
        }
        for (slot, param) in insert.params.iter().enumerate() {
            rows.push(
                unkeyed_row(
                    theme,
                    state,
                    at.key.with("param").with(slot),
                    param.label.clone(),
                    Readout::printed(reading(param), param.value, digits(param), unit(param)),
                    columns,
                    move |state, by| {
                        insert_rack::apply(
                            state,
                            Gesture::Nudge {
                                target,
                                index,
                                slot,
                                by,
                            },
                        )
                    },
                    cx,
                )
                .into_any_element(),
            );
        }
        // A clip insert takes no key: the model keys a lane or the master.
        if insert.reads_key && target.clip().is_none() {
            rows.push(keys(theme, at, insert.key, chain.keys, cx));
        }
    }
    let full = target
        .limit()
        .is_some_and(|limit| chain.inserts.len() >= limit);
    if full {
        rows.push(note(theme, "Every insert slot is in use."));
    } else {
        rows.push(
            under_labels(
                columns,
                [pressed(
                    components::ghost_button(
                        ("insert-add", base.with("add")),
                        "+ Audio effect",
                        theme,
                    ),
                    move || Gesture::TogglePicker(target),
                    cx,
                )
                .into_any_element()],
            )
            .into_any_element(),
        );
        if picker == Some(target) {
            rows.push(add_list(theme, target, base, columns, cx));
        }
    }
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .children(rows)
        .into_any_element()
}

/// Where one insert row sits.
#[derive(Clone, Copy)]
struct Place {
    target: AudioInsertTarget,
    index: usize,
    key: MotionKey,
    last: bool,
    expanded: bool,
}

/// The chain's identity, folded from the clip or lane it sits on.
fn chain_key(target: AudioInsertTarget) -> MotionKey {
    match target {
        AudioInsertTarget::Clip(clip) => MotionKey::new("insert-clip", clip.raw()),
        AudioInsertTarget::Track(track) => MotionKey::new("insert-lane", track.raw()),
        AudioInsertTarget::Master => MotionKey::new("insert-master", 0),
    }
}

/// The insert's bypass switch, name, move-earlier and move-later controls, and
/// remove button. On a strip the name opens and closes the parameters.
fn header(
    theme: &Theme,
    at: Place,
    insert: &UiInsert,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let Place {
        target,
        index,
        key,
        last,
        expanded,
    } = at;
    let name = div()
        .id(key.element_id("insert-name"))
        .text_size(Theme::TEXT_TITLE)
        .text_color(if insert.bypass {
            theme.text_faint
        } else {
            theme.text
        })
        .child(insert.label.clone());
    let name = if expanded {
        name
    } else {
        pressed(name, move || Gesture::Open { target, index }, cx)
    };
    let bypass = pressed(
        components::switch(
            ("insert-bypass", key.with("bypass")),
            Some(!insert.bypass),
            theme,
        )
        .when_some(crate::tooltip::builder("insert-bypass"), |el, tip| {
            el.with_tip(tip)
        }),
        move || Gesture::Bypass { target, index },
        cx,
    );
    // An insert that cannot move one way keeps the room of that control, so
    // the controls after it stand in the columns the other rows draw them in.
    let up = if index > 0 {
        pressed(
            components::icon_button(
                ("insert-up", key.with("up")),
                crate::icons::CHEVRON_UP,
                theme,
            )
            .debug_selector(move || format!("insert-up-{index}")),
            move || Gesture::MoveUp { target, index },
            cx,
        )
        .into_any_element()
    } else {
        components::icon_space().into_any_element()
    };
    let down = if last {
        components::icon_space().into_any_element()
    } else {
        pressed(
            components::icon_button(
                ("insert-down", key.with("down")),
                crate::icons::CHEVRON_DOWN,
                theme,
            )
            .debug_selector(move || format!("insert-down-{index}")),
            move || Gesture::MoveDown { target, index },
            cx,
        )
        .into_any_element()
    };
    let remove = pressed(
        components::icon_button(("insert-remove", key), crate::icons::TRASH, theme),
        move || Gesture::Remove { target, index },
        cx,
    );
    div()
        .flex()
        .flex_row()
        .wraps()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_XXS))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.wash(0.04))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .min_w_0()
                .child(bypass)
                .child(name),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XXS))
                .child(up)
                .child(down)
                .child(remove),
        )
        .into_any_element()
}

/// The lanes that may key this insert, and a chip for no key.
fn keys(
    theme: &Theme,
    at: Place,
    current: Option<u64>,
    choices: &[UiKeyChoice],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let Place {
        target, index, key, ..
    } = at;
    let mut chips: Vec<gpui::AnyElement> = Vec::with_capacity(choices.len() + 1);
    chips.push(
        pressed(
            components::chip(
                ("insert-key-none", key.with("key-none")),
                "No key",
                current.is_none(),
                ChipSize::Compact,
                theme,
            ),
            move || Gesture::Key {
                target,
                index,
                key: None,
            },
            cx,
        )
        .into_any_element(),
    );
    for choice in choices {
        let lane = TrackId::from_raw(choice.track);
        chips.push(
            pressed(
                components::chip(
                    ("insert-key", key.with("key").with(choice.track)),
                    choice.name.clone(),
                    current == Some(choice.track),
                    ChipSize::Compact,
                    theme,
                ),
                move || Gesture::Key {
                    target,
                    index,
                    key: Some(lane),
                },
                cx,
            )
            .into_any_element(),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .px(px(Theme::SPACE_XS))
        .child(
            div()
                .text_size(Theme::TEXT_SMALL)
                .text_color(theme.text_muted)
                .child("Key from"),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .wraps()
                .gap(px(Theme::SPACE_XXS))
                .children(chips),
        )
        .into_any_element()
}

/// Every audio effect in the catalog, shipped and registered, as a chip that
/// appends it to the chain, starting where the labels of `columns` start.
fn add_list(
    theme: &Theme,
    target: AudioInsertTarget,
    base: MotionKey,
    columns: Columns,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let catalog = crate::catalog_text::audio_effects();
    let chips: Vec<gpui::AnyElement> = catalog
        .iter()
        .map(|choice| {
            let effect = choice.id.clone();
            pressed(
                components::chip(
                    ("insert-add-effect", base.with("add").with(&choice.id)),
                    choice.label.clone(),
                    false,
                    ChipSize::Compact,
                    theme,
                ),
                move || Gesture::Add {
                    target,
                    effect_id: effect.to_string(),
                },
                cx,
            )
            .into_any_element()
        })
        .collect();
    under_labels(columns, chips).into_any_element()
}

fn note(theme: &Theme, text: &'static str) -> gpui::AnyElement {
    div()
        .px(px(Theme::SPACE_XS))
        .text_size(Theme::TEXT_SMALL)
        .text_color(theme.text_faint)
        .child(text)
        .into_any_element()
}

/// Digits a parameter prints: fewer the wider its range.
fn digits(param: &UiInsertParam) -> u8 {
    let span = param.max - param.min;
    if span >= 1000.0 {
        0
    } else if span >= 10.0 {
        1
    } else {
        2
    }
}

/// A parameter's value at the chain's first tick, marked when it is keyed.
fn reading(param: &UiInsertParam) -> String {
    let digits = usize::from(digits(param));
    if param.automated {
        format!("{:.digits$} keyed", param.value)
    } else {
        format!("{:.digits$}", param.value)
    }
}

/// One unit of drag: a hundredth of the parameter's range.
fn unit(param: &UiInsertParam) -> f32 {
    ((param.max - param.min) / 100.0).max(0.01)
}

/// `el`, with a left press that applies the gesture `gesture` builds.
fn pressed(
    el: Stateful<Div>,
    gesture: impl Fn() -> Gesture + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Stateful<Div> {
    el.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                if let Err(err) = insert_rack::apply(state, gesture()) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }),
    )
}
