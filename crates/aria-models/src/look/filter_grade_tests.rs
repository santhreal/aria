//! A filter recipe carries one offset per adjustment row, and a key it does
//! not know refuses the file.
//!
//! # Why
//!
//! The recipe was a second hand-written copy of the adjustment list. The two
//! drifted, and because a recipe read with `#[serde(default)]`, a misspelled
//! key was skipped: a preset that looked installed and did nothing. These
//! cases derive the key set from [`crate::adjust_slider_rows`] at run time, so
//! an adjustment added to the list is covered by existing and one the recipe
//! cannot carry fails here; the documented table is checked against the same
//! rows, so a doc that lists yesterday's keys fails too.
//!
//! What this does not catch: what an offset does to a pixel. That is the
//! compositor's grade suite.

use super::*;

use crate::ColorAdjustments;

/// Parse one filter entry, as a catalog file would.
fn entry(body: &str) -> Result<FilterSpec, toml::de::Error> {
    toml::from_str::<FilterSpec>(body)
}

#[test]
fn a_recipe_carries_one_offset_per_adjustment_row() {
    let names: Vec<&str> = ColorAdjustments::SLIDERS
        .iter()
        .map(|slider| slider.name)
        .collect();
    assert_eq!(
        FilterGrade::KEYS.to_vec(),
        names,
        "the recipe's keys are not the adjustment rows, in row order"
    );
    assert_eq!(FilterGrade::COUNT, ColorAdjustments::COUNT);
}

#[test]
fn every_offset_writes_only_its_own_key() {
    for offset in FilterGrade::OFFSETS {
        let parsed = entry(&format!(
            "id = \"t\"\nlabel = \"T\"\n{} = 0.5\n",
            offset.name
        ))
        .unwrap_or_else(|err| panic!("the recipe refused its own key {}: {err}", offset.name));
        for other in FilterGrade::OFFSETS {
            let want = if other.name == offset.name { 0.5 } else { 0.0 };
            assert_eq!(
                (other.get)(&parsed.grade),
                want,
                "{} wrote {} as well",
                offset.name,
                other.name
            );
        }
    }
}

#[test]
fn a_recipe_that_states_nothing_changes_nothing() {
    let parsed = entry("id = \"t\"\nlabel = \"T\"\n").expect("a recipe may state nothing");
    assert!(parsed.grade.is_empty());
    for offset in FilterGrade::OFFSETS {
        assert_eq!(
            (offset.get)(&parsed.grade),
            0.0,
            "{} rests somewhere other than no change",
            offset.name
        );
    }
}

#[test]
fn a_key_the_recipe_does_not_know_refuses_the_entry() {
    let refusal = entry("id = \"t\"\nlabel = \"T\"\nbrightnes = 0.5\n")
        .expect_err("a misspelled key is refused, not skipped")
        .to_string();
    assert!(
        refusal.contains("brightnes"),
        "the refusal does not name the key: {refusal}"
    );
    let doubled = entry("id = \"t\"\nlabel = \"T\"\nbrightness = 0.5\nbrightness = 0.2\n")
        .expect_err("a key given twice is refused");
    assert!(
        doubled.to_string().contains("brightness"),
        "the refusal does not name the doubled key: {doubled}"
    );
}

#[test]
fn every_shipped_filter_parses_and_states_something() {
    let filters = filter_catalog();
    assert!(!filters.is_empty(), "the shipped catalog is empty");
    for filter in filters {
        assert!(
            !filter.grade.is_empty(),
            "shipped filter '{}' changes nothing",
            filter.id
        );
    }
}

/// The catalog documentation carries a table of these keys. A reader installing
/// a filter reads that table, so it is pinned to the rows rather than kept in
/// step by hand.
#[test]
fn the_documented_key_table_is_the_row_list() {
    let doc = include_str!("../../../../docs/catalogs.md");
    let filters = doc
        .split("## Filters")
        .nth(1)
        .expect("the catalogs doc has a Filters section")
        .split("\n## ")
        .next()
        .expect("the section ends at the next heading");
    let documented: Vec<&str> = filters
        .lines()
        .filter_map(|line| {
            let row = line.strip_prefix("| `")?;
            row.split('`').next()
        })
        .collect();
    assert_eq!(
        documented,
        FilterGrade::KEYS.to_vec(),
        "docs/catalogs.md lists other keys than a recipe carries"
    );
}
