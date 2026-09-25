//! `missing_file` cases.

use toml_edit::Item;

use super::*;

#[test]
fn missing_file_is_all_defaults() {
    let s = load(Path::new("/nonexistent/aria/config.toml")).unwrap();
    assert_eq!(s, Settings::default());
    assert!(!s.ai.is_configured());
    assert_eq!(s.appearance.theme, ThemeChoice::DarkBlue);
    assert_eq!(s.storage, StorageSettings::default());
    assert_eq!(s.storage.download_quota_mib, DEFAULT_DOWNLOAD_QUOTA_MIB);
}

#[test]
fn storage_valid_values_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut s = Settings::default();
    s.storage.root = Some(dir.path().join("storage"));
    s.storage.download_quota_mib = 4_096;
    // Built and asserted from the key list, so a tenth override is covered the
    // moment it exists rather than the next time someone remembers this file.
    for key in StoragePathOverrides::KEYS {
        *s.storage
            .paths
            .slot(key)
            .unwrap_or_else(|| panic!("{key} has no slot")) = Some(dir.path().join(key));
    }

    save(&path, &s).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("[storage]"), "{raw}");
    assert!(raw.contains("download_quota_mib = 4096"), "{raw}");
    assert!(raw.contains("[storage.paths]"), "{raw}");
    for key in StoragePathOverrides::KEYS {
        assert!(raw.contains(&format!("{key} = ")), "missing {key}: {raw}");
    }

    let loaded = load(&path).unwrap();
    assert_eq!(loaded.storage, s.storage);
    for key in StoragePathOverrides::KEYS {
        assert!(loaded.storage.paths.get(key).is_some(), "{key}");
    }
    assert_eq!(loaded.storage.paths.get("future"), None);
}

#[test]
fn storage_paths_must_be_absolute() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let absolute_download = dir.path().join("download");
    std::fs::write(
        &path,
        format!(
            "[storage]\n\
                 root = \"relative/root\"\n\
                 [storage.paths]\n\
                 proxies = \"relative/proxies\"\n\
                 analysis = \"relative/analysis\"\n\
                 ai_models = \"relative/ai-models\"\n\
                 download = {:?}\n\
                 catalog = 3\n\
                 luts = \"\"\n",
            absolute_download.to_str().unwrap()
        ),
    )
    .unwrap();

    let mut s = load(&path).unwrap();
    assert_eq!(s.storage.root, None);
    assert_eq!(s.storage.paths.proxies, None);
    assert_eq!(s.storage.paths.analysis, None);
    assert_eq!(s.storage.paths.ai_models, None);
    assert_eq!(
        s.storage.paths.download.as_deref(),
        Some(absolute_download.as_path())
    );
    assert_eq!(s.storage.paths.catalog, None);
    assert_eq!(s.storage.paths.luts, None);

    let original = std::fs::read_to_string(&path).unwrap();
    s.storage.root = Some(PathBuf::from("relative/root"));
    let error = save(&path, &s).unwrap_err();
    assert!(error.contains("absolute path"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "failed validation must not rewrite the file"
    );
    assert_no_transaction_artifacts(dir.path());

    s.storage.root = None;
    s.storage.paths.analysis = Some(PathBuf::from("relative/analysis"));
    let error = save(&path, &s).unwrap_err();
    assert!(error.contains("paths.analysis"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "failed analysis-path validation must not rewrite the file"
    );
    assert_no_transaction_artifacts(dir.path());

    s.storage.paths.analysis = None;
    s.storage.paths.ai_models = Some(PathBuf::from("relative/ai-models"));
    let error = save(&path, &s).unwrap_err();
    assert!(error.contains("paths.ai_models"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "failed AI-models-path validation must not rewrite the file"
    );
    assert_no_transaction_artifacts(dir.path());
}

#[test]
fn invalid_storage_quotas_fall_back_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for value in [
        "0".to_string(),
        "-1".to_string(),
        (MAX_DOWNLOAD_QUOTA_MIB + 1).to_string(),
        "\"2048\"".to_string(),
        "3.5".to_string(),
    ] {
        std::fs::write(&path, format!("[storage]\ndownload_quota_mib = {value}\n")).unwrap();
        assert_eq!(
            load(&path).unwrap().storage.download_quota_mib,
            DEFAULT_DOWNLOAD_QUOTA_MIB,
            "value {value} should fall back"
        );
    }

    for value in [MIN_DOWNLOAD_QUOTA_MIB, MAX_DOWNLOAD_QUOTA_MIB] {
        std::fs::write(&path, format!("[storage]\ndownload_quota_mib = {value}\n")).unwrap();
        assert_eq!(load(&path).unwrap().storage.download_quota_mib, value);
    }

    let original = std::fs::read_to_string(&path).unwrap();
    let mut s = Settings::default();
    s.storage.download_quota_mib = 0;
    let error = save(&path, &s).unwrap_err();
    assert!(error.contains("download_quota_mib"), "{error}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert_no_transaction_artifacts(dir.path());
}

#[test]
fn default_storage_values_are_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    save(&path, &Settings::default()).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("[storage]"), "{raw}");
    assert!(!raw.contains("download_quota_mib"), "{raw}");

    std::fs::write(
        &path,
        "[storage]\n\
             root = \"\"\n\
             download_quota_mib = 2048\n\
             [storage.paths]\n\
             proxies = \"\"\n",
    )
    .unwrap();
    let s = load(&path).unwrap();
    assert_eq!(s.storage, StorageSettings::default());
    save(&path, &s).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("[storage]"), "{raw}");
}

