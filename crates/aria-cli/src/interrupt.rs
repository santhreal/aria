//! Stop a running command at its next safe point when the shell signals it.
//!
//! Without a handler, SIGINT, SIGTERM or SIGHUP ended `aria export` mid-encode,
//! and the export's staging directory, with a partial file in it, stayed in the
//! output folder. After [`Interrupt::install`], the first of those signals sets
//! a flag the command polls between frames; the command returns through its own
//! cleanup and exits with [`Interrupt::status`]. A second signal ends the
//! process at once, with the status a shell reports for that signal.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use signal_hook::consts::{SIGINT, SIGTERM};

use crate::fail::Failure;

/// The signals that stop a command. SIGHUP arrives when the terminal closes.
#[cfg(unix)]
const SIGNALS: [i32; 3] = [signal_hook::consts::SIGHUP, SIGINT, SIGTERM];
#[cfg(not(unix))]
const SIGNALS: [i32; 2] = [SIGINT, SIGTERM];

/// The signal that arrived, if any, since [`Interrupt::install`].
#[derive(Debug)]
pub(crate) struct Interrupt {
    signal: Arc<AtomicUsize>,
}

impl Interrupt {
    /// Replace the default action of [`SIGNALS`] for the rest of the process.
    pub(crate) fn install() -> Result<Self, Failure> {
        let raised = Arc::new(AtomicBool::new(false));
        let signal = Arc::new(AtomicUsize::new(0));
        let failed = |err: std::io::Error| {
            Failure::Failed(format!("could not install the signal handlers: {err}"))
        };
        for sig in SIGNALS {
            // Registered first, so it reads `raised` before this signal sets it:
            // only a second signal finds it set.
            signal_hook::flag::register_conditional_shutdown(sig, 128 + sig, Arc::clone(&raised))
                .map_err(failed)?;
            signal_hook::flag::register(sig, Arc::clone(&raised)).map_err(failed)?;
            signal_hook::flag::register_usize(sig, Arc::clone(&signal), sig as usize)
                .map_err(failed)?;
        }
        Ok(Self { signal })
    }

    /// The exit status for the signal that arrived: 128 plus its number, as a
    /// shell reports a process that signal ended. `None` before any arrived.
    pub(crate) fn status(&self) -> Option<i32> {
        match self.signal.load(Ordering::SeqCst) {
            0 => None,
            sig => Some(128 + sig as i32),
        }
    }
}
