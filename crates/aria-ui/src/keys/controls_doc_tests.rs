//! Reference documentation coverage checks for `docs/reference/controls.md`.
//!
//! Compares the control tables in `docs/reference/controls.md` against
//! `crate::tooltip::CONTROL_TIPS` and the documented id of every shared-widget
//! call in the crate's non-test sources, in both directions. The calls, and the
//! forms an id takes at a call, are read by
//! [`crate::test_support::control_sites`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::test_support::control_sites::control_ids;
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

/// Every valid UI control identifier from `CONTROL_TIPS` and shared widget calls.
pub(super) fn shipped_control_ids() -> BTreeSet<String> {
    let mut ids = control_ids();
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
/// non-empty label, minimum action length, and shortcut chord all match. A
/// shared-widget call whose id the census cannot read fails `control_ids`, so a
/// control drawn through a variable, a table or a wrapper is covered once the
/// call states its id or a forwarder or table record resolves it. Does not
/// catch a control drawn by its own element rather than a shared widget, such
/// as the Viewer's zoom steps and mark buttons.
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

    let extracted = control_ids();
    let undocumented: Vec<&str> = extracted
        .iter()
        .map(String::as_str)
        .filter(|id| !documented.contains_key(*id))
        .collect();
    assert!(
        undocumented.is_empty(),
        "{} source controls extracted from crates/aria-ui/src are missing from docs/reference/controls.md: {undocumented:?}",
        undocumented.len()
    );

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