#[test]
fn clearing_storage_values_preserves_unknown_nested_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let absolute = dir.path().join("cache");
    let absolute = absolute.to_str().unwrap();
    std::fs::write(
        &path,
        format!(
            "# storage heading\n\
                 [storage]\n\
                 root = {absolute:?}\n\
                 download_quota_mib = 4096\n\
                 future_policy = \"keep\" # future policy comment\n\
                 [storage.paths]\n\
                 proxies = {absolute:?}\n\
                 analysis = {absolute:?}\n\
                 ai_models = {absolute:?}\n\
                 download = {absolute:?}\n\
                 catalog = {absolute:?}\n\
                 luts = {absolute:?}\n\
                 lottie = {absolute:?}\n\
                 templates = {absolute:?}\n\
                 future_cache = {absolute:?} # future cache comment\n"
        ),
    )
    .unwrap();

    let mut s = load(&path).unwrap();
    s.storage = StorageSettings::default();
    save(&path, &s).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    let doc = raw.parse::<DocumentMut>().unwrap();
    let storage = doc.get("storage").and_then(Item::as_table).unwrap();
    assert!(storage.get("root").is_none(), "{raw}");
    assert!(storage.get("download_quota_mib").is_none(), "{raw}");
    assert_eq!(
        storage.get("future_policy").and_then(Item::as_str),
        Some("keep")
    );
    let paths = storage.get("paths").and_then(Item::as_table).unwrap();
    for key in [
        "proxies",
        "analysis",
        "ai_models",
        "download",
        "catalog",
        "luts",
        "lottie",
        "templates",
    ] {
        assert!(paths.get(key).is_none(), "{key} was not cleared: {raw}");
    }
    assert_eq!(
        paths.get("future_cache").and_then(Item::as_str),
        Some(absolute)
    );
    assert!(raw.contains("# storage heading"), "{raw}");
    assert!(raw.contains("# future policy comment"), "{raw}");
    assert!(raw.contains("# future cache comment"), "{raw}");
}

