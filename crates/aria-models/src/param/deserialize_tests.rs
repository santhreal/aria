//! [`Param`] reads back every shape it writes, in place.
//!
//! WHY: a constant is the bare value of its type and a curve is
//! `{"kf": [...]}`, and a constant may itself be a map: a crop rectangle, or a
//! command's `ParamValue`. The reader selects the shape by the first key, so a
//! constant map has to reach its own reader with that key put back, whether
//! the key was borrowed from the text or unescaped into a new string. These
//! cover each value type the model animates, as a constant and as a curve,
//! read from compact text, pretty text and a `serde_json::Value`. What these do
//! not catch: a value type added to the model without a case here.

use std::fmt::Debug;

use serde::de::value::{BorrowedStrDeserializer, Error, SeqDeserializer};
use serde::de::{DeserializeOwned, IntoDeserializer};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{CropRect, Easing, Keyframe, Param, ParamValue, Scale2, ShapePathPoint};

fn reads_back<T: Serialize + DeserializeOwned + PartialEq + Debug>(param: &Param<T>) {
    let compact = serde_json::to_string(param).unwrap();
    let from_compact: Param<T> = serde_json::from_str(&compact).unwrap();
    assert_eq!(&from_compact, param, "{compact}");
    let pretty = serde_json::to_string_pretty(param).unwrap();
    let from_pretty: Param<T> = serde_json::from_str(&pretty).unwrap();
    assert_eq!(&from_pretty, param, "{pretty}");
    let value = serde_json::to_value(param).unwrap();
    let from_value: Param<T> = serde_json::from_value(value).unwrap();
    assert_eq!(&from_value, param, "{compact} as a Value");
}

fn constant_and_curve<T>(constant: T, first: T, last: T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    reads_back(&Param::Constant(constant));
    reads_back(&Param::Keyframed {
        keyframes: vec![
            Keyframe::new(0, first, Easing::Linear),
            Keyframe::new(12, last, Easing::EaseInOut),
        ],
    });
}

#[test]
fn every_animated_value_type_reads_back_as_written() {
    constant_and_curve(0.5f32, 0.0, 1.0);
    constant_and_curve([0.25f32, -0.5], [0.0, 0.0], [1.0, 1.0]);
    constant_and_curve([0.1f32, 0.2, 0.3], [0.0; 3], [1.0; 3]);
    constant_and_curve([0.1f32, 0.2, 0.3, 0.4], [0.0; 4], [1.0; 4]);
    constant_and_curve([10u8, 20, 30, 255], [0; 4], [255; 4]);
    constant_and_curve(
        Scale2::uniform(1.5),
        Scale2 { x: 1.0, y: 2.0 },
        Scale2::uniform(0.5),
    );
    constant_and_curve(
        CropRect {
            x: 0.1,
            y: 0.2,
            w: 0.5,
            h: 0.25,
        },
        CropRect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
        CropRect {
            x: 0.25,
            y: 0.25,
            w: 0.5,
            h: 0.5,
        },
    );
    constant_and_curve(
        ParamValue::Scalar(1.0),
        ParamValue::Vec2([0.0, 1.0]),
        ParamValue::Color([1, 2, 3, 4]),
    );
    let path = |x: f32| {
        vec![
            ShapePathPoint::corner([x, 0.0]),
            ShapePathPoint::corner([0.0, x]),
        ]
    };
    constant_and_curve(path(0.5), path(0.0), path(1.0));
}

