//! [`content_hash`] separates every two values whose JSON differs.
//!
//! WHY: a lane's revision is this hash, and the timeline view rebuilds a lane
//! only when its revision moved. Two lanes of different content that hash
//! alike leave a stale lane on screen. The encoding has to stay prefix-free
//! for that: without a string's length, a string ending in the string tag's
//! byte followed by another string feeds the same bytes as the two strings
//! split one byte later; without a container's end, a container followed by a
//! value feeds the same bytes as a container of the same shape that holds the
//! value as its last entry. What these do not catch: a SipHash collision, and a
//! `Serialize` impl that writes nothing for a field it holds.

use serde::Serialize;

use super::content_hash;
use crate::{Clip, Generator, Rational, TimeRange, Timeline, Track, TrackKind};

/// Hashes of `values`, asserting each pair hashes apart.
fn all_apart<T: Serialize + std::fmt::Debug>(values: &[T]) {
    let hashes: Vec<u64> = values.iter().map(content_hash).collect();
    for (i, a) in hashes.iter().enumerate() {
        for (j, b) in hashes.iter().enumerate().skip(i + 1) {
            assert_ne!(a, b, "{:?} and {:?} hash alike", values[i], values[j]);
        }
    }
}

#[test]
fn strings_and_bytes_keep_their_boundaries() {
    let str_tag = char::from(super::tag::STR);
    all_apart(&[
        (format!("a{str_tag}"), String::from("b")),
        (String::from("a"), format!("{str_tag}b")),
        (format!("a{str_tag}b"), String::new()),
        (String::new(), format!("a{str_tag}b")),
    ]);
    let bytes_tag = super::tag::BYTES;
    all_apart(&[
        (Bytes(&[1, bytes_tag]), Bytes(&[2])),
        (Bytes(&[1]), Bytes(&[bytes_tag, 2])),
    ]);
}

/// Each container shape serde writes, untagged so that each is its bare JSON.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Nest {
    Leaf(u32),
    Flag(bool),
    Name(&'static str),
    List(Vec<Nest>),
    Seq(Vec<u32>),
    Tuple2((u32, u32)),
    Tuple3((u32, u32, u32)),
    Row2(Row2),
    Row3(Row3),
    Map(std::collections::BTreeMap<&'static str, u32>),
    Rec1(Rec1),
    Rec2(Rec2),
    Short(Short),
    Long(Long),
}

#[derive(Debug, Serialize)]
struct Row2(u32, u32);

#[derive(Debug, Serialize)]
struct Row3(u32, u32, u32);

#[derive(Debug, Serialize)]
struct Rec1 {
    a: u32,
}

#[derive(Debug, Serialize)]
struct Rec2 {
    a: u32,
    b: u32,
}

/// Variants that share their names with [`Long`]'s and hold one value fewer.
#[derive(Debug, Serialize)]
enum Short {
    Row(u32, u32),
    Rec { a: u32 },
}

#[derive(Debug, Serialize)]
enum Long {
    Row(u32, u32, u32),
    Rec { a: u32, b: u32 },
}

#[test]
fn containers_keep_their_boundaries() {
    use Nest::{Leaf, List, Name};
    // Each shape, holding one value fewer and followed by it, against the same
    // shape holding that value: without the container's end, the same bytes.
    let pairs = [
        (
            vec![Nest::Seq(vec![1, 2]), Leaf(3)],
            Nest::Seq(vec![1, 2, 3]),
        ),
        (vec![Nest::Tuple2((1, 2)), Leaf(3)], Nest::Tuple3((1, 2, 3))),
        (
            vec![Nest::Row2(Row2(1, 2)), Leaf(3)],
            Nest::Row3(Row3(1, 2, 3)),
        ),
        (
            vec![Nest::Short(Short::Row(1, 2)), Leaf(3)],
            Nest::Long(Long::Row(1, 2, 3)),
        ),
        (
            vec![Nest::Map([("a", 1)].into()), Name("b"), Leaf(2)],
            Nest::Map([("a", 1), ("b", 2)].into()),
        ),
        (
            vec![Nest::Rec1(Rec1 { a: 1 }), Name("b"), Leaf(2)],
            Nest::Rec2(Rec2 { a: 1, b: 2 }),
        ),
        (
            vec![Nest::Short(Short::Rec { a: 1 }), Name("b"), Leaf(2)],
            Nest::Long(Long::Rec { a: 1, b: 2 }),
        ),
    ];
    for (shorter, longer) in pairs {
        all_apart(&[List(shorter), List(vec![longer])]);
    }
    all_apart(&[
        vec![vec![1u32], vec![]],
        vec![vec![], vec![1]],
        vec![vec![1, 1]],
        vec![vec![1], vec![1]],
    ]);
    all_apart(&[None, Some(None), Some(Some(0u8))]);
}

/// A struct whose skipped fields would otherwise leave the same value in the
/// same place.
#[derive(Debug, Serialize)]
struct Sparse {
    #[serde(skip_serializing_if = "Option::is_none")]
    left: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    right: Option<u32>,
}

#[derive(Debug, Serialize)]
enum Shape {
    Unit,
    Other,
    Newtype(u32),
    Tuple(u32, u32),
    Struct {
        a: u32,
    },
    Sparse {
        #[serde(skip_serializing_if = "Option::is_none")]
        left: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        right: Option<u32>,
    },
}

#[test]
fn field_and_variant_names_are_part_of_the_content() {
    all_apart(&[
        Sparse {
            left: Some(1),
            right: None,
        },
        Sparse {
            left: None,
            right: Some(1),
        },
        Sparse {
            left: None,
            right: None,
        },
    ]);
    all_apart(&[
        Shape::Unit,
        Shape::Other,
        Shape::Newtype(1),
        Shape::Tuple(1, 1),
        Shape::Struct { a: 1 },
        Shape::Sparse {
            left: Some(1),
            right: None,
        },
        Shape::Sparse {
            left: None,
            right: Some(1),
        },
    ]);
}

#[test]
fn every_number_width_and_sign_hashes_apart() {
    all_apart(&[0i64, 1, -1, 63, 64, -64, -65, 127, 128, i64::MAX, i64::MIN]);
    all_apart(&[0u64, 1, 127, 128, 1 << 40, 1 << 41, u64::MAX]);
    all_apart(&[0.0f32, -0.0, 1.0, f32::MIN_POSITIVE, f32::INFINITY]);
    all_apart(&[0.0f64, -0.0, 1.0, f64::MIN_POSITIVE]);
    all_apart(&[0i128, 1, -1, i128::MAX, i128::MIN]);
    all_apart(&[0u128, 1, u128::MAX]);
    all_apart(&['a', 'b', '\u{10FFFF}']);
    all_apart(&[false, true]);
}

/// Asserts no two of `values` hash alike, for more values than [`all_apart`]
/// compares pair by pair.
fn all_distinct<T: Serialize + std::fmt::Debug + Copy>(values: impl IntoIterator<Item = T>) {
    let mut seen = std::collections::HashMap::new();
    for value in values {
        if let Some(earlier) = seen.insert(content_hash(&value), value) {
            panic!("{earlier:?} and {value:?} hash alike");
        }
    }
}

#[test]
fn integers_hash_apart_across_every_encoded_byte() {
    // Every value up to two LEB128 bytes and a bit past, and every value with
    // one or two bits set, so each bit of each encoded byte is flipped alone.
    let pairs = || (0..64).flat_map(|a| (a..64).map(move |b| (1u64 << a) | (1u64 << b)));
    let unsigned: std::collections::BTreeSet<u64> = (0..1 << 15).chain(pairs()).collect();
    all_distinct(unsigned.iter().copied());
    let signed: std::collections::BTreeSet<i64> = (-(1 << 14)..1 << 14)
        .chain(pairs().flat_map(|bits| [bits as i64, (bits as i64).wrapping_neg()]))
        .collect();
    all_distinct(signed.iter().copied());
    // A value ends at the byte without the continuation bit, not before.
    all_apart(&[
        Nest::List(vec![Nest::Leaf(128)]),
        Nest::List(vec![Nest::Leaf(0), Nest::Flag(true)]),
    ]);
}

#[test]
fn content_past_one_buffered_slice_is_hashed() {
    let long = "x".repeat(super::CHUNK * 3);
    let mut changed = long.clone();
    changed.replace_range(long.len() - 1.., "y");
    all_apart(&[long, changed]);
    // A last field that straddles the buffered slice.
    for pad in 0..super::CHUNK + 8 {
        let filler = "p".repeat(pad);
        all_apart(&[(filler.clone(), 1u64 << 50), (filler, 1u64 << 51)]);
    }
}

#[test]
fn equal_values_hash_alike() {
    let value = (vec![Shape::Struct { a: 7 }], "text", 1.5f32, Some(9u64));
    assert_eq!(content_hash(&value), content_hash(&value));
    let lane = lanes()[3].clone();
    assert_eq!(content_hash(&lane), content_hash(&lane.clone()));
}

/// Bytes written through `serialize_bytes`.
#[derive(Debug)]
struct Bytes<'a>(&'a [u8]);

