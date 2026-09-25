//! The Provider keys section: one row per BYOK provider.

use gpui::{Entity, IntoElement};

use crate::app::AppState;
use crate::theme::Theme;

use super::widgets::kv;

/// One row per BYOK provider, whether or not it has a key.
///
/// The row that matters is the unset one, so the list comes from the provider table
/// and never from the config's map of configured entries.
pub(super) fn provider_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let mut rows: Vec<gpui::AnyElement> = state
        .read(cx)
        .provider_key_status()
        .into_iter()
        .map(|status| {
            let value = if status.configured {
                format!("{} — key from {}", status.purpose, status.source)
            } else {
                format!("{} — no key ({})", status.purpose, status.source)
            };
            kv(theme, status.name, value).into_any_element()
        })
        .collect();
    // Configured is not the same as reachable. A `api_key_env` naming an unset
    // variable makes the row above say "key from $VAR" and still leaves the
    // built provider keyless, and a keyless direct search answers with no
    // results rather than an error — which reads as "nothing found". This row
    // asks the provider that would actually be built.
    rows.push(kv(theme, "Stock search", state.read(cx).stock_search_reach()).into_any_element());
    rows
}
