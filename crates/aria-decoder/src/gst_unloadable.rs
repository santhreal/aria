//! Plugins that failed to load, remembered across processes.
//!
//! Loading a plugin maps it and every library it links, and GStreamer never
//! unloads one. The registry can list an element whose plugin no longer
//! provides it: a CUDA plugin scanned while the driver worked loads, finds no
//! usable device, and registers nothing, which leaves about 12.8 MiB mapped in
//! every process that tries it. GStreamer records no such failure, so each
//! process pays again: in the device warm-up, and in the first `uridecodebin`
//! that autoplugs one of the plugin's decoders by rank.
//!
//! A failed load is recorded with the plugin file it read and the boot it ran
//! on. A later process on the same boot with the same plugin file does not load
//! the plugin and takes every feature the plugin lists out of autoplugging. A
//! new boot (which a mismatched kernel module and driver library need) or a
//! changed plugin file loads it again, and a load that succeeds clears the
//! record.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use gstreamer as gst;
use gstreamer::prelude::*;

/// The first line of the record file. A file that starts otherwise has another
/// layout: its records are ignored and the next write replaces them.
const HEADER: &str = "aria unloadable plugins 1";

/// One plugin file on one boot, as a load read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    plugin: String,
    file: PathBuf,
    size: u64,
    modified_ns: u128,
    boot: String,
}

impl Stamp {
    /// The plugin that provides `factory`, on this boot. `None` when the
    /// registry names no file for it, or the file or the boot id cannot be
    /// read.
    pub(crate) fn of(factory: &gst::ElementFactory) -> Option<Self> {
        let plugin = factory.plugin_name()?;
        let file = gst::Registry::get().find_plugin(&plugin)?.filename()?;
        let meta = std::fs::metadata(&file).ok()?;
        let modified_ns = meta
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        Some(Self {
            plugin: plugin.to_string(),
            file,
            size: meta.len(),
            modified_ns,
            boot: boot.trim().to_owned(),
        })
    }

    fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.plugin,
            self.file.display(),
            self.size,
            self.modified_ns,
            self.boot
        )
    }

    /// Whether `line` records this plugin, whatever file or boot it names.
    fn names(&self, line: &str) -> bool {
        line.split('\t').next() == Some(self.plugin.as_str())
    }
}

/// The record file for this account, under GLib's per-account cache directory.
pub(crate) fn user_records() -> PathBuf {
    gst::glib::user_cache_dir()
        .join("aria")
        .join("unloadable-plugins")
}

/// Whether `records` holds a failed load of `stamp`'s plugin file on its boot.
pub(crate) fn failed_before(records: &Path, stamp: &Stamp) -> bool {
    let Ok(text) = std::fs::read_to_string(records) else {
        return false;
    };
    let mut lines = text.lines();
    lines.next() == Some(HEADER) && lines.any(|line| line == stamp.line())
}

/// Record whether a load of `stamp`'s plugin `failed`, replacing any earlier
/// record of the plugin. The file is replaced whole, so a reader in another
/// process sees the old records or the new ones.
pub(crate) fn record(records: &Path, stamp: &Stamp, failed: bool) -> std::io::Result<()> {
    let text = std::fs::read_to_string(records).unwrap_or_default();
    let mut lines = text.lines();
    let kept: Vec<&str> = if lines.next() == Some(HEADER) {
        lines.filter(|line| !stamp.names(line)).collect()
    } else {
        Vec::new()
    };
    if !failed && kept.len() == text.lines().count().saturating_sub(1) {
        // No record of the plugin to clear.
        return Ok(());
    }
    let mut next = String::from(HEADER);
    next.push('\n');
    for line in kept {
        next.push_str(line);
        next.push('\n');
    }
    if failed {
        next.push_str(&stamp.line());
        next.push('\n');
    }
    if let Some(dir) = records.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let staged = records.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&staged, next)?;
    std::fs::rename(&staged, records)
}

/// Take every feature of `stamp`'s plugin out of autoplugging for this
/// process, without loading the plugin.
pub(crate) fn demote(stamp: &Stamp) {
    for feature in gst::Registry::get().features_by_plugin(&stamp.plugin) {
        feature.set_rank(gst::Rank::NONE);
    }
    crate::gst_dmabuf_watch::invalidate_dmabuf_decoder_cache();
}

#[cfg(test)]
#[path = "gst_unloadable_tests.rs"]
mod tests;
