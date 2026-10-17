//! The plugin's registrations and the read every format starts from:
//! mounting lists one importer and one exporter per entry of [`FORMATS`] and
//! [`SUBTITLE_FORMATS`], unmounting takes them all away, and a timeline file
//! is read whole, without its byte-order mark, and refused past a size limit.
//!
//! Not caught here: what a format reads or writes; each format's suite
//! covers that.

use std::path::Path;

use aria_plugin::PluginContext;

use super::{
    FORMATS, InterchangePlugin, MAX_FILE_BYTES, SUBTITLE_FORMATS, read_at_most, read_text,
};

const PLUGIN: &str = "aria.interchange";

fn mounted() -> PluginContext {
    let ctx = PluginContext::new();
    ctx.mount(&InterchangePlugin).expect("mounts");
    ctx
}

/// One format of either table: id, name, the extensions read, and the one
/// written.
struct Listed {
    id: &'static str,
    name: &'static str,
    reads: Vec<&'static str>,
    extension: &'static str,
}

/// Every format of both tables, in registration order.
fn listed() -> Vec<Listed> {
    FORMATS
        .iter()
        .map(|format| Listed {
            id: format.id,
            name: format.name,
            reads: format.reads.to_vec(),
            extension: format.extension,
        })
        .chain(SUBTITLE_FORMATS.iter().map(|format| Listed {
            id: format.id,
            name: format.name,
            reads: vec![format.extension],
            extension: format.extension,
        }))
        .collect()
}

#[test]
fn mounting_lists_an_importer_and_an_exporter_for_every_format() {
    let ctx = mounted();

    let importers: Vec<(String, String, Vec<String>)> = ctx
        .importers()
        .iter()
        .map(|spec| (spec.id.clone(), spec.name.clone(), spec.extensions.clone()))
        .collect();
    let exporters: Vec<(String, String, String)> = ctx
        .exporters()
        .iter()
        .map(|spec| (spec.id.clone(), spec.name.clone(), spec.extension.clone()))
        .collect();

    let formats = listed();
    let expected_importers: Vec<(String, String, Vec<String>)> = formats
        .iter()
        .map(|format| {
            (
                format.id.to_owned(),
                format.name.to_owned(),
                format.reads.iter().map(|ext| (*ext).to_owned()).collect(),
            )
        })
        .collect();
    let expected_exporters: Vec<(String, String, String)> = formats
        .iter()
        .map(|format| {
            (
                format.id.to_owned(),
                format.name.to_owned(),
                format.extension.to_owned(),
            )
        })
        .collect();
    assert_eq!(importers, expected_importers);
    assert_eq!(exporters, expected_exporters);
}

#[test]
fn every_extension_a_format_reads_routes_to_that_format() {
    let ctx = mounted();
    for format in listed() {
        assert!(
            format.reads.contains(&format.extension),
            "{}: the extension written is one it reads",
            format.id
        );
        for ext in &format.reads {
            for name in [
                format!("Cut.{ext}"),
                format!("Cut.{}", ext.to_ascii_uppercase()),
            ] {
                let spec = ctx
                    .importer_for(Path::new(&name))
                    .unwrap_or_else(|| panic!("{name} has an importer"));
                assert_eq!(spec.id, format.id, "{name}");
            }
        }
    }
}

#[test]
fn format_ids_and_extensions_are_unique() {
    let formats = listed();
    for (index, format) in formats.iter().enumerate() {
        for other in &formats[index + 1..] {
            assert_ne!(format.id, other.id);
            assert_ne!(format.extension, other.extension);
            for ext in &format.reads {
                assert!(
                    !other.reads.contains(ext),
                    "{} and {} both read .{ext}",
                    format.id,
                    other.id
                );
            }
        }
    }
}

#[test]
fn unmounting_removes_every_importer_and_exporter() {
    let ctx = mounted();
    ctx.unmount(PLUGIN).expect("unmounts");
    assert!(ctx.importers().is_empty());
    assert!(ctx.exporters().is_empty());
    assert!(ctx.importer_for(Path::new("Cut.edl")).is_none());
}

#[test]
fn a_byte_order_mark_is_dropped_and_only_at_the_start() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("cut.edl");

    std::fs::write(&path, "\u{feff}TITLE: Promo\n").expect("write");
    assert_eq!(read_text(&path).expect("reads"), "TITLE: Promo\n");

    std::fs::write(&path, "TITLE: \u{feff}Promo\n").expect("write");
    assert_eq!(read_text(&path).expect("reads"), "TITLE: \u{feff}Promo\n");
}

#[test]
fn bytes_that_are_not_utf8_read_as_replacement_characters() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("cut.edl");
    std::fs::write(&path, b"TITLE: Caf\xe9\n").expect("write");
    assert_eq!(read_text(&path).expect("reads"), "TITLE: Caf\u{fffd}\n");
}

#[test]
fn a_file_past_the_limit_is_refused_before_it_is_read() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("huge.edl");
    // Sparse: the length is set without writing the bytes.
    let file = std::fs::File::create(&path).expect("create");
    file.set_len(MAX_FILE_BYTES + 1).expect("set length");

    let err = read_text(&path).expect_err("refused");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(err.to_string(), "timeline files over 256 MiB are not read");
}

#[test]
fn a_file_at_the_limit_reads_and_one_byte_more_does_not() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("cut.edl");

    std::fs::write(&path, "0123456789abcdef").expect("write");
    assert_eq!(read_at_most(&path, 16).expect("reads"), "0123456789abcdef");

    std::fs::write(&path, "0123456789abcdefg").expect("write");
    let err = read_at_most(&path, 16).expect_err("refused");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(err.to_string(), "timeline files over 16 bytes are not read");
}

/// A device states no length, so only the bounded read stops it: without
/// the bound this read never ends.
#[cfg(target_os = "linux")]
#[test]
fn an_endless_device_is_read_only_to_the_limit() {
    let err = read_at_most(Path::new("/dev/zero"), 16).expect_err("refused");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn a_missing_file_is_a_not_found_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let err = read_text(&dir.path().join("absent.edl")).expect_err("absent");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}
