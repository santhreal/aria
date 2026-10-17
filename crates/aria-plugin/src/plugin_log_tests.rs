//! Tests for the plugin log ring and the lines the context logs.
//!
//! The class closed: a line lost, kept unbounded, attributed to the wrong
//! plugin, or a caught panic logged without where it happened. Not covered
//! here: the WASM and native tiers' calls, tested in their own crates.

use super::*;
use crate::{Plugin, PluginContext, PluginError, PluginManifest};

fn texts(lines: &[LogLine]) -> Vec<&str> {
    lines.iter().map(|line| line.text.as_ref()).collect()
}

#[test]
fn the_ring_keeps_the_newest_lines_and_drops_the_oldest_first() {
    let logs = PluginLogs::default();
    for index in 0..LOG_LINES_PER_PLUGIN {
        logs.push("test.ring", LogLevel::Info, &format!("line {index}"));
    }
    let full = logs.lines("test.ring");
    assert_eq!(full.len(), LOG_LINES_PER_PLUGIN);
    assert_eq!(&*full[0].text, "line 0");

    logs.push("test.ring", LogLevel::Warn, "line 256");
    let lines = logs.lines("test.ring");
    assert_eq!(
        lines.len(),
        LOG_LINES_PER_PLUGIN,
        "the 257th line dropped one"
    );
    assert_eq!(&*lines[0].text, "line 1", "the oldest line dropped first");
    let last = lines.last().expect("a newest line");
    assert_eq!((last.level, &*last.text), (LogLevel::Warn, "line 256"));
    assert_eq!(logs.count("test.ring"), LOG_LINES_PER_PLUGIN);
    assert!(logs.lines("test.other").is_empty(), "rings are per plugin");
}

#[test]
fn a_line_is_cut_to_the_byte_limit_on_a_character_boundary() {
    let logs = PluginLogs::default();
    let exact = "a".repeat(LOG_LINE_MAX_BYTES);
    logs.push("test.cut", LogLevel::Info, &exact);
    let over = "b".repeat(LOG_LINE_MAX_BYTES + 1);
    logs.push("test.cut", LogLevel::Info, &over);
    // A three-byte character straddles the cut, which falls one byte into it.
    let keep = LOG_LINE_MAX_BYTES - LOG_LINE_TRUNCATED.len();
    let straddling = format!("{}€{}", "c".repeat(keep - 1), "d".repeat(64));
    logs.push("test.cut", LogLevel::Info, &straddling);

    let lines = logs.lines("test.cut");
    assert_eq!(&*lines[0].text, exact, "a line at the limit is kept whole");
    let cut = &lines[1].text;
    assert_eq!(cut.len(), LOG_LINE_MAX_BYTES);
    assert_eq!(
        cut.as_ref(),
        format!("{}{LOG_LINE_TRUNCATED}", "b".repeat(keep))
    );
    assert_eq!(
        lines[2].text.as_ref(),
        format!("{}{LOG_LINE_TRUNCATED}", "c".repeat(keep - 1)),
        "the character the cut fell inside is dropped whole"
    );
}

#[test]
fn the_revision_moves_on_each_line_and_on_a_dropped_ring_only() {
    let logs = PluginLogs::default();
    let start = logs.revision();
    logs.push("test.rev", LogLevel::Debug, "one");
    logs.push("test.rev", LogLevel::Debug, "two");
    assert_eq!(logs.revision(), start + 2);
    assert!(!logs.clear("test.none"));
    assert_eq!(
        logs.revision(),
        start + 2,
        "dropping nothing changes nothing"
    );
    assert!(logs.clear("test.rev"));
    assert_eq!(logs.revision(), start + 3);
    assert!(logs.lines("test.rev").is_empty());
}

#[test]
fn the_backtrace_starts_at_the_frame_after_the_panic_machinery() {
    let rendered = "   0: std::backtrace::Backtrace::force_capture
             at /rustc/library/std/src/backtrace.rs:312:9
   1: aria_plugin::plugin_log::capture_panics::{{closure}}
   2: std::panicking::rust_panic_with_hook
             at /rustc/library/std/src/panicking.rs:841:13
   3: std::sys::backtrace::__rust_end_short_backtrace
   4: __rustc::rust_begin_unwind
   5: core::panicking::panic_fmt
   6: my_plugin::Glow::render
             at src/lib.rs:40:9
   7: my_plugin::tests::a_panicking_render
   8: core::panicking::assert_failed
";
    assert_eq!(
        from_panic_site(rendered),
        "   6: my_plugin::Glow::render
             at src/lib.rs:40:9
   7: my_plugin::tests::a_panicking_render
   8: core::panicking::assert_failed
",
        "frames after the site are kept even when their names match"
    );
    let no_machinery = "   0: my_plugin::Glow::render\n";
    assert_eq!(from_panic_site(no_machinery), no_machinery);
}

