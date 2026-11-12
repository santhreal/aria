//! Every field a save writes is a field an open reads, for the clip, track,
//! timeline, sequence and project shapes.
//!
//! WHY: a key the save writes and the open does not read is dropped on open
//! with no error, because the derived readers ignore unknown keys. `Track` is
//! the exposed case: it saves through its derived `Serialize` and opens through
//! a separate `TrackData` reader, so a field added to one and not the other
//! compiles. A byte round trip finds the loss only when its value moves the
//! field off its default. Both field sets here come from the types: the
//! written set from a `Serializer` that records keys (serde passes a
//! `skip_serializing_if` field to `skip_field`, so the set does not depend on
//! the value), the read set from the `FIELDS` slice serde passes to
//! `deserialize_struct`, aliases included.
//!
//! Does not catch: a field read under its name and then replaced in a
//! hand-written constructor, or a reader that drops a key on a shape nested
//! below these five.

use std::collections::BTreeSet;
use std::fmt;

use aria_models::{
    Clip, MediaId, Project, Rational, Sequence, TimeRange, Timeline, Track, TrackKind,
};
use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde::ser::{self, Impossible, Serialize, SerializeStruct, Serializer};

/// Names an open reads that no save writes: `(shape, name, reason)`.
const READ_ONLY: &[(&str, &str, &str)] = &[(
    "Project",
    "schema_version",
    "files saved before the schema identity hold it under this name",
)];

struct Shape {
    name: &'static str,
    written: BTreeSet<&'static str>,
    read: BTreeSet<&'static str>,
}

fn shapes() -> Vec<Shape> {
    let rate = Rational::new(30, 1);
    let range = TimeRange::at_rate(0, 30, rate);
    vec![
        shape::<Clip>("Clip", &Clip::from_media(MediaId::next(), range, range)),
        shape::<Track>("Track", &Track::new(TrackKind::Video, "V1")),
        shape::<Timeline>("Timeline", &Timeline::new(rate)),
        shape::<Sequence>("Sequence", &Sequence::new("S1", rate)),
        shape::<Project>("Project", &Project::new("P", rate)),
    ]
}

fn shape<T: Serialize + for<'de> Deserialize<'de>>(name: &'static str, value: &T) -> Shape {
    let mut written = BTreeSet::new();
    if let Err(err) = value.serialize(Keys(&mut written)) {
        panic!("{name} did not save as a struct: {err}");
    }
    let read = match T::deserialize(Fields) {
        Err(Probe::Fields(fields)) => fields.iter().copied().collect(),
        Err(Probe::Other(err)) => panic!("{name} did not open as a struct: {err}"),
        Ok(_) => panic!("{name} opened from no input"),
    };
    assert!(!written.is_empty(), "{name} saved no fields");
    Shape {
        name,
        written,
        read,
    }
}

#[test]
fn every_field_a_save_writes_is_a_field_an_open_reads() {
    let dropped: Vec<String> = shapes()
        .iter()
        .flat_map(|shape| {
            shape
                .written
                .difference(&shape.read)
                .map(|key| format!("{}.{key}", shape.name))
        })
        .collect();
    assert!(
        dropped.is_empty(),
        "written on save and dropped on open: {dropped:?}"
    );
}

#[test]
fn every_name_an_open_reads_is_saved_or_recorded_as_read_only() {
    let shapes = shapes();
    let unrecorded: Vec<String> = shapes
        .iter()
        .flat_map(|shape| {
            shape
                .read
                .difference(&shape.written)
                .filter(|key| {
                    !READ_ONLY
                        .iter()
                        .any(|(s, k, _)| *s == shape.name && k == *key)
                })
                .map(|key| format!("{}.{key}", shape.name))
        })
        .collect();
    assert!(
        unrecorded.is_empty(),
        "read on open, never written, and missing from READ_ONLY: {unrecorded:?}"
    );
    for (name, key, reason) in READ_ONLY {
        let shape = shapes
            .iter()
            .find(|shape| shape.name == *name)
            .unwrap_or_else(|| panic!("READ_ONLY names no checked shape {name}"));
        assert!(
            shape.read.contains(key) && !shape.written.contains(key),
            "stale READ_ONLY entry {name}.{key} ({reason}): read {}, written {}",
            shape.read.contains(key),
            shape.written.contains(key)
        );
    }
}

#[derive(Debug)]
enum Probe {
    Fields(&'static [&'static str]),
    Other(String),
}

impl fmt::Display for Probe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fields(fields) => write!(f, "struct fields {fields:?}"),
            Self::Other(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Probe {}

impl ser::Error for Probe {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Other(msg.to_string())
    }
}

