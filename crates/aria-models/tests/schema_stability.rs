//! Golden project-format compatibility across every supported schema version.
//!
//! WHY: a parser-only migration test can stay green after a released document
//! shape becomes unreadable. These fixtures exercise the public load/save API
//! and require one checked-in document for every version through
//! `PROJECT_SCHEMA_VERSION`. This does not prove compatibility with future
//! versions, which the loader rejects by contract.

use std::collections::BTreeSet;
use std::path::PathBuf;

use aria_models::{PROJECT_SCHEMA_KIND, PROJECT_SCHEMA_VERSION, Project};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema")
}

#[test]
fn every_supported_schema_fixture_roundtrips_through_the_public_api() {
    let mut found = BTreeSet::new();

    for entry in std::fs::read_dir(fixtures()).expect("schema fixture directory") {
        let path = entry.expect("schema fixture entry").path();
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("aria"),
            "unexpected file in schema fixture directory: {}",
            path.display()
        );

        let source = std::fs::read_to_string(&path).expect("read schema fixture");
        let document: serde_json::Value =
            serde_json::from_str(&source).expect("fixture is valid JSON");
        let version = document["schema"]["version"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .expect("fixture schema version is a u32");
        assert!(
            found.insert(version),
            "more than one fixture declares schema v{version}"
        );
        assert_eq!(document["schema"]["kind"], PROJECT_SCHEMA_KIND);
        assert_eq!(document["name"], format!("schema-v{version}"));

        let loaded = Project::load_from_string(&source)
            .unwrap_or_else(|error| panic!("{} did not load: {error}", path.display()));
        assert_eq!(loaded.schema.version, version);
        assert_eq!(loaded.media_count(), 0);
        assert_eq!(loaded.timeline().track_count(), 0);

        let saved = loaded.save_to_string().expect("save migrated fixture");
        let saved_document: serde_json::Value =
            serde_json::from_str(&saved).expect("saved project is valid JSON");
        assert_eq!(saved_document["schema"]["version"], PROJECT_SCHEMA_VERSION);
        assert_eq!(saved_document["schema"]["kind"], PROJECT_SCHEMA_KIND);

        let reloaded = Project::load_from_string(&saved).expect("reload migrated fixture");
        assert_eq!(
            reloaded.save_to_string().expect("save reloaded fixture"),
            saved,
            "schema v{version} did not reach a stable current representation"
        );
    }

    let expected: BTreeSet<u32> = (1..=PROJECT_SCHEMA_VERSION).collect();
    assert_eq!(
        found, expected,
        "add one golden .aria fixture for every supported schema version"
    );
}
