//! `ai_protocol` cases.

use super::*;

#[test]
fn clearing_an_optional_key_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let mut s = Settings::default();
    s.ai.base_url = "http://x/v1".into();
    s.ai.model = "m".into();
    s.ai.api_key = Some("sk-secret".into());
    save(&path, &s).unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("api_key"));

    s.ai.api_key = None;
    save(&path, &s).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        !raw.contains("api_key"),
        "cleared key left no literal: {raw}"
    );
    assert_eq!(load(&path).unwrap().ai.api_key, None);
}

#[test]
fn save_creates_parent_dir() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("deep").join("config.toml");
    save(&path, &Settings::default()).unwrap();
    assert!(path.exists());
    assert_eq!(load(&path).unwrap(), Settings::default());
    assert_no_transaction_artifacts(path.parent().unwrap());
}

#[test]
fn save_replaces_existing_file_without_transaction_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "# keep this comment\n[ai]\nmodel = \"old\"\n\n[future]\nkeep = true\n";
    std::fs::write(&path, original).unwrap();

    let mut settings = load(&path).unwrap();
    settings.ai.model = "new".into();
    save(&path, &settings).unwrap();

    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("model = \"new\""), "{saved}");
    assert!(saved.contains("# keep this comment"), "{saved}");
    assert!(saved.contains("[future]"), "{saved}");
    assert_no_transaction_artifacts(dir.path());
}

/// WHY: the config holds provider API keys, so an existing mode that let the
/// group or the world read it is tightened on save rather than preserved. A
/// group-readable file shows no symptom, so nothing else reports it.
#[cfg(unix)]
#[test]
fn save_tightens_loose_existing_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[ai]\nmodel = \"old\"\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

    let mut settings = load(&path).unwrap();
    settings.ai.model = "new".into();
    save(&path, &settings).unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "a group-readable config was left readable");
    assert_no_transaction_artifacts(dir.path());
}

#[cfg(unix)]
#[test]
fn new_temporary_config_is_private_before_installation() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("config.toml");
    let temporary = write_synced_temp(&destination, b"secret = \"value\"\n", None).unwrap();

    let mode = std::fs::metadata(&temporary).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    std::fs::remove_file(temporary).unwrap();
    assert_no_transaction_artifacts(dir.path());
}

#[test]
fn fallback_swap_replaces_existing_file_and_cleans_backup() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("config.toml");
    let original = b"version = \"old\"\n";
    let replacement = b"version = \"new\"\n";
    std::fs::write(&destination, original).unwrap();
    let temporary = write_synced_temp(&destination, replacement, None).unwrap();
    let fs = FaultFs::failing(&[1], &[]);

    install_temp_with_ops(&destination, &temporary, &fs).unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), replacement);
    assert_no_transaction_artifacts(dir.path());
}

#[test]
fn fallback_install_failure_restores_original_and_cleans_temp() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("config.toml");
    let original = b"version = \"old\"\n";
    let replacement = b"version = \"new\"\n";
    std::fs::write(&destination, original).unwrap();
    let temporary = write_synced_temp(&destination, replacement, None).unwrap();
    let fs = FaultFs::failing(&[1, 3], &[]);

    let error = install_temp_with_ops(&destination, &temporary, &fs).unwrap_err();

    assert!(
        error.contains("original configuration was restored"),
        "{error}"
    );
    assert_eq!(std::fs::read(&destination).unwrap(), original);
    assert_no_transaction_artifacts(dir.path());
}

#[test]
fn fallback_reports_failed_rollback_and_retains_original_backup() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("config.toml");
    let original = b"version = \"old\"\n";
    let replacement = b"version = \"new\"\n";
    std::fs::write(&destination, original).unwrap();
    let temporary = write_synced_temp(&destination, replacement, None).unwrap();
    let fs = FaultFs::failing(&[1, 3, 4], &[]);

    let error = install_temp_with_ops(&destination, &temporary, &fs).unwrap_err();

    assert!(error.contains("rollback failed"), "{error}");
    assert!(error.contains("backup was retained"), "{error}");
    assert!(!destination.exists());
    let artifacts = transaction_artifacts(dir.path());
    assert_eq!(artifacts.len(), 1, "{artifacts:?}");
    assert!(
        artifacts[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(".aria-backup-")
    );
    assert_eq!(std::fs::read(&artifacts[0]).unwrap(), original);
}