struct Logs {
    manifest: PluginManifest,
    /// The view kept from mount, the way a handler keeps it.
    kept: parking_lot::Mutex<Option<PluginContext>>,
}

impl Plugin for Logs {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.log(LogLevel::Info, "mounting");
        *self.kept.lock() = Some(ctx.clone());
        Ok(())
    }
}

struct PanicsOnMount {
    manifest: PluginManifest,
}

impl Plugin for PanicsOnMount {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, _: &PluginContext) -> Result<(), PluginError> {
        panic!("the glow table is empty")
    }
}

#[test]
fn a_view_logs_under_its_plugin_and_the_lines_outlive_an_unmount() {
    let ctx = PluginContext::new();
    let plugin = Logs {
        manifest: PluginManifest::new("test.logs", "Logs", "0.1.0", &[], &[]),
        kept: parking_lot::Mutex::new(None),
    };
    ctx.mount(&plugin).expect("mounts");
    let kept = plugin.kept.lock().clone().expect("the view");
    let before = ctx.logs_revision();
    kept.log(LogLevel::Error, "after mount");
    assert!(ctx.logs_revision() > before);
    ctx.log(LogLevel::Info, "the host's own line");
    ctx.log_for("test.other", LogLevel::Debug, "for another plugin");

    let lines = ctx.plugin_logs("test.logs");
    assert_eq!(texts(&lines), ["mounting", "after mount"]);
    assert_eq!(lines[1].level, LogLevel::Error);
    assert_eq!(
        texts(&ctx.plugin_logs("test.other")),
        ["for another plugin"]
    );
    assert_eq!(ctx.plugin_log_count("test.logs"), 2);

    ctx.unmount("test.logs").expect("unmounts");
    assert_eq!(ctx.plugin_log_count("test.logs"), 2, "kept for the session");
    assert!(ctx.clear_logs("test.logs"));
    assert!(ctx.plugin_logs("test.logs").is_empty());
}

#[test]
fn a_panic_is_logged_with_its_message_and_where_it_happened() {
    let ctx = PluginContext::new();
    let plugin = PanicsOnMount {
        manifest: PluginManifest::new("test.panics", "Panics", "0.1.0", &[], &[]),
    };
    ctx.mount(&plugin).expect_err("the mount panicked");

    let lines = ctx.plugin_logs("test.panics");
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert_eq!(line.level, LogLevel::Error);
    let (reason, backtrace) = line.text.split_once('\n').expect("a backtrace line");
    assert!(
        reason.ends_with("panic: the glow table is empty"),
        "{reason}"
    );
    assert!(backtrace.contains("PanicsOnMount"), "{backtrace}");
    assert!(
        !backtrace.contains("rust_begin_unwind"),
        "the panic machinery is cut: {backtrace}"
    );
}

#[test]
fn a_failure_after_an_unrelated_panic_carries_no_backtrace() {
    let ctx = PluginContext::new();
    let plugin = Logs {
        manifest: PluginManifest::new("test.fails", "Fails", "0.1.0", &[], &[]),
        kept: parking_lot::Mutex::new(None),
    };
    ctx.mount(&plugin).expect("mounts");
    let caught = std::panic::catch_unwind(|| panic!("an unrelated panic"));
    assert!(caught.is_err());

    assert!(ctx.fail("test.fails", "it stopped answering"));
    ctx.mount(&plugin).expect("mounts again");
    ctx.report_fault("test.fails", "missed three deadlines");
    let lines = ctx.plugin_logs("test.fails");
    let tail: Vec<(LogLevel, &str)> = lines[1..]
        .iter()
        .map(|line| (line.level, line.text.as_ref()))
        .collect();
    assert_eq!(
        tail,
        [
            (LogLevel::Error, "it stopped answering"),
            (LogLevel::Info, "mounting"),
            (LogLevel::Warn, "missed three deadlines"),
        ]
    );
}

#[test]
fn every_level_spells_its_lowercase_name() {
    let names: Vec<String> = LogLevel::ALL.iter().map(ToString::to_string).collect();
    assert_eq!(names, ["trace", "debug", "info", "warn", "error"]);
    assert!(LogLevel::ALL.windows(2).all(|pair| pair[0] < pair[1]));
}
