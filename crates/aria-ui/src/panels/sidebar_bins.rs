//! Pool folders: the breadcrumb, the folder rows, and the drops onto them.
//!
//! One folder is open at a time and the pool below shows what is in it, so this
//! strip is the only way through the tree. That is why the root crumb is always
//! drawn: a folder you cannot leave is worse than no folders.
//!
//! A row is a drop target for both payloads the pool can produce — a source and
//! another folder — because dragging is the gesture a file tree teaches. The
//! project refuses a folder dropped into its own subtree; this file does not
//! duplicate that rule, it only reports the refusal.

use gpui::{
    AppContext, CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::components;
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::surfaces::bin_strip::{BIN_ACTIONS, BinStrip};
use crate::surfaces::pool_drop::{self, Dropped};
use crate::theme::Theme;
use crate::types::{BinDrag, MediaDrag};

/// The folder strip: where you are, what is inside, and how to make more.
pub fn strip(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let (path, children, open, rename, strip) = {
        let read = state.read(cx);
        (
            read.bin_path(),
            read.child_bins(),
            read.current_bin,
            read.bin_rename.clone(),
            // Read once, for every button: two of them act on the open folder,
            // and a button that read the folder again on press could act on a
            // different one.
            BinStrip::of(read),
        )
    };

    let mut crumbs: Vec<gpui::AnyElement> = Vec::with_capacity(path.len() + 1);
    crumbs.push(crumb(theme, "Pool".into(), None, open.is_none(), cx));
    for item in path.iter() {
        crumbs.push(
            div()
                .text_size(px(10.))
                .text_color(theme.text_faint)
                .child("/")
                .into_any_element(),
        );
        crumbs.push(crumb(
            theme,
            item.name.clone(),
            Some(item.id),
            open == Some(item.id),
            cx,
        ));
    }

    let mut rows: Vec<gpui::AnyElement> = Vec::with_capacity(children.len());
    for (index, item) in children.iter().enumerate() {
        let editing = rename
            .as_ref()
            .filter(|active| active.bin == item.id)
            .map(|active| active.draft.clone());
        rows.push(row(theme, item.id, item.name.clone(), editing, index, cx));
    }

    let mut buttons: Vec<gpui::AnyElement> = Vec::with_capacity(BIN_ACTIONS.len());
    for action in BIN_ACTIONS {
        if !(action.when)(&strip) {
            continue;
        }
        buttons.push(
            components::ghost_button(action.id, action.label, theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = (action.run)(state, &strip) {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                )
                .into_any_element(),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .id("bin-crumbs")
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .children(crumbs),
        )
        .when(!rows.is_empty(), |el| {
            el.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_XXS))
                    .children(rows),
            )
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .children(buttons),
        )
        .into_any_element()
}

/// One step of the breadcrumb. Also a drop target, so a source can be moved up
/// the tree without opening the folder it is going to.
fn crumb(
    theme: &Theme,
    label: SharedString,
    bin: Option<u64>,
    current: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = match bin {
        Some(bin) => gpui::ElementId::named_usize("bin-crumb", bin as usize),
        None => "bin-crumb-root".into(),
    };
    let key = motion::MotionKey::new("sidebar.crumb", &id);
    let lit = motion::hover_t(key);
    let accent = theme.accent;
    div()
        .id(id)
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(px(11.))
        .text_color(motion::mix(
            if current {
                theme.text
            } else {
                theme.text_muted
            },
            theme.text,
            lit,
        ))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .on_hover(motion::hover_listener(key))
        .drag_over::<MediaDrag>(move |style, _, _, _| style.border_color(accent).border_1())
        .on_drop(cx.listener(move |this, drag: &MediaDrag, _, cx| {
            let media = drag.media;
            this.state.update(cx, |state, _| {
                if let Err(err) = pool_drop::apply(state, Dropped::MediaOnBin { media, bin }) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }))
        .drag_over::<BinDrag>(move |style, _, _, _| style.border_color(accent).border_1())
        .on_drop(cx.listener(move |this, drag: &BinDrag, _, cx| {
            let moved = drag.bin;
            this.state.update(cx, |state, _| {
                if let Err(err) = pool_drop::apply(
                    state,
                    Dropped::BinOnBin {
                        bin: moved,
                        parent: bin,
                    },
                ) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.open_bin(bin));
                cx.notify();
            }),
        )
        .child(label)
        .into_any_element()
}

/// A folder inside the open one: click to enter, drop onto it to file something
/// there, drag it to move it.
fn row(
    theme: &Theme,
    bin: u64,
    label: SharedString,
    editing: Option<String>,
    index: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = motion::MotionKey::new("sidebar.bin", bin);
    let lit = motion::hover_t(key);
    let accent = theme.accent;
    if let Some(draft) = editing {
        return div()
            .px(px(Theme::SPACE_XS))
            .child(components::text_box(
                "bin-name", &draft, "Bin name", true, theme,
            ))
            .into_any_element();
    }
    let card = div()
        .id(gpui::ElementId::named_usize("bin-row", bin as usize))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .bg(motion::hover_blend(key, theme.wash(0.0), theme.wash(0.06)))
        .on_hover(motion::hover_listener(key))
        .drag_over::<MediaDrag>(move |style, _, _, _| style.border_color(accent).border_1())
        .on_drop(cx.listener(move |this, drag: &MediaDrag, _, cx| {
            let media = drag.media;
            this.state.update(cx, |state, _| {
                if let Err(err) = pool_drop::apply(
                    state,
                    Dropped::MediaOnBin {
                        media,
                        bin: Some(bin),
                    },
                ) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }))
        .drag_over::<BinDrag>(move |style, _, _, _| style.border_color(accent).border_1())
        .on_drop(cx.listener(move |this, drag: &BinDrag, _, cx| {
            let moved = drag.bin;
            this.state.update(cx, |state, _| {
                if let Err(err) = pool_drop::apply(
                    state,
                    Dropped::BinOnBin {
                        bin: moved,
                        parent: Some(bin),
                    },
                ) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }))
        .on_drag(
            BinDrag {
                bin,
                label: label.clone(),
            },
            {
                move |drag, _offset, _window, cx| {
                    let label = drag.label.clone();
                    cx.new(|_| super::DragGhost { label })
                }
            },
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if event.click_count >= 2 {
                        state.begin_bin_rename(bin);
                    } else {
                        state.open_bin(Some(bin));
                    }
                });
                cx.notify();
            }),
        )
        // The mark leans the way the click goes as the row lights: with one
        // folder open at a time, "the one you are about to descend into" is the
        // only disclosure state this tree has, so the chevron stays shut and
        // travels instead of turning.
        .child(
            div()
                .relative()
                .left(px(gpui::motion::lerp(0.0, 3.0, lit)))
                .child(crate::components::chevron(
                    motion::MotionKey::new("bin-chev", bin),
                    false,
                    motion::mix(theme.text_faint, theme.accent, lit),
                    cx.reduce_motion(),
                )),
        )
        .child(div().text_size(px(12.)).text_color(theme.text).child(label));
    crate::pending::arriving(motion::MotionKey::new("bin-in", bin), index, card)
}
