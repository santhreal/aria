//! Relink All matching cases.
//!
//! What these defend: an entry is bound only when exactly one same-named file
//! agrees on every fact the project remembers, and every entry that is not
//! bound carries a reason. The old behaviour — bind the first file whose name
//! matched — passes none of `two_matches_are_ambiguous`,
//! `a_different_size_is_refused`, `a_different_duration_is_refused` or
//! `a_different_audio_track_is_refused`.
//!
//! What they do not catch: the walk and the probe. [`super::walk`] reads real
//! directories and [`super::probe_facts`] opens a real container, so those are
//! exercised by the app path, not here.

use std::path::{Path, PathBuf};

use super::{Fact, Facts, Refusal, plan};
use crate::types::{UiClipKind, UiMediaItem};

/// A missing pool entry: `1920x1080`, 240 frames, silent, unless changed.
fn entry(id: u64, path: &str) -> UiMediaItem {
    UiMediaItem {
        id,
        name: "clip".into(),
        path: path.into(),
        width: 1920,
        height: 1080,
        frame_rate: aria_core::Rational { num: 30, den: 1 },
        duration_ticks: 240,
        duration_label: "00:00:08:00".into(),
        has_audio: false,
        kind: UiClipKind::Media,
        missing: true,
        label: None,
        imported_at: None,
        video_codec: None,
        audio_codec: None,
        bin: None,
        proxied: false,
        position: None,
    }
}

/// The facts an entry built by [`entry`] is looking for.
fn same() -> Facts {
    Facts {
        width: 1920,
        height: 1080,
        duration_ticks: 240,
        has_audio: false,
    }
}

