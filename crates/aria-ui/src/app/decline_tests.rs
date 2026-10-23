//! Contracts for a declined command.
//!
//! The class closed here is "a command that finds nothing to act on does
//! nothing and says nothing". What is asserted:
//!
//! - Every reason has its own words, walked from the registry rather than
//!   retyped, so a reason added without wording is red here.
//! - The shared reader declines once for the whole family: a command that acts
//!   on the selection and finds none says so through it, and says it in the
//!   status line rather than as an error, because nothing failed.
//! - No clip being selected is worded once, by the registry: no production
//!   source outside it writes the words, so a menu row's tooltip and the line
//!   its command writes read the same.
//!
//! The transition family's own three refusals are asserted next to the family,
//! in `transition_tests.rs`.

use std::collections::BTreeSet;
use std::path::Path;

use super::super::premiere_tests::with_clip;
use super::Decline;
use crate::test_support::collect_rs;
use crate::test_support::sources::{string_literals, without_test_items};

#[test]
fn every_reason_has_its_own_words() {
    let mut seen = BTreeSet::new();
    for why in Decline::ALL {
        let reason = why.reason();
        assert!(!reason.trim().is_empty(), "{why:?} has no wording");
        assert!(
            seen.insert(reason),
            "{why:?} repeats another reason: {reason}"
        );
    }
}

#[test]
fn a_command_with_no_selection_says_so_and_does_not_fail() {
    let (mut state, _clip) = with_clip();
    state.clear_selection();
    state.status_message = None;

    state
        .set_selected_transition("crossfade")
        .expect("declining is not an error");

    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoSelection.reason())
    );
    assert_eq!(state.last_error, None);
}

/// The reader is the one place the selection is read for a command, so it is
/// also the one place the refusal is written: a caller cannot forget it.
#[test]
fn the_shared_reader_returns_the_selection_and_says_nothing_when_there_is_one() {
    let (mut state, clip) = with_clip();
    state.status_message = None;

    assert_eq!(state.selected_clip_or_decline(), Some(clip));
    assert_eq!(state.status_message, None, "a selection is not a refusal");

    state.clear_selection();
    assert_eq!(state.selected_clip_or_decline(), None);
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoSelection.reason())
    );
}

/// **No clip being selected is worded once, by the registry.**
///
/// The class closed here: a command that found no clip selected and wrote its
/// own line for it, so `Select a clip first`, `No clip is selected` and
/// `Nothing is selected` stated one state three ways, and a menu row's tooltip
/// read differently from the line the same command wrote when run from the
/// palette. Every string literal in the crate's production sources is read
/// with test items blanked, and one that words an empty selection fails unless
/// it is in this module.
///
/// Not covered: a wording outside `WORDINGS`, such as a new phrasing of the
/// same state. A panel's hint to select a clip, as the Info panel's `Select a
/// clip.` is, states how to fill the panel rather than why a command did
/// nothing, and is not read as one.
#[test]
fn only_the_registry_words_an_empty_selection() {
    const WORDINGS: [&str; 5] = [
        "select a clip first",
        "no clip is selected",
        "no clip selected",
        "nothing is selected",
        "nothing selected",
    ];
    assert!(
        WORDINGS.contains(&Decline::NoSelection.reason().to_lowercase().as_str()),
        "the registry's wording is not one the census reads"
    );
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    assert!(
        files.len() > 200,
        "found {} files under {src:?}, which cannot be this crate",
        files.len()
    );
    let mut read = 0usize;
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "app/decline.rs" || rel.starts_with("test_support/") || rel.ends_with("_tests.rs")
        {
            continue;
        }
        let code = std::fs::read_to_string(file)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", file.display()));
        for (line, literal) in string_literals(&without_test_items(&code)) {
            read += 1;
            let words = literal.trim().trim_end_matches('.').to_lowercase();
            if WORDINGS.contains(&words.as_str()) {
                offenders.push(format!("{rel}:{line}: {literal:?}"));
            }
        }
    }
    assert!(read > 1000, "read only {read} string literals");
    assert!(
        offenders.is_empty(),
        "state an empty selection through Decline::NoSelection:\n{}",
        offenders.join("\n")
    );
}
