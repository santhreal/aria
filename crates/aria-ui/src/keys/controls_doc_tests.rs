//! Reference documentation coverage checks for `docs/reference/controls.md`.
//!
//! Compares the control tables in `docs/reference/controls.md` against
//! `crate::tooltip::CONTROL_TIPS` and all shared widget helper calls in
//! non-test `crates/aria-ui/src/**/*.rs` files (`components::button`,
//! `components::ghost_button`, `components::danger_button`,
//! `components::chip`, `components::text_box`, `components::icon_button`, and
//! `pending::stop_button`) in both directions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::test_support::collect_rs;
use crate::test_support::sources::without_test_items;
use crate::tooltip::CONTROL_TIPS;

const EXPECTED_SURFACES: &[&str] = &[
    "Tool Strip",
    "Viewer & Source Monitor",
    "Timeline",
    "Inspector",
    "Project Panel",
    "Effects Browser",
    "Audio Mixer",
    "Captions",
    "Text",
    "Markers",
    "Color",
    "Scopes",
    "Audio Meters",
    "Alignment",
    "Keyframes",
    "History",
    "AI Assistant",
    "Export & Transcode",
    "Settings",
    "Project Home",
    "Window & Dock Chrome",
    "Modal Dialogs",
];

#[derive(Debug)]
struct DocControlRow {
    line: usize,
    label: String,
    action: String,
    shortcut: String,
}