impl de::Error for Probe {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Other(msg.to_string())
    }
}

fn not_a_struct(method: &str) -> Probe {
    Probe::Other(format!("called {method} where a struct was expected"))
}

/// Records the key of every field a struct writes or skips.
struct Keys<'a>(&'a mut BTreeSet<&'static str>);

macro_rules! save_not_a_struct {
    ($($method:ident($($ty:ty),*) -> $ret:ty;)*) => {
        $(
            fn $method(self, $(_: $ty),*) -> Result<$ret, Probe> {
                Err(not_a_struct(stringify!($method)))
            }
        )*
    };
}

impl<'a> Serializer for Keys<'a> {
    type Ok = ();
    type Error = Probe;
    type SerializeSeq = Impossible<(), Probe>;
    type SerializeTuple = Impossible<(), Probe>;
    type SerializeTupleStruct = Impossible<(), Probe>;
    type SerializeTupleVariant = Impossible<(), Probe>;
    type SerializeMap = Impossible<(), Probe>;
    type SerializeStruct = Keys<'a>;
    type SerializeStructVariant = Impossible<(), Probe>;

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Probe> {
        Ok(self)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<(), Probe> {
        Err(not_a_struct("serialize_some"))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<(), Probe> {
        Err(not_a_struct("serialize_newtype_struct"))
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<(), Probe> {
        Err(not_a_struct("serialize_newtype_variant"))
    }

    save_not_a_struct! {
        serialize_bool(bool) -> ();
        serialize_i8(i8) -> ();
        serialize_i16(i16) -> ();
        serialize_i32(i32) -> ();
        serialize_i64(i64) -> ();
        serialize_u8(u8) -> ();
        serialize_u16(u16) -> ();
        serialize_u32(u32) -> ();
        serialize_u64(u64) -> ();
        serialize_f32(f32) -> ();
        serialize_f64(f64) -> ();
        serialize_char(char) -> ();
        serialize_str(&str) -> ();
        serialize_bytes(&[u8]) -> ();
        serialize_none() -> ();
        serialize_unit() -> ();
        serialize_unit_struct(&'static str) -> ();
        serialize_unit_variant(&'static str, u32, &'static str) -> ();
        serialize_seq(Option<usize>) -> Self::SerializeSeq;
        serialize_tuple(usize) -> Self::SerializeTuple;
        serialize_tuple_struct(&'static str, usize) -> Self::SerializeTupleStruct;
        serialize_tuple_variant(&'static str, u32, &'static str, usize) -> Self::SerializeTupleVariant;
        serialize_map(Option<usize>) -> Self::SerializeMap;
        serialize_struct_variant(&'static str, u32, &'static str, usize) -> Self::SerializeStructVariant;
    }
}

impl SerializeStruct for Keys<'_> {
    type Ok = ();
    type Error = Probe;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        _: &T,
    ) -> Result<(), Probe> {
        self.0.insert(key);
        Ok(())
    }

    fn skip_field(&mut self, key: &'static str) -> Result<(), Probe> {
        self.0.insert(key);
        Ok(())
    }

    fn end(self) -> Result<(), Probe> {
        Ok(())
    }
}

/// Fails every read, returning the field names serde asked a struct for.
struct Fields;

macro_rules! open_not_a_struct {
    ($($method:ident($($ty:ty),*);)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, $(_: $ty,)* _: V) -> Result<V::Value, Probe> {
                Err(not_a_struct(stringify!($method)))
            }
        )*
    };
}

impl<'de> Deserializer<'de> for Fields {
    type Error = Probe;

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Probe> {
        Err(Probe::Fields(fields))
    }

    open_not_a_struct! {
        deserialize_any();
        deserialize_bool();
        deserialize_i8();
        deserialize_i16();
        deserialize_i32();
        deserialize_i64();
        deserialize_u8();
        deserialize_u16();
        deserialize_u32();
        deserialize_u64();
        deserialize_f32();
        deserialize_f64();
        deserialize_char();
        deserialize_str();
        deserialize_string();
        deserialize_bytes();
        deserialize_byte_buf();
        deserialize_option();
        deserialize_unit();
        deserialize_unit_struct(&'static str);
        deserialize_newtype_struct(&'static str);
        deserialize_seq();
        deserialize_tuple(usize);
        deserialize_tuple_struct(&'static str, usize);
        deserialize_map();
        deserialize_enum(&'static str, &'static [&'static str]);
        deserialize_identifier();
        deserialize_ignored_any();
    }
}