#[test]
fn fallback_backup_cleanup_failure_does_not_uncommit_installation() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("config.toml");
    let original = b"version = \"old\"\n";
    let replacement = b"version = \"new\"\n";
    std::fs::write(&destination, original).unwrap();
    let temporary = write_synced_temp(&destination, replacement, None).unwrap();
    let fs = FaultFs::failing(&[1], &[1]);

    install_temp_with_ops(&destination, &temporary, &fs).unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), replacement);
    let artifacts = transaction_artifacts(dir.path());
    assert_eq!(artifacts.len(), 1, "{artifacts:?}");
    assert!(
        artifacts[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(".aria-backup-")
    );
    assert_eq!(std::fs::read(&artifacts[0]).unwrap(), original);
}

#[test]
fn providers_and_cloud_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let mut s = Settings::default();
    s.providers.insert(
        "pexels".into(),
        ProviderSettings {
            api_key: None,
            api_key_env: Some("PEXELS_API_KEY".into()),
        },
    );
    s.providers.insert(
        "elevenlabs".into(),
        ProviderSettings {
            api_key: Some("sk-11".into()),
            api_key_env: None,
        },
    );
    s.cloud.base_url = "https://staging.api.aria.sh".into();
    save(&path, &s).unwrap();

    let loaded = load(&path).unwrap();
    assert_eq!(
        loaded.provider("pexels").api_key_env.as_deref(),
        Some("PEXELS_API_KEY")
    );
    assert!(loaded.provider("pexels").is_configured());
    assert_eq!(
        loaded.provider("elevenlabs").api_key.as_deref(),
        Some("sk-11")
    );
    assert!(!loaded.provider("nonexistent").is_configured());
    assert_eq!(loaded.cloud.base_url, "https://staging.api.aria.sh");
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("[cloud]"), "{raw}");

    // Dropping a provider removes its table; clearing the cloud
    // override removes the key.
    let mut s = loaded;
    s.providers.remove("elevenlabs");
    s.cloud.base_url.clear();
    save(&path, &s).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("elevenlabs"), "{raw}");
    assert!(!raw.contains("base_url = \"https://staging"), "{raw}");
    assert!(raw.contains("[providers.pexels]"), "{raw}");
}

#[test]
fn legacy_account_base_url_loads_into_cloud() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[account]\nbase_url = \"https://legacy.api.aria.sh\"\nauth_base_url = \"https://aria.sh\"\n",
    )
    .unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.cloud.base_url, "https://legacy.api.aria.sh");

    // Saving migrates to `[cloud]` and drops `[account]`.
    save(&path, &loaded).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("[cloud]"), "{raw}");
    assert!(!raw.contains("[account]"), "{raw}");
    assert!(!raw.contains("auth_base_url"), "{raw}");
}

