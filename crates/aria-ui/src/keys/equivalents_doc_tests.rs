//! Bidirectional reference documentation checks for `docs/reference/equivalents.md`.
//!
//! WHY: closes the defect class where a Premiere Pro or After Effects parity
//! row in `docs/reference/equivalents.md` names a non-existent Aria binding,
//! menu item, command-palette entry, or UI control ID, states a stale default
//! chord or label after a registry change, omits an `Aria id == "none"` row
//! from the trailing summary table, or leaves a newly added Premiere/AE-derived
//! shortcut in `SHIPPED_BINDINGS` undocumented. Does not catch whether the
//! Premiere Pro or After Effects command name in the `Command` column is the
//! best editorial match for a workflow when all Aria registry identifiers,
//! labels, and chords match.

use std::collections::{BTreeMap, BTreeSet};

use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::chrome::menu_items::items_for;
use crate::commands::{BUILTIN_COMMANDS, BUILTIN_JOB_COMMANDS};
use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::keys::controls_doc_tests::shipped_control_ids;
use crate::test_support::doc_tables::read_doc;
use crate::tool_catalog::tool_specs;
use crate::types::AppMenu;

const AREA_SECTIONS: &[&str] = &[
    "Project and media",
    "Editing",
    "Trimming",
    "Playback and navigation",
    "Markers",
    "Layers and properties",
    "Keyframes and graph",
    "Masks and mattes",
    "Text and shapes",
    "Audio",
    "Export",
    "Workspace and panels",
];

const UNMAPPED_SECTION: &str = "Commands with no Aria equivalent";

/// Bindings in `SHIPPED_BINDINGS` that have no Premiere Pro or After Effects
/// counterpart and therefore must not appear in `docs/reference/equivalents.md`.
const ARIA_ONLY_BINDINGS: &[&str] = &[
    // Toggles Aria's AI Assistant dock panel (`ActivePanel::Ai`).
    "key-assistant",
    // Runs Aria's local beat-onset detector job across the sequence.
    "key-detect-beats",
    // Runs Aria's local RMS silence detector job across the sequence.
    "key-detect-silence",
    // Cycles Aria's sidechain ducking attenuation step preset (`-6`..`-24 dB`).
    "key-duck-amount",
    // Dismisses an open modal, context menu, or menu drop on the Home route.
    "key-home-escape",
    // Home-route duplicate of `key-new` (`Ctrl+Alt+N`).
    "key-home-new",
    // Home-route binding that switches the Home rail to `HomeRail::Open`.
    "key-home-open",
    // Home-route duplicate of `key-quit` (`Ctrl+Q`).
    "key-home-quit",
    // Home-route duplicate of `key-settings` (`Ctrl+,`).
    "key-home-settings",
    // Focuses the application menu bar at `File` (`F10` platform convention).
    "key-menu-bar",
    // Opens Aria's searchable Command Palette (`aria.dialog.palette`).
    "key-palette",
];

fn unquote_inline_code(cell: &str) -> Option<&str> {
    let trimmed = cell.trim();
    if let Some(inner) = trimmed
        .strip_prefix("`` ")
        .and_then(|s| s.strip_suffix(" ``"))
    {
        return Some(inner);
    }
    if let Some(inner) = trimmed
        .strip_prefix("``")
        .and_then(|s| s.strip_suffix("``"))
    {
        return Some(inner.trim());
    }
    let inner = trimmed.strip_prefix('`')?.strip_suffix('`')?;
    if inner.contains('`') {
        return None;
    }
    Some(inner.trim())
}

fn split_table_cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