/// Read a table instead of a file. Any path not listed fails to open, which
/// is also what an unreadable candidate does.
fn table<const N: usize>(
    rows: [(&'static str, Facts); N],
) -> impl FnMut(&Path) -> Result<Facts, String> {
    move |path: &Path| {
        rows.iter()
            .find(|(name, _)| Path::new(name) == path)
            .map(|(_, facts)| *facts)
            .ok_or_else(|| "no such file".to_string())
    }
}

fn at(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[test]
fn one_agreeing_file_binds() {
    let missing = [entry(7, "/gone/a.mp4")];
    let found = [at("/card/a.mp4")];
    let plan = plan(&missing, &found, table([("/card/a.mp4", same())]));
    assert_eq!(plan.binds, vec![(7, at("/card/a.mp4"))]);
    assert!(plan.refused.is_empty(), "{:?}", plan.refused);
}

#[test]
fn the_name_is_matched_case_folded() {
    let missing = [entry(7, "/gone/C0001.MP4")];
    let found = [at("/card/c0001.mp4")];
    let plan = plan(&missing, &found, table([("/card/c0001.mp4", same())]));
    assert_eq!(plan.binds, vec![(7, at("/card/c0001.mp4"))]);
}

#[test]
fn the_folder_is_searched_by_name_not_position() {
    let missing = [entry(7, "/gone/a.mp4")];
    let found = [at("/card/b.mp4"), at("/card/nested/a.mp4")];
    let plan = plan(
        &missing,
        &found,
        table([("/card/b.mp4", same()), ("/card/nested/a.mp4", same())]),
    );
    assert_eq!(plan.binds, vec![(7, at("/card/nested/a.mp4"))]);
}

#[test]
fn two_matches_are_ambiguous() {
    let missing = [entry(7, "/gone/a.mp4")];
    let found = [at("/card/one/a.mp4"), at("/card/two/a.mp4")];
    let plan = plan(
        &missing,
        &found,
        table([("/card/one/a.mp4", same()), ("/card/two/a.mp4", same())]),
    );
    assert!(plan.binds.is_empty(), "an ambiguous name must not bind");
    assert_eq!(
        plan.refused,
        vec![(7, "a.mp4".into(), Refusal::Ambiguous(2))]
    );
}

#[test]
fn the_one_agreeing_candidate_wins_over_a_same_named_stranger() {
    let missing = [entry(7, "/gone/a.mp4")];
    let found = [at("/card/one/a.mp4"), at("/card/two/a.mp4")];
    let other = Facts {
        width: 1280,
        ..same()
    };
    let plan = plan(
        &missing,
        &found,
        table([("/card/one/a.mp4", other), ("/card/two/a.mp4", same())]),
    );
    assert_eq!(plan.binds, vec![(7, at("/card/two/a.mp4"))]);
    assert!(plan.refused.is_empty(), "{:?}", plan.refused);
}

#[test]
fn a_missing_name_is_refused() {
    let missing = [entry(7, "/gone/a.mp4")];
    let plan = plan(&missing, &[at("/card/b.mp4")], table([]));
    assert_eq!(
        plan.refused,
        vec![(7, "a.mp4".into(), Refusal::NoCandidate)]
    );
}

#[test]
fn a_different_size_is_refused() {
    let missing = [entry(7, "/gone/a.mp4")];
    let facts = Facts {
        height: 720,
        ..same()
    };
    let plan = plan(
        &missing,
        &[at("/card/a.mp4")],
        table([("/card/a.mp4", facts)]),
    );
    assert!(
        plan.binds.is_empty(),
        "a different frame size must not bind"
    );
    assert_eq!(
        plan.refused,
        vec![(
            7,
            "a.mp4".into(),
            Refusal::Differs {
                fact: Fact::Size,
                seen: 1
            }
        )]
    );
}

#[test]
fn a_different_duration_is_refused() {
    let missing = [entry(7, "/gone/a.mp4")];
    let facts = Facts {
        duration_ticks: 239,
        ..same()
    };
    let plan = plan(
        &missing,
        &[at("/card/a.mp4")],
        table([("/card/a.mp4", facts)]),
    );
    assert_eq!(
        plan.refused,
        vec![(
            7,
            "a.mp4".into(),
            Refusal::Differs {
                fact: Fact::Duration,
                seen: 1
            }
        )]
    );
}

#[test]
fn a_different_audio_track_is_refused() {
    let missing = [entry(7, "/gone/a.mp4")];
    let facts = Facts {
        has_audio: true,
        ..same()
    };
    let plan = plan(
        &missing,
        &[at("/card/a.mp4")],
        table([("/card/a.mp4", facts)]),
    );
    assert_eq!(
        plan.refused,
        vec![(
            7,
            "a.mp4".into(),
            Refusal::Differs {
                fact: Fact::Audio,
                seen: 1
            }
        )]
    );
}

#[test]
fn an_unreadable_candidate_reports_the_failure() {
    let missing = [entry(7, "/gone/a.mp4")];
    let plan = plan(&missing, &[at("/card/a.mp4")], table([]));
    assert_eq!(
        plan.refused,
        vec![(
            7,
            "a.mp4".into(),
            Refusal::Unreadable("no such file".into())
        )]
    );
}

#[test]
fn a_candidate_is_read_once_however_many_entries_want_it() {
    let missing = [entry(7, "/gone/a.mp4"), entry(8, "/elsewhere/a.mp4")];
    let mut reads = 0usize;
    let plan = plan(&missing, &[at("/card/a.mp4")], |path| {
        reads += 1;
        assert_eq!(path, Path::new("/card/a.mp4"));
        Ok(same())
    });
    assert_eq!(reads, 1, "a probe opens the file; do it once per candidate");
    assert_eq!(plan.binds.len(), 2, "both entries agree with the same file");
}

#[test]
fn every_entry_is_accounted_for() {
    let missing = [
        entry(1, "/gone/a.mp4"),
        entry(2, "/gone/b.mp4"),
        entry(3, "/gone/c.mp4"),
    ];
    let found = [at("/card/a.mp4"), at("/card/c.mp4")];
    let wrong = Facts {
        width: 640,
        ..same()
    };
    let plan = plan(
        &missing,
        &found,
        table([("/card/a.mp4", same()), ("/card/c.mp4", wrong)]),
    );
    assert_eq!(
        plan.binds.len() + plan.refused.len(),
        missing.len(),
        "a Relink All that skips silently is the defect this closes"
    );
}

#[test]
fn every_refusal_states_a_reason() {
    let refusals = [
        Refusal::NoCandidate,
        Refusal::Ambiguous(3),
        Refusal::Differs {
            fact: Fact::Size,
            seen: 1,
        },
        Refusal::Differs {
            fact: Fact::Duration,
            seen: 4,
        },
        Refusal::Differs {
            fact: Fact::Audio,
            seen: 1,
        },
        Refusal::Unreadable("permission denied".into()),
    ];
    for refusal in refusals {
        let line = refusal.describe();
        assert!(!line.is_empty(), "{refusal:?} described itself as nothing");
        let first = line.chars().next();
        assert!(
            first.is_some_and(|c| !c.is_uppercase()),
            "{refusal:?} reads as a clause after the file name: {line}"
        );
    }
}
