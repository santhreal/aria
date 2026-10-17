//! `ai_source` cases.

use super::*;

#[test]
fn ai_source_parses_migrates_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    // Missing source + empty AI → Local default, not configured.
    std::fs::write(&path, "[ai]\n").unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.ai.source, AiSource::Local);
    assert!(!loaded.ai.is_configured());

    // Explicit openrouter needs a key to be configured.
    std::fs::write(
        &path,
        "[ai]\nsource = \"openrouter\"\nmodel = \"openai/gpt-5.6-sol\"\napi_key = \"sk-or\"\n",
    )
    .unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.ai.source, AiSource::OpenRouter);
    assert!(loaded.ai.is_configured());

    std::fs::write(
        &path,
        "[ai]\nsource = \"openrouter\"\nmodel = \"openai/gpt-5.6-sol\"\n",
    )
    .unwrap();
    assert!(!load(&path).unwrap().ai.is_configured());

    // Round-trip non-default source.
    let mut settings = Settings::default();
    settings.ai.source = AiSource::OpenRouter;
    settings.ai.model = "openai/gpt-5.6-sol".into();
    settings.ai.api_key = Some("sk-or".into());
    save(&path, &settings).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("source = \"openrouter\""), "{raw}");
    assert_eq!(load(&path).unwrap().ai.source, AiSource::OpenRouter);

    // Default Local omits the key.
    settings.ai.source = AiSource::Local;
    settings.ai.base_url = "http://localhost:11434/v1".into();
    settings.ai.model = "qwen3:14b".into();
    settings.ai.api_key = None;
    save(&path, &settings).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("source"), "{raw}");
}

/// Download timeouts are a config the downloader will reject if they are wrong, so the
/// class closed here is "a value that reaches `HttpDownloader` unusable": zero, negative,
/// absurd, or a garbage type. Each keeps the default on load and is refused on save.
#[test]
fn transcription_timeouts_round_trip_and_reject_unusable_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let defaults = TranscriptionSettings::default();
    assert!(TranscriptionSettings::is_valid_timeout_secs(
        defaults.connect_timeout_secs
    ));

    // Defaults are omitted: an absent table and an absent file behave identically.
    save(&path, &Settings::default()).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("transcription"), "{raw}");

    let mut settings = Settings::default();
    settings.transcription.connect_timeout_secs = 5;
    settings.transcription.read_timeout_secs = 120;
    settings.transcription.total_timeout_secs = 3_600;
    save(&path, &settings).unwrap();
    assert_eq!(load(&path).unwrap().transcription, settings.transcription);

    for bad in [0, MAX_DOWNLOAD_TIMEOUT_SECS + 1] {
        let mut broken = settings.clone();
        broken.transcription.read_timeout_secs = bad;
        let err = save(&path, &broken).expect_err("an unusable timeout was written");
        assert!(err.contains("read_timeout_secs"), "{err}");
        // The refused save left the last good file alone.
        assert_eq!(load(&path).unwrap().transcription, settings.transcription);
    }

    // A hand-edited file keeps the default for every value the downloader would
    // refuse, rather than failing the load.
    std::fs::write(
        &path,
        "[transcription]\nconnect_timeout_secs = 0\nread_timeout_secs = -3\ntotal_timeout_secs = \"soon\"\n",
    )
    .unwrap();
    assert_eq!(load(&path).unwrap().transcription, defaults);
}

