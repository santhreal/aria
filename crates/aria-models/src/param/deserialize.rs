//! Reading a [`Param`] in place: a map whose first key is `kf` is a keyframed
//! curve, and any other value is the constant itself.
//!
//! `#[serde(untagged)]` copied each param into serde's buffered content tree
//! and built the keyframed shape's error before it read a constant. On the
//! Server machine (i7-11700K) 40,000 constant `Param<f32>` read in 5.9 ms
//! that way, against 0.38 ms for the same numbers read as `f32`.
//!
//! A map's first key selects the shape, so a curve has to name `kf` first, as
//! the writer does. A map that does not is the constant: a [`CropRect`] is
//! `{"x": ..}`, a [`ParamValue`] is `{"scalar": ..}`. Keys after `kf` are
//! skipped, as the derived reader skipped unknown fields.
//!
//! [`CropRect`]: crate::CropRect
//! [`ParamValue`]: crate::ParamValue

use std::fmt;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::de::value::{BorrowedStrDeserializer, MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{
    self, DeserializeSeed, Deserializer, IgnoredAny, IntoDeserializer, MapAccess, SeqAccess,
    Visitor,
};

use super::Param;

/// The key of a keyframed curve's keyframe list.
const KEYFRAMES: &str = "kf";

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Param<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ParamVisitor(PhantomData))
    }
}

fn constant<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Param<T>, D::Error> {
    T::deserialize(deserializer).map(Param::Constant)
}

struct ParamVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for ParamVisitor<T> {
    type Value = Param<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a constant value or a keyframed curve {\"kf\": [...]}")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        constant(v.into_deserializer())
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        constant(v.into_deserializer())
    }

    fn visit_i128<E: de::Error>(self, v: i128) -> Result<Self::Value, E> {
        constant(v.into_deserializer())
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        constant(v.into_deserializer())
    }

    fn visit_u128<E: de::Error>(self, v: u128) -> Result<Self::Value, E> {
        constant(v.into_deserializer())
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        constant(v.into_deserializer())
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        constant(v.into_deserializer())
    }

    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        constant(BorrowedStrDeserializer::new(v))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        constant(v.into_deserializer())
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        constant(().into_deserializer())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        constant(SeqAccessDeserializer::new(seq))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let first = match map.next_key_seed(KeySeed)? {
            Some(Key::Keyframes) => {
                let mut keyframes: Vec<_> = map.next_value()?;
                // JSON text has no element count, so the list grew by doubling
                // while it was read; shrinking it drops the spare capacity.
                keyframes.shrink_to_fit();
                while let Some(key) = map.next_key_seed(KeySeed)? {
                    if matches!(key, Key::Keyframes) {
                        return Err(de::Error::duplicate_field(KEYFRAMES));
                    }
                    map.next_value::<IgnoredAny>()?;
                }
                return Ok(Param::Keyframed { keyframes });
            }
            Some(Key::Other(key)) => Some(key),
            None => None,
        };
        constant(MapAccessDeserializer::new(Replay { first, map }))
    }
}

/// A map key, read before the shape is known.
enum Key<'de> {
    Keyframes,
    Other(Text<'de>),
}

/// A constant map's first key, read ahead of the constant's own reader.
enum Text<'de> {
    Borrowed(&'de str),
    Owned(String),
}

impl<'de> Text<'de> {
    /// Hand the key read ahead to the seed that reads the constant's keys.
    fn replay<S: DeserializeSeed<'de>, E: de::Error>(self, seed: S) -> Result<S::Value, E> {
        match self {
            Text::Borrowed(key) => seed.deserialize(BorrowedStrDeserializer::new(key)),
            Text::Owned(key) => seed.deserialize(key.into_deserializer()),
        }
    }
}

struct KeySeed;

impl<'de> DeserializeSeed<'de> for KeySeed {
    type Value = Key<'de>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Key<'de>, D::Error> {
        deserializer.deserialize_str(KeyVisitor)
    }
}

struct KeyVisitor;

impl<'de> Visitor<'de> for KeyVisitor {
    type Value = Key<'de>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a map key")
    }

    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Key<'de>, E> {
        Ok(if v == KEYFRAMES {
            Key::Keyframes
        } else {
            Key::Other(Text::Borrowed(v))
        })
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Key<'de>, E> {
        Ok(if v == KEYFRAMES {
            Key::Keyframes
        } else {
            Key::Other(Text::Owned(v.to_owned()))
        })
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Key<'de>, E> {
        Ok(if v == KEYFRAMES {
            Key::Keyframes
        } else {
            Key::Other(Text::Owned(v))
        })
    }
}

/// The map a constant reads, with its first key put back.
struct Replay<'de, A> {
    /// The first key, until the constant's reader takes it.
    first: Option<Text<'de>>,
    map: A,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Replay<'de, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        match self.first.take() {
            Some(key) => key.replay(seed).map(Some),
            None => self.map.next_key_seed(seed),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, A::Error> {
        self.map.next_value_seed(seed)
    }
}

#[cfg(test)]
#[path = "deserialize_tests.rs"]
mod tests;
