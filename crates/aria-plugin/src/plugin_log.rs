//! The lines each plugin logged, kept for the Plugins list.
//!
//! Every tier writes through one call: an in-process plugin with
//! [`PluginContext::log`](crate::PluginContext::log) on its view, a WASM
//! component through the `log` import, and a native library through the `log`
//! host call, both of which reach
//! [`PluginContext::log_for`](crate::PluginContext::log_for). The host keeps the
//! newest [`LOG_LINES_PER_PLUGIN`] lines of each plugin for the session and
//! sends every line to `tracing` as well.
//!
//! A failure, a fault, and a caught panic are logged too, a panic with the
//! backtrace of the thread that panicked.

use std::backtrace::Backtrace;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::sync::Once;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;

/// Lines kept per plugin. The oldest drops first.
pub const LOG_LINES_PER_PLUGIN: usize = 256;

/// Bytes kept of one line, marker included. A longer line is cut on a
/// character boundary and ends with [`LOG_LINE_TRUNCATED`].
pub const LOG_LINE_MAX_BYTES: usize = 4096;

/// What a cut line ends with.
pub const LOG_LINE_TRUNCATED: &str = " [truncated]";

/// How severe a log line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Every level, least severe first.
    pub const ALL: [Self; 5] = [
        Self::Trace,
        Self::Debug,
        Self::Info,
        Self::Warn,
        Self::Error,
    ];

    /// The lowercase name: `trace`, `debug`, `info`, `warn`, `error`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One logged line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub level: LogLevel,
    /// At most [`LOG_LINE_MAX_BYTES`] bytes.
    pub text: Arc<str>,
    pub at: SystemTime,
}

/// Every plugin's ring of lines, keyed by plugin id.
///
/// A ring outlives its plugin's unmount and reload, so the lines that explain
/// a failure stay readable after it; [`clear`](Self::clear) drops one.
#[derive(Default)]
pub(crate) struct PluginLogs {
    rings: RwLock<FxHashMap<Box<str>, Mutex<VecDeque<LogLine>>>>,
    revision: AtomicU64,
}

impl PluginLogs {
    /// Keep `text` as `plugin`'s newest line and send it to `tracing`.
    pub(crate) fn push(&self, plugin: &str, level: LogLevel, text: &str) {
        trace(plugin, level, text);
        let line = LogLine {
            level,
            text: bounded(text),
            at: SystemTime::now(),
        };
        {
            let rings = self.rings.read();
            if let Some(ring) = rings.get(plugin) {
                push_line(&mut ring.lock(), line);
                self.revision.fetch_add(1, Ordering::Release);
                return;
            }
        }
        let mut rings = self.rings.write();
        let ring = rings
            .entry(plugin.into())
            .or_insert_with(|| Mutex::new(VecDeque::with_capacity(LOG_LINES_PER_PLUGIN)));
        push_line(ring.get_mut(), line);
        drop(rings);
        self.revision.fetch_add(1, Ordering::Release);
    }

    /// `plugin`'s lines, oldest first.
    pub(crate) fn lines(&self, plugin: &str) -> Vec<LogLine> {
        self.rings
            .read()
            .get(plugin)
            .map(|ring| ring.lock().iter().cloned().collect())
            .unwrap_or_default()
    }

    /// How many lines `plugin` has.
    pub(crate) fn count(&self, plugin: &str) -> usize {
        self.rings
            .read()
            .get(plugin)
            .map_or(0, |ring| ring.lock().len())
    }

    /// Drop `plugin`'s lines. Returns whether it had any.
    pub(crate) fn clear(&self, plugin: &str) -> bool {
        let removed = self.rings.write().remove(plugin).is_some();
        if removed {
            self.revision.fetch_add(1, Ordering::Release);
        }
        removed
    }

    /// Moves on every line kept and every ring dropped.
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
}

impl fmt::Debug for PluginLogs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginLogs")
            .field("plugins", &self.rings.read().len())
            .field("revision", &self.revision())
            .finish()
    }
}

