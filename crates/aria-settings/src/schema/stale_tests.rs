//! Stale-format tests for every shape `config.toml` persists.
//!
//! The class closed here is a persisted table whose meaning changed being read
//! as if it had not, and a persisted table shipping with no recorded version at
//! all. Every case is driven from [`PersistedShape::all`] and from the tables
//! [`save`] actually writes, so a table added to the writer without a registry
//! row, and a registry row without a stale-file case, both fail. What these
//! tests do not catch: a meaning change made *within* a version, where the
//! author changed how a key is read and left the version number alone.

use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use super::*;
use crate::{
    AiSource, Autonomy, DEFAULT_DOWNLOAD_QUOTA_MIB, PluginLimitSettings, Settings, ThemeChoice,
    TranscriptionSettings, load, save,
};

/// One shape's stale-format case: the file a build at the shape's oldest
/// migratable version wrote, and what this build must read out of it.
struct StaleCase {
    file: String,
    expect: fn(&Settings),
}

/// The stale file and expected values for one shape.
///
/// Exhaustive on purpose: a new shape makes this match fail to compile, so it
/// cannot ship without a stale-file case and a recorded version.
fn stale_case(shape: PersistedShape) -> StaleCase {
    match shape {
        PersistedShape::Ai => StaleCase {
            // Version 1: no `source` key, a managed-account flag that no
            // longer exists, and `confirm` for the autonomy level now spelled
            // `ask`.
            file: "[ai]\nbase_url = \"http://localhost:11434/v1\"\nmodel = \"qwen3:14b\"\n\
                   use_account = true\nautonomy = \"confirm\"\n"
                .into(),
            expect: |s| {
                assert_eq!(s.ai.source, AiSource::Custom);
                assert_eq!(s.ai.base_url, "http://localhost:11434/v1");
                assert_eq!(s.ai.model, "qwen3:14b");
                assert_eq!(s.ai.autonomy, Autonomy::Ask);
                assert!(s.ai.is_configured());
            },
        },
        PersistedShape::Appearance => StaleCase {
            file: "[appearance]\ntheme = \"ember\"\n".into(),
            expect: |s| assert_eq!(s.appearance.theme, ThemeChoice::Ember),
        },
        PersistedShape::Cloud => StaleCase {
            // Version 1 kept the API host in `[account]`.
            file: "[account]\nbase_url = \"https://legacy.api.aria.sh\"\n\
                   auth_base_url = \"https://aria.sh\"\n"
                .into(),
            expect: |s| assert_eq!(s.cloud.base_url, "https://legacy.api.aria.sh"),
        },
        PersistedShape::PluginLimits => StaleCase {
            file: "[plugin_limits]\nwasm_fuel_per_call = 2000000000\nwasm_memory_mib = 256\n"
                .into(),
            expect: |s| {
                assert_eq!(s.plugin_limits.wasm_fuel_per_call, 2_000_000_000);
                assert_eq!(s.plugin_limits.wasm_memory_mib, 256);
            },
        },
        PersistedShape::Plugins => StaleCase {
            file: "[plugins]\n\"notes.directory\" = \"/srv/notes\"\n".into(),
            expect: |s| assert_eq!(s.plugin_value("notes.directory"), Some("/srv/notes")),
        },
        PersistedShape::Providers => StaleCase {
            file: "[providers.pexels]\napi_key_env = \"PEXELS_API_KEY\"\n".into(),
            expect: |s| {
                assert_eq!(
                    s.provider("pexels").api_key_env.as_deref(),
                    Some("PEXELS_API_KEY")
                );
                assert!(s.provider("pexels").is_configured());
            },
        },
        PersistedShape::Storage => StaleCase {
            file: format!(
                "[storage]\nroot = \"{}\"\ndownload_quota_mib = 4096\n\
                 [storage.paths]\nproxies = \"{}\"\n",
                crate::fixture_abs("srv/aria"),
                crate::fixture_abs("srv/aria/proxies"),
            ),
            expect: |s| {
                let root = crate::fixture_abs("srv/aria");
                let proxies = crate::fixture_abs("srv/aria/proxies");
                assert_eq!(s.storage.root.as_deref(), Some(Path::new(&root)));
                assert_eq!(s.storage.download_quota_mib, 4_096);
                assert_eq!(
                    s.storage.paths.proxies.as_deref(),
                    Some(Path::new(&proxies))
                );
            },
        },
        PersistedShape::Transcription => StaleCase {
            file: "[transcription]\nconnect_timeout_secs = 5\nread_timeout_secs = 120\n\
                   total_timeout_secs = 3600\n"
                .into(),
            expect: |s| {
                assert_eq!(s.transcription.connect_timeout_secs, 5);
                assert_eq!(s.transcription.read_timeout_secs, 120);
                assert_eq!(s.transcription.total_timeout_secs, 3_600);
            },
        },
    }
}

