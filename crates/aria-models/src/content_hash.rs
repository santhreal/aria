//! The hash of a value's serialized content, fed to the hasher field by field
//! without formatting it.
//!
//! [`Track::revision`](crate::Track::revision) hashes a lane on its first read
//! after a write. Hashing the lane's JSON formatted every number and fed each
//! token to SipHash as its own slice: 2,059,950 writes for the 3.6 MB of
//! `project-10k`'s lanes, 11.6 ms of a 29.8 ms projection of the opened project
//! on the Server machine (i7-11700K). This serializer writes each value as a
//! tag byte and its raw bits into a stack buffer, and the hasher reads the
//! buffer in 256-byte slices.
//!
//! The encoding is prefix-free: every value starts with a tag, every container
//! ends with [`tag::END`], and lengths precede strings and bytes. Two values
//! that hash alike write the same JSON, unless SipHash collides. The converse
//! does not hold: a unit variant and the string of its name, a struct and the
//! map of its fields, or two NaN payloads write the same JSON and hash apart.

use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::ser;

/// Bytes buffered before they reach the hasher.
const CHUNK: usize = 256;

/// The most bytes a `u64` takes in LEB128.
const LEB128_MAX: usize = 10;

/// The tag byte that starts each encoded value.
mod tag {
    pub(super) const FALSE: u8 = 0;
    pub(super) const TRUE: u8 = 1;
    /// A zigzag LEB128 signed integer up to 64 bits.
    pub(super) const INT: u8 = 2;
    /// A LEB128 unsigned integer up to 64 bits.
    pub(super) const UINT: u8 = 3;
    pub(super) const INT128: u8 = 4;
    pub(super) const UINT128: u8 = 5;
    pub(super) const F32: u8 = 6;
    pub(super) const F64: u8 = 7;
    pub(super) const CHAR: u8 = 8;
    pub(super) const STR: u8 = 9;
    pub(super) const BYTES: u8 = 10;
    pub(super) const NONE: u8 = 11;
    pub(super) const SOME: u8 = 12;
    pub(super) const UNIT: u8 = 13;
    pub(super) const UNIT_VARIANT: u8 = 14;
    pub(super) const NEWTYPE_VARIANT: u8 = 15;
    pub(super) const SEQ: u8 = 16;
    pub(super) const TUPLE_VARIANT: u8 = 17;
    pub(super) const MAP: u8 = 18;
    pub(super) const STRUCT: u8 = 19;
    pub(super) const STRUCT_VARIANT: u8 = 20;
    /// Closes a sequence, map, struct, or variant body.
    pub(super) const END: u8 = 21;
}

/// A hash of `value`'s serialized content.
pub(crate) fn content_hash<T: Serialize + ?Sized>(value: &T) -> u64 {
    let mut feed = Feed {
        hasher: DefaultHasher::new(),
        buf: [0; CHUNK],
        len: 0,
    };
    // A `Serialize` impl may stop with its own error; the hash then covers what
    // it wrote before stopping, as the JSON writer's did.
    let _ = value.serialize(&mut feed);
    feed.finish()
}

std::thread_local! {
    /// Set while [`summary_hash`] runs on this thread.
    static SUMMARY: Cell<bool> = const { Cell::new(false) };
}

/// [`content_hash`] of `value`, with each part that holds a [`Kept`] hash of
/// itself written as that hash instead of its content. A part tests
/// [`in_summary`] to choose. A lane hashed this way re-hashes only the clips
/// written to since its last hash, and two values that hash alike still hold
/// the same content, unless SipHash collides.
pub(crate) fn summary_hash<T: Serialize + ?Sized>(value: &T) -> u64 {
    /// Puts back the flag the summary found, on return and on unwind.
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            SUMMARY.with(|summary| summary.set(self.0));
        }
    }
    let _restore = Restore(SUMMARY.with(|summary| summary.replace(true)));
    content_hash(value)
}

/// Whether the value being serialized on this thread is being hashed by
/// [`summary_hash`], so a part with a [`Kept`] hash writes the hash.
pub(crate) fn in_summary() -> bool {
    SUMMARY.with(Cell::get)
}