/// WASM plugin limits reach the WASM host, so the class closed here is a limit
/// that reaches it unusable: zero, beyond a 32-bit memory, negative, or not a
/// number. Each keeps the default on load and is refused on save, at both ends
/// of each range.
#[test]
fn plugin_limits_round_trip_and_reject_unusable_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    save(&path, &Settings::default()).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("plugin_limits"), "{raw}");

    let mut settings = Settings::default();
    settings.plugin_limits.wasm_fuel_per_call = MIN_WASM_FUEL_PER_CALL;
    settings.plugin_limits.wasm_memory_mib = MAX_WASM_MEMORY_MIB;
    save(&path, &settings).unwrap();
    assert_eq!(load(&path).unwrap().plugin_limits, settings.plugin_limits);

    let fuel = |value| PluginLimitSettings {
        wasm_fuel_per_call: value,
        ..settings.plugin_limits
    };
    let memory = |value| PluginLimitSettings {
        wasm_memory_mib: value,
        ..settings.plugin_limits
    };
    for (bad, key) in [
        (fuel(MIN_WASM_FUEL_PER_CALL - 1), "wasm_fuel_per_call"),
        (fuel(MAX_WASM_FUEL_PER_CALL + 1), "wasm_fuel_per_call"),
        (memory(MIN_WASM_MEMORY_MIB - 1), "wasm_memory_mib"),
        (memory(MAX_WASM_MEMORY_MIB + 1), "wasm_memory_mib"),
    ] {
        let mut broken = settings.clone();
        broken.plugin_limits = bad;
        let err = save(&path, &broken).expect_err("an unusable limit was written");
        assert!(err.contains(key), "{err}");
        assert_eq!(load(&path).unwrap().plugin_limits, settings.plugin_limits);
    }

    std::fs::write(
        &path,
        "[plugin_limits]\nwasm_fuel_per_call = 0\nwasm_memory_mib = 1048576\n",
    )
    .unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.plugin_limits, PluginLimitSettings::default());
    let keys: Vec<&str> = loaded.rejected.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "plugin_limits.wasm_fuel_per_call",
            "plugin_limits.wasm_memory_mib"
        ]
    );
    std::fs::write(&path, "[plugin_limits]\nwasm_memory_mib = \"lots\"\n").unwrap();
    assert_eq!(
        load(&path).unwrap().plugin_limits,
        PluginLimitSettings::default()
    );
}

/// A plugin's declared value is stored under `[plugins]` and read back verbatim.
///
/// The class: a key this crate does not own used to be dropped on write and
/// missing on read, so a plugin could not persist a setting at all. Clearing is
/// asserted too, because "unset" and "set to empty" are different states and the
/// old value must not survive a clear.
#[test]
fn plugin_values_round_trip_and_clear() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut s = Settings::default();
    s.set_plugin_value("notes.directory", Some("/tmp/notes"));
    s.set_plugin_value("notes.token", Some(" spaced "));
    s.set_plugin_value("notes.empty", Some("   "));
    save(&path, &s).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("[plugins]"), "{raw}");
    assert!(raw.contains(r#""notes.directory" = "/tmp/notes""#), "{raw}");
    assert!(
        !raw.contains("notes.empty"),
        "a blank value was stored: {raw}"
    );

    let loaded = load(&path).unwrap();
    assert_eq!(loaded.plugin_value("notes.directory"), Some("/tmp/notes"));
    assert_eq!(
        loaded.plugin_value("notes.token"),
        Some("spaced"),
        "the value was not trimmed on the way in"
    );
    assert_eq!(loaded.plugin_value("notes.missing"), None);

    let mut cleared = loaded;
    cleared.set_plugin_value("notes.directory", None);
    save(&path, &cleared).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        !raw.contains("notes.directory"),
        "a cleared key stayed in the file: {raw}"
    );
    assert_eq!(
        load(&path).unwrap().plugin_value("notes.token"),
        Some("spaced")
    );

    // Dropping every plugin value drops the table, so a file with no plugin
    // settings has no empty section in it.
    let bare = Settings::default();
    save(&path, &bare).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("[plugins]"), "{raw}");

    // A non-string entry under `[plugins]` belongs to something this crate does
    // not model, so pruning a string key must leave it, its comment, and the
    // table itself alone.
    std::fs::write(
        &path,
        "[plugins]\nenabled = true # foreign\n\"notes.directory\" = \"/tmp/notes\"\n",
    )
    .unwrap();
    let mut mixed = load(&path).unwrap();
    assert_eq!(mixed.plugin_value("notes.directory"), Some("/tmp/notes"));
    mixed.set_plugin_value("notes.directory", None);
    save(&path, &mixed).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("enabled = true"), "{raw}");
    assert!(raw.contains("# foreign"), "{raw}");
    assert!(!raw.contains("notes.directory"), "{raw}");
}
