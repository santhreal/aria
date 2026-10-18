//! WHY: GStreamer keeps no record of a plugin that failed to load, so every
//! process loaded the stale CUDA plugin again and kept about 12.8 MiB of its
//! libraries mapped. These tests hold the record that replaces it: a failed
//! load is found again only for the same plugin file on the same boot, a
//! success clears it, other plugins' records survive either write, and a file
//! of another layout is never read as a record.
//!
//! Not caught here: whether the warm-up consults the record and demotes the
//! plugin before the first decoder autoplugs. That path is shown on a host
//! whose CUDA plugin does not load, with GStreamer's plugin-loading log.

use std::path::PathBuf;

use super::{HEADER, Stamp, failed_before, record};

fn stamp() -> Stamp {
    Stamp {
        plugin: "nvcodec".into(),
        file: PathBuf::from("/usr/lib/gstreamer-1.0/libgstnvcodec.so"),
        size: 699_160,
        modified_ns: 1_713_139_200_000_000_000,
        boot: "5f0c6a1e-0000-4000-8000-000000000001".into(),
    }
}

fn records(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("aria-unloadable-tests").join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("unloadable-plugins")
}

/// Each way a later load can differ from the recorded one.
fn changes() -> Vec<(&'static str, fn(&mut Stamp))> {
    vec![
        ("plugin", |s: &mut Stamp| s.plugin = "nvcodec2".into()),
        ("file", |s: &mut Stamp| {
            s.file = PathBuf::from("/opt/gst/libgstnvcodec.so")
        }),
        ("size", |s: &mut Stamp| s.size += 1),
        ("modified", |s: &mut Stamp| s.modified_ns += 1),
        ("boot", |s: &mut Stamp| {
            s.boot = "5f0c6a1e-0000-4000-8000-000000000002".into();
        }),
    ]
}

#[test]
fn a_failed_load_is_found_again_for_the_same_file_on_the_same_boot() {
    let at = records("same");
    assert!(!failed_before(&at, &stamp()), "no file records nothing");
    record(&at, &stamp(), true).expect("record a failed load");
    assert!(failed_before(&at, &stamp()));
}

#[test]
fn a_changed_file_or_a_new_boot_loads_the_plugin_again() {
    let at = records("changed");
    record(&at, &stamp(), true).expect("record a failed load");
    for (field, change) in changes() {
        let mut later = stamp();
        change(&mut later);
        assert!(
            !failed_before(&at, &later),
            "a record still matched after its {field} changed"
        );
    }
}

#[test]
fn a_load_that_succeeds_clears_only_its_own_plugin() {
    let at = records("clear");
    let mut other = stamp();
    other.plugin = "va".into();
    record(&at, &other, true).expect("record the other plugin");
    record(&at, &stamp(), true).expect("record this plugin");
    record(&at, &stamp(), false).expect("clear this plugin");
    assert!(!failed_before(&at, &stamp()), "a success left the record");
    assert!(
        failed_before(&at, &other),
        "clearing one plugin dropped another"
    );
}

#[test]
fn a_new_failure_replaces_the_plugins_older_record() {
    let at = records("replace");
    let mut older = stamp();
    older.boot = "5f0c6a1e-0000-4000-8000-000000000000".into();
    record(&at, &older, true).expect("record the older failure");
    record(&at, &stamp(), true).expect("record the new failure");
    let text = std::fs::read_to_string(&at).expect("read the records");
    assert_eq!(text, format!("{HEADER}\n{}\n", stamp().line()));
}

#[test]
fn a_file_of_another_layout_is_never_read_as_a_record() {
    let at = records("layout");
    std::fs::create_dir_all(at.parent().expect("a parent")).expect("mkdir");
    std::fs::write(
        &at,
        format!("aria unloadable plugins 0\n{}\n", stamp().line()),
    )
    .expect("write an old layout");
    assert!(!failed_before(&at, &stamp()));
    record(&at, &stamp(), true).expect("record over the old layout");
    assert_eq!(
        std::fs::read_to_string(&at).expect("read the records"),
        format!("{HEADER}\n{}\n", stamp().line())
    );
}

#[test]
fn clearing_a_plugin_with_no_record_writes_nothing() {
    let at = records("absent");
    record(&at, &stamp(), false).expect("clear with no file");
    assert!(
        !at.exists(),
        "a clear with nothing to clear created the file"
    );
}
