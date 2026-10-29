//! The Inspector's choice rows: the chip each marks and how its chips read.
//!
//! Closes the class "a choice row whose mark disagrees with its value". An
//! effect's layer slot left at None showed None and marked no chip. Every
//! choice row records the choice its value column shows and each chip's mark,
//! so a picker of any kind marks the choice it shows, one at most, and none
//! while it shows a mixed value.
//!
//! Also closes the class "two choices of one row that read alike". The Parent
//! row named each clip it offers by the clip's name alone, so clips cut from
//! one file drew chips no reader could tell apart. No two chips of a recorded
//! choice row read alike.
//!
//! The walks of `columns_tests` hold every choice row they draw to both, and
//! draw the Transform tab with two clips of the selected clip's file on the
//! lanes above it, so its Parent row offers both. Not covered: a picker drawn
//! outside a choice row.

use super::blocks::widgets::{DrawnChoice, MIXED, picked};

/// Every choice row marks, of its chips, the one its value column shows and
/// no other, and marks none while it shows a mixed value.
pub(super) fn every_choice_marks_what_it_shows(tab: &str, choices: &[DrawnChoice]) {
    for choice in choices {
        let marked: Vec<&str> = choice
            .chips
            .iter()
            .filter(|(_, on)| *on)
            .map(|(label, _)| &**label)
            .collect();
        let shown: Vec<&str> = choice
            .chips
            .iter()
            .filter(|(label, _)| &*choice.current != MIXED && *label == choice.current)
            .map(|(label, _)| &**label)
            .collect();
        assert!(
            marked == shown && marked.len() <= 1,
            "{tab}: {} shows {:?} and marks {marked:?} of {:?}",
            choice.row,
            choice.current,
            choice.chips
        );
    }
}

/// No two chips of one choice row read alike, so every choice can be told
/// from the others before it is pressed.
pub(super) fn no_two_choices_read_alike(tab: &str, choices: &[DrawnChoice]) {
    for choice in choices {
        let mut labels: Vec<&str> = choice.chips.iter().map(|(label, _)| &**label).collect();
        labels.sort_unstable();
        let alike: Vec<&str> = labels
            .windows(2)
            .filter(|pair| pair[0] == pair[1])
            .map(|pair| pair[0])
            .collect();
        assert!(
            alike.is_empty(),
            "{tab}: {} draws chips that read alike: {alike:?} among {:?}",
            choice.row,
            choice.chips
        );
    }
}

/// The choice row whose label's debug selector is `row` shows `shown` and
/// marks the chip of that name.
pub(super) fn shows_and_marks(tab: &str, choices: &[DrawnChoice], row: &str, shown: &str) {
    let choice = choices
        .iter()
        .find(|choice| choice.row == row)
        .unwrap_or_else(|| panic!("{tab}: no choice row {row} among {choices:?}"));
    assert!(
        &*choice.current == shown
            && choice
                .chips
                .iter()
                .any(|(label, on)| *on && &**label == shown),
        "{tab}: {row} does not show and mark {shown:?}: {choice:?}"
    );
}

#[test]
fn a_choice_in_force_is_marked_and_no_other() {
    assert_eq!(&*picked("Best", true), "• Best");
    assert_eq!(&*picked("Best", false), "Best");
}