/// WHY: two resolvers used to disagree about a BYOK key. `aria_ai::config`
/// preferred the named environment variable and rejected a blank value, while
/// this one preferred the literal and accepted whitespace, so the same
/// `config.toml` could pick different keys depending on which subsystem asked,
/// and the stock panel could report `$VAR` as the source while sending the
/// literal. Pins one precedence and one blank rule for both.
///
/// Does not cover `aria_ai::config::resolve_api_key`, which additionally
/// reports a named-but-unset variable as an error rather than `None`.
#[test]
fn provider_key_resolution_prefers_the_named_environment_variable() {
    const VAR: &str = "ARIA_TEST_PROVIDER_KEY_RESOLUTION";
    let with_env = |value: Option<&str>| {
        // SAFETY: the name is unique to this test, so no other test observes it.
        unsafe {
            match value {
                Some(v) => std::env::set_var(VAR, v),
                None => std::env::remove_var(VAR),
            }
        }
    };
    let both = ProviderSettings {
        api_key: Some("literal".into()),
        api_key_env: Some(VAR.into()),
    };

    with_env(Some("from-env"));
    assert_eq!(both.resolve_key().as_deref(), Some("from-env"));

    // A blank or whitespace-only value is not a key: fall back to the literal.
    for blank in ["", "   ", "\t\n"] {
        with_env(Some(blank));
        assert_eq!(
            both.resolve_key().as_deref(),
            Some("literal"),
            "blank {blank:?} must not resolve"
        );
    }

    with_env(None);
    assert_eq!(both.resolve_key().as_deref(), Some("literal"));

    let env_only = ProviderSettings {
        api_key: None,
        api_key_env: Some(VAR.into()),
    };
    assert_eq!(env_only.resolve_key(), None);
    assert!(
        env_only.is_configured(),
        "env-named key counts as configured"
    );

    // A blank literal is not a key either.
    let blank_literal = ProviderSettings {
        api_key: Some("   ".into()),
        api_key_env: None,
    };
    assert_eq!(blank_literal.resolve_key(), None);
}

/// Every theme choice reads back from the key it writes, and the keys and
/// labels are distinct, so a new family is covered by default. The keys from
/// before the accent families read as their successor.
#[test]
fn theme_key_round_trip_and_legacy_keys() {
    let mut keys = std::collections::BTreeSet::new();
    let mut labels = std::collections::BTreeSet::new();
    for theme in ThemeChoice::ALL {
        assert_eq!(ThemeChoice::from_key(theme.key()), Some(theme));
        assert!(keys.insert(theme.key()), "{theme:?} reuses a key");
        assert!(labels.insert(theme.label()), "{theme:?} reuses a label");
    }
    for legacy in ["default", "dark-blue", "dark_blue"] {
        assert_eq!(
            ThemeChoice::from_key(legacy),
            Some(ThemeChoice::Blue),
            "{legacy}"
        );
    }
    assert_eq!(ThemeChoice::from_key("ember"), Some(ThemeChoice::Ember));
    assert_eq!(ThemeChoice::from_key("nonsense"), None);
    assert_eq!(ThemeChoice::default(), ThemeChoice::Blue);
}

#[test]
fn autonomy_parses_and_tolerates_missing_or_garbage_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    std::fs::write(&path, "[ai]\nmodel = \"m\"\nautonomy = \"full\"\n").unwrap();
    assert_eq!(load(&path).unwrap().ai.autonomy, Autonomy::Full);

    // Missing key keeps the default.
    std::fs::write(&path, "[ai]\nmodel = \"m\"\n").unwrap();
    assert_eq!(load(&path).unwrap().ai.autonomy, Autonomy::Ask);

    // Unrecognized value keeps the default rather than failing the load.
    std::fs::write(&path, "[ai]\nautonomy = \"yolo\"\n").unwrap();
    assert_eq!(load(&path).unwrap().ai.autonomy, Autonomy::Ask);

    // "confirm" is the tolerated alias for Ask.
    std::fs::write(&path, "[ai]\nautonomy = \"confirm\"\n").unwrap();
    assert_eq!(load(&path).unwrap().ai.autonomy, Autonomy::Ask);
}

#[test]
fn autonomy_round_trips_and_preserves_unrelated_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "# my aria config\n[ai]\nbase_url = \"http://x/v1\"  # local\nmodel = \"m\"\n",
    )
    .unwrap();

    let mut s = load(&path).unwrap();
    s.ai.autonomy = Autonomy::Full;
    save(&path, &s).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("autonomy = \"full\""), "{raw}");
    assert!(raw.contains("# my aria config"), "leading comment kept");
    assert!(raw.contains("# local"), "inline comment kept");
    assert_eq!(load(&path).unwrap().ai.autonomy, Autonomy::Full);

    // Back to the default removes the key (absence = default).
    let mut s = load(&path).unwrap();
    s.ai.autonomy = Autonomy::Ask;
    save(&path, &s).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("autonomy"), "default left no literal: {raw}");
    assert_eq!(load(&path).unwrap().ai.autonomy, Autonomy::Ask);
}