fn write_config(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("config.toml");
    std::fs::write(&path, contents).unwrap();
    path
}

fn document(path: &Path) -> DocumentMut {
    std::fs::read_to_string(path).unwrap().parse().unwrap()
}

/// Settings with a value in every shape, so the writer produces every table it
/// is capable of producing.
fn populated() -> Settings {
    let mut s = Settings::default();
    s.ai.source = AiSource::OpenRouter;
    s.ai.model = "openai/gpt-5.6-sol".into();
    s.ai.api_key_env = Some("OPENROUTER_API_KEY".into());
    s.appearance.theme = ThemeChoice::Ember;
    s.cloud.base_url = "https://staging.api.aria.sh".into();
    s.storage.root = Some(PathBuf::from(crate::fixture_abs("srv/aria")));
    s.storage.download_quota_mib = DEFAULT_DOWNLOAD_QUOTA_MIB * 2;
    s.storage.paths.proxies = Some(PathBuf::from(crate::fixture_abs("srv/aria/proxies")));
    s.providers.insert(
        "pexels".into(),
        crate::ProviderSettings {
            api_key: None,
            api_key_env: Some("PEXELS_API_KEY".into()),
        },
    );
    s.transcription = TranscriptionSettings {
        connect_timeout_secs: 5,
        read_timeout_secs: 120,
        total_timeout_secs: 3_600,
    };
    s.plugin_limits = PluginLimitSettings {
        wasm_fuel_per_call: 2_000_000_000,
        wasm_memory_mib: 256,
    };
    s.set_plugin_value("notes.directory", Some("/srv/notes"));
    s
}

#[test]
fn registry_rows_are_coherent() {
    let mut tables: Vec<&str> = Vec::new();
    for shape in PersistedShape::all() {
        let table = shape.table();
        assert!(!table.is_empty(), "{shape:?} owns no table");
        assert!(
            !tables.contains(&table),
            "two shapes claim [{table}]: {shape:?}"
        );
        tables.push(table);

        let current = shape.current_version();
        let oldest = shape.oldest_migratable_version();
        assert!(current >= LEGACY_VERSION, "{shape:?} has no version");
        assert!(
            (LEGACY_VERSION..=current).contains(&oldest),
            "{shape:?} migrates from {oldest} but is at {current}"
        );

        assert_eq!(shape.decision(current), VersionDecision::Current);
        assert_eq!(
            shape.decision(current + 1),
            VersionDecision::Refuse(RefusalReason::FromNewerBuild)
        );
        assert_eq!(
            shape.decision(oldest - 1),
            VersionDecision::Refuse(RefusalReason::TooOld)
        );
        for stale in oldest..current {
            assert_eq!(
                shape.decision(stale),
                VersionDecision::Migrate { from: stale },
                "{shape:?} version {stale} has no migration decision"
            );
        }
    }
    assert!(tables.len() >= 2, "the registry is empty");
}

/// The fail-by-default check: the tables the writer produces and the registry
/// are the same set. A newly persisted table with no registry row has no
/// recorded version, and this is where that shows up.
#[test]
fn every_persisted_table_is_a_registered_shape() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    save(&path, &populated()).unwrap();

    let doc = document(&path);
    let mut written: Vec<String> = doc
        .iter()
        .map(|(key, _)| key.to_string())
        .filter(|key| key != SCHEMA_TABLE)
        .collect();
    written.sort();
    let mut registered: Vec<String> = PersistedShape::all()
        .map(|shape| shape.table().to_string())
        .collect();
    registered.sort();
    assert_eq!(
        written, registered,
        "a persisted table has no schema version, or a registered shape is never written"
    );

    for shape in PersistedShape::all() {
        assert_eq!(
            recorded_version(&doc, shape).unwrap(),
            shape.current_version(),
            "[{}] was written without its current version",
            shape.table()
        );
    }
}

