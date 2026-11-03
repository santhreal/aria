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

#[test]
fn unsupported_schema_versions_fail_closed_naming_both_versions_and_corrective_action() {
    for future_version in [PROJECT_SCHEMA_VERSION + 1, PROJECT_SCHEMA_VERSION + 42] {
        let doc = serde_json::json!({
            "schema": { "version": future_version, "kind": PROJECT_SCHEMA_KIND },
            "id": 1,
            "name": "future",
            "metadata": {},
            "media": [],
            "timeline": {
                "frame_rate": { "num": 24, "den": 1 },
                "tracks": [],
                "order": [],
                "clip_index": []
            }
        });
        let err = Project::load_from_string(&doc.to_string())
            .expect_err("future schema version must fail closed");
        let msg = err.to_string();
        assert!(
            msg.contains(&format!("version {future_version}")),
            "error must name file version {future_version}: {msg}"
        );
        assert!(
            msg.contains(&format!("versions 1 to {PROJECT_SCHEMA_VERSION}")),
            "error must name supported versions 1 to {PROJECT_SCHEMA_VERSION}: {msg}"
        );
        assert!(
            msg.contains("update Aria"),
            "error must name corrective action: {msg}"
        );
    }

    let zero_doc = serde_json::json!({
        "schema": { "version": 0, "kind": PROJECT_SCHEMA_KIND },
        "id": 1,
        "name": "zero",
        "metadata": {},
        "media": [],
        "timeline": {
            "frame_rate": { "num": 24, "den": 1 },
            "tracks": [],
            "order": [],
            "clip_index": []
        }
    });
    let zero_err = Project::load_from_string(&zero_doc.to_string())
        .expect_err("schema v0 must fail closed")
        .to_string();
    assert!(zero_err.contains("version 0"), "{zero_err}");
    assert!(
        zero_err.contains(&format!("versions 1 to {PROJECT_SCHEMA_VERSION}")),
        "{zero_err}"
    );
    assert!(
        zero_err.contains("open a valid .aria project file"),
        "{zero_err}"
    );

    let foreign_doc = serde_json::json!({
        "schema": { "version": PROJECT_SCHEMA_VERSION, "kind": "aria.not_a_project" },
        "id": 1,
        "name": "foreign",
        "metadata": {},
        "media": [],
        "timeline": {
            "frame_rate": { "num": 24, "den": 1 },
            "tracks": [],
            "order": [],
            "clip_index": []
        }
    });
    let foreign_err = Project::load_from_string(&foreign_doc.to_string())
        .expect_err("foreign schema kind must fail closed")
        .to_string();
    assert!(foreign_err.contains("aria.not_a_project"), "{foreign_err}");
    assert!(
        foreign_err.contains(&format!("versions 1 to {PROJECT_SCHEMA_VERSION}")),
        "{foreign_err}"
    );
    assert!(
        foreign_err.contains("open a valid .aria project file"),
        "{foreign_err}"
    );
}

#[test]
fn project_schema_guide_documents_the_current_version_and_every_migration() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let guide_path = crate_root.join("../../docs/guide/settings-and-storage.md");
    let guide = std::fs::read_to_string(&guide_path).expect("read settings-and-storage.md");
    assert!(
        guide.contains(PROJECT_SCHEMA_KIND),
        "settings-and-storage.md must document {PROJECT_SCHEMA_KIND}"
    );
    assert!(
        guide.contains(&format!("version = {PROJECT_SCHEMA_VERSION}")),
        "settings-and-storage.md must document current version {PROJECT_SCHEMA_VERSION}"
    );
    for v in 1..=PROJECT_SCHEMA_VERSION {
        assert!(
            guide.contains(&format!("| `{v}` |")),
            "settings-and-storage.md must document schema version {v} in the history table"
        );
    }
}