#[test]
fn storage_save_preserves_comments_unknown_keys_and_unrelated_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let absolute = dir.path().join("cache");
    let absolute = absolute.to_str().unwrap();
    std::fs::write(
        &path,
        format!(
            "# config heading\n\
                 [storage]\n\
                 root = {absolute:?} # root comment\n\
                 download_quota_mib = 4096 # quota comment\n\
                 future_policy = \"keep\" # storage unknown\n\
                 [storage.paths]\n\
                 proxies = {absolute:?} # proxy comment\n\
                 analysis = {absolute:?} # analysis comment\n\
                 ai_models = {absolute:?} # AI models comment\n\
                 future_cache = {absolute:?} # paths unknown\n\
                 [plugins]\n\
                 enabled = true # unrelated\n"
        ),
    )
    .unwrap();

    let mut s = load(&path).unwrap();
    s.ai.model = "changed-elsewhere".into();
    save(&path, &s).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    for comment in [
        "# config heading",
        "# root comment",
        "# quota comment",
        "# storage unknown",
        "# proxy comment",
        "# analysis comment",
        "# AI models comment",
        "# paths unknown",
        "# unrelated",
    ] {
        assert!(raw.contains(comment), "lost {comment}: {raw}");
    }
    assert!(raw.contains("future_policy = \"keep\""), "{raw}");
    assert!(raw.contains("future_cache = "), "{raw}");
    assert!(raw.contains("[plugins]"), "{raw}");
    assert!(raw.contains("enabled = true"), "{raw}");
}

#[cfg(unix)]
#[test]
fn non_utf8_storage_path_returns_error_without_rewriting() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "# keep me\n[future]\nvalue = true\n";
    std::fs::write(&path, original).unwrap();

    let mut s = Settings::default();
    s.storage.root = Some(PathBuf::from(OsString::from_vec(
        b"/tmp/aria-\xff".to_vec(),
    )));
    let error = save(&path, &s).unwrap_err();
    assert!(error.contains("UTF-8"), "{error}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn parses_each_section_and_tolerates_unknown_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
[editor]
something_else = true

[ai]
base_url = "http://localhost:11434/v1"
model = "qwen3:14b"
api_key_env = "OPENAI_API_KEY"

[appearance]
theme = "ember"
"#,
    )
    .unwrap();

    let s = load(&path).unwrap();
    assert_eq!(s.ai.base_url, "http://localhost:11434/v1");
    assert_eq!(s.ai.model, "qwen3:14b");
    assert_eq!(s.ai.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
    // Pre-source configs with URL+model migrate to freeform Advanced.
    assert_eq!(s.ai.source, AiSource::Custom);
    assert!(s.ai.is_configured());
    assert_eq!(s.appearance.theme, ThemeChoice::Ember);
}

#[test]
fn malformed_file_is_an_error_not_a_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let secret = "sk-must-not-appear-in-errors";
    let malformed = format!("[ai]\napi_key = {secret}\n");
    std::fs::write(&path, &malformed).unwrap();
    assert!(load(&path).unwrap_err().contains("could not parse"));
    let save_error = save(&path, &Settings::default()).unwrap_err();
    assert!(save_error.contains("could not parse"), "{save_error}");
    assert!(!save_error.contains(secret), "{save_error}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), malformed);
    assert_no_transaction_artifacts(dir.path());
}

#[test]
fn save_round_trips_and_preserves_comments_and_unknown_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
            &path,
            "# my aria config\n[ai]\nbase_url = \"http://x/v1\"  # local\nmodel = \"m\"\n\n[plugins]\nkeep = true\n",
        )
        .unwrap();

    let mut s = load(&path).unwrap();
    s.appearance.theme = ThemeChoice::Default;
    s.ai.model = "qwen3:14b".into();
    save(&path, &s).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("# my aria config"), "leading comment kept");
    assert!(raw.contains("# local"), "inline comment kept");
    assert!(raw.contains("[plugins]"), "unknown table kept");
    assert!(raw.contains("keep = true"));

    let reloaded = load(&path).unwrap();
    assert_eq!(reloaded.ai.model, "qwen3:14b");
    assert_eq!(reloaded.appearance.theme, ThemeChoice::Default);
}

#[test]
fn preserves_tables_from_other_builds() {
    // A config written by a build that still had a `[cache]` table (or any
    // future section) must survive a save from this one untouched.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[cache]\nbudget_mb = 1024\n").unwrap();

    let mut s = load(&path).unwrap();
    s.ai.base_url = "http://x/v1".into();
    s.ai.model = "m".into();
    save(&path, &s).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("[cache]"), "unowned table kept: {raw}");
    assert!(raw.contains("budget_mb = 1024"));
}
