//! Cache: 9 of the cases in [`super`].
//!
//! Split out of `settings_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

#[path = "settings_tests_cache_g1.rs"]
mod settings_tests_cache_g1;

use super::*;

/// Every registered cache reaches the panel with its kind, tier, and eviction order, and
/// every disk cache resolves to a real directory under the storage root.
///
/// Enumerated from the registry rather than from a list here, so a cache added to
/// `aria-storage` with no row fails this test instead of shipping invisible.
#[test]
fn every_registered_cache_has_a_row_that_describes_it() {
    let (state, _) = with_clip();
    let rows = state.cache_rows();
    assert_eq!(rows.len(), CacheId::ALL.len(), "a cache has no row");

    let mut keys: Vec<&str> = rows.iter().map(|row| row.key).collect();
    keys.sort_unstable();
    let unique = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), unique, "two caches share a key");

    let root = state.storage_layout().root().to_path_buf();
    for (row, id) in rows.iter().zip(CacheId::ALL) {
        assert_eq!(row.key, id.as_str(), "rows are not in registry order");
        assert!(!row.tier.is_empty(), "{} has no tier label", row.key);
        assert!(
            !row.eviction_order.is_empty(),
            "{} has no eviction phrase",
            row.key
        );
        assert!(
            row.override_error.is_none(),
            "a fresh session refused an override it never set: {:?}",
            row.override_error
        );
        match id.descriptor().kind {
            CacheKind::Memory => {
                assert_eq!(row.kind, "Memory");
                assert!(!row.on_disk(), "{} claims a directory", row.key);
            }
            CacheKind::Disk => {
                assert_eq!(row.kind, "Disk");
                let path = row.path.as_ref().expect("a disk cache resolves");
                assert!(
                    path.starts_with(&root),
                    "{} resolves outside the storage root: {}",
                    row.key,
                    path.display()
                );
                assert!(!row.overridden, "a fresh session has an override");
            }
        }
        assert!(
            row.usage.is_none(),
            "usage was reported before a measurement"
        );
    }

    // Disposable data is evicted before downloadable data, which is evicted before
    // anything the operator owns. The order is what makes the label useful.
    let disposable = rows
        .iter()
        .find(|row| row.key == "proxies")
        .expect("proxies is registered");
    let download = rows
        .iter()
        .find(|row| row.key == "download")
        .expect("download is registered");
    assert!(
        disposable.evicted <= download.evicted,
        "regenerable data is not evicted before downloadable data"
    );

    // Ranks are a dense order over the tiers the registry actually uses, not a
    // fixed 0..=2: a tier added to `aria-storage` must take the next rank and a
    // phrase, and this fails if one is given a gap or a duplicate rank.
    let mut ranks: Vec<u8> = rows.iter().map(|row| row.evicted).collect();
    ranks.sort_unstable();
    ranks.dedup();
    let expected: Vec<u8> = (0..ranks.len() as u8).collect();
    assert_eq!(ranks, expected, "eviction ranks are not a dense order");

    // Rank and phrase come out of one row, so a rank always means one phrase.
    let mut phrases: std::collections::BTreeMap<u8, &str> = std::collections::BTreeMap::new();
    for row in &rows {
        let previous = phrases.insert(row.evicted, row.eviction_order);
        if let Some(previous) = previous {
            assert_eq!(
                previous, row.eviction_order,
                "rank {} has two phrases",
                row.evicted
            );
        }
    }
}
/// Measuring reports what is on disk, clearing removes it, and a memory cache refuses by
/// name instead of quietly doing nothing.
#[test]
fn caches_are_measured_and_cleared_by_row() {
    let (mut state, _) = with_clip();
    let proxies = state
        .storage_layout()
        .resolve(CacheId::Proxies)
        .expect("proxies is a disk cache");
    std::fs::create_dir_all(&proxies).expect("create the cache directory");
    std::fs::write(proxies.join("a.bin"), vec![0u8; 2048]).expect("write a cached file");
    std::fs::write(proxies.join("b.bin"), vec![0u8; 1024]).expect("write a cached file");

    state.measure_caches().expect("measure");
    let row = state
        .cache_rows()
        .into_iter()
        .find(|row| row.key == "proxies")
        .expect("the proxies row");
    assert_eq!(
        row.usage,
        Some((3072, 2)),
        "the measurement does not match what was written"
    );

    state.clear_registered_cache("proxies").expect("clear");
    assert!(
        proxies.exists(),
        "clearing removed the directory instead of its contents"
    );
    assert_eq!(
        std::fs::read_dir(&proxies).expect("read the cache").count(),
        0,
        "a cached file survived the clear"
    );
    let row = state
        .cache_rows()
        .into_iter()
        .find(|row| row.key == "proxies")
        .expect("the proxies row");
    assert_eq!(row.usage, Some((0, 0)), "the row still shows the old size");

    // Clearing again is a message, not an error.
    state
        .clear_registered_cache("proxies")
        .expect("a second clear is inert");

    let err = state
        .clear_registered_cache("preview_frames")
        .expect_err("a memory cache was cleared from disk");
    assert!(err.contains("memory"), "{err}");
    let err = state
        .clear_registered_cache("not_a_cache")
        .expect_err("an unknown key was accepted");
    assert!(err.contains("not_a_cache"), "{err}");
}
/// An override is validated, persisted under its own key, resolved through the layout,
/// and reset back to the default.
#[test]
fn a_cache_override_is_validated_persisted_and_resettable() {
    let (mut state, _) = with_clip();
    let elsewhere = state.data_dir.join("elsewhere").join("luts");
    std::fs::create_dir_all(&elsewhere).expect("create the destination");

    let err = state
        .set_cache_override("luts", PathBuf::from("relative/path"))
        .expect_err("a relative override was accepted");
    assert!(err.contains("absolute"), "{err}");
    assert!(
        state.cloud_settings().storage.paths.get("luts").is_none(),
        "a refused override was persisted"
    );

    state
        .set_cache_override("luts", &elsewhere)
        .expect("set the override");
    assert_eq!(
        state.cloud_settings().storage.paths.get("luts"),
        Some(elsewhere.as_path()),
        "the override did not reach [storage.paths]"
    );
    assert_eq!(
        state.storage_layout().resolve(CacheId::Luts),
        Some(elsewhere.clone()),
        "the layout ignores the override"
    );
    let row = state
        .cache_rows()
        .into_iter()
        .find(|row| row.key == "luts")
        .expect("the luts row");
    assert!(row.overridden, "the row does not say it is overridden");

    // Two caches cannot share a directory: the layout refuses it rather than letting one
    // cache's clear delete the other's files.
    let err = state
        .set_cache_override("lottie", &elsewhere)
        .expect_err("two caches were pointed at one directory");
    assert!(
        !err.is_empty(),
        "the collision was refused without a reason"
    );

    state.clear_cache_override("luts").expect("reset");
    assert!(
        state.cloud_settings().storage.paths.get("luts").is_none(),
        "the override survived the reset"
    );
    assert!(
        state
            .storage_layout()
            .resolve(CacheId::Luts)
            .is_some_and(|path| path.starts_with(&state.data_dir)),
        "the cache did not return to the storage root"
    );
    state
        .clear_cache_override("luts")
        .expect("a second reset is inert");
}
/// A relocation carries the files and only then names the new path in the config.
#[test]
fn relocating_a_cache_moves_its_files_before_the_config_names_them() {
    let (mut state, _) = with_clip();
    let source = state
        .storage_layout()
        .resolve(CacheId::Catalog)
        .expect("catalog is a disk cache");
    std::fs::create_dir_all(&source).expect("create the cache");
    std::fs::write(source.join("feed.json"), b"{}").expect("write a cached file");
    let destination = state.data_dir.join("moved").join("catalog");

    state
        .relocate_registered_cache("catalog", &destination)
        .expect("relocate");
    assert!(
        destination.join("feed.json").exists(),
        "the cached file did not arrive"
    );
    assert_eq!(
        state.cloud_settings().storage.paths.get("catalog"),
        Some(destination.as_path()),
        "the config does not name the new path"
    );
    assert_eq!(
        state.storage_layout().resolve(CacheId::Catalog),
        Some(destination)
    );
}
/// The support report names every resolved disk path, so a bug report does not need a
/// screenshot of the dialog.
#[test]
fn the_storage_report_lists_every_disk_cache() {
    let (state, _) = with_clip();
    let report = state.storage_report();
    assert!(report.contains("storage root:"), "{report}");
    for id in CacheId::ALL {
        match id.descriptor().kind {
            CacheKind::Disk => assert!(
                report.contains(id.as_str()),
                "{} is missing from the report",
                id.as_str()
            ),
            // A memory cache has no path; listing one would be a lie in a support report.
            CacheKind::Memory => assert!(
                !report.contains(&format!("{}: ", id.as_str())),
                "{} was reported with a path",
                id.as_str()
            ),
        }
    }
}
/// Every typed field round-trips through the real config file, and Esc keeps what was
/// saved.
#[test]
fn every_settings_field_round_trips_and_esc_keeps_the_saved_value() {
    let (mut state, _) = with_clip();
    for field in SettingsField::ALL {
        if field == SettingsField::Filter {
            continue;
        }
        let value = match field {
            SettingsField::AiBaseUrl => "http://localhost:11434/v1",
            SettingsField::CloudBaseUrl => "https://cloud.example.test",
            SettingsField::AiKeyEnv => "ARIA_KEY",
            _ => "value-one",
        };
        retype(&mut state, field, value);
        assert!(
            state.setting_draft(field).1,
            "{} did not take focus",
            field.id()
        );
        state.commit_setting().expect("save the field");
        assert_eq!(
            state.setting_draft(field).0,
            value,
            "{} did not persist",
            field.id()
        );
        assert_eq!(
            state.settings_form.focused, None,
            "focus survived the commit"
        );

        // Esc discards the draft rather than writing it.
        state.focus_setting(Some(field));
        type_text(&mut state, "-discarded");
        state.cancel_setting();
        assert_eq!(
            state.setting_draft(field).0,
            value,
            "{} kept a cancelled draft",
            field.id()
        );
    }

    // Backspace edits the draft, and an emptied value clears the setting rather than
    // storing an empty string.
    state.focus_setting(Some(SettingsField::AiKeyEnv));
    for _ in 0.."ARIA_KEY".len() {
        state.setting_pop();
    }
    state.commit_setting().expect("clear the variable name");
    assert!(
        state.cloud_settings().ai.api_key_env.is_none(),
        "an empty value was stored as a string"
    );

    // A stored key is never shown back as text.
    retype(&mut state, SettingsField::AiKey, "sk-secret");
    state.commit_setting().expect("store the key");
    assert!(
        matches!(
            state
                .setting_look(SettingsField::AiKey)
                .map(|look| look.kind),
            Some(aria_plugin::SettingKind::Secret)
        ),
        "the key field is not marked secret"
    );
    assert_eq!(state.setting_draft(SettingsField::AiKey).0, "sk-secret");
}
/// A value the rest of the app cannot use is refused by name, and the file keeps the
/// previous value.
#[test]
fn a_refused_setting_leaves_the_saved_value_alone() {
    let (mut state, _) = with_clip();
    retype(
        &mut state,
        SettingsField::AiBaseUrl,
        "http://localhost:1234/v1",
    );
    state.commit_setting().expect("save a good URL");

    for bad in ["localhost:1234", "ftp://host/x", "http://"] {
        retype(&mut state, SettingsField::AiBaseUrl, bad);
        let err = state
            .commit_setting()
            .expect_err("a malformed URL was accepted");
        assert!(!err.is_empty(), "the refusal gave no reason");
        assert_eq!(
            state.cloud_settings().ai.base_url,
            "http://localhost:1234/v1",
            "a refused URL overwrote the saved one"
        );
        // The draft survives so the operator can fix it rather than retype it.
        assert_eq!(
            state.settings_form.focused,
            Some(SettingsField::AiBaseUrl),
            "a refusal dropped focus"
        );
    }

    retype(&mut state, SettingsField::AiKeyEnv, "not a name");
    let err = state
        .commit_setting()
        .expect_err("a malformed variable name was accepted");
    assert!(err.contains("environment variable"), "{err}");
}
/// Every enumerated choice persists, and the picker's list is the enum's own.
#[test]
fn every_enumerated_choice_persists_under_its_own_key() {
    let (mut state, _) = with_clip();
    for source in AiSource::ALL {
        state.set_ai_source(source).expect("save the source");
        assert_eq!(state.cloud_settings().ai.source, source);
        assert_eq!(
            AiSource::from_key(source.key()),
            Some(source),
            "{} does not round-trip",
            source.key()
        );
    }
    for protocol in AiApiProtocol::ALL {
        state.set_ai_protocol(protocol).expect("save the protocol");
        assert_eq!(state.cloud_settings().ai.api_protocol, protocol);
        assert_eq!(AiApiProtocol::from_key(protocol.key()), Some(protocol));
    }
    for autonomy in Autonomy::ALL {
        state.set_autonomy(autonomy).expect("save the autonomy");
        assert_eq!(state.cloud_settings().ai.autonomy, autonomy);
        assert_eq!(Autonomy::from_key(autonomy.key()), Some(autonomy));
    }
    for summary in ReasoningSummary::ALL {
        state
            .set_reasoning_summary(summary)
            .expect("save the summary");
        assert_eq!(state.cloud_settings().ai.reasoning_summary, summary);
        assert_eq!(ReasoningSummary::from_key(summary.key()), Some(summary));
    }
    for choice in ThemeChoice::ALL {
        state.set_theme_choice(choice).expect("save the theme");
        assert_eq!(state.theme_choice(), choice);
        assert_eq!(ThemeChoice::from_key(choice.key()), Some(choice));
    }
}
