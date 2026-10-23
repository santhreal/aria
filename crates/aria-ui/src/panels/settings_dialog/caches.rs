//! The Caches section: one row per registered cache, with its tier, its resolved
//! path, and the actions that empty, move, or redirect it.
//!
//! The list is `AppState::cache_rows`, which comes from `aria-storage`'s registry, so a
//! cache added there shows up here. Memory caches are listed too — they cannot be
//! cleared from disk, and saying so is the point: it answers where the bytes are.
//!
//! Usage is blank until **Measure** is pressed. Measuring walks every cache tree, and
//! doing that while the dialog renders would turn opening Settings into a disk scan.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::AppState;
use crate::app::caches::UiCache;
use crate::components;
use crate::theme::Theme;

use super::widgets::kv;

/// One row's whole story on a line: what it is, what losing it costs, where it
/// lives, and why it is not living where the config says.
fn describe(row: &UiCache) -> String {
    let mut parts = vec![
        row.kind.to_string(),
        row.tier.to_string(),
        row.eviction_order.to_string(),
    ];
    if let Some((bytes, files)) = row.usage {
        parts.push(format!(
            "{} in {files} file{}",
            crate::app::storage::format_bytes(bytes),
            crate::app::project_reach::plural(files as usize),
        ));
    }
    match &row.path {
        Some(path) if row.overridden => parts.push(format!("{} (override)", path.display())),
        Some(path) => parts.push(path.display().to_string()),
        None => {}
    }
    // A configured override the layout refused is why this row shows the
    // default path. Saying so here is the difference between "the move did not
    // work" and a disk that quietly keeps filling under the storage root.
    if let Some(reason) = &row.override_error {
        parts.push(format!("override refused — {reason}"));
    }
    parts.join(" — ")
}

pub(super) fn cache_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let rows = state.read(cx).cache_rows();
    let measured = state.read(cx).caches.measured;
    let mut elements: Vec<gpui::AnyElement> = vec![
        div()
            .flex()
            .flex_row()
            .gap(px(Theme::SPACE_SM))
            .child(
                components::ghost_button("caches-measure", "Measure", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = state.measure_caches() {
                                state.report_error(err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            )
            .child(
                components::ghost_button("caches-copy-paths", "Copy paths", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        let report = this.state.read(cx).storage_report();
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(report));
                        this.state.update(cx, |state, _| {
                            state.status_message = Some("Storage paths copied".into());
                        });
                        cx.notify();
                    }),
                ),
            )
            .into_any_element(),
    ];
    if !measured {
        elements.push(
            kv(
                theme,
                "Usage",
                "not measured — Measure walks every cache directory",
            )
            .into_any_element(),
        );
    }
    // A line of the config the loader could not use. It sits above the caches
    // because it explains why one of them is not where the file says: a
    // relative `[storage].root` is refused, and without this the only
    // evidence is a directory that never fills.
    for refusal in state.read(cx).config_refusals() {
        elements.push(kv(theme, "Config refused", refusal).into_any_element());
    }
    for row in rows {
        let key = row.key;
        elements.push(kv(theme, row.label, describe(&row)).into_any_element());
        if !row.on_disk() {
            continue;
        }
        let mut actions = div().flex().flex_row().gap(px(Theme::SPACE_SM));
        actions = actions.child(
            components::ghost_button(
                crate::motion::MotionKey::new("cache-clear", key),
                "Clear",
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = state.clear_registered_cache(key) {
                            state.report_error(err);
                        }
                    });
                    cx.notify();
                }),
            ),
        );
        actions = actions.child(
            components::ghost_button(
                crate::motion::MotionKey::new("cache-move", key),
                "Move…",
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.pick_cache_move(key));
                    cx.notify();
                }),
            ),
        );
        actions = actions.child(
            components::ghost_button(
                crate::motion::MotionKey::new("cache-redirect", key),
                "Redirect…",
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, _| state.pick_cache_redirect(key));
                    cx.notify();
                }),
            ),
        );
        // A refused override is exactly the case that needs the button: the
        // config names a path the layout will not take, and clearing it is the
        // fix. Gating on `overridden` alone hid the button on the only row that
        // was broken.
        if row.overridden || row.override_error.is_some() {
            actions = actions.child(
                components::ghost_button(
                    crate::motion::MotionKey::new("cache-reset", key),
                    "Use default",
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = state.clear_cache_override(key) {
                                state.report_error(err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            );
        }
        elements.push(actions.into_any_element());
    }
    elements
}