/// A shape whose table is absent from the file loses its version record, so a
/// configuration holding nothing but defaults still holds no empty sections.
#[test]
fn absent_tables_are_not_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    save(&path, &Settings::default()).unwrap();

    let doc = document(&path);
    let recorded: Vec<&str> = PersistedShape::all()
        .map(PersistedShape::table)
        .filter(|table| {
            doc.get(SCHEMA_TABLE)
                .and_then(|item| item.as_table_like())
                .is_some_and(|schema| schema.get(table).is_some())
        })
        .collect();
    assert_eq!(recorded, ["ai", "appearance"]);
    for absent in [
        "cloud",
        "plugin_limits",
        "plugins",
        "providers",
        "storage",
        "transcription",
    ] {
        assert!(
            !std::fs::read_to_string(&path).unwrap().contains(absent),
            "an absent shape was recorded: {absent}"
        );
    }
}

#[test]
fn a_file_without_a_schema_table_is_at_the_legacy_version() {
    let doc: DocumentMut = "[ai]\nmodel = \"m\"\n".parse().unwrap();
    for shape in PersistedShape::all() {
        assert_eq!(recorded_version(&doc, shape).unwrap(), LEGACY_VERSION);
    }

    // A record for one shape says nothing about the others.
    let doc: DocumentMut = "[schema]\nai = 2\n".parse().unwrap();
    assert_eq!(recorded_version(&doc, PersistedShape::Ai).unwrap(), 2);
    assert_eq!(
        recorded_version(&doc, PersistedShape::Storage).unwrap(),
        LEGACY_VERSION
    );
}

/// Every shape, driven from its stale file: the values must come out migrated,
/// the save must record the current version, and reading the saved file back
/// must produce the same values without running the migration again.
#[test]
fn stale_file_of_every_shape_is_migrated_to_current_values() {
    for shape in PersistedShape::all() {
        let dir = tempfile::tempdir().unwrap();
        let case = stale_case(shape);
        let path = write_config(dir.path(), &case.file);

        let loaded = load(&path).unwrap_or_else(|e| panic!("[{}] stale file: {e}", shape.table()));
        (case.expect)(&loaded);

        save(&path, &loaded).unwrap();
        let doc = document(&path);
        assert_eq!(
            recorded_version(&doc, shape).unwrap(),
            shape.current_version(),
            "[{}] was migrated without recording its version",
            shape.table()
        );

        let reloaded = load(&path).unwrap();
        (case.expect)(&reloaded);
        assert_eq!(
            reloaded,
            loaded,
            "[{}] does not read back the way it was migrated",
            shape.table()
        );
    }
}

#[test]
fn a_version_from_a_newer_build_is_refused_by_load_and_save() {
    for shape in PersistedShape::all() {
        let dir = tempfile::tempdir().unwrap();
        let future = shape.current_version() + 1;
        let file = format!(
            "{}\n[{SCHEMA_TABLE}]\n{} = {future}\n",
            stale_case(shape).file,
            shape.table()
        );
        let path = write_config(dir.path(), &file);

        let error = load(&path).expect_err("a newer build's shape was served");
        for fragment in [
            &format!("[{}]", shape.table()),
            &format!("records schema version {future}"),
            &format!("reads version {}", shape.current_version()),
        ] {
            assert!(error.contains(fragment.as_str()), "{error}");
        }

        let error =
            save(&path, &Settings::default()).expect_err("a newer build's file was rewritten");
        assert!(error.contains("was left in place"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), file);
    }
}

#[test]
fn a_version_older_than_the_oldest_migration_is_refused() {
    for shape in PersistedShape::all() {
        let dir = tempfile::tempdir().unwrap();
        let too_old = shape.oldest_migratable_version() - 1;
        let file = format!(
            "{}\n[{SCHEMA_TABLE}]\n{} = {too_old}\n",
            stale_case(shape).file,
            shape.table()
        );
        let path = write_config(dir.path(), &file);

        let error = load(&path).expect_err("a shape too old to migrate was served");
        assert!(error.contains(&format!("[{}]", shape.table())), "{error}");
        assert!(
            error.contains(&format!(
                "migrates version {} and newer",
                shape.oldest_migratable_version()
            )),
            "{error}"
        );
    }
}

