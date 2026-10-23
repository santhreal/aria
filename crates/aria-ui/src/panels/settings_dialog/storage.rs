//! The Storage section: the download cache, the registered caches, and their paths.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::picker::{Ask, FileSpec};
use crate::components;
use crate::theme::Theme;

use super::widgets::kv;

/// Cache size against quota, the quota steppers, Clear, and Relocate.
///
/// The size is read once per render rather than polled: it walks the cache tree, so
/// a per-frame reading would be a disk scan per frame.
pub(super) fn storage_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let read = state.read(cx);
    let root = read.download_cache_root().display().to_string();
    let used = crate::app::storage::format_bytes(read.download_cache_size_bytes());
    let quota = read.download_quota_mib();
    let protected = read.protected_download_count();
    let busy = read.download_cache_busy();
    let analysis_cache = format!(
        "{} at {}",
        crate::app::storage::format_bytes(read.analysis_cache_size_bytes()),
        read.analysis_cache_root().display()
    );

    let mut rows = vec![
        kv(theme, "Storage root", root).into_any_element(),
        kv(theme, "Download cache", format!("{used} of {quota} MiB")).into_any_element(),
    ];
    if protected > 0 {
        rows.push(
            kv(
                theme,
                "In use",
                format!(
                    "{protected} file{} pinned",
                    crate::app::project_reach::plural(protected),
                ),
            )
            .into_any_element(),
        );
    }
    rows.push(quota_row(theme, quota, cx).into_any_element());
    rows.push(
        div()
            .flex()
            .flex_row()
            .gap(px(Theme::SPACE_SM))
            .child(
                components::ghost_button("storage-clear", "Clear cache", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = state.clear_download_cache() {
                                state.report_error(err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            )
            .child(
                components::ghost_button("storage-relocate", "Move storage…", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            state.request_pick(
                                Ask::Folder(FileSpec::new("Choose a storage root")),
                                |state, answer| {
                                    let Some(picked) = answer.one() else {
                                        return;
                                    };
                                    if let Err(err) = state.relocate_storage_root(&picked) {
                                        state.report_error(err);
                                    }
                                },
                            );
                        });
                        cx.notify();
                    }),
                ),
            )
            .into_any_element(),
    );
    rows.push(
        kv(
            theme,
            "Maintenance",
            if busy {
                "held — a download is in flight"
            } else {
                "available"
            },
        )
        .into_any_element(),
    );
    rows.push(kv(theme, "Analysis cache", analysis_cache).into_any_element());
    rows.push(
        components::ghost_button("analysis-cache-clear", "Clear analysis", theme)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = state.clear_analysis_cache() {
                            state.report_error(err);
                        }
                    });
                    cx.notify();
                }),
            )
            .into_any_element(),
    );
    rows
}

/// The quota steppers. A quota is a number set in GiB, so the
/// step is 512 MiB rather than one.
fn quota_row(
    theme: &Theme,
    quota: u64,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    const STEP: u64 = 512;
    let dec = components::ghost_button("quota-dec", "−512 MiB", theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                let next = state.download_quota_mib().saturating_sub(STEP);
                if let Err(err) = state.set_download_quota_mib(next) {
                    state.report_error(err);
                }
            });
            cx.notify();
        }),
    );
    let inc = components::ghost_button("quota-inc", "+512 MiB", theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                let next = state.download_quota_mib().saturating_add(STEP);
                if let Err(err) = state.set_download_quota_mib(next) {
                    state.report_error(err);
                }
            });
            cx.notify();
        }),
    );
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .flex_1()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .child(SharedString::from(format!("Quota {quota} MiB"))),
        )
        .child(dec)
        .child(inc)
}
