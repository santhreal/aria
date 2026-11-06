//! Standard output and error that a closed reader cannot crash.
//!
//! `std`'s `print!` family panics when a write fails, so `aria export … | head
//! -1` ended in a panic (exit 101) once `head` closed the pipe, and the panic
//! skipped [`aria_decoder::exit_process`]. The crate's `print!`, `println!`,
//! `eprint!` and `eprintln!` (defined in `main.rs` ahead of every module) write
//! through here instead. A closed reader stops further output on its stream and
//! the command runs to its end. Any other failed write to stdout fails the
//! command with exit 1, since its results never arrived.

use std::fmt;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

/// One standard stream: whether output to it stopped, and the reason when the
/// reader did not close it.
pub(crate) struct Stream {
    stopped: AtomicBool,
    failure: Mutex<Option<String>>,
}

static STDOUT: Stream = Stream::new();
static STDERR: Stream = Stream::new();

impl Stream {
    const fn new() -> Self {
        Self {
            stopped: AtomicBool::new(false),
            failure: Mutex::new(None),
        }
    }

    /// Write `args` to `sink`, or nothing once output to this stream stopped.
    fn write(&self, sink: &mut impl Write, args: fmt::Arguments<'_>) {
        if self.stopped.load(Ordering::Relaxed) {
            return;
        }
        if let Err(err) = sink.write_fmt(args) {
            self.stop(&err);
        }
    }

    /// Flush `sink`, unless output to this stream stopped.
    fn flush(&self, sink: &mut impl Write) {
        if self.stopped.load(Ordering::Relaxed) {
            return;
        }
        if let Err(err) = sink.flush() {
            self.stop(&err);
        }
    }

    /// Stop output after `err`, keeping the first reason that is not a closed
    /// reader.
    fn stop(&self, err: &io::Error) {
        self.stopped.store(true, Ordering::Relaxed);
        if err.kind() != io::ErrorKind::BrokenPipe {
            self.failure
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert_with(|| err.to_string());
        }
    }

    /// The exit code of a command that ended with `code`, given what happened to
    /// its output on this stream: a success whose output failed to write is a
    /// failure, and any other code stands.
    fn exit_code(&self, code: u8) -> u8 {
        let failure = self
            .failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match failure {
            Some(err) if code == 0 => {
                stderr(format_args!("aria: could not write the output: {err}\n"));
                1
            }
            _ => code,
        }
    }
}

/// `print!` and `println!`.
pub(crate) fn stdout(args: fmt::Arguments<'_>) {
    STDOUT.write(&mut io::stdout().lock(), args);
}

/// `eprint!` and `eprintln!`.
pub(crate) fn stderr(args: fmt::Arguments<'_>) {
    STDERR.write(&mut io::stderr().lock(), args);
}

/// Flush stdout and return the process exit code for a command that ended with
/// `code`.
pub(crate) fn finish(code: u8) -> u8 {
    STDOUT.flush(&mut io::stdout().lock());
    STDOUT.exit_code(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sink that fails every write with `fails`, or takes every byte, and
    /// counts the writes it receives.
    struct Sink {
        fails: Option<io::ErrorKind>,
        writes: usize,
        bytes: Vec<u8>,
    }

    impl Sink {
        fn new(fails: Option<io::ErrorKind>) -> Self {
            Self {
                fails,
                writes: 0,
                bytes: Vec::new(),
            }
        }
    }

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            match self.fails {
                Some(kind) => Err(io::Error::from(kind)),
                None => {
                    self.bytes.extend_from_slice(buf);
                    Ok(buf.len())
                }
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            match self.fails {
                Some(kind) => Err(io::Error::from(kind)),
                None => Ok(()),
            }
        }
    }

    /// WHY: a reader that closed the pipe (`aria export … | head -1`) made the
    /// next print panic, exit 101, and skip `exit_process`. Closes the class
    /// "a closed reader ends or fails the command" for writes and flushes on
    /// either stream, which share `Stream`. Not caught: a library that writes
    /// to the descriptor directly rather than through these macros.
    #[test]
    fn a_closed_reader_stops_the_stream_and_the_command_still_succeeds() {
        for flush_first in [false, true] {
            let stream = Stream::new();
            let mut sink = Sink::new(Some(io::ErrorKind::BrokenPipe));
            if flush_first {
                stream.flush(&mut sink);
            } else {
                stream.write(&mut sink, format_args!("wrote {} frames\n", 98));
            }
            let attempts = sink.writes;
            stream.write(&mut sink, format_args!("encoder nvh264enc\n"));
            stream.flush(&mut sink);
            assert_eq!(
                sink.writes, attempts,
                "a stream whose reader closed is written again (flush first: {flush_first})"
            );
            assert_eq!(stream.exit_code(0), 0, "flush first: {flush_first}");
            assert_eq!(stream.exit_code(2), 2, "flush first: {flush_first}");
        }
    }

    /// WHY: a write that fails for any reason but a closed reader (a full
    /// disk behind `> out.txt`) lost the results while the command reported
    /// success. The first such failure turns exit 0 into exit 1, stops the
    /// stream, and leaves a usage or command failure's own code in place.
    #[test]
    fn a_failed_write_that_is_not_a_closed_reader_fails_a_successful_command() {
        for kind in [
            io::ErrorKind::StorageFull,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Other,
        ] {
            let stream = Stream::new();
            let mut sink = Sink::new(Some(kind));
            stream.write(&mut sink, format_args!("first\n"));
            stream.write(&mut sink, format_args!("second\n"));
            assert_eq!(sink.writes, 1, "{kind:?}: written after it failed");
            assert_eq!(stream.exit_code(0), 1, "{kind:?}");
            assert_eq!(stream.exit_code(2), 2, "{kind:?}");
        }
    }

    /// A stream with a working reader passes every byte through in order.
    #[test]
    fn a_working_stream_writes_every_line_in_order() {
        let stream = Stream::new();
        let mut sink = Sink::new(None);
        stream.write(&mut sink, format_args!("wrote {} frames\n", 98));
        stream.write(&mut sink, format_args!("encoder {}\n", "x264enc"));
        stream.flush(&mut sink);
        assert_eq!(sink.bytes, b"wrote 98 frames\nencoder x264enc\n");
        assert_eq!(stream.exit_code(0), 0);
    }
}
