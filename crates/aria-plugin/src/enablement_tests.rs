//! Enablement conditions parse to what they say and refuse what they cannot.
//!
//! Closes the class of a condition that reads a key other than the one named,
//! binds `&&` and `||` the wrong way round, or accepts a misspelled key that then
//! never holds. Does not cover how a host computes the keys.

use super::*;

fn keys(on: &[ContextKey]) -> ContextKeys {
    on.iter()
        .fold(ContextKeys::NONE, |keys, key| keys.with(*key, true))
}

/// The position each key has in [`ContextKey::ALL`]. A new key does not
/// compile here until it is placed.
fn position(key: ContextKey) -> usize {
    match key {
        ContextKey::Clips => 0,
        ContextKey::Selection => 1,
        ContextKey::SingleSelection => 2,
        ContextKey::MarkIn => 3,
        ContextKey::MarkOut => 4,
        ContextKey::InOut => 5,
        ContextKey::Playing => 6,
        ContextKey::FocusMedia => 7,
        ContextKey::FocusViewer => 8,
        ContextKey::FocusTimeline => 9,
        ContextKey::FocusInspector => 10,
        ContextKey::FocusAssistant => 11,
        ContextKey::FocusMixer => 12,
    }
}

#[test]
fn every_key_is_listed_once_and_parses_to_itself_alone() {
    for (at, key) in ContextKey::ALL.into_iter().enumerate() {
        assert_eq!(position(key), at, "{key} is out of place in ALL");
        assert_eq!(ContextKey::from_name(key.name()), Some(key));
        let only = Enablement::parse(key.name()).expect("a key name parses");
        assert!(only.holds(keys(&[key])), "{key} does not read itself");
        let others: Vec<ContextKey> = ContextKey::ALL
            .into_iter()
            .filter(|other| *other != key)
            .collect();
        assert!(!only.holds(keys(&others)), "{key} reads another key");
    }
}

#[test]
fn keys_set_and_clear_independently() {
    let all = keys(&ContextKey::ALL);
    for key in ContextKey::ALL {
        let cleared = all.with(key, false);
        for other in ContextKey::ALL {
            assert_eq!(cleared.has(other), other != key, "{key} cleared {other}");
        }
        assert!(ContextKeys::NONE.with(key, true).has(key));
    }
}

#[test]
fn operators_bind_and_before_or_and_not_tightest() {
    use ContextKey::{Playing, Selection, SingleSelection};
    let cases: &[(&str, &[ContextKey], bool)] = &[
        ("selection && !playing", &[Selection], true),
        ("selection && !playing", &[Selection, Playing], false),
        ("selection && !playing", &[], false),
        // `&&` binds tighter: playing || (selection && single-selection).
        ("playing || selection && single-selection", &[Playing], true),
        (
            "playing || selection && single-selection",
            &[Selection],
            false,
        ),
        (
            "(playing || selection) && single-selection",
            &[Playing],
            false,
        ),
        (
            "(playing || selection) && single-selection",
            &[Selection, SingleSelection],
            true,
        ),
        ("!(selection || playing)", &[], true),
        ("!(selection || playing)", &[Playing], false),
        ("!!selection", &[Selection], true),
        (
            "  selection&&single-selection  ",
            &[Selection, SingleSelection],
            true,
        ),
    ];
    for (source, on, want) in cases {
        let parsed = Enablement::parse(source).expect(source);
        assert_eq!(parsed.holds(keys(on)), *want, "{source} under {on:?}");
    }
}

#[test]
fn the_source_is_kept_trimmed() {
    let parsed = Enablement::parse("  in-out && !playing ").expect("parses");
    assert_eq!(parsed.source(), "in-out && !playing");
    assert_eq!(parsed.to_string(), "in-out && !playing");
}

#[test]
fn malformed_conditions_are_refused_with_the_column() {
    let cases: &[(&str, &str)] = &[
        ("", "the enablement condition is empty"),
        ("   ", "the enablement condition is empty"),
        (
            "selection &&",
            "the enablement condition `selection &&` expects a key, `!`, or `(` at column 13",
        ),
        (
            "selection & playing",
            "the enablement condition `selection & playing` expects `&&`, `||`, or the end at column 11",
        ),
        (
            "(selection",
            "the enablement condition `(selection` expects `)` at column 11",
        ),
        (
            "selection)",
            "the enablement condition `selection)` expects `&&`, `||`, or the end at column 10",
        ),
        (
            "selection playing",
            "the enablement condition `selection playing` expects `&&`, `||`, or the end at column 11",
        ),
    ];
    for (source, want) in cases {
        assert_eq!(
            Enablement::parse(source).as_ref().map_err(String::as_str),
            Err(*want)
        );
    }
}

#[test]
fn an_unknown_key_is_refused_with_the_keys_listed() {
    let err = Enablement::parse("selection && selcted").expect_err("misspelled");
    assert!(
        err.starts_with(
            "the enablement condition names an unknown key `selcted` at column 14; the keys are clips, selection, single-selection,"
        ),
        "{err}"
    );
    assert!(err.ends_with("focus.mixer"), "{err}");
}

#[test]
fn nesting_and_length_are_bounded() {
    let deep = format!("{}selection", "!".repeat(ENABLEMENT_MAX_DEPTH));
    assert!(Enablement::parse(&deep).is_ok(), "the limit itself parses");
    let deeper = format!("{}selection", "!".repeat(ENABLEMENT_MAX_DEPTH + 1));
    assert_eq!(
        Enablement::parse(&deeper),
        Err(format!(
            "the enablement condition nests deeper than {ENABLEMENT_MAX_DEPTH} at column {}",
            ENABLEMENT_MAX_DEPTH + 1
        ))
    );
    let parens = format!(
        "{}selection{}",
        "(".repeat(ENABLEMENT_MAX_DEPTH + 1),
        ")".repeat(ENABLEMENT_MAX_DEPTH + 1)
    );
    assert!(
        Enablement::parse(&parens)
            .expect_err("too deep")
            .starts_with("the enablement condition nests deeper than")
    );
    let long = vec!["selection"; 30].join(" && ");
    assert!(long.len() > ENABLEMENT_MAX_LEN);
    assert_eq!(
        Enablement::parse(&long),
        Err(format!(
            "the enablement condition is {} bytes; the limit is {ENABLEMENT_MAX_LEN}",
            long.len()
        ))
    );
}