#[test]
fn a_constant_map_reads_its_first_key_however_it_was_spelled() {
    let rect = CropRect {
        x: 0.1,
        y: 0.2,
        w: 0.5,
        h: 0.25,
    };
    for text in [
        r#"{"w":0.5,"h":0.25,"x":0.1,"y":0.2}"#,
        r#"{"\u0077":0.5,"h":0.25,"x":0.1,"y":0.2}"#,
    ] {
        let read: Param<CropRect> = serde_json::from_str(text).unwrap();
        assert_eq!(read, Param::Constant(rect), "{text}");
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        let read: Param<CropRect> = serde_json::from_value(value).unwrap();
        assert_eq!(read, Param::Constant(rect), "{text} as a Value");
    }
    let read: Param<ParamValue> = serde_json::from_str(r#"{"scalar":2.0}"#).unwrap();
    assert_eq!(read, Param::Constant(ParamValue::Scalar(2.0)));
}

#[test]
fn a_curve_names_kf_first_and_once() {
    let curve = Param::Keyframed {
        keyframes: vec![Keyframe::new(0, 1.0f32, Easing::Linear)],
    };
    for text in [
        r#"{"kf":[{"t":0,"v":1.0}]}"#,
        r#"{"\u006bf":[{"t":0,"v":1.0}]}"#,
        r#"{"kf":[{"t":0,"v":1.0}],"later":{"any":[1,2]}}"#,
    ] {
        let read: Param<f32> = serde_json::from_str(text).unwrap();
        assert_eq!(read, curve, "{text}");
    }
    let duplicate = serde_json::from_str::<Param<f32>>(r#"{"kf":[],"kf":[]}"#).unwrap_err();
    assert!(
        duplicate.to_string().contains("duplicate field `kf`"),
        "{duplicate}"
    );
    for text in [
        r#"{"later":1,"kf":[{"t":0,"v":1.0}]}"#,
        "{}",
        r#"{"x":1.0}"#,
    ] {
        assert!(
            serde_json::from_str::<Param<f32>>(text).is_err(),
            "{text} read as a scalar param"
        );
    }
}

#[test]
fn a_bare_number_of_any_spelling_is_a_scalar_constant() {
    for (text, want) in [("2", 2.0f32), ("-3", -3.0), ("0.25", 0.25), ("1e2", 100.0)] {
        let read: Param<f32> = serde_json::from_str(text).unwrap();
        assert_eq!(read, Param::Constant(want), "{text}");
    }
    let read: Param<Scale2> = serde_json::from_str("2").unwrap();
    assert_eq!(read, Param::Constant(Scale2::uniform(2.0)));
    assert!(serde_json::from_str::<Param<f32>>(r#""1.0""#).is_err());
    assert!(serde_json::from_str::<Param<f32>>("null").is_err());
    assert!(serde_json::from_str::<Param<[f32; 2]>>("[1.0]").is_err());
}

fn read<'de, T: Deserialize<'de>>(from: impl Deserializer<'de, Error = Error>) -> Param<T> {
    Param::deserialize(from).unwrap()
}

#[test]
fn every_value_but_a_map_is_the_constant_itself() {
    assert_eq!(
        read::<bool>(true.into_deserializer()),
        Param::Constant(true)
    );
    assert_eq!(
        read::<i64>((-3i64).into_deserializer()),
        Param::Constant(-3)
    );
    assert_eq!(
        read::<i128>((-3i128).into_deserializer()),
        Param::Constant(-3)
    );
    assert_eq!(read::<u64>(3u64.into_deserializer()), Param::Constant(3));
    assert_eq!(read::<u128>(3u128.into_deserializer()), Param::Constant(3));
    assert_eq!(
        read::<f64>(0.5f64.into_deserializer()),
        Param::Constant(0.5)
    );
    let text = String::from("a");
    assert_eq!(
        read::<String>(text.as_str().into_deserializer()),
        Param::Constant(text.clone())
    );
    assert_eq!(
        read::<&str>(BorrowedStrDeserializer::new("a")),
        Param::Constant("a")
    );
    assert_eq!(
        read::<String>(text.clone().into_deserializer()),
        Param::Constant(text)
    );
    assert_eq!(
        read::<Option<f32>>(().into_deserializer()),
        Param::Constant(None)
    );
    assert_eq!(
        read::<Vec<u8>>(SeqDeserializer::new(vec![1u8, 2].into_iter())),
        Param::Constant(vec![1, 2])
    );
}

#[test]
fn a_curve_read_from_text_holds_no_spare_capacity() {
    for count in 1..=9 {
        let keyframes: Vec<String> = (0..count)
            .map(|t| format!(r#"{{"t":{t},"v":1.0}}"#))
            .collect();
        let text = format!(r#"{{"kf":[{}]}}"#, keyframes.join(","));
        let Param::Keyframed { keyframes } = serde_json::from_str::<Param<f32>>(&text).unwrap()
        else {
            panic!("{text} read as a constant");
        };
        assert_eq!(keyframes.len(), count, "{text}");
        assert_eq!(keyframes.capacity(), count, "{text}");
    }
}