/// A [`content_hash`] held beside the value it hashes: computed on the first
/// read and held until the holder's next write drops it.
///
/// A clone holds the same hash, since it holds the same content. Two holders
/// compare equal whatever they hold: the hash follows the content, so equal
/// content cannot hold two different hashes, and a holder that has not hashed
/// yet holds none.
#[derive(Default)]
pub(crate) struct Kept(AtomicU64);

impl Kept {
    /// The held hash, or the result of `hash`, which is then held.
    pub(crate) fn get_or(&self, hash: impl FnOnce() -> u64) -> u64 {
        let kept = self.0.load(Ordering::Relaxed);
        if kept != 0 {
            return kept;
        }
        // Zero marks a hash not computed yet.
        let hashed = hash().max(1);
        self.0.store(hashed, Ordering::Relaxed);
        hashed
    }

    /// Drop the held hash, so the next read computes it again.
    pub(crate) fn forget(&mut self) {
        *self.0.get_mut() = 0;
    }
}

impl Clone for Kept {
    fn clone(&self) -> Self {
        Self(AtomicU64::new(self.0.load(Ordering::Relaxed)))
    }
}

impl PartialEq for Kept {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Kept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Kept")
    }
}

/// Encoded bytes on their way to the hasher.
struct Feed {
    hasher: DefaultHasher,
    buf: [u8; CHUNK],
    len: usize,
}

impl Feed {
    #[inline(always)]
    fn byte(&mut self, byte: u8) {
        if self.len == CHUNK {
            self.flush();
        }
        self.buf[self.len] = byte;
        self.len += 1;
    }

    #[inline]
    fn bytes(&mut self, bytes: &[u8]) {
        if bytes.len() > CHUNK - self.len {
            self.flush();
            if bytes.len() > CHUNK {
                self.hasher.write(bytes);
                return;
            }
        }
        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
    }

    /// `value` in LEB128, written into the buffer in place: most values here
    /// are lengths and small integers that take one byte.
    #[inline(always)]
    fn leb128(&mut self, mut value: u64) {
        if CHUNK - self.len < LEB128_MAX {
            self.flush();
        }
        while value >= 0x80 {
            self.buf[self.len] = value as u8 | 0x80;
            self.len += 1;
            value >>= 7;
        }
        self.buf[self.len] = value as u8;
        self.len += 1;
    }

    #[inline(always)]
    fn str(&mut self, text: &str) {
        self.byte(tag::STR);
        self.leb128(text.len() as u64);
        self.bytes(text.as_bytes());
    }

    /// A variant's tag and name. The name, not the index, because the JSON
    /// form holds the name, and the variants of two enums an untagged enum
    /// wraps share indices.
    fn variant(&mut self, tag: u8, variant: &str) {
        self.byte(tag);
        self.str(variant);
    }

    fn flush(&mut self) {
        self.hasher.write(&self.buf[..self.len]);
        self.len = 0;
    }

    fn finish(mut self) -> u64 {
        self.flush();
        self.hasher.finish()
    }
}

/// The error a `Serialize` impl stops with. The content serializer itself
/// never fails.
#[derive(Debug)]
struct Stopped;

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the value stopped its own serialization")
    }
}

impl std::error::Error for Stopped {}

impl ser::Error for Stopped {
    fn custom<T: std::fmt::Display>(_: T) -> Self {
        Self
    }
}

type Done = Result<(), Stopped>;