/// Replaces line and block comments with spaces while preserving string
/// literals and newlines so source scans do not match commented-out calls.
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(b' ');
                i += 1;
            }
        } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            let mut depth = 1usize;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    depth += 1;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                } else if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    depth -= 1;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                } else {
                    out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
            }
        } else if bytes[i] == b'"' {
            out.push(bytes[i]);
            i += 1;
            while i < bytes.len() {
                out.push(bytes[i]);
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 1;
                    out.push(bytes[i]);
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("valid utf-8 after stripping comments")
}

fn extract_source_controls() -> BTreeSet<String> {
    let needles = [
        concat!("components::", "button("),
        concat!("ghost_", "button("),
        concat!("components::", "danger_button("),
        concat!("components::", "chip("),
        concat!("components::", "text_box("),
        concat!("icon_", "button("),
    ];
    let mk_prefixes = [
        concat!("MotionKey::", "new("),
        concat!("motion::MotionKey::", "new("),
        concat!("crate::motion::MotionKey::", "new("),
    ];
    let stop_needle = concat!("stop_", "button(");

    let crate_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&crate_src, &mut files);
    files.sort();

    let mut extracted = BTreeSet::new();
    for file in &files {
        let rel = file
            .strip_prefix(&crate_src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "components.rs"
            || rel.starts_with("test_support")
            || rel.ends_with("_tests.rs")
            || rel.ends_with("/tests.rs")
            || rel.contains("/tests/")
        {
            continue;
        }
        let Ok(raw_src) = std::fs::read_to_string(file) else {
            continue;
        };
        let stripped = strip_comments(&without_test_items(&raw_src));
        for needle in needles {
            let mut cursor = 0usize;
            while let Some(pos) = stripped[cursor..].find(needle) {
                let abs_pos = cursor + pos;
                let after = abs_pos + needle.len();
                cursor = after;
                if !needle.contains("::")
                    && abs_pos > 0
                    && (stripped.as_bytes()[abs_pos - 1].is_ascii_alphanumeric()
                        || stripped.as_bytes()[abs_pos - 1] == b'_')
                {
                    continue;
                }
                let rest =
                    stripped[after..].trim_start_matches(|c: char| c.is_whitespace() || c == '(');
                if let Some(quoted) = rest.strip_prefix('"') {
                    if let Some(id) = quoted.split('"').next()
                        && !id.is_empty()
                        && !id.contains('{')
                        && !id.contains('$')
                    {
                        extracted.insert(id.to_string());
                    }
                } else {
                    for prefix in mk_prefixes {
                        if let Some(after_mk) = rest.strip_prefix(prefix) {
                            let after_mk = after_mk.trim_start();
                            if let Some(quoted) = after_mk.strip_prefix('"')
                                && let Some(kind) = quoted.split('"').next()
                                && !kind.is_empty()
                                && !kind.contains('{')
                                && !kind.contains('$')
                            {
                                extracted.insert(kind.to_string());
                            }
                            break;
                        }
                    }
                }
            }
        }
        if stripped.contains(stop_needle) {
            extracted.insert("job-stop".to_string());
        }
    }
    extracted
}

/// Every valid UI control identifier from `CONTROL_TIPS` and shared widget calls.
pub(super) fn shipped_control_ids() -> BTreeSet<String> {
    let mut ids = extract_source_controls();
    for tip in CONTROL_TIPS {
        ids.insert(tip.control.to_string());
    }
    ids
}

/// WHY: closes the defect class where a named UI control, tooltip entry, or
/// bound shortcut is added, renamed, or removed in `aria-ui` while
/// `docs/reference/controls.md` drifts out of step, or where a row in
/// `controls.md` has an empty cell or a stale shortcut chord. Does not catch
/// inaccurate prose descriptions in the `Action` column when the control ID,
/// non-empty label, minimum action length, and shortcut chord all match, nor a
/// control whose ID reaches the shared widget through a variable or a private
/// wrapper rather than as a literal at the call: a wrapper around
/// `components::chip` kept the Analysis and Captions chips out of this census.
#[test]
fn controls_doc_matches_source() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc_path = manifest_dir.join("../../docs/reference/controls.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", doc_path.display()));
    let readme_path = manifest_dir.join("../../docs/reference/README.md");
    let readme = std::fs::read_to_string(&readme_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", readme_path.display()));

    assert!(
        readme.contains("[controls.md](controls.md)"),
        "docs/reference/README.md must link to [controls.md](controls.md)"
    );
    assert!(
        doc.contains("## Example"),
        "docs/reference/controls.md must contain a `## Example` section"
    );
    assert!(
        !doc.contains('\u{2014}'),
        "docs/reference/controls.md must not contain em-dash characters (U+2014)"
    );

    for surface in EXPECTED_SURFACES {
        let heading = format!("## {surface}");
        assert!(
            doc.contains(&heading),
            "docs/reference/controls.md is missing surface section `{heading}`"
        );
    }

    let mut documented: BTreeMap<String, DocControlRow> = BTreeMap::new();
    for (idx, line) in doc.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        assert_eq!(
            cells.len(),
            4,
            "docs/reference/controls.md:{line_no}: expected 4 table columns, found {}: {line}",
            cells.len()
        );
        if cells[0] == "Control ID"
            || cells
                .iter()
                .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
        {
            continue;
        }
        let raw_id = cells[0]
            .strip_suffix(" (per row)")
            .unwrap_or(cells[0])
            .trim();
        let id = raw_id
            .strip_prefix('`')
            .and_then(|s| s.strip_suffix('`'))
            .unwrap_or_else(|| {
                panic!(
                    "docs/reference/controls.md:{line_no}: control ID `{}` must be wrapped in backticks",
                    cells[0]
                )
            })
            .trim();
        assert!(
            !id.is_empty(),
            "docs/reference/controls.md:{line_no}: control ID must not be empty"
        );
        assert!(
            !cells[1].is_empty(),
            "docs/reference/controls.md:{line_no}: label for `{id}` must not be empty"
        );
        assert!(
            cells[2].chars().count() >= 8,
            "docs/reference/controls.md:{line_no}: action for `{id}` must be at least 8 chars, found {:?}",
            cells[2]
        );
        assert!(
            !cells[3].is_empty(),
            "docs/reference/controls.md:{line_no}: shortcut cell for `{id}` must not be empty"
        );
        if let Some(prev) = documented.insert(
            id.to_string(),
            DocControlRow {
                line: line_no,
                label: cells[1].to_string(),
                action: cells[2].to_string(),
                shortcut: cells[3].to_string(),
            },
        ) {
            panic!(
                "docs/reference/controls.md:{line_no}: duplicate control ID `{id}` (first at line {})",
                prev.line
            );
        }
    }

    let shipped_chords = super::doc_tests::shipped_binding_chords();
    let mut tip_controls: BTreeSet<&str> = BTreeSet::new();

    for tip in CONTROL_TIPS {
        tip_controls.insert(tip.control);
        let row = documented.get(tip.control).unwrap_or_else(|| {
            panic!(
                "CONTROL_TIPS entry `{}` ({:?}) is not documented in docs/reference/controls.md",
                tip.control, tip.name
            )
        });
        assert!(
            !row.label.is_empty() && !row.action.is_empty(),
            "docs/reference/controls.md:{}: `{}` must have non-empty label and action",
            row.line,
            tip.control
        );
        match tip.binding {
            Some(binding_id) => {
                let chord_label = shipped_chords.get(binding_id).unwrap_or_else(|| {
                    panic!(
                        "CONTROL_TIPS entry `{}` references unknown binding `{binding_id}`",
                        tip.control
                    )
                });
                let expected_tick = format!("`{chord_label}`");
                assert!(
                    row.shortcut.contains(&expected_tick),
                    "docs/reference/controls.md:{}: shortcut for `{}` must contain {expected_tick}, found {:?}",
                    row.line,
                    tip.control,
                    row.shortcut
                );
            }
            None => {
                assert!(
                    row.shortcut == "None" || row.shortcut == "\u{2014}",
                    "docs/reference/controls.md:{}: `{}` has no CONTROL_TIPS binding so Shortcut must be `None`, found {:?}",
                    row.line,
                    tip.control,
                    row.shortcut
                );
            }
        }
    }

    let extracted = extract_source_controls();
    for id in &extracted {
        assert!(
            documented.contains_key(id),
            "source control `{id}` extracted from crates/aria-ui/src is missing from docs/reference/controls.md"
        );
    }

    for (id, row) in &documented {
        assert!(
            tip_controls.contains(id.as_str()) || extracted.contains(id),
            "docs/reference/controls.md:{}: phantom control `{id}` does not exist in CONTROL_TIPS or crates/aria-ui/src helper calls",
            row.line
        );
        if !tip_controls.contains(id.as_str()) {
            assert!(
                row.shortcut == "None" || row.shortcut == "\u{2014}",
                "docs/reference/controls.md:{}: control `{id}` has no registered shortcut binding so Shortcut must be `None`, found {:?}",
                row.line,
                row.shortcut
            );
        }
    }
}
