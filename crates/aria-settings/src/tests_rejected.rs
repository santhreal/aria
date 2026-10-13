//! What the loader dropped, and how it says so.
//!
//! The class closed here is "a value in the file had no effect and nothing
//! said so". `root = "cache"` under `[storage]` is the case that was found in
//! the field: a relative path is refused, the caches stay under the profile
//! directory, and before this the operator had a config line claiming
//! otherwise and no way to see it was dead. Every read of an owned key now
//! records what it stepped over, the session still starts on the defaults, and
//! the last test here fails if a new read is added that cannot report.

use super::*;

/// Load a document held in memory. No file, because nothing here is about IO:
/// [`load`] is a read plus a parse plus this.
fn loaded(raw: &str) -> Settings {
    let doc = raw
        .parse::<DocumentMut>()
        .unwrap_or_else(|e| panic!("{raw}: {e}"));
    Settings::from_document(&doc).unwrap_or_else(|e| panic!("{raw}: {e}"))
}

/// One fixture per refusal the loader can produce, so `every_refusal_is_produced`
/// goes red when a variant arrives without a case.
const REFUSED: &[&str] = &[
    "[storage]\nroot = \"cache\"\n",
    "[ai]\nmodel = 3\n",
    "[appearance]\ntheme = \"purple\"\n",
    "[storage]\ndownload_quota_mib = 0\n",
];

#[test]
fn a_relative_storage_root_is_rejected_and_the_rest_of_the_table_still_loads() {
    let s = loaded("[storage]\nroot = \"cache\"\ndownload_quota_mib = 4096\n");

    assert_eq!(s.storage.root, None);
    assert_eq!(s.storage.download_quota_mib, 4096);
    assert_eq!(
        s.rejected,
        vec![Rejected {
            key: "storage.root".into(),
            value: "\"cache\"".into(),
            why: Refusal::NotAbsolute,
        }]
    );
    let line = s.rejected[0].line();
    assert!(line.starts_with("storage.root = \"cache\": "), "{line}");
    assert!(line.contains("absolute path"), "{line}");
}

#[test]
fn a_relative_override_under_storage_paths_is_rejected_by_its_dotted_key() {
    let analysis = crate::fixture_abs("mnt/a");
    let s = loaded(&format!(
        "[storage.paths]\nproxies = \"proxies\"\nanalysis = \"{analysis}\"\n"
    ));

    assert_eq!(s.storage.paths.get("proxies"), None);
    assert_eq!(s.storage.paths.get("analysis"), Some(Path::new(&analysis)));
    assert_eq!(
        s.rejected,
        vec![Rejected {
            key: "storage.paths.proxies".into(),
            value: "\"proxies\"".into(),
            why: Refusal::NotAbsolute,
        }]
    );
}

#[test]
fn a_wrongly_typed_value_is_rejected_and_its_key_keeps_the_default() {
    let s = loaded("[ai]\nmodel = 3\nbase_url = \"http://host/v1\"\n");

    assert_eq!(s.ai.model, Settings::default().ai.model);
    assert_eq!(s.ai.base_url, "http://host/v1");
    assert_eq!(
        s.rejected,
        vec![Rejected {
            key: "ai.model".into(),
            value: "3".into(),
            why: Refusal::WrongType,
        }]
    );
}

#[test]
fn a_table_where_a_value_belongs_is_rejected_as_a_table() {
    let s = loaded("[ai.model]\nname = \"qwen3:14b\"\n");

    assert_eq!(s.ai.model, Settings::default().ai.model);
    assert_eq!(
        s.rejected,
        vec![Rejected {
            key: "ai.model".into(),
            value: "a table".into(),
            why: Refusal::WrongType,
        }]
    );
}

#[test]
fn a_top_level_table_that_is_not_a_table_is_rejected_by_its_own_name() {
    let s = loaded("appearance = 3\n");

    assert_eq!(s.appearance, Settings::default().appearance);
    assert_eq!(
        s.rejected,
        vec![Rejected {
            key: "appearance".into(),
            value: "3".into(),
            why: Refusal::WrongType,
        }]
    );
}

#[test]
fn a_quota_outside_the_accepted_range_is_rejected_as_out_of_range() {
    for (raw, value) in [
        ("[storage]\ndownload_quota_mib = 0\n", "0"),
        ("[storage]\ndownload_quota_mib = -1\n", "-1"),
        (
            &format!(
                "[storage]\ndownload_quota_mib = {}\n",
                MAX_DOWNLOAD_QUOTA_MIB + 1
            ),
            "1048577",
        ),
    ] {
        let s = loaded(raw);
        assert_eq!(
            s.storage.download_quota_mib,
            Settings::default().storage.download_quota_mib,
            "{raw}"
        );
        assert_eq!(
            s.rejected,
            vec![Rejected {
                key: "storage.download_quota_mib".into(),
                value: value.into(),
                why: Refusal::OutOfRange,
            }],
            "{raw}"
        );
    }
}

#[test]
fn a_timeout_outside_the_accepted_range_is_rejected_by_key() {
    let s = loaded("[transcription]\nread_timeout_secs = 0\ntotal_timeout_secs = 60\n");

    assert_eq!(
        s.transcription.read_timeout_secs,
        Settings::default().transcription.read_timeout_secs
    );
    assert_eq!(s.transcription.total_timeout_secs, 60);
    assert_eq!(
        s.rejected,
        vec![Rejected {
            key: "transcription.read_timeout_secs".into(),
            value: "0".into(),
            why: Refusal::OutOfRange,
        }]
    );
}

