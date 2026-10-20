//! Do the shortcut doc and the dispatcher agree?
//!
//! The class this closes: a chord and its documentation are two lists, and the
//! second one is maintained by hand. Six chords had shipped undocumented by the
//! time this test was written — every reach binding added after the doc was last
//! read — so an operator's only way to learn them was to read the source. The
//! reverse hole is the same shape: a documented key whose binding was renamed or
//! re-chorded reads as a promise the app does not keep.
//!
//! Both directions are asserted against the shipped table at run time, so a new
//! chord is red until the doc names it and a doc row is red until something
//! dispatches it. Neither list is written down twice here.
//!
//! Not covered: whether the *description* beside a chord is true. Prose cannot be
//! checked against a function pointer, and a wrong description is a smaller lie
//! than a missing or dead row.

use super::bindings::SHIPPED_BINDINGS;

/// The doc, read at compile time from the file the operator reads.
const DOC: &str = include_str!("../../../../docs/keyboard-shortcuts.md");

/// Sections whose first column is not a chord.
///
/// `Inside a text box` documents what a focused field does with keys the
/// dispatcher never sees, and `Pointer` documents mouse gestures. Both are
/// asserted to exist below, so renaming one turns the check off loudly rather
/// than quietly.
const NOT_CHORDS: &[&str] = &["Inside a text box", "Pointer"];

/// Every chord the doc names, lowercased.
///
/// A cell is a chord when it has no space in it: that is what separates `Ctrl+V`
/// from `Any printable key` and `Click clip`. A cell offering alternatives —
/// `+ / =`, `Ctrl+Shift+Z or Ctrl+Y` — yields one token per alternative, because
/// each is a separate binding.
fn documented() -> Vec<String> {
    let mut out = Vec::new();
    let mut skipping = false;
    for line in DOC.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            skipping = NOT_CHORDS.contains(&heading.trim());
            continue;
        }
        if skipping || !line.starts_with('|') {
            continue;
        }
        let Some(cell) = line.split('|').nth(1).map(str::trim) else {
            continue;
        };
        if matches!(cell, "Shortcut" | "Gesture" | "---" | "") {
            continue;
        }
        for token in cell.split(" / ").flat_map(|part| part.split(" or ")) {
            let token = token.trim();
            if !token.is_empty() && !token.contains(' ') {
                out.push(token.to_lowercase());
            }
        }
    }
    out
}

/// Every chord that ships, lowercased, as the tooltips render it.
fn shipped() -> Vec<String> {
    SHIPPED_BINDINGS
        .iter()
        .map(|binding| binding.chord.label().to_lowercase())
        .collect()
}

/// Every shipped binding id paired with its display chord label.
pub(super) fn shipped_binding_chords() -> std::collections::BTreeMap<&'static str, String> {
    SHIPPED_BINDINGS
        .iter()
        .map(|binding| (binding.id, binding.chord.label()))
        .collect()
}

#[test]
fn the_sections_the_check_skips_are_still_there() {
    // Without this, renaming a heading would silently exempt every row under it.
    for heading in NOT_CHORDS {
        assert!(
            DOC.contains(&format!("## {heading}")),
            "`{heading}` is gone from the doc, so the chord check is skipping \
             nothing and the rows it used to skip are now unchecked"
        );
    }
    let count = documented().len();
    assert!(
        count > 50,
        "only {count} chords parsed out of the doc — the table shape changed and \
         the check has stopped seeing rows"
    );
}

#[test]
fn every_shipped_chord_is_documented() {
    let documented = documented();
    let missing: Vec<(&str, String)> = SHIPPED_BINDINGS
        .iter()
        .map(|binding| (binding.id, binding.chord.label()))
        .filter(|(_, label)| !documented.contains(&label.to_lowercase()))
        .collect();
    assert!(
        missing.is_empty(),
        "these chords ship with no row in docs/keyboard-shortcuts.md: {missing:?}"
    );
}

#[test]
fn every_documented_chord_is_dispatched() {
    let shipped = shipped();
    let dead: Vec<String> = documented()
        .into_iter()
        .filter(|token| !shipped.contains(token))
        .collect();
    assert!(
        dead.is_empty(),
        "docs/keyboard-shortcuts.md promises keys nothing dispatches: {dead:?}"
    );
}
