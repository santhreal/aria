//! Pretty JSON byte for byte as `serde_json::to_vec_pretty` writes it, with
//! each line break written as one slice.
//!
//! `serde_json`'s `PrettyFormatter` writes the comma, the newline, and then
//! one two-byte `write_all` per indent level for every key and array element.
//! A `project-10k` document nests clips seven to ten levels deep, so those
//! per-level writes and their `memmove` calls were most of a save's
//! serialization on the Server machine (i7-11700K): 428 of 537 perf samples
//! under `save_to_file` were in `Project::serialize`, with `memmove` the top
//! leaf at 144.

use std::io;

use serde_json::ser::Formatter;

/// Indent levels one slice of [`LINE`] covers; deeper levels write more runs
/// of its spaces.
const LEVELS: usize = 32;

/// A comma, a line break, and [`LEVELS`] two-space indent levels. A line
/// break without the comma starts at byte 1; indent past [`LEVELS`] repeats
/// the spaces from byte 2.
const LINE: [u8; 2 + 2 * LEVELS] = {
    let mut bytes = [b' '; 2 + 2 * LEVELS];
    bytes[0] = b',';
    bytes[1] = b'\n';
    bytes
};

/// `serde_json`'s `PrettyFormatter` with a two-space indent: the same state,
/// the same bytes, fewer writes.
#[derive(Default)]
pub(super) struct Pretty {
    level: usize,
    has_value: bool,
}

impl Pretty {
    /// An optional comma, a line break, and the indent of the current level.
    fn line<W: ?Sized + io::Write>(&self, writer: &mut W, comma: bool) -> io::Result<()> {
        let mut spaces = 2 * self.level;
        let run = spaces.min(2 * LEVELS);
        writer.write_all(&LINE[usize::from(!comma)..2 + run])?;
        spaces -= run;
        while spaces > 0 {
            let run = spaces.min(2 * LEVELS);
            writer.write_all(&LINE[2..2 + run])?;
            spaces -= run;
        }
        Ok(())
    }

    fn open<W: ?Sized + io::Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        self.level += 1;
        self.has_value = false;
        writer.write_all(bracket)
    }

    fn close<W: ?Sized + io::Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        self.level -= 1;
        if self.has_value {
            self.line(writer, false)?;
        }
        writer.write_all(bracket)
    }
}

impl Formatter for Pretty {
    fn begin_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"[")
    }

    fn end_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"]")
    }

    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.line(writer, !first)
    }

    fn end_array_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    fn begin_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"{")
    }

    fn end_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"}")
    }

    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.line(writer, !first)
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }

    fn end_object_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }
}

/// Write `value` as two-space pretty JSON, matching the bytes
/// `serde_json::to_writer_pretty` writes for it.
pub(super) fn to_writer<W: io::Write, T: ?Sized + serde::Serialize>(
    mut writer: W,
    value: &T,
) -> serde_json::Result<()> {
    value.serialize(&mut serde_json::Serializer::with_formatter(
        &mut writer,
        Pretty::default(),
    ))
}

/// `value` as two-space pretty JSON, the bytes `serde_json::to_vec_pretty`
/// writes for it.
pub(super) fn to_vec<T: ?Sized + serde::Serialize>(value: &T) -> serde_json::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(128);
    to_writer(&mut out, value)?;
    Ok(out)
}

#[cfg(test)]
#[path = "pretty_tests.rs"]
mod tests;
