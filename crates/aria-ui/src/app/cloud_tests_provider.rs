//! Providers and keys: 5 of the cases in [`super`].
//!
//! Split out of `cloud_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

#[test]
fn a_preset_with_a_nonsense_font_size_keeps_the_current_one() {
    let mut state = crate::app::text_tests::with_title();
    let before = {
        let clip = state.selected_clip().expect("a title");
        let engine = state.engine().lock();
        let found = engine.project().clip(clip).expect("the clip");
        match &found.content {
            aria_models::ClipSource::Generated(aria_models::Generator::Text { style, .. }) => {
                style.size.constant().expect("a constant size")
            }
            _ => panic!("not a title"),
        }
    };
    let preset = aria_cloud::dto::TextPreset {
        id: "b".into(),
        name: "Broken".into(),
        category: String::new(),
        font_family: String::new(),
        font_size: 0.0,
        fill: [1, 2, 3, 4],
        animation_in: None,
        animation_out: None,
        animation_combo: None,
        sample_text: String::new(),
    };
    state.apply_text_preset(&preset).expect("applies");

    let clip = state.selected_clip().expect("a title");
    let engine = state.engine().lock();
    let found = engine.project().clip(clip).expect("the clip");
    let aria_models::ClipSource::Generated(aria_models::Generator::Text { style, .. }) =
        &found.content
    else {
        panic!("not a title")
    };
    assert_eq!(
        style.size.constant(),
        Some(before),
        "a zero font size was written through"
    );
}
#[test]
fn every_provider_is_listed_even_with_no_key() {
    // The row that matters is the unset one. Enumerating the config's map of
    // *configured* providers would hide exactly the case the operator is looking for.
    let (state, _) = with_clip();
    let rows = state.provider_key_status();
    assert_eq!(rows.len(), crate::cloud::BYOK_PROVIDERS.len());
    for row in &rows {
        assert!(!row.configured, "a fresh session reported a key");
        assert_eq!(row.source, "not set");
        assert!(
            !row.purpose.trim().is_empty(),
            "{} has no purpose",
            row.name
        );
    }

    // Every provider that can route stock search must be a listed provider, or a key
    // that changes routing would never appear in Settings.
    for name in STOCK_PROVIDERS {
        assert!(
            rows.iter().any(|row| row.name == name),
            "{name} routes stock search but is not listed"
        );
    }
}
#[test]
fn a_key_naming_an_unset_environment_variable_is_not_a_key() {
    // `is_configured` is true for a *named* variable; only `resolve_key` knows whether
    // it holds anything. Reporting "configured" for an unset variable is how a call
    // fails with no explanation.
    let named = ProviderSettings {
        api_key: None,
        api_key_env: Some("ARIA_TEST_PROVIDER_KEY_THAT_IS_NOT_SET".into()),
    };
    assert!(named.is_configured(), "the setting names a variable");
    assert!(
        named.resolve_key().is_none(),
        "an unset variable must not resolve to a key"
    );
}
#[test]
fn a_download_in_flight_holds_off_cache_maintenance() {
    // A Clear or a Move landing mid-download deletes or moves the partial file the
    // download is still writing, and the import that follows references a path that is
    // gone. The guard belongs to the work that would be corrupted, so it is held for
    // the download and released whichever way the download ends.
    let (mut state, _) = with_clip();
    assert!(!state.download_cache_busy(), "idle by default");
    state.clear_download_cache().expect("an idle cache clears");

    state.hold_download_guard();
    assert!(state.download_cache_busy());
    let err = state
        .clear_download_cache()
        .expect_err("a clear during a download must be refused");
    assert!(
        err.contains("protected"),
        "the refusal did not explain itself: {err}"
    );
    let elsewhere = state.download_cache_root().join("elsewhere");
    let err = state
        .relocate_storage_root(&elsewhere)
        .expect_err("a move during a download must be refused");
    assert!(!err.is_empty());

    state.release_download_guard();
    assert!(!state.download_cache_busy(), "the guard was not released");
    state
        .clear_download_cache()
        .expect("maintenance resumes once the download ends");
}
#[test]
fn the_help_menu_offers_the_update_check() {
    // The check is reachable from the bar, not only from a palette string.
    let labels = crate::chrome::labels_for(crate::types::AppMenu::Help, &with_clip().0);
    assert!(
        labels.contains(&"Check for Updates"),
        "Help lost the update check: {labels:?}"
    );
}
