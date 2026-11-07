//! Cloud provider and key configuration tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `cloud_tests.rs` through `use super::*`.

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
    // *configured* providers would hide the unconfigured case this row is for.
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
/// WHY: closes the class of a Provider keys row whose key no feature reads.
/// An ElevenLabs row stated voice generation while the Voice kind of the
/// Generate panel sent its request with the fal.ai key, so a key entered
/// there changed nothing. Every listed provider must be a stock search
/// provider or the generation provider, the two readers of a key, so a row
/// added for a provider nothing reads is red here.
///
/// What this does not catch: a reader that stops reading a key it is still
/// listed under in `STOCK_PROVIDERS` or `GENERATE_PROVIDER`.
#[test]
fn every_listed_provider_key_has_a_reader() {
    let readers: Vec<&str> = STOCK_PROVIDERS
        .into_iter()
        .chain([crate::app::generate::GENERATE_PROVIDER])
        .collect();
    let unread: Vec<&str> = crate::cloud::BYOK_PROVIDERS
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !readers.contains(name))
        .collect();
    assert!(unread.is_empty(), "no feature reads the key of {unread:?}");
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
/// A Clear or a Move landing mid-download deletes or moves the partial file the
/// download is still writing, so both are refused while the download holds the
/// cache, with one reason that names the download, and Move storage… refuses
/// before it opens a folder picker. The refusals read "the project inventory
/// is scanned" and "protected until project media inventory completes",
/// `aria-cloud`'s wording for its other holder, and Move asked for a folder
/// first. The drawn half is in the Settings value_row tests.
#[test]
fn a_download_in_flight_holds_off_cache_maintenance() {
    let (mut state, _) = with_clip();
    assert_eq!(state.download_cache_held(), None, "idle by default");
    state.clear_download_cache().expect("an idle cache clears");

    state.hold_download_guard();
    let held = state
        .download_cache_held()
        .expect("a download holds the cache");
    assert_eq!(
        held,
        "A download is in flight. Clear and Move run once it finishes"
    );
    let err = state
        .clear_download_cache()
        .expect_err("a clear during a download must be refused");
    assert_eq!(err, held);
    let elsewhere = state.download_cache_root().join("elsewhere");
    let err = state
        .relocate_storage_root(&elsewhere)
        .expect_err("a move during a download must be refused");
    assert_eq!(err, held);
    let asked = state.picks_asked();
    state.pick_storage_root();
    assert_eq!(
        state.picks_asked(),
        asked,
        "Move storage… asked for a folder"
    );
    assert_eq!(state.last_error.as_deref(), Some(held));

    state.release_download_guard();
    assert_eq!(
        state.download_cache_held(),
        None,
        "the guard was not released"
    );
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