fn push_line(ring: &mut VecDeque<LogLine>, line: LogLine) {
    if ring.len() == LOG_LINES_PER_PLUGIN {
        ring.pop_front();
    }
    ring.push_back(line);
}

pub(crate) fn trace(plugin: &str, level: LogLevel, text: &str) {
    match level {
        LogLevel::Trace => tracing::trace!(plugin = %plugin, "{text}"),
        LogLevel::Debug => tracing::debug!(plugin = %plugin, "{text}"),
        LogLevel::Info => tracing::info!(plugin = %plugin, "{text}"),
        LogLevel::Warn => tracing::warn!(plugin = %plugin, "{text}"),
        LogLevel::Error => tracing::error!(plugin = %plugin, "{text}"),
    }
}

/// `text` cut to [`LOG_LINE_MAX_BYTES`] on a character boundary, marker
/// included.
fn bounded(text: &str) -> Arc<str> {
    if text.len() <= LOG_LINE_MAX_BYTES {
        return Arc::from(text);
    }
    let cut = text.floor_char_boundary(LOG_LINE_MAX_BYTES - LOG_LINE_TRUNCATED.len());
    let mut kept = String::with_capacity(cut + LOG_LINE_TRUNCATED.len());
    kept.push_str(&text[..cut]);
    kept.push_str(LOG_LINE_TRUNCATED);
    Arc::from(kept)
}

thread_local! {
    /// The message and backtrace of the last panic on this thread, until a
    /// failure that names the message takes it.
    static PANICKED: RefCell<Option<(String, Backtrace)>> = const { RefCell::new(None) };
}

/// Keep the backtrace of every panic on its thread, then run the hook that was
/// installed before. Installed once per process, by the first context.
pub(crate) fn capture_panics() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            let backtrace = Backtrace::force_capture();
            let _ = PANICKED.try_with(|slot| {
                if let Ok(mut slot) = slot.try_borrow_mut() {
                    *slot = Some((message, backtrace));
                }
            });
            previous(info);
        }));
    });
}

/// The backtrace of this thread's last panic, when `reason` includes its
/// message: a failure built from a caught panic. Taken either way, so a later
/// failure on this thread never reads an older panic.
pub(crate) fn take_panic_backtrace(reason: &str) -> Option<String> {
    let (message, backtrace) = PANICKED
        .try_with(|slot| slot.try_borrow_mut().ok().and_then(|mut slot| slot.take()))
        .ok()
        .flatten()?;
    (!message.is_empty() && reason.contains(message.as_str()))
        .then(|| from_panic_site(&backtrace.to_string()))
}

/// A rendered backtrace from the first frame after the panic machinery, so it
/// starts at the code that panicked. The whole text when no frame of the
/// machinery is found.
fn from_panic_site(rendered: &str) -> String {
    let mut cut = 0;
    let mut offset = 0;
    let mut machinery = false;
    for line in rendered.split_inclusive('\n') {
        if let Some(symbol) = frame_symbol(line) {
            let inside = PANIC_MACHINERY.iter().any(|name| symbol.contains(name));
            if machinery && !inside {
                break;
            }
            machinery |= inside;
        }
        offset += line.len();
        if machinery {
            cut = offset;
        }
    }
    rendered[cut..].to_owned()
}

/// Symbols of the frames between a `panic!` and the hook.
const PANIC_MACHINERY: [&str; 4] = [
    "std::panicking::",
    "core::panicking::",
    "rust_begin_unwind",
    "__rust_end_short_backtrace",
];

/// The symbol of a backtrace line that opens a frame, `  12: symbol`.
fn frame_symbol(line: &str) -> Option<&str> {
    let (index, symbol) = line.trim_start().split_once(": ")?;
    (!index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())).then_some(symbol)
}

#[cfg(test)]
#[path = "plugin_log_tests.rs"]
mod tests;