impl ser::Serializer for &mut Feed {
    type Ok = ();
    type Error = Stopped;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, v: bool) -> Done {
        self.byte(if v { tag::TRUE } else { tag::FALSE });
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Done {
        self.serialize_i64(v.into())
    }

    fn serialize_i16(self, v: i16) -> Done {
        self.serialize_i64(v.into())
    }

    fn serialize_i32(self, v: i32) -> Done {
        self.serialize_i64(v.into())
    }

    fn serialize_i64(self, v: i64) -> Done {
        self.byte(tag::INT);
        self.leb128(((v << 1) ^ (v >> 63)) as u64);
        Ok(())
    }

    fn serialize_i128(self, v: i128) -> Done {
        self.byte(tag::INT128);
        self.bytes(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Done {
        self.serialize_u64(v.into())
    }

    fn serialize_u16(self, v: u16) -> Done {
        self.serialize_u64(v.into())
    }

    fn serialize_u32(self, v: u32) -> Done {
        self.serialize_u64(v.into())
    }

    fn serialize_u64(self, v: u64) -> Done {
        self.byte(tag::UINT);
        self.leb128(v);
        Ok(())
    }

    fn serialize_u128(self, v: u128) -> Done {
        self.byte(tag::UINT128);
        self.bytes(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Done {
        self.byte(tag::F32);
        self.bytes(&v.to_bits().to_le_bytes());
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Done {
        self.byte(tag::F64);
        self.bytes(&v.to_bits().to_le_bytes());
        Ok(())
    }

    fn serialize_char(self, v: char) -> Done {
        self.byte(tag::CHAR);
        self.leb128(u32::from(v).into());
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Done {
        self.str(v);
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Done {
        self.byte(tag::BYTES);
        self.leb128(v.len() as u64);
        self.bytes(v);
        Ok(())
    }

    fn serialize_none(self) -> Done {
        self.byte(tag::NONE);
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Done {
        self.byte(tag::SOME);
        value.serialize(self)
    }

    fn serialize_unit(self) -> Done {
        self.byte(tag::UNIT);
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Done {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Done {
        self.variant(tag::UNIT_VARIANT, variant);
        Ok(())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Done {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Done {
        self.variant(tag::NEWTYPE_VARIANT, variant);
        value.serialize(self)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self, Stopped> {
        self.byte(tag::SEQ);
        Ok(self)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self, Stopped> {
        self.byte(tag::SEQ);
        Ok(self)
    }

    fn serialize_tuple_struct(self, _name: &'static str, _len: usize) -> Result<Self, Stopped> {
        self.byte(tag::SEQ);
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self, Stopped> {
        self.variant(tag::TUPLE_VARIANT, variant);
        Ok(self)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self, Stopped> {
        self.byte(tag::MAP);
        Ok(self)
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self, Stopped> {
        self.byte(tag::STRUCT);
        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self, Stopped> {
        self.variant(tag::STRUCT_VARIANT, variant);
        Ok(self)
    }
}

impl ser::SerializeSeq for &mut Feed {
    type Ok = ();
    type Error = Stopped;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        value.serialize(&mut **self)
    }

    fn end(self) -> Done {
        self.byte(tag::END);
        Ok(())
    }
}

impl ser::SerializeTuple for &mut Feed {
    type Ok = ();
    type Error = Stopped;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        value.serialize(&mut **self)
    }

    fn end(self) -> Done {
        self.byte(tag::END);
        Ok(())
    }
}

impl ser::SerializeTupleStruct for &mut Feed {
    type Ok = ();
    type Error = Stopped;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        value.serialize(&mut **self)
    }

    fn end(self) -> Done {
        self.byte(tag::END);
        Ok(())
    }
}

impl ser::SerializeTupleVariant for &mut Feed {
    type Ok = ();
    type Error = Stopped;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        value.serialize(&mut **self)
    }

    fn end(self) -> Done {
        self.byte(tag::END);
        Ok(())
    }
}

impl ser::SerializeMap for &mut Feed {
    type Ok = ();
    type Error = Stopped;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Done {
        key.serialize(&mut **self)
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Done {
        value.serialize(&mut **self)
    }

    fn end(self) -> Done {
        self.byte(tag::END);
        Ok(())
    }
}

impl ser::SerializeStruct for &mut Feed {
    type Ok = ();
    type Error = Stopped;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Done {
        self.str(key);
        value.serialize(&mut **self)
    }

    fn end(self) -> Done {
        self.byte(tag::END);
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut Feed {
    type Ok = ();
    type Error = Stopped;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Done {
        self.str(key);
        value.serialize(&mut **self)
    }

    fn end(self) -> Done {
        self.byte(tag::END);
        Ok(())
    }
}

#[cfg(test)]
#[path = "content_hash_tests.rs"]
mod tests;