fn is_separator_row(cells: &[&str]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn is_menu_prefix(id: &str) -> bool {
    [
        "file-", "edit-", "clip-", "seq-", "mark-", "marker-", "fx-", "anim-", "view-", "win-",
        "help-",
    ]
    .iter()
    .any(|prefix| id.starts_with(prefix))
}

fn parse_controls_doc(controls_doc: &str) -> BTreeMap<String, (String, String)> {
    let mut surface = String::new();
    let mut map = BTreeMap::new();
    for line in controls_doc.lines() {
        let trimmed = line.trim();
        if let Some(h2) = trimmed.strip_prefix("## ") {
            surface = h2.trim().to_string();
            continue;
        }
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells = split_table_cells(trimmed);
        if cells.len() != 4 || cells[0] == "Control ID" || is_separator_row(&cells) {
            continue;
        }
        let raw_id = cells[0].strip_suffix(" (per row)").unwrap_or(cells[0]);
        if let Some(id) = unquote_inline_code(raw_id) {
            let label = format!("{surface}: {}", cells[1]);
            let shortcut = if cells[3] == "None" {
                "none".to_string()
            } else {
                unquote_inline_code(cells[3])
                    .unwrap_or(cells[3])
                    .to_string()
            };
            map.insert(id.to_string(), (label, shortcut));
        }
    }
    map
}

#[test]
fn equivalents_doc_matches_registries() {
    let readme = read_doc("reference/README.md");
    assert!(
        readme.contains("[equivalents.md](equivalents.md)"),
        "docs/reference/README.md must link to [equivalents.md](equivalents.md)"
    );

    let doc = read_doc("reference/equivalents.md");
    assert!(
        doc.starts_with("# Premiere Pro and After Effects equivalents\n"),
        "docs/reference/equivalents.md must open with `# Premiere Pro and After Effects equivalents`"
    );

    let dir = crate::scratch_pool::scratch("equivalents-doc-test");
    let state = AppState::new(PluginContext::new(), dir).expect("build headless AppState");

    let binding_chords: BTreeMap<&'static str, String> = SHIPPED_BINDINGS
        .iter()
        .map(|b| (b.id, b.chord.label()))
        .collect();

    let tool_labels: BTreeMap<&'static str, String> = tool_specs()
        .iter()
        .map(|spec| (spec.id, format!("{} tool", spec.label)))
        .collect();

    let mut menu_rows: BTreeMap<String, (String, Option<&'static str>)> = BTreeMap::new();
    let mut all_menu_paths: BTreeSet<String> = BTreeSet::new();
    for menu in AppMenu::ALL {
        for item in items_for(menu, &state).iter() {
            let path = match item.under {
                Some(sub) => format!("{} > {sub} > {}", menu.label(), item.label),
                None => format!("{} > {}", menu.label(), item.label),
            };
            all_menu_paths.insert(path.clone());
            menu_rows.insert(item.id.to_string(), (path, item.key));
        }
    }

    let mut palette_commands: BTreeMap<&'static str, &'static str> = BTreeMap::new();
    let mut all_palette_labels: BTreeSet<&'static str> = BTreeSet::new();
    for entry in BUILTIN_COMMANDS {
        palette_commands.insert(entry.id, entry.label);
        all_palette_labels.insert(entry.label);
    }
    for entry in BUILTIN_JOB_COMMANDS {
        palette_commands.insert(entry.id, entry.label);
        all_palette_labels.insert(entry.label);
    }

    let shipped_controls = shipped_control_ids();
    let controls_doc = read_doc("reference/controls.md");
    let controls_map = parse_controls_doc(&controls_doc);

    let mut seen_sections: Vec<String> = Vec::new();
    let mut current_section: Option<String> = None;
    let mut section_row_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut unmapped_in_areas: Vec<(usize, String, String)> = Vec::new();
    let mut unmapped_summary: Vec<(usize, String, String)> = Vec::new();
    let mut covered_bindings: BTreeSet<&str> = BTreeSet::new();

    for (idx, line) in doc.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();
        if let Some(h2) = trimmed.strip_prefix("## ") {
            let sec = h2.trim().to_string();
            seen_sections.push(sec.clone());
            current_section = Some(sec);
            continue;
        }
        if !trimmed.starts_with('|') {
            continue;
        }
        let Some(sec) = current_section.as_deref() else {
            panic!("docs/reference/equivalents.md:{line_no}: table row before any `## ` section");
        };
        let cells = split_table_cells(trimmed);
        if is_separator_row(&cells) {
            continue;
        }

        if sec == UNMAPPED_SECTION {
            assert_eq!(
                cells.len(),
                3,
                "docs/reference/equivalents.md:{line_no}: expected 3 columns in `{UNMAPPED_SECTION}`, found {}: {line}",
                cells.len()
            );
            if cells[0] == "App" {
                assert_eq!(
                    cells,
                    vec!["App", "Command", "Status"],
                    "docs/reference/equivalents.md:{line_no}: unexpected header in `{UNMAPPED_SECTION}`"
                );
                continue;
            }
            let app = cells[0];
            let command = cells[1];
            let status = cells[2];
            assert!(
                matches!(app, "Premiere Pro" | "After Effects"),
                "docs/reference/equivalents.md:{line_no}: App must be `Premiere Pro` or `After Effects`, found `{app}`"
            );
            assert!(
                !command.is_empty(),
                "docs/reference/equivalents.md:{line_no}: Command must not be empty"
            );
            let word_count = status.split_whitespace().count();
            assert!(
                (3..=8).contains(&word_count),
                "docs/reference/equivalents.md:{line_no}: Status `{status}` for `{command}` must be 3 to 8 words, found {word_count}"
            );
            assert!(
                status == "not in Aria"
                    || status.starts_with("covered by ")
                    || status.starts_with("deferred: see docs/deferred.md#"),
                "docs/reference/equivalents.md:{line_no}: Status `{status}` must be `not in Aria`, `covered by <control>`, or `deferred: see docs/deferred.md#<anchor>`"
            );
            unmapped_summary.push((line_no, app.to_string(), command.to_string()));
            continue;
        }

        assert_eq!(
            cells.len(),
            6,
            "docs/reference/equivalents.md:{line_no}: expected 6 columns in `{sec}`, found {}: {line}",
            cells.len()
        );
        if cells[0] == "App" {
            assert_eq!(
                cells,
                vec![
                    "App",
                    "Command",
                    "Shortcut there",
                    "Aria",
                    "Aria id",
                    "Aria shortcut"
                ],
                "docs/reference/equivalents.md:{line_no}: unexpected table header in `{sec}`"
            );
            continue;
        }

        *section_row_counts.entry(sec.to_string()).or_insert(0) += 1;

        let app = cells[0];
        let command = cells[1];
        let shortcut_there = unquote_inline_code(cells[2]).unwrap_or_else(|| {
            panic!(
                "docs/reference/equivalents.md:{line_no}: `Shortcut there` cell `{}` must be wrapped in backticks",
                cells[2]
            )
        });
        let aria = cells[3];
        let aria_id = unquote_inline_code(cells[4]).unwrap_or_else(|| {
            panic!(
                "docs/reference/equivalents.md:{line_no}: `Aria id` cell `{}` must be wrapped in backticks",
                cells[4]
            )
        });
        let aria_shortcut = unquote_inline_code(cells[5]).unwrap_or_else(|| {
            panic!(
                "docs/reference/equivalents.md:{line_no}: `Aria shortcut` cell `{}` must be wrapped in backticks",
                cells[5]
            )
        });

        assert!(
            matches!(app, "Premiere Pro" | "After Effects"),
            "docs/reference/equivalents.md:{line_no}: App must be `Premiere Pro` or `After Effects`, found `{app}`"
        );
        assert!(
            !command.is_empty(),
            "docs/reference/equivalents.md:{line_no}: Command must not be empty"
        );
        assert!(
            !shortcut_there.is_empty(),
            "docs/reference/equivalents.md:{line_no}: `Shortcut there` must not be empty"
        );

        if aria_id == "none" {
            let unquoted_aria = unquote_inline_code(aria).unwrap_or("");
            assert_eq!(
                unquoted_aria, "none",
                "docs/reference/equivalents.md:{line_no}: row with `Aria id` = `none` must have `Aria` = `` `none` ``, found `{aria}`"
            );
            assert_eq!(
                aria_shortcut, "none",
                "docs/reference/equivalents.md:{line_no}: row with `Aria id` = `none` must have `Aria shortcut` = `` `none` ``, found `{aria_shortcut}`"
            );
            unmapped_in_areas.push((line_no, app.to_string(), command.to_string()));
            continue;
        }

        if aria_id.starts_with("key-") {
            let Some((&shipped_id, expected_chord)) = binding_chords.get_key_value(aria_id) else {
                panic!(
                    "docs/reference/equivalents.md:{line_no}: unknown key binding id `{aria_id}` (not in SHIPPED_BINDINGS)"
                );
            };
            assert_eq!(
                aria_shortcut, expected_chord,
                "docs/reference/equivalents.md:{line_no}: Aria shortcut `{aria_shortcut}` for `{aria_id}` does not match shipped chord `{expected_chord}`"
            );
            if let Some(tool_suffix) = aria_id.strip_prefix("key-tool-") {
                let tool_id = format!("tool-{tool_suffix}");
                let expected_tool_label = tool_labels.get(tool_id.as_str()).unwrap_or_else(|| {
                    panic!(
                        "docs/reference/equivalents.md:{line_no}: `{aria_id}` has no matching `{tool_id}` in tool_catalog::tool_specs()"
                    )
                });
                assert_eq!(
                    aria, expected_tool_label,
                    "docs/reference/equivalents.md:{line_no}: Aria label `{aria}` for `{aria_id}` does not match tool label `{expected_tool_label}`"
                );
            } else if aria.contains(" > ") {
                assert!(
                    all_menu_paths.contains(aria),
                    "docs/reference/equivalents.md:{line_no}: Aria menu path `{aria}` for `{aria_id}` does not exist in items_for(AppMenu, &state)"
                );
            } else if let Some(cmd_label) = aria.strip_prefix("Command Palette: ") {
                assert!(
                    all_palette_labels.contains(cmd_label),
                    "docs/reference/equivalents.md:{line_no}: Aria palette label `{cmd_label}` for `{aria_id}` does not exist in BUILTIN_COMMANDS or BUILTIN_JOB_COMMANDS"
                );
            } else {
                let expected_label = crate::keys::binding_label(aria_id);
                assert_eq!(
                    aria, expected_label,
                    "docs/reference/equivalents.md:{line_no}: Aria label `{aria}` for `{aria_id}` does not match binding_label `{expected_label}`"
                );
            }
            covered_bindings.insert(shipped_id);
        } else if aria_id.starts_with("pal-") {
            let Some(&expected_label) = palette_commands.get(aria_id) else {
                panic!(
                    "docs/reference/equivalents.md:{line_no}: unknown command palette id `{aria_id}` (not in BUILTIN_COMMANDS or BUILTIN_JOB_COMMANDS)"
                );
            };
            assert_eq!(
                aria_shortcut, "none",
                "docs/reference/equivalents.md:{line_no}: palette command `{aria_id}` must have Aria shortcut `none`, found `{aria_shortcut}`"
            );
            let expected_aria = format!("Command Palette: {expected_label}");
            assert_eq!(
                aria, expected_aria,
                "docs/reference/equivalents.md:{line_no}: Aria label `{aria}` for `{aria_id}` does not match `{expected_aria}`"
            );
        } else if menu_rows.contains_key(aria_id)
            || (is_menu_prefix(aria_id) && !controls_map.contains_key(aria_id))
        {
            let Some((expected_path, item_key)) = menu_rows.get(aria_id) else {
                panic!(
                    "docs/reference/equivalents.md:{line_no}: unknown menu row id `{aria_id}` (not in items_for(AppMenu, &state))"
                );
            };
            assert_eq!(
                aria, expected_path,
                "docs/reference/equivalents.md:{line_no}: Aria label `{aria}` for menu row `{aria_id}` does not match shipped menu path `{expected_path}`"
            );
            match item_key {
                Some(key_id) => {
                    let expected_chord = binding_chords.get(key_id).unwrap_or_else(|| {
                        panic!(
                            "docs/reference/equivalents.md:{line_no}: menu row `{aria_id}` references missing binding `{key_id}`"
                        )
                    });
                    assert_eq!(
                        aria_shortcut, expected_chord,
                        "docs/reference/equivalents.md:{line_no}: Aria shortcut `{aria_shortcut}` for menu row `{aria_id}` (key `{key_id}`) does not match shipped chord `{expected_chord}`"
                    );
                    covered_bindings.insert(key_id);
                }
                None => {
                    assert_eq!(
                        aria_shortcut, "none",
                        "docs/reference/equivalents.md:{line_no}: menu row `{aria_id}` has no key binding (item.key is None), expected Aria shortcut `none`, found `{aria_shortcut}`"
                    );
                }
            }
        } else {
            assert!(
                shipped_controls.contains(aria_id),
                "docs/reference/equivalents.md:{line_no}: unknown control id `{aria_id}` (not in shipped control registries)"
            );
            let Some((expected_aria, expected_shortcut)) = controls_map.get(aria_id) else {
                panic!(
                    "docs/reference/equivalents.md:{line_no}: control id `{aria_id}` is not documented in docs/reference/controls.md"
                );
            };
            assert_eq!(
                aria, expected_aria,
                "docs/reference/equivalents.md:{line_no}: Aria label `{aria}` for control `{aria_id}` does not match `{expected_aria}`"
            );
            assert_eq!(
                aria_shortcut, expected_shortcut,
                "docs/reference/equivalents.md:{line_no}: Aria shortcut `{aria_shortcut}` for control `{aria_id}` does not match `{expected_shortcut}`"
            );
        }
    }

    let mut expected_sections: Vec<String> =
        AREA_SECTIONS.iter().map(|s| (*s).to_string()).collect();
    expected_sections.push(UNMAPPED_SECTION.to_string());
    assert_eq!(
        seen_sections, expected_sections,
        "docs/reference/equivalents.md sections drifted from expected order"
    );

    for &area in AREA_SECTIONS {
        let count = section_row_counts.get(area).copied().unwrap_or(0);
        assert!(
            count >= 5,
            "docs/reference/equivalents.md section `## {area}` must have at least 5 rows, found {count}"
        );
    }

    let area_pairs: Vec<(String, String)> = unmapped_in_areas
        .iter()
        .map(|(_, app, cmd)| (app.clone(), cmd.clone()))
        .collect();
    let summary_pairs: Vec<(String, String)> = unmapped_summary
        .iter()
        .map(|(_, app, cmd)| (app.clone(), cmd.clone()))
        .collect();
    if area_pairs != summary_pairs {
        for (line_no, app, cmd) in &unmapped_in_areas {
            assert!(
                summary_pairs
                    .iter()
                    .any(|(s_app, s_cmd)| s_app == app && s_cmd == cmd),
                "docs/reference/equivalents.md:{line_no}: row `{app} | {cmd}` has `Aria id` = `none` in its area table but is missing from `## {UNMAPPED_SECTION}`"
            );
        }
        for (line_no, app, cmd) in &unmapped_summary {
            assert!(
                area_pairs
                    .iter()
                    .any(|(a_app, a_cmd)| a_app == app && a_cmd == cmd),
                "docs/reference/equivalents.md:{line_no}: `## {UNMAPPED_SECTION}` row `{app} | {cmd}` has no matching `Aria id` = `none` row in the area tables"
            );
        }
        assert_eq!(
            area_pairs, summary_pairs,
            "docs/reference/equivalents.md `## {UNMAPPED_SECTION}` order or duplicate rows drifted from the `Aria id` = `none` rows in the area tables"
        );
    }

    for &allow_id in ARIA_ONLY_BINDINGS {
        assert!(
            binding_chords.contains_key(allow_id),
            "ARIA_ONLY_BINDINGS entry `{allow_id}` does not exist in SHIPPED_BINDINGS; remove stale allowlist entry"
        );
        assert!(
            !covered_bindings.contains(allow_id),
            "ARIA_ONLY_BINDINGS entry `{allow_id}` is referenced in docs/reference/equivalents.md; remove it from ARIA_ONLY_BINDINGS"
        );
    }

    let mut uncovered: Vec<String> = Vec::new();
    for binding in SHIPPED_BINDINGS {
        if ARIA_ONLY_BINDINGS.contains(&binding.id) {
            continue;
        }
        if !covered_bindings.contains(binding.id) {
            uncovered.push(format!("{} (`{}`)", binding.id, binding.chord.label()));
        }
    }
    assert!(
        uncovered.is_empty(),
        "SHIPPED_BINDINGS entries missing from docs/reference/equivalents.md (add a row or list in ARIA_ONLY_BINDINGS):\n  {}",
        uncovered.join("\n  ")
    );
}
