//! Does the gallery still show everything the design system ships?
//!
//! The class this closes: the gallery is the only place a person can look at
//! every component and every motion spec at once, and it is a hand-written list
//! of call sites. A component added to `aria_ui::components` or a spec added to
//! `aria_ui::motion` is invisible here until somebody remembers it, which is how
//! `chevron`, `band_fill` and `OPEN` came to be shipped and unreviewable.
//!
//! Both surfaces are read out of the crate's own source at run time, so the list
//! cannot go stale: a new `pub fn` in `components.rs` or a new `MotionSpec` in
//! `motion.rs` turns this red until the gallery draws it.
//!
//! Not covered: whether the specimen is a good one. A call site proves the thing
//! is on screen, not that its states are interesting — that is what a reviewer
//! looking at the window is for.

/// The three surfaces the gallery claims to cover, as source.
const COMPONENTS: &str = include_str!("../../aria-ui/src/components.rs");
const MOTION: &str = include_str!("../../aria-ui/src/motion.rs");
const CATALOG: &str = include_str!("../../aria-ui/src/motion_catalog.rs");

/// The gallery itself, as source. Every section file, because a specimen may
/// live in any of them.
const SECTION_SOURCES: &[&str] = &[
    include_str!("main.rs"),
    include_str!("gallery_controls.rs"),
    include_str!("gallery_icons.rs"),
    include_str!("gallery_kit.rs"),
    include_str!("gallery_loaders.rs"),
    include_str!("gallery_motion.rs"),
];

/// Does any section file contain `needle`?
fn drawn(needle: &str) -> bool {
    SECTION_SOURCES.iter().any(|source| source.contains(needle))
}

/// Every public entry of `aria_ui::components`, read from its declaration.
///
/// Generics are cut with the arguments: `row<Label, Body>(…)` would be the entry
/// `row`, and a needle carrying its type parameters matched nothing, which is
/// how a component with no specimen at all read as covered.
fn components() -> Vec<&'static str> {
    COMPONENTS
        .lines()
        .filter_map(|line| line.strip_prefix("pub fn "))
        .filter_map(|rest| rest.split(['(', '<']).next())
        .collect()
}

/// Every `MotionSpec` the catalog declares, read from its declaration.
fn specs() -> Vec<&'static str> {
    MOTION
        .lines()
        .filter_map(|line| line.strip_prefix("pub const "))
        .filter(|rest| rest.contains(": MotionSpec"))
        .filter_map(|rest| rest.split(':').next())
        .collect()
}

#[test]
fn the_source_the_check_reads_is_still_shaped_that_way() {
    // A parse that finds nothing would pass every assertion below.
    assert!(
        components().len() >= 8,
        "only {} components parsed out of components.rs — the declaration shape \
         changed and this check has stopped seeing them",
        components().len()
    );
    assert!(
        specs().len() >= 12,
        "only {} specs parsed out of motion.rs — the declaration shape changed \
         and this check has stopped seeing them",
        specs().len()
    );
}

#[test]
fn every_component_has_a_specimen() {
    let missing: Vec<&str> = components()
        .into_iter()
        .filter(|name| !drawn(&format!("components::{name}(")))
        .collect();
    assert!(
        missing.is_empty(),
        "these components ship with no specimen in the gallery: {missing:?}"
    );
}

/// The gallery plots the motion catalog itself, so a spec cannot ship unseen.
///
/// Two halves, and both are needed: the section draws every
/// `motion::catalog::CATALOG` row rather than a list of its own, and every spec
/// declared in `motion.rs` has a row to be drawn from. `aria-ui` owns the second
/// claim too — it is asserted here because the gallery's coverage rests on it,
/// and because a spec with no row used to mean a spec with no specimen
/// (`CARET_BLINK` shipped that way).
#[test]
fn every_motion_spec_is_on_screen() {
    assert!(
        drawn("motion::catalog::CATALOG"),
        "the motion section stopped drawing the catalog, so a new spec no longer \
         appears by itself"
    );
    let missing: Vec<&str> = specs()
        .into_iter()
        .filter(|name| !CATALOG.contains(&format!("name: \"{name}\"")))
        .collect();
    assert!(
        missing.is_empty(),
        "these specs have no catalog row, so the gallery cannot draw them: {missing:?}"
    );
}
