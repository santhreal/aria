//! The Storage section: the download cache, the registered caches, and their paths.

use aria_core::count::counted;
use gpui::{Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent};

use crate::app::AppState;
use crate::components;
use crate::theme::Theme;

use super::value_row::value_row;
use super::widgets::kv;

/// The storage root with Move storage…, the download cache's size against its
/// quota with Clear cache, the quota steppers, and the analysis cache with
/// Clear analysis: each action at the end of the row it acts on.
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

    let relocate = components::ghost_button("storage-relocate", "Move storage…", theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.pick_storage_root());
                cx.notify();
            }),
        );
    let clear = components::ghost_button("storage-clear", "Clear cache", theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                if let Err(err) = state.clear_download_cache() {
                    state.report_error(err);
                }
            });
            cx.notify();
        }),
    );
    let mut rows = vec![
        value_row(theme, "Storage root", root, [relocate.into_any_element()]),
        value_row(
            theme,
            "Download cache",
            format!("{used} of {quota} MiB"),
            [clear.into_any_element()],
        ),
    ];
    if protected > 0 {
        rows.push(
            kv(
                theme,
                "In use",
                format!("{} pinned", counted(protected, "file", "files")),
            )
            .into_any_element(),
        );
    }
    rows.push(quota_row(theme, quota, cx));
    rows.push(
        kv(
            theme,
            "Maintenance",
            if busy {
                "Held; a download is in flight"
            } else {
                "Available"
            },
        )
        .into_any_element(),
    );
    let clear_analysis = components::ghost_button("analysis-cache-clear", "Clear analysis", theme)
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
        );
    rows.push(value_row(
        theme,
        "Analysis cache",
        analysis_cache,
        [clear_analysis.into_any_element()],
    ));
    rows
}

/// The quota and its steppers. A quota is a number set in GiB, so the
/// step is 512 MiB rather than one.
fn quota_row(
    theme: &Theme,
    quota: u64,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
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
    value_row(
        theme,
        "Quota",
        format!("{quota} MiB"),
        [dec.into_any_element(), inc.into_any_element()],
    )
}