#[test]
fn an_unreadable_version_record_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for shape in PersistedShape::all() {
        for record in ["\"two\"", "-1", "1.5", "true"] {
            let file = format!("[{SCHEMA_TABLE}]\n{} = {record}\n", shape.table());
            let path = write_config(dir.path(), &file);
            let error = load(&path).expect_err("an unreadable version was served");
            assert!(
                error.contains(&format!("[{SCHEMA_TABLE}].{}", shape.table())),
                "{error}"
            );
        }
    }

    let path = write_config(dir.path(), "schema = 3\n[ai]\nmodel = \"m\"\n");
    let error = load(&path).expect_err("a file with no readable versions was served");
    assert!(error.contains("[schema] is not a table"), "{error}");
}

/// Version 1 had no `source` key, so a version 1 file with a base URL and a
/// model was the freeform endpoint. From version 2 an absent `source` is the
/// Local default, which is exactly what a Local configuration writes: without
/// the version gate, saving a Local endpoint and reading it back turned it into
/// a Custom one.
#[test]
fn stale_ai_source_is_migrated_and_a_current_local_config_is_not_reinferred() {
    let dir = tempfile::tempdir().unwrap();
    let keys = "[ai]\nbase_url = \"http://localhost:11434/v1\"\nmodel = \"qwen3:14b\"\n";

    let path = write_config(dir.path(), keys);
    assert_eq!(load(&path).unwrap().ai.source, AiSource::Custom);

    let path = write_config(dir.path(), &format!("{keys}[{SCHEMA_TABLE}]\nai = 2\n"));
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.ai.source, AiSource::Local);
    assert_eq!(loaded.ai.base_url, "http://localhost:11434/v1");

    let mut settings = Settings::default();
    settings.ai.source = AiSource::Local;
    settings.ai.base_url = "http://localhost:11434/v1".into();
    settings.ai.model = "qwen3:14b".into();
    let path = dir.path().join("fresh.toml");
    save(&path, &settings).unwrap();
    assert_eq!(load(&path).unwrap().ai.source, AiSource::Local);
}

/// Version 1 kept the cloud host in `[account].base_url`. Version 2 keeps it in
/// `[cloud]`, so `[account]` is read only while migrating: in a current file
/// that table belongs to something this crate does not own.
#[test]
fn stale_cloud_account_host_is_migrated_and_a_current_file_ignores_account() {
    let dir = tempfile::tempdir().unwrap();
    let account = "[account]\nbase_url = \"https://legacy.api.aria.sh\"\n";

    let path = write_config(dir.path(), account);
    assert_eq!(
        load(&path).unwrap().cloud.base_url,
        "https://legacy.api.aria.sh"
    );

    let path = write_config(
        dir.path(),
        &format!("{account}[{SCHEMA_TABLE}]\ncloud = 2\n"),
    );
    assert_eq!(
        load(&path).unwrap().cloud.base_url,
        "",
        "a version 1 host was served to a version 2 reader"
    );

    let path = write_config(
        dir.path(),
        &format!("{account}[cloud]\nbase_url = \"https://api.aria.sh\"\n"),
    );
    assert_eq!(load(&path).unwrap().cloud.base_url, "https://api.aria.sh");
}

/// The theme key was spelled `dark_blue` before it was spelled `dark-blue`. The
/// value did not change, so this is tolerance rather than a version bump: the
/// old spelling still reads, and a save writes the canonical one.
#[test]
fn stale_appearance_theme_spelling_is_read_and_rewritten_canonically() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(dir.path(), "[appearance]\ntheme = \"dark_blue\"\n");

    let loaded = load(&path).unwrap();
    assert_eq!(loaded.appearance.theme, ThemeChoice::DarkBlue);

    save(&path, &loaded).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("theme = \"dark-blue\""), "{raw}");
    assert!(!raw.contains("dark_blue"), "{raw}");
}

/// Versioning gates a changed meaning, not an absent key. A current-version
/// file that sets nothing still loads, and every optional key still keeps its
/// default rather than failing the load.
#[test]
fn a_current_file_with_no_values_still_loads_all_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = format!("[{SCHEMA_TABLE}]\n");
    for shape in PersistedShape::all() {
        file.push_str(&format!(
            "{} = {}\n",
            shape.table(),
            shape.current_version()
        ));
    }
    for shape in PersistedShape::all() {
        file.push_str(&format!("[{}]\n", shape.table()));
    }
    let path = write_config(dir.path(), &file);

    assert_eq!(load(&path).unwrap(), Settings::default());
}
