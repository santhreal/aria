//! Contracts for the cache registry surface and the editable settings.
//!
//! The class these close is "a backend crate owns a whole model and the UI shows one
//! read-only line of it". `aria-storage` had twelve registered caches, tiers, resolved
//! paths, a measured walk, a clear, and a transactional relocation; `aria-settings` had
//! an AI table, a theme choice, and a cloud endpoint. Between them the dialog exposed
//! one cache and no editable field.
//!
//! So what is asserted is the surface's agreement with the crates that own the model:
//!
//! - Rows are enumerated from `CacheId::ALL` and the settings enums' own `ALL`, at run
//!   time. Adding a cache, a source, a protocol, an autonomy level, a reasoning mode, or
//!   a theme without giving it a row or a key fails here.
//! - A path override is validated before it is persisted, and reset restores the
//!   default. A config that names a path the data never reached is the failure being
//!   prevented.
//! - Every typed field round-trips through the real file, and a refused value leaves the
//!   previous one intact. Nothing here opens a socket: `ai_status` reads structure only.

use super::AppState;
use super::premiere_tests::with_clip;
use super::settings_form::SettingsField;
use aria_settings::{
    AiApiProtocol, AiSource, Autonomy, ReasoningSummary, StoragePathOverrides, ThemeChoice,
};
use aria_storage::{CacheId, CacheKind};
use std::path::PathBuf;
/// Type `text` into the focused field.
fn type_text(state: &mut AppState, text: &str) {
    for ch in text.chars() {
        state.setting_push(ch);
    }
}
/// Focus a field and replace what is there.
///
/// Focusing seeds the draft from the saved value — the operator is usually correcting
/// one — so a test that wants a fresh value clears it the way a person would.
fn retype(state: &mut AppState, field: SettingsField, text: &str) {
    state.focus_setting(Some(field));
    while !state.setting_draft(field).0.is_empty() {
        state.setting_pop();
    }
    type_text(state, text);
}

// The cases live in sibling files, declared here so the fixtures above stay in
// one place. Only `#[test]` fns moved.
#[path = "settings_tests_cache.rs"]
mod settings_tests_cache;
#[path = "settings_tests_settings.rs"]
mod settings_tests_settings;