#[test]
fn ai_protocol_and_reasoning_summary_are_tolerant() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    std::fs::write(
        &path,
        "[ai]\napi_protocol = \"responses\"\nreasoning_summary = \"off\"\n",
    )
    .unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.ai.api_protocol, AiApiProtocol::Responses);
    assert_eq!(loaded.ai.reasoning_summary, ReasoningSummary::Off);

    std::fs::write(
        &path,
        "[ai]\napi_protocol = \"chat-completions\"\nreasoning_summary = \"on\"\n",
    )
    .unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.ai.api_protocol, AiApiProtocol::ChatCompletions);
    assert_eq!(loaded.ai.reasoning_summary, ReasoningSummary::Auto);

    std::fs::write(
        &path,
        "[ai]\napi_protocol = \"future\"\nreasoning_summary = \"verbose\"\n",
    )
    .unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.ai.api_protocol, AiApiProtocol::default());
    assert_eq!(loaded.ai.reasoning_summary, ReasoningSummary::default());
}

#[test]
fn ai_protocol_and_reasoning_summary_round_trip_canonical_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "# keep me\n[ai]\nmodel = \"gpt-5\"\n").unwrap();

    let mut settings = load(&path).unwrap();
    settings.ai.api_protocol = AiApiProtocol::Responses;
    settings.ai.reasoning_summary = ReasoningSummary::Off;
    save(&path, &settings).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("api_protocol = \"responses\""), "{raw}");
    assert!(raw.contains("reasoning_summary = \"off\""), "{raw}");
    assert!(raw.contains("# keep me"), "{raw}");
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.ai.api_protocol, AiApiProtocol::Responses);
    assert_eq!(loaded.ai.reasoning_summary, ReasoningSummary::Off);

    settings.ai.api_protocol = AiApiProtocol::default();
    settings.ai.reasoning_summary = ReasoningSummary::default();
    save(&path, &settings).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("api_protocol"), "{raw}");
    assert!(!raw.contains("reasoning_summary"), "{raw}");
}

#[test]
fn corrupt_non_table_section_is_replaced_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "ai = 3\n").unwrap();
    // `ai = 3` parses fine; saving must overwrite it with a real table
    // rather than panic.
    let mut s = Settings::default();
    s.ai.base_url = "http://x/v1".into();
    s.ai.model = "m".into();
    save(&path, &s).unwrap();
    assert!(load(&path).unwrap().ai.is_configured());
}

#[test]
fn api_keys_redacted_in_debug_output() {
    let mut settings = Settings::default();
    settings.ai.api_key = Some("super_secret_ai_key_12345".into());
    settings.providers.insert(
        "pexels".into(),
        ProviderSettings {
            api_key: Some("super_secret_pexels_key_67890".into()),
            api_key_env: Some("PEXELS_KEY".into()),
        },
    );

    let ai_debug = format!("{:?}", settings.ai);
    assert!(!ai_debug.contains("super_secret_ai_key_12345"));
    assert!(ai_debug.contains("<redacted>"));

    let provider_debug = format!("{:?}", settings.providers.get("pexels").unwrap());
    assert!(!provider_debug.contains("super_secret_pexels_key_67890"));
    assert!(provider_debug.contains("<redacted>"));

    let settings_debug = format!("{settings:?}");
    assert!(!settings_debug.contains("super_secret_ai_key_12345"));
    assert!(!settings_debug.contains("super_secret_pexels_key_67890"));
}

#[cfg(unix)]
#[test]
fn unix_permissions_enforced_0600_file_and_0700_dir() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("nested_config_dir");
    let config_path = config_dir.join("config.toml");

    let s = Settings::default();
    save(&config_path, &s).unwrap();

    let dir_meta = std::fs::metadata(&config_dir).unwrap();
    let dir_mode = dir_meta.permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700, "directory must be 0700");

    let file_meta = std::fs::metadata(&config_path).unwrap();
    let file_mode = file_meta.permissions().mode() & 0o777;
    assert_eq!(file_mode, 0o600, "config.toml must be 0600");
}
