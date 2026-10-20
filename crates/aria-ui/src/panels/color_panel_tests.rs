//! The Color panel's section table, held to the catalog's `grade` group.
//!
//! The class closed: a grade property the model gains reaches the panel under
//! "Other" instead of vanishing, and its section is a decision, so the table is
//! checked against the catalog in both directions and for rows placed twice.
//! Not caught: whether a row sits in the section a colourist expects; that is a
//! reading of the table.

use std::collections::BTreeSet;

use super::{SECTIONS, section_of};
use crate::app::grade_controls::{COLOR_SECTIONS, GradeControls};
use crate::panels::inspector::blocks::wheels::WHEEL_RADIUS;
use crate::test_support::doc_tables::{heading_body, read_doc, strip_ticks, table_rows};

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

/// The Color panel sections table in `docs/reference/color.md` lists every
/// section in order with its title, whether it starts open, and its rows, and
/// the page states `COLOR_SECTIONS` and `WHEEL_RADIUS`. Fails on a section or
/// a row added, moved, renamed, or opened by default without the table, and
/// on a row for a section the panel does not have.
/// Not caught: the prose around the table.
#[test]
fn the_color_reference_lists_every_section_and_its_rows() {
    let doc = read_doc("reference/color.md");
    let panel_sec = heading_body(
        &doc,
        "## Color panel sections and grading wheels (`SECTIONS` and `WheelControl`)",
    );
    assert!(
        panel_sec.contains(&format!("COLOR_SECTIONS = {COLOR_SECTIONS}")),
        "color.md must document COLOR_SECTIONS = {COLOR_SECTIONS}"
    );
    let section_rows = table_rows(panel_sec.split("\n### ").next().unwrap_or(panel_sec));
    let documented: Vec<(usize, String, bool, usize, Vec<String>)> = section_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed SECTIONS row: {r:?}");
            let idx = strip_ticks(r[0]).parse::<usize>().expect("section index");
            let count = strip_ticks(r[3]).parse::<usize>().expect("row count");
            let ids: Vec<String> = r[4]
                .split(',')
                .map(|s| strip_ticks(s).to_string())
                .collect();
            (
                idx,
                r[1].to_string(),
                strip_ticks(r[2]) == "yes",
                count,
                ids,
            )
        })
        .collect();
    let controls = GradeControls::default();
    let expected: Vec<(usize, String, bool, usize, Vec<String>)> = SECTIONS
        .iter()
        .enumerate()
        .map(|(idx, &(title, ids))| {
            (
                idx,
                title.to_string(),
                controls.section_open(idx),
                ids.len(),
                ids.iter().map(|s| (*s).to_string()).collect(),
            )
        })
        .collect();
    assert_eq!(
        documented, expected,
        "docs/reference/color.md Color panel sections table drifted from color_panel::SECTIONS"
    );

    let wheel_sec = heading_body(&doc, "### Grading wheels (`WheelControl`)");
    assert!(
        wheel_sec.contains(&format!("WHEEL_RADIUS = {WHEEL_RADIUS:.1}")),
        "color.md must document WHEEL_RADIUS = {WHEEL_RADIUS:.1}"
    );
}