#[test]
fn a_name_outside_a_key_s_set_of_choices_is_rejected_as_not_one_of_them() {
    let s = loaded("[appearance]\ntheme = \"purple\"\n");

    assert_eq!(s.appearance.theme, Settings::default().appearance.theme);
    assert_eq!(
        s.rejected,
        vec![Rejected {
            key: "appearance.theme".into(),
            value: "\"purple\"".into(),
            why: Refusal::NotOneOf,
        }]
    );
}

#[test]
fn a_provider_entry_that_is_not_a_table_is_rejected_by_provider_name() {
    let s =
        loaded("[providers]\npexels = \"sk-literal\"\n\n[providers.pixabay]\napi_key = \"k\"\n");

    assert_eq!(s.provider("pexels"), ProviderSettings::default());
    assert_eq!(s.provider("pixabay").api_key.as_deref(), Some("k"));
    assert_eq!(
        s.rejected,
        vec![Rejected {
            key: "providers.pexels".into(),
            value: "\"sk-literal\"".into(),
            why: Refusal::WrongType,
        }]
    );
}

#[test]
fn a_plugin_value_that_is_not_a_string_is_rejected_by_its_declared_key() {
    let s = loaded("[plugins]\n\"notes.directory\" = 3\n\"notes.title\" = \"Notes\"\n");

    assert_eq!(s.plugin_value("notes.directory"), None);
    assert_eq!(s.plugin_value("notes.title"), Some("Notes"));
    assert_eq!(
        s.rejected,
        vec![Rejected {
            key: "plugins.notes.directory".into(),
            value: "3".into(),
            why: Refusal::WrongType,
        }]
    );
}

/// An absent key is the normal state of a partially-written file and is not a
/// rejection: nothing was dropped, so there is nothing to report. Neither is an
/// unknown key, which belongs to a build this one is not.
fn a_file_this_build_understands_leaves_nothing_rejected() {
    let media_root = crate::fixture_abs("mnt/media");
    for raw in [
        String::new(),
        "[ai]\nmodel = \"qwen3:14b\"\n".into(),
        format!("[storage]\nroot = \"{media_root}\"\n"),
        "[storage]\nroot = \"\"\n".into(),
        "[appearance]\ntheme = \"ember\"\nfuture_key = 3\n".into(),
        "[transcription]\nread_timeout_secs = 45\n".into(),
    ] {
        assert_eq!(loaded(&raw).rejected, Vec::new(), "{raw}");
    }
}

/// A save never writes a value the loader would refuse, so a round trip has
/// nothing to report. This is the invariant that keeps `rejected` a statement
/// about the file rather than about this crate.
#[test]
fn nothing_this_crate_writes_is_rejected_when_it_is_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let notes_dir = crate::fixture_abs("mnt/notes");
    let mut settings = Settings::default();
    settings.storage.root = Some(PathBuf::from(crate::fixture_abs("mnt/media/aria")));
    settings.appearance.theme = ThemeChoice::Ember;
    settings.ai.api_key = Some("sk-literal".into());
    settings.set_plugin_value("notes.directory", Some(&notes_dir));
    settings.providers.insert(
        "pexels".into(),
        ProviderSettings {
            api_key_env: Some("PEXELS_API_KEY".into()),
            ..ProviderSettings::default()
        },
    );

    save(&path, &settings).unwrap();
    let back = load(&path).unwrap();

    assert_eq!(back.rejected, Vec::new());
    assert_eq!(back, settings);
}

#[test]
fn every_refusal_is_produced_by_a_fixture_and_reads_as_its_own_reason() {
    let produced: Vec<Refusal> = REFUSED
        .iter()
        .flat_map(|raw| loaded(raw).rejected)
        .map(|rejected| rejected.why)
        .collect();

    for why in Refusal::ALL {
        assert!(
            produced.contains(&why),
            "no fixture in REFUSED produces {why:?}, so its wording is untested"
        );
        let reason = why.reason();
        assert!(!reason.is_empty(), "{why:?} has no reason");
        assert!(!reason.ends_with('.'), "{why:?}: {reason}");
        assert_eq!(
            Refusal::ALL
                .iter()
                .filter(|other| other.reason() == reason)
                .count(),
            1,
            "{why:?} shares its reason with another refusal"
        );
    }
}

/// The rule that makes the rest of this file more than a set of examples: a key
/// read straight out of the table cannot report anything, which is how
/// `storage.root` went missing. Adding one to the loader turns this red, and
/// the fix is to call a helper rather than to widen this list.
#[test]
fn nothing_in_the_loader_reads_a_value_without_a_way_to_reject_it() {
    let body = from_document_body();

    for direct in [
        ".as_str(",
        ".as_integer(",
        ".as_bool(",
        ".as_float(",
        ".as_table(",
        ".as_table_like(",
        ".as_value(",
        ".get(",
    ] {
        assert!(
            !body.contains(direct),
            "from_document reads a value with {direct}, which cannot report a \
             rejected value; use one of the reading helpers"
        );
    }

    for helper in [
        "section_at(",
        "table_at(",
        "table_item(",
        "text_at(",
        "text_item(",
        "choice_at(",
        "absolute_path_at(",
        "bounded_at(",
    ] {
        assert!(
            body.contains(helper),
            "{helper} is defined but the loader no longer calls it"
        );
    }
}

/// The text of `Settings::from_document`, brace-matched out of its own source.
fn from_document_body() -> &'static str {
    const SOURCE: &str = include_str!("read.rs");
    let at = SOURCE
        .find("fn from_document")
        .expect("from_document lives in read.rs");
    let body = &SOURCE[at..];
    let mut depth = 0usize;
    for (offset, ch) in body.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &body[..=offset];
                }
            }
            _ => {}
        }
    }
    panic!("from_document has no closing brace");
}
