//! WHY: the damaged files the loader census reads come from
//! [`for_each_damaged_file`], so a value it skips is a value no census
//! damages. Each value below the root is replaced by each hostile value and
//! removed exactly once, keys holding the pointer's own separators included.
//! Does not judge which hostile values are worth trying.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::for_each_damaged_file;

fn visits(doc: &Value) -> (usize, BTreeMap<String, Value>) {
    let mut seen = BTreeMap::new();
    let count = for_each_damaged_file(doc, |what, edited| {
        assert!(
            seen.insert(what.clone(), edited.clone()).is_none(),
            "{what} visited twice"
        );
    });
    (count, seen)
}

#[test]
fn every_value_is_replaced_by_each_hostile_value_and_removed_once() {
    let doc = json!({ "a": [1, { "b": 2 }], "c": "x" });
    let (count, seen) = visits(&doc);
    // /a/0, /a/1/b, /a/1, /a and /c.
    assert_eq!(count, 5);
    assert_eq!(seen.len(), 5 * 12);
    assert_eq!(
        seen["/a/1/b = null"],
        json!({ "a": [1, { "b": null }], "c": "x" })
    );
    assert_eq!(seen["/a/0 removed"], json!({ "a": [{ "b": 2 }], "c": "x" }));
    assert_eq!(seen["/a/1 removed"], json!({ "a": [1], "c": "x" }));
    assert_eq!(seen["/a removed"], json!({ "c": "x" }));
    assert_eq!(
        seen["/c = 9223372036854775807"],
        json!({ "a": [1, { "b": 2 }], "c": i64::MAX })
    );
    for (what, edited) in &seen {
        assert_ne!(edited, &doc, "{what} left the file as it was");
    }
}

#[test]
fn a_key_holding_a_separator_is_damaged_as_itself() {
    let doc = json!({ "a/b": 1, "c~d": 2 });
    let (count, seen) = visits(&doc);
    assert_eq!(count, 2);
    assert_eq!(seen["/a~1b removed"], json!({ "c~d": 2 }));
    assert_eq!(seen["/c~0d removed"], json!({ "a/b": 1 }));
    assert_eq!(seen["/c~0d = true"], json!({ "a/b": 1, "c~d": true }));
}

#[test]
fn a_scalar_file_has_nothing_below_its_root() {
    let (count, seen) = visits(&json!(3));
    assert_eq!(count, 0);
    assert!(seen.is_empty());
}
