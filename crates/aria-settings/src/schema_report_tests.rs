//! What [`report`] must say about a configuration this build did not write.
//!
//! The class closed here is "a diagnostics row that agrees with itself instead
//! of with the loader". Every case below reads the shape set from
//! [`PersistedShape::all`] at run time and compares sets, so a shape added to
//! the enum joins these tests by existing; none of them holds a list of tables
//! or a count.
//!
//! What this does not catch: whether the loader's refusal *message* names the
//! same numbers. That is `schema/stale_tests.rs`, which drives `load` itself.

use std::collections::BTreeSet;

use super::*;
use crate::schema::RefusalReason;

fn shapes() -> BTreeSet<PersistedShape> {
    PersistedShape::all().collect()
}

#[test]
fn every_shape_is_reported_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let rows = report(&dir.path().join("config.toml"));
    let reported: BTreeSet<PersistedShape> = rows.iter().map(|row| row.shape).collect();
    assert_eq!(
        reported,
        shapes(),
        "a persisted shape is missing from the report"
    );
    assert_eq!(
        rows.len(),
        reported.len(),
        "a shape is reported more than once"
    );
}

#[test]
fn a_missing_file_is_legacy_and_absent_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    let rows = report(&dir.path().join("config.toml"));
    for row in &rows {
        assert_eq!(
            row.recorded,
            Some(crate::schema::LEGACY_VERSION),
            "{}: a file that does not exist records nothing",
            row.table
        );
        assert!(!row.present, "{}: nothing is in a missing file", row.table);
        assert!(
            !row.refused(),
            "{}: a missing file must still load: {}",
            row.table,
            row.summary()
        );
    }
}

#[test]
fn a_shape_this_build_migrates_says_so_and_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    // Written at the legacy version for every shape, which is what a file from
    // before `[schema]` existed looks like.
    std::fs::write(&path, "[ai]\nmodel = \"qwen3:14b\"\n").unwrap();

    let rows = report(&path);
    let ai = rows
        .iter()
        .find(|row| row.shape == PersistedShape::Ai)
        .expect("the ai shape is reported");
    assert!(ai.present, "the [ai] table is in the file");
    assert_eq!(
        ai.decision,
        Some(VersionDecision::Migrate {
            from: crate::schema::LEGACY_VERSION
        }),
        "{}",
        ai.summary()
    );
    assert!(!ai.refused(), "a migratable shape loads: {}", ai.summary());
    assert!(
        ai.summary().contains(&ai.current.to_string()),
        "the line must name the version it migrates to: {}",
        ai.summary()
    );
}

#[test]
fn a_version_above_this_build_is_refused_per_shape() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    // One entry per shape, each one past what this build reads. Derived from the
    // registry so a new shape is covered without editing this test.
    let mut raw = String::from("[schema]\n");
    for shape in PersistedShape::all() {
        raw.push_str(&format!(
            "{} = {}\n",
            shape.table(),
            shape.current_version() + 1
        ));
    }
    std::fs::write(&path, raw).unwrap();

    for row in report(&path) {
        assert!(
            row.refused(),
            "{}: a version above {} must be refused, not served: {}",
            row.table,
            row.current,
            row.summary()
        );
        assert!(
            row.summary().contains(&row.current.to_string()),
            "{}: the refusal must name what this build reads: {}",
            row.table,
            row.summary()
        );
    }
}

#[test]
fn an_unparseable_file_reports_every_shape_as_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "this is not = = toml\n").unwrap();

    let rows = report(&path);
    let reported: BTreeSet<PersistedShape> = rows.iter().map(|row| row.shape).collect();
    assert_eq!(reported, shapes(), "every shape is still accounted for");
    for row in &rows {
        assert_eq!(row.recorded, None, "{}: nothing is readable", row.table);
        assert_eq!(
            row.decision, None,
            "{}: there is nothing to decide",
            row.table
        );
        assert!(
            row.refused(),
            "{}: an unreadable file is refused",
            row.table
        );
    }
}

#[test]
fn a_report_never_claims_a_shape_the_loader_would_serve_is_refused() {
    // Both sides of the same file: what `report` says per shape, and whether
    // `load` accepts it. They must not disagree.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut raw = String::from("[schema]\n");
    for shape in PersistedShape::all() {
        raw.push_str(&format!(
            "{} = {}\n",
            shape.table(),
            shape.current_version()
        ));
    }
    std::fs::write(&path, raw).unwrap();

    let rows = report(&path);
    assert!(
        rows.iter().all(|row| !row.refused()),
        "a file recording exactly what this build writes must not be refused"
    );
    assert!(
        crate::load(&path).is_ok(),
        "the loader disagrees with the report"
    );
}

#[test]
fn every_table_individually_migrates_or_reports_correctly() {
    for shape in PersistedShape::all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let table_name = shape.table();
        let current_ver = shape.current_version();

        // 1. Current version in [schema]
        std::fs::write(
            &path,
            format!("[schema]\n{table_name} = {current_ver}\n[{table_name}]\n"),
        )
        .unwrap();
        let rows = report(&path);
        let row = rows
            .iter()
            .find(|r| r.shape == shape)
            .expect("shape must be reported");
        assert!(row.present);
        assert_eq!(row.recorded, Some(current_ver));
        assert_eq!(row.decision, Some(VersionDecision::Current));
        assert!(!row.refused());

        // 2. Future version rejected
        let future_ver = current_ver + 1;
        std::fs::write(
            &path,
            format!("[schema]\n{table_name} = {future_ver}\n[{table_name}]\n"),
        )
        .unwrap();
        let rows = report(&path);
        let row = rows
            .iter()
            .find(|r| r.shape == shape)
            .expect("shape must be reported");
        assert!(row.refused());
        assert_eq!(
            row.decision,
            Some(VersionDecision::Refuse(RefusalReason::FromNewerBuild))
        );
    }
}

/// WHY: a served table's row printed `Version 1, current — in the file`, and
/// the Settings dialog showed the em dash beside rows that separate a clause
/// with a semicolon. Every shape under every decision, present and absent,
/// reads as one line with no em dash, and a served table states where it is
/// read from after the semicolon. Does not check the wording of the clauses.
#[test]
fn every_summary_reads_without_an_em_dash() {
    for shape in PersistedShape::all() {
        let current = shape.current_version();
        let decisions = [
            None,
            Some(VersionDecision::Current),
            Some(VersionDecision::Migrate { from: 0 }),
            Some(VersionDecision::Refuse(RefusalReason::FromNewerBuild)),
            Some(VersionDecision::Refuse(RefusalReason::TooOld)),
        ];
        for (decision, present) in decisions
            .into_iter()
            .flat_map(|decision| [(decision, true), (decision, false)])
        {
            let row = ShapeStatus {
                shape,
                table: shape.table(),
                recorded: decision.map(|_| current),
                current,
                oldest_migratable: shape.oldest_migratable_version(),
                decision,
                present,
            };
            let line = row.summary();
            assert!(
                !line.contains('\u{2014}') && !line.contains('\n'),
                "{shape:?} {decision:?}: {line}"
            );
            if matches!(
                decision,
                Some(VersionDecision::Current | VersionDecision::Migrate { .. })
            ) {
                let place = if present {
                    "in the file"
                } else {
                    "not in the file"
                };
                assert!(
                    line.split_once("; ")
                        .is_some_and(|(_, from)| from.starts_with(place)),
                    "{shape:?} {decision:?}: {line}"
                );
            }
        }
    }
}
