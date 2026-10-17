//! The Color panel's section table, held to the catalog's `grade` group.
//!
//! The class closed: a grade property the model gains reaches the panel under
//! "Other" instead of vanishing, and its section is a decision, so the table is
//! checked against the catalog in both directions and for rows placed twice.
//! Not caught: whether a row sits in the section a colourist expects; that is a
//! reading of the table.

use std::collections::BTreeSet;

use super::{SECTIONS, section_of};

#[test]
fn every_grade_row_has_a_section() {
    let grade: Vec<String> = aria_models::clip_param_catalog()
        .iter()
        .filter(|spec| spec.group == "grade")
        .map(|spec| spec.id.to_string())
        .collect();
    assert!(!grade.is_empty(), "the catalog has no grade group");

    let unplaced: Vec<&String> = grade.iter().filter(|id| section_of(id).is_none()).collect();
    assert!(
        unplaced.is_empty(),
        "grade rows with no Color panel section: {unplaced:?}"
    );

    let named: Vec<&str> = SECTIONS
        .iter()
        .flat_map(|(_, rows)| rows.iter().copied())
        .collect();
    let unique: BTreeSet<&str> = named.iter().copied().collect();
    assert_eq!(unique.len(), named.len(), "a row is placed in two sections");

    let stale: Vec<&str> = named
        .into_iter()
        .filter(|id| !grade.iter().any(|row| row == id))
        .collect();
    assert!(
        stale.is_empty(),
        "sections name rows the grade group does not have: {stale:?}"
    );
}