impl Serialize for Bytes<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

/// Lanes that differ from one base lane by one write each, plus repeats of
/// writes that land on content another lane already holds.
fn lanes() -> Vec<Track> {
    let rate = Rational::FPS_24;
    let solid = Generator::SolidColor {
        rgba: [200, 40, 40, 255],
    };
    let kind = TrackKind::for_generator(&solid).expect("a generator names its lane");
    let mut timeline = Timeline::new(rate);
    let id = timeline.add_track(Track::new(kind, "V1"));
    for i in 0..4 {
        let range = TimeRange::at_rate(i * 24, 24, rate);
        timeline
            .add_clip(id, Clip::generated(solid.clone(), range))
            .expect("abutting clips fit");
    }
    let base = timeline.track(id).expect("lane exists").clone();
    let mut out = vec![base.clone()];
    let edits: [fn(&mut Track); 9] = [
        |t| t.muted = true,
        |t| t.solo = true,
        |t| t.name = "V2".into(),
        |t| t.gain = 0.5,
        |t| t.pan = -0.25,
        |t| t.pan = 0.25,
        |t| t.name = "V1 ".into(),
        |t| t.muted = false,
        |t| t.gain = 1.0,
    ];
    for edit in edits {
        let mut lane = base.clone();
        edit(&mut lane);
        out.push(lane);
    }
    out
}

/// The hash partitions lanes exactly as their JSON does: equal JSON, equal
/// hash; different JSON, different hash.
#[test]
fn lanes_hash_alike_exactly_when_their_json_matches() {
    let lanes = lanes();
    for a in &lanes {
        for b in &lanes {
            let same_json = serde_json::to_string(a).unwrap() == serde_json::to_string(b).unwrap();
            assert_eq!(
                content_hash(a) == content_hash(b),
                same_json,
                "{} and {} disagree with their JSON",
                a.name,
                b.name
            );
        }
    }
}
