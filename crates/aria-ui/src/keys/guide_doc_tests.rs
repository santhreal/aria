//! User guide coverage checks for `docs/getting-started.md` and `docs/guide/`.
//!
//! Markup convention enforced across every guide chapter:
//! 1. **Table of contents (`docs/guide/README.md`)**: every `.md` chapter under
//!    `docs/guide/` (excluding `README.md`) must be linked in the `## Chapters`
//!    section of `docs/guide/README.md`, and every relative `.md` link in
//!    `docs/guide/README.md` must exist on disk.
//! 2. **Menu paths (`**Menu > Item**` or `**Menu > Submenu > Item**`)**: every
//!    bold span containing ` > ` outside fenced code blocks and inline code is
//!    parsed as a menu path (with internal line wraps collapsed to single
//!    spaces) and must match a shipped row or submenu caption from
//!    [`super::reference_doc_tests::shipped_menu_paths`].
//! 3. **Control IDs (`` [`<id>`](...reference/controls.md#<surface>) `` or
//!    `` `<id>` control ``)**: every markdown link targeting
//!    `reference/controls.md` whose label is a backticked identifier, and every
//!    inline backticked identifier immediately followed by ` control`, must
//!    exist in [`super::controls_doc_tests::shipped_control_ids`].
//! 4. **Key chords paired with an action (`<kbd>Chord</kbd> (\`key-<id>\`)`)**:
//!    every `<kbd>...</kbd>` tag and every `` `key-...` `` code span in a guide
//!    must appear as a paired `<kbd>Chord</kbd> (\`key-<id>\`)` token, where
//!    `key-<id>` exists in [`super::doc_tests::shipped_binding_chords`] and its
//!    shipped chord label equals `Chord`.
//! 5. **Palette commands (`**<Label>** (\`pal-<id>\`)`)**: every `` `pal-...` ``
//!    code span in a guide must appear as a paired `**<Label>** (\`pal-<id>\`)`
//!    token, where `pal-<id>` is registered in
//!    [`super::reference_doc_tests::shipped_palette_commands`] with exact label
//!    `<Label>`.
//! 6. **Worked examples (`## Example`)**: every chapter (`docs/getting-started.md`
//!    and every chapter under `docs/guide/`) must include a `## Example` section
//!    containing at least three numbered walkthrough steps (`1. `, `2. `, `3. `),
//!    and every numbered step must reference at least one verified menu path,
//!    control ID, key chord pair, or palette command.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::controls_doc_tests::shipped_control_ids;
use super::doc_tests::shipped_binding_chords;
use super::reference_doc_tests::{
    check_markdown_links_in_files, collect_md_files, extract_markdown_links, shipped_menu_paths,
    shipped_palette_commands, strip_inline_code,
};
use crate::test_support::doc_tables::section_slice;

fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn non_fenced_blocks(doc: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut start_line = 1usize;
    let mut in_fence = false;

    for (idx, line) in doc.lines().enumerate() {
        let line_no = idx + 1;
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            if !in_fence && !current.trim().is_empty() {
                blocks.push((start_line, std::mem::take(&mut current)));
            }
            in_fence = !in_fence;
            start_line = line_no + 1;
            continue;
        }
        if in_fence {
            continue;
        }
        if line.trim().is_empty() {
            if !current.trim().is_empty() {
                blocks.push((start_line, std::mem::take(&mut current)));
            }
            start_line = line_no + 1;
        } else {
            if current.is_empty() {
                start_line = line_no;
            } else {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.trim().is_empty() {
        blocks.push((start_line, current));
    }
    blocks
}

fn extract_bold_spans(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'*') {
                j += 1;
            }
            if j + 1 < bytes.len() {
                out.push(collapse_ws(&text[start..j]));
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn extract_control_refs(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = block.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'[' && bytes.get(i + 1) == Some(&b'`') {
            let id_start = i + 2;
            if let Some(id_end_rel) = block[id_start..].find("`](") {
                let id = &block[id_start..id_start + id_end_rel];
                let url_start = id_start + id_end_rel + 3;
                if let Some(url_end_rel) = block[url_start..].find(')') {
                    let url = &block[url_start..url_start + url_end_rel];
                    if url.contains("reference/controls.md") {
                        out.push(id.trim().to_string());
                    }
                    i = url_start + url_end_rel + 1;
                    continue;
                }
            }
        }
        if bytes[i] == b'`' {
            let id_start = i + 1;
            if let Some(id_end_rel) = block[id_start..].find('`') {
                let id = &block[id_start..id_start + id_end_rel];
                let after = &block[id_start + id_end_rel + 1..];
                let after_ws = after.trim_start_matches([' ', '\n', '\r', '\t']);
                if after_ws.len() < after.len()
                    && let Some(rest) = after_ws.strip_prefix("control")
                    && rest
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                {
                    out.push(id.trim().to_string());
                }
                i = id_start + id_end_rel + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn extract_kbd_tags(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(start) = rest.find("<kbd>") {
        let after = &rest[start + 5..];
        let Some(end) = after.find("</kbd>") else {
            out.push("<unclosed-kbd>".to_string());
            break;
        };
        out.push(after[..end].trim().to_string());
        rest = &after[end + 6..];
    }
    out
}

fn extract_prefixed_ticks(block: &str, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_tick = false;
    let mut start = 0usize;
    for (idx, ch) in block.char_indices() {
        if ch == '`' {
            if in_tick {
                let token = block[start..idx].trim();
                if token.starts_with(prefix) {
                    out.push(token.to_string());
                }
                in_tick = false;
            } else {
                in_tick = true;
                start = idx + 1;
            }
        }
    }
    out
}

fn extract_key_pairs(block: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut rest = block;
    while let Some(pos) = rest.find("<kbd>") {
        let after_open = &rest[pos + 5..];
        let Some(close_pos) = after_open.find("</kbd>") else {
            break;
        };
        let chord = after_open[..close_pos].trim().to_string();
        let after_close = &after_open[close_pos + 6..];
        let trimmed = after_close.trim_start_matches([' ', '\n', '\r', '\t']);
        if let Some(after_paren) = trimmed.strip_prefix("(`")
            && let Some(end_tick) = after_paren.find("`)")
        {
            let bid = after_paren[..end_tick].trim().to_string();
            pairs.push((chord, bid));
            rest = &after_paren[end_tick + 2..];
            continue;
        }
        rest = after_close;
    }
    pairs
}

fn extract_palette_pairs(block: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let bytes = block.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'*') {
                j += 1;
            }
            if j + 1 < bytes.len() {
                let label = collapse_ws(&block[start..j]);
                let after_bold = &block[j + 2..];
                let trimmed = after_bold.trim_start_matches([' ', '\n', '\r', '\t']);
                if let Some(after_paren) = trimmed.strip_prefix("(`pal-")
                    && let Some(end_tick) = after_paren.find("`)")
                {
                    let cmd_id = format!("pal-{}", after_paren[..end_tick].trim());
                    pairs.push((label, cmd_id));
                }
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    pairs
}

/// WHY: closes the defect class where a user guide chapter under `docs/guide/`
/// is omitted from the table of contents in `docs/guide/README.md`, contains a
/// broken relative link or `#anchor`, lacks a worked example (`## Example`),
/// or references a menu path, control ID, keyboard shortcut chord, or Command
/// Palette command that does not exist or has a different label or chord in
/// the runtime registries.
/// Does not catch inaccurate prose in the step outcome sentences when all
/// referenced menu paths, control IDs, key chords, and palette commands match.
#[test]
fn every_guide_names_real_controls_menus_and_keys() {
    let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let guide_dir = docs_root.join("guide");
    let readme_path = guide_dir.join("README.md");
    let getting_started_path = docs_root.join("getting-started.md");

    let readme_text = std::fs::read_to_string(&readme_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", readme_path.display()));
    assert!(
        readme_text.contains("## Chapters"),
        "docs/guide/README.md must contain a `## Chapters` table of contents section"
    );
    let chapters_section = section_slice(&readme_text, "Chapters");

    let mut listed_chapters: BTreeSet<PathBuf> = BTreeSet::new();
    let mut errors: Vec<String> = Vec::new();

    for line in chapters_section.lines() {
        for link in extract_markdown_links(line) {
            let path_part = link.split('#').next().unwrap_or(&link).trim();
            if path_part.is_empty() || path_part == "../getting-started.md" {
                continue;
            }
            let target = guide_dir.join(path_part);
            if !target.is_file() {
                errors.push(format!(
                    "docs/guide/README.md: listed chapter `{path_part}` does not exist on disk"
                ));
            } else if let Ok(canon) = std::fs::canonicalize(&target) {
                listed_chapters.insert(canon);
            }
        }
    }

    let mut guide_md_files = Vec::new();
    collect_md_files(&guide_dir, &mut guide_md_files);
    let readme_canon = std::fs::canonicalize(&readme_path).expect("canonicalize guide/README.md");

    let mut chapter_files: Vec<PathBuf> = Vec::new();
    for path in &guide_md_files {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        if canon == readme_canon {
            continue;
        }
        chapter_files.push(path.clone());
        if !listed_chapters.contains(&canon) {
            errors.push(format!(
                "{}: chapter file is not listed under `## Chapters` in docs/guide/README.md",
                path.display()
            ));
        }
    }

    assert!(
        chapter_files.len() >= 20,
        "expected at least 20 chapter files under docs/guide/, found {}",
        chapter_files.len()
    );

    let valid_menus = shipped_menu_paths();
    let valid_controls = shipped_control_ids();
    let valid_bindings = shipped_binding_chords();
    let valid_commands = shipped_palette_commands();

    let mut all_chapters = vec![getting_started_path];
    all_chapters.extend(chapter_files);

    let mut link_check_files = vec![readme_path];
    link_check_files.extend(all_chapters.iter().cloned());
    let (checked_guide_links, broken_guide_links) =
        check_markdown_links_in_files(&link_check_files);
    assert!(
        checked_guide_links >= 150,
        "expected at least 150 internal links across guide files, checked {checked_guide_links}"
    );
    errors.extend(broken_guide_links);

    let mut total_menus = 0usize;
    let mut total_controls = 0usize;
    let mut total_keys = 0usize;
    let mut total_commands = 0usize;
    let mut total_example_steps = 0usize;

    for chapter in &all_chapters {
        let text = std::fs::read_to_string(chapter)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", chapter.display()));
        let display = chapter.display();

        for (line_no, block) in non_fenced_blocks(&text) {
            let block_no_code = strip_inline_code(&block);
            for bold in extract_bold_spans(&block_no_code) {
                if bold.contains(" > ") {
                    total_menus += 1;
                    if !valid_menus.contains(&bold) {
                        errors.push(format!(
                            "{display}:{line_no}: menu path `**{bold}**` is not a shipped menu row"
                        ));
                    }
                }
            }

            for ctrl_id in extract_control_refs(&block) {
                total_controls += 1;
                if !valid_controls.contains(&ctrl_id) {
                    errors.push(format!(
                        "{display}:{line_no}: control id `{ctrl_id}` is not in CONTROL_TIPS or widget helper calls"
                    ));
                }
            }

            let kbds = extract_kbd_tags(&block);
            let key_ticks = extract_prefixed_ticks(&block, "key-");
            let key_pairs = extract_key_pairs(&block);
            if kbds.len() != key_pairs.len() || key_ticks.len() != key_pairs.len() {
                errors.push(format!(
                    "{display}:{line_no}: every `<kbd>Chord</kbd>` and `` `key-...` `` must be paired as `<kbd>Chord</kbd> (`key-id`)` (found {} `<kbd>` tags, {} `key-*` ids, {} pairs)",
                    kbds.len(),
                    key_ticks.len(),
                    key_pairs.len()
                ));
            }
            for (chord, bid) in key_pairs {
                total_keys += 1;
                match valid_bindings.get(bid.as_str()) {
                    Some(expected_chord) => {
                        if !expected_chord.eq_ignore_ascii_case(&chord) {
                            errors.push(format!(
                                "{display}:{line_no}: key chord `<kbd>{chord}</kbd>` paired with `{bid}` does not match shipped binding `{expected_chord}`"
                            ));
                        }
                    }
                    None => {
                        errors.push(format!(
                            "{display}:{line_no}: key binding `{bid}` paired with `<kbd>{chord}</kbd>` is not in SHIPPED_BINDINGS"
                        ));
                    }
                }
            }

            let pal_ticks = extract_prefixed_ticks(&block, "pal-");
            let pal_pairs = extract_palette_pairs(&block);
            if pal_ticks.len() != pal_pairs.len() {
                errors.push(format!(
                    "{display}:{line_no}: every `` `pal-...` `` command must be paired with its bold label `**Label** (`pal-id`)` (found {} `pal-*` ids, {} pairs)",
                    pal_ticks.len(),
                    pal_pairs.len()
                ));
            }
            for (label, cmd_id) in pal_pairs {
                total_commands += 1;
                match valid_commands.get(cmd_id.as_str()) {
                    Some(expected_label) => {
                        if *expected_label != label {
                            errors.push(format!(
                                "{display}:{line_no}: palette command `{cmd_id}` has label `**{label}**`, expected `**{expected_label}**`"
                            ));
                        }
                    }
                    None => {
                        errors.push(format!(
                            "{display}:{line_no}: palette command `{cmd_id}` (`**{label}**`) is not registered in BUILTIN_COMMANDS or BUILTIN_JOB_COMMANDS"
                        ));
                    }
                }
            }
        }

        if !text.contains("\n## Example\n") {
            errors.push(format!(
                "{display}: missing `## Example` task walkthrough section"
            ));
            continue;
        }

        let example = section_slice(&text, "Example");
        let mut steps: Vec<(usize, String)> = Vec::new();
        for (idx, line) in example.lines().enumerate() {
            let trimmed = line.trim_start();
            if let Some((num, rest)) = trimmed.split_once(". ")
                && !num.is_empty()
                && num.bytes().all(|b| b.is_ascii_digit())
            {
                steps.push((idx + 1, rest.to_string()));
            } else if !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && let Some((_, last)) = steps.last_mut()
            {
                last.push(' ');
                last.push_str(trimmed);
            }
        }

        if steps.len() < 3 {
            errors.push(format!(
                "{display}: `## Example` must contain at least 3 numbered walkthrough steps, found {}",
                steps.len()
            ));
        }

        for (step_idx, step_text) in steps {
            total_example_steps += 1;
            let step_no_code = strip_inline_code(&step_text);
            let step_menus = extract_bold_spans(&step_no_code)
                .into_iter()
                .filter(|b| b.contains(" > "))
                .count();
            let step_controls = extract_control_refs(&step_text).len();
            let step_keys = extract_key_pairs(&step_text).len();
            let step_cmds = extract_palette_pairs(&step_text).len();
            if step_menus + step_controls + step_keys + step_cmds == 0 {
                errors.push(format!(
                    "{display}: `## Example` step {step_idx} does not reference any menu path, control ID, key chord, or palette command: {step_text:?}"
                ));
            }
        }
    }

    assert!(
        errors.is_empty(),
        "guide documentation verification failed:\n{}",
        errors.join("\n")
    );

    assert!(
        total_menus >= 100,
        "expected at least 100 menu path references across guides, found {total_menus}"
    );
    assert!(
        total_controls >= 90,
        "expected at least 90 control ID references across guides, found {total_controls}"
    );
    assert!(
        total_keys >= 60,
        "expected at least 60 `<kbd>Chord</kbd> (`key-id`)` references across guides, found {total_keys}"
    );
    assert!(
        total_commands >= 35,
        "expected at least 35 palette command references across guides, found {total_commands}"
    );
    assert!(
        total_example_steps >= 80,
        "expected at least 80 numbered walkthrough steps across guides, found {total_example_steps}"
    );
}
