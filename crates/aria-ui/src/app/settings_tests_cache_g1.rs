//! Split from `settings_tests_cache.rs`: whole `#[test]` functions, moved verbatim.
//! `use super::*` reaches the parent's private helpers and tables.

use super::*;

/// A configured override the layout refuses reads as refused, not as a decoration
/// on the default path.
///
/// The row used to take `overridden` from the settings table and `path` from the
/// layout, so an override the layout dropped — the case `storage_layout`
/// tolerates so the other caches keep working — showed "(override)" beside the
/// default directory with no way to learn nothing had moved.
#[test]
fn a_refused_override_reads_as_refused() {
    let (mut state, _) = with_clip();
    let shared = state
        .storage_layout()
        .resolve(CacheId::Luts)
        .expect("luts is a disk cache");
    let lottie = |state: &AppState| {
        state
            .cache_rows()
            .into_iter()
            .find(|row| row.key == "lottie")
            .expect("the lottie row")
    };

    // Straight into the config: `set_cache_override` validates, and the refusal
    // under test is the one that reaches `cache_rows` from a file already saved.
    let path = state.config_path();
    let mut settings = aria_settings::load(&path).expect("load the config");
    *settings.storage.paths.slot("lottie").expect("overridable") = Some(shared.clone());
    aria_settings::save(&path, &settings).expect("save the config");

    let row = lottie(&state);
    assert!(!row.overridden, "a refused override reads as in force");
    let reason = row.override_error.as_deref().expect("no refusal reason");
    assert!(
        reason.contains(&shared.display().to_string()),
        "the reason does not name the refused path: {reason}"
    );
    assert_ne!(
        row.path.as_ref(),
        Some(&shared),
        "the refused path is serving anyway"
    );
    assert!(
        state
            .storage_report()
            .contains("refused override for lottie"),
        "the support report hides the refusal"
    );

    // Clearing is offered for a refusal and drops it from the config.
    state.clear_cache_override("lottie").expect("clear it");
    assert!(
        lottie(&state).override_error.is_none(),
        "the refusal survived a clear"
    );
}

/// A value the config loader refused reaches the operator.
///
/// The class is any dropped value, not the storage case that found it: the
/// loader records what it stepped over and this reader prints the whole list,
/// so a key added to the config cannot be droppable and unreportable. The
/// session still starts on the defaults, which is the other half of the rule —
/// a hand edit says something wrong, it does not stop the editor opening.
#[test]
fn a_value_the_config_loader_refused_reaches_the_report() {
    let (state, _) = with_clip();
    let path = state.config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the config directory");
    }
    std::fs::write(
        &path,
        "[storage]\nroot = \"cache\"\n\n[appearance]\ntheme = \"purple\"\n",
    )
    .expect("write a hand-edited config");

    let refusals = state.config_refusals();
    assert_eq!(refusals.len(), 2, "{refusals:?}");
    // In the loader's own read order: `[appearance]` is read before
    // `[storage]`, and the list is a record of what it met, not a sorted set.
    assert!(
        refusals[0].starts_with("appearance.theme = \"purple\": "),
        "{refusals:?}"
    );
    assert!(
        refusals[1].starts_with("storage.root = \"cache\": "),
        "{refusals:?}"
    );

    let root = state.storage_layout().root().to_path_buf();
    assert!(root.is_absolute(), "{}", root.display());
    assert!(!root.ends_with("cache"), "{}", root.display());

    let report = state.storage_report();
    for refusal in &refusals {
        assert!(
            report.contains(refusal.as_str()),
            "the report hides {refusal}: {report}"
        );
    }
}

/// The report is a copy an operator has to ask for; the Caches section is the
/// surface they are already looking at when a cache is not where the config
/// says. A refusal that reached one and not the other is the state this started
/// in, so the panel's own source is read rather than trusted.
#[test]
fn the_caches_section_draws_the_config_refusals() {
    const PANEL: &str = include_str!("../panels/settings_dialog/caches.rs");
    assert!(
        PANEL.contains("config_refusals()"),
        "the Caches section no longer draws the lines the config loader refused"
    );
}
