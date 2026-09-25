//! `moments` tests covering query and order. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `moments` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn query_time_boundaries_distinguish_spans_and_point_events() {
    let key = content_key(1);
    let records = vec![
        record(
            key,
            0.0,
            1.0,
            "silence",
            1.0,
            "aria/test",
            1,
            Some("span-ending-at-start"),
        ),
        record(
            key,
            0.5,
            1.25,
            "silence",
            1.0,
            "aria/test",
            1,
            Some("span-overlapping-start"),
        ),
        record(
            key,
            1.0,
            1.0,
            "beat",
            1.0,
            "aria/test",
            1,
            Some("point-at-start"),
        ),
        record(
            key,
            1.0,
            1.4,
            "silence",
            1.0,
            "aria/test",
            1,
            Some("span-at-start"),
        ),
        record(
            key,
            1.5,
            1.5,
            "beat",
            1.0,
            "aria/test",
            1,
            Some("point-inside"),
        ),
        record(
            key,
            1.75,
            2.0,
            "silence",
            1.0,
            "aria/test",
            1,
            Some("span-ending-at-end"),
        ),
        record(
            key,
            2.0,
            2.0,
            "beat",
            1.0,
            "aria/test",
            1,
            Some("point-at-end"),
        ),
        record(
            key,
            2.0,
            2.5,
            "silence",
            1.0,
            "aria/test",
            1,
            Some("span-at-end"),
        ),
    ];
    let query = MomentQuery::new(key)
        .with_time_range(TimeSpan::new(1.0, 2.0).expect("query range is valid"));

    let labels: Vec<_> = filter_and_sort_moments(&records, &query)
        .into_iter()
        .map(payload_label)
        .collect();
    assert_eq!(
        labels,
        [
            "span-overlapping-start",
            "point-at-start",
            "span-at-start",
            "point-inside",
            "span-ending-at-end",
        ]
    );

    let empty_query = MomentQuery::new(key)
        .with_time_range(TimeSpan::new(1.0, 1.0).expect("empty query range is valid"));
    assert!(filter_and_sort_moments(&records, &empty_query).is_empty());
}

#[test]
fn query_isolates_content_and_applies_exact_kind_and_inclusive_confidence() {
    let target = content_key(1);
    let other = content_key(2);
    let records = vec![
        record(
            target,
            0.0,
            0.0,
            "beat",
            0.5,
            "aria/beats",
            1,
            Some("inclusive-threshold"),
        ),
        record(
            target,
            1.0,
            1.0,
            "beat",
            0.49,
            "aria/beats",
            1,
            Some("below-threshold"),
        ),
        record(
            target,
            2.0,
            3.0,
            "silence",
            0.9,
            "aria/silence",
            1,
            Some("wrong-kind"),
        ),
        record(
            other,
            0.0,
            0.0,
            "beat",
            1.0,
            "aria/beats",
            1,
            Some("wrong-content"),
        ),
    ];
    let query = MomentQuery::new(target)
        .with_kind(MomentKind::BEAT)
        .with_minimum_confidence(MomentConfidence::new(0.5).expect("threshold is valid"));

    let matches = filter_and_sort_moments(&records, &query);
    assert_eq!(matches.len(), 1);
    assert_eq!(payload_label(matches[0]), "inclusive-threshold");
}

#[test]
fn canonical_order_is_independent_of_input_order_and_uses_all_tie_breakers() {
    let key = content_key(3);
    let records = vec![
        record(key, 1.0, 2.0, "silence", 0.5, "source/a", 2, Some("b")),
        record(key, 2.0, 3.0, "beat", 0.5, "source/a", 1, Some("late")),
        record(
            key,
            1.0,
            2.0,
            "silence",
            0.5,
            "source/b",
            1,
            Some("source-b"),
        ),
        record(
            key,
            0.0,
            1.0,
            "beat",
            0.5,
            "source/a",
            1,
            Some("early-span"),
        ),
        record(key, 1.0, 2.0, "silence", 0.5, "source/a", 2, None),
        record(key, 1.0, 2.0, "beat", 0.5, "source/z", 1, Some("kind-beat")),
        record(
            key,
            1.0,
            2.0,
            "silence",
            0.9,
            "source/a",
            2,
            Some("high-confidence"),
        ),
        record(
            key,
            0.0,
            0.0,
            "vision_moment",
            0.5,
            "source/a",
            1,
            Some("early-point"),
        ),
        record(
            key,
            1.0,
            2.0,
            "silence",
            0.5,
            "source/a",
            1,
            Some("version-1"),
        ),
        record(key, 1.0, 2.0, "silence", 0.5, "source/a", 2, Some("a")),
    ];
    let query = MomentQuery::new(key);
    let expected = [
        "early-point",
        "early-span",
        "kind-beat",
        "version-1",
        "high-confidence",
        "<none>",
        "a",
        "b",
        "source-b",
        "late",
    ];

    let labels: Vec<_> = filter_and_sort_moments(&records, &query)
        .into_iter()
        .map(payload_label)
        .collect();
    assert_eq!(labels, expected);

    for shift in 0..records.len() {
        let mut shuffled = records.clone();
        shuffled.rotate_left(shift);
        if shift % 2 == 0 {
            shuffled.reverse();
        }
        let repeated: Vec<_> = filter_and_sort_moments(&shuffled, &query)
            .into_iter()
            .map(payload_label)
            .collect();
        assert_eq!(repeated, expected, "ordering changed for shuffle {shift}");
    }
}
