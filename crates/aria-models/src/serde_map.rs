//! Serialize [`Map`] as a stable key-sorted sequence of pairs for JSON project
//! files, and read the pairs back with each key once.

use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;

use serde::de::{Error as _, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Map;

/// The refusal for a key listed twice.
const LISTED_TWICE: &str = "a key is listed twice, and each entry needs a key of its own";

/// The most entries reserved before they are read. A size hint states the
/// length of a list a file claims, so a larger one grows as entries arrive.
const MAX_RESERVED: usize = 4096;

/// Above this entry count the pairs are collected and sorted instead of being
/// selected in place. A project's maps are effect parameters, color channels,
/// and vector components: single digits. Selection is `n²` comparisons of
/// borrowed keys against one heap allocation, which is the cheaper trade only
/// while `n` stays small.
const INLINE_SELECT_LIMIT: usize = 8;

pub(crate) fn serialize<K, V, S>(map: &Map<K, V>, serializer: S) -> Result<S::Ok, S::Error>
where
    K: Serialize + Ord,
    V: Serialize,
    S: Serializer,
{
    if map.is_empty() {
        return serializer.collect_seq(std::iter::empty::<(&K, &V)>());
    }
    if map.len() == 1 {
        return serializer.collect_seq(map.iter());
    }
    if map.len() <= INLINE_SELECT_LIMIT {
        // Emit in key order by selecting the next-largest key each step. Keys of
        // a map are unique, so the last key emitted is an exact exclusive bound.
        let mut seq = serializer.serialize_seq(Some(map.len()))?;
        let mut last: Option<&K> = None;
        for _ in 0..map.len() {
            let mut next: Option<(&K, &V)> = None;
            for (key, value) in map.iter() {
                if last.is_some_and(|bound| key <= bound) {
                    continue;
                }
                if next.is_none_or(|(candidate, _)| key < candidate) {
                    next = Some((key, value));
                }
            }
            let (key, value) = next.expect("one unemitted key remains per step");
            seq.serialize_element(&(key, value))?;
            last = Some(key);
        }
        return seq.end();
    }
    let mut sorted: Vec<(&K, &V)> = map.iter().collect();
    sorted.sort_unstable_by_key(|(left, _)| *left);
    serializer.collect_seq(sorted)
}

/// The pairs as a map, refusing a key listed twice: a map keeps one entry per
/// key, so the rest of the file would drop the others without a word.
pub(crate) fn deserialize<'de, K, V, D>(deserializer: D) -> Result<Map<K, V>, D::Error>
where
    K: Deserialize<'de> + Eq + Hash,
    V: Deserialize<'de>,
    D: Deserializer<'de>,
{
    deserializer.deserialize_seq(Pairs(PhantomData))
}

struct Pairs<K, V>(PhantomData<(K, V)>);

impl<'de, K, V> Visitor<'de> for Pairs<K, V>
where
    K: Deserialize<'de> + Eq + Hash,
    V: Deserialize<'de>,
{
    type Value = Map<K, V>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a list of [key, value] pairs")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut pairs: A) -> Result<Self::Value, A::Error> {
        let reserved = pairs.size_hint().unwrap_or(0).min(MAX_RESERVED);
        let mut map = Map::with_capacity_and_hasher(reserved, Default::default());
        while let Some((key, value)) = pairs.next_element::<(K, V)>()? {
            if map.insert(key, value).is_some() {
                return Err(A::Error::custom(LISTED_TWICE));
            }
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wrapper {
        #[serde(with = "crate::serde_map")]
        entries: Map<String, i32>,
    }

    #[test]
    fn serde_map_roundtrip_empty_and_populated() {
        let mut w = Wrapper {
            entries: Map::default(),
        };
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(json, r#"{"entries":[]}"#);
        let loaded: Wrapper = serde_json::from_str(&json).unwrap();
        assert_eq!(w, loaded);

        w.entries.insert("b".into(), 2);
        w.entries.insert("a".into(), 1);
        w.entries.insert("c".into(), 3);
        let json2 = serde_json::to_string(&w).unwrap();
        assert_eq!(json2, r#"{"entries":[["a",1],["b",2],["c",3]]}"#);
        let loaded2: Wrapper = serde_json::from_str(&json2).unwrap();
        assert_eq!(w, loaded2);
    }

    /// WHY: a map keeps one entry per key, so a file listing a key twice lost
    /// every entry but the last on load, without a refusal: a clip listed twice
    /// on a lane loaded as one. Every `serde_map` field reads through
    /// [`deserialize`], so this covers lanes, clips, the clip index, media,
    /// sequences and parameters, read from text (the current-schema path) and
    /// from a parsed `Value` (the migrating path). Not caught: a map a project
    /// file holds without `serde_map`.
    #[test]
    fn a_key_listed_twice_is_refused_and_distinct_keys_load() {
        let cases: [(&str, Option<&[(&str, i32)]>); 5] = [
            (r#"{"entries":[["a",1],["a",2]]}"#, None),
            (r#"{"entries":[["a",1],["b",2],["a",1]]}"#, None),
            (
                r#"{"entries":[["a",1],["b",2]]}"#,
                Some(&[("a", 1), ("b", 2)]),
            ),
            (
                r#"{"entries":[["b",2],["a",1]]}"#,
                Some(&[("a", 1), ("b", 2)]),
            ),
            (r#"{"entries":[]}"#, Some(&[])),
        ];
        for (text, want) in cases {
            let value: serde_json::Value = serde_json::from_str(text).expect("JSON");
            let read = [
                (
                    "text",
                    serde_json::from_str::<Wrapper>(text).map_err(|e| e.to_string()),
                ),
                (
                    "value",
                    serde_json::from_value::<Wrapper>(value).map_err(|e| e.to_string()),
                ),
            ];
            for (reader, got) in read {
                match (want, got) {
                    (Some(entries), Ok(loaded)) => {
                        let want: Map<String, i32> = entries
                            .iter()
                            .map(|&(key, v)| (key.to_owned(), v))
                            .collect();
                        assert_eq!(loaded.entries, want, "{text} ({reader})");
                    }
                    (None, Err(err)) => {
                        assert!(err.starts_with(LISTED_TWICE), "{text} ({reader}): {err}");
                    }
                    (want, got) => panic!("{text} ({reader}): want {want:?}, got {got:?}"),
                }
            }
        }
    }

    /// WHY: the serialized order is the file format. A hash map iterates in
    /// whatever order its seeds produce, so two projects holding the same
    /// entries must still write the same bytes regardless of insertion order.
    /// Both branches are covered: the in-place selection used up to
    /// [`INLINE_SELECT_LIMIT`] entries, and the collect-and-sort above it.
    #[test]
    fn serialized_order_is_independent_of_insertion_order() {
        for count in [2, INLINE_SELECT_LIMIT, INLINE_SELECT_LIMIT + 1, 64] {
            let keys: Vec<String> = (0..count).map(|i| format!("k{i:02}")).collect();
            let build = |reversed: bool| Wrapper {
                entries: if reversed {
                    keys.iter()
                        .enumerate()
                        .rev()
                        .map(|(i, k)| (k.clone(), i as i32))
                        .collect()
                } else {
                    keys.iter()
                        .enumerate()
                        .map(|(i, k)| (k.clone(), i as i32))
                        .collect()
                },
            };
            let forward = build(false);
            let forward_json = serde_json::to_string(&forward).unwrap();
            assert_eq!(
                forward_json,
                serde_json::to_string(&build(true)).unwrap(),
                "{count} entries wrote different bytes for a different insertion order"
            );

            let value: serde_json::Value = serde_json::from_str(&forward_json).unwrap();
            let written: Vec<&str> = value["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| pair[0].as_str().unwrap())
                .collect();
            let mut expected: Vec<&str> = keys.iter().map(String::as_str).collect();
            expected.sort_unstable();
            assert_eq!(written, expected, "{count} entries were not key-ordered");

            let loaded: Wrapper = serde_json::from_str(&forward_json).unwrap();
            assert_eq!(loaded, forward, "{count} entries did not round-trip");
        }
    }
}
