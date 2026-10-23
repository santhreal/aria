//! WHY: Command palette entries, menu rows, submenu captions, and tool labels
//! share a single UI surface and previously mixed sentence case ("Split at
//! playhead", "Shift lane one second later", "Ripple trim") with Title Case
//! ("Add Cross Fade at Cut", "Paste To Fit"). This module enumerates
//! `BUILTIN_COMMANDS`, `BUILTIN_JOB_COMMANDS`, `tool_specs()`, and the live
//! `AppMenu::ALL` drop entries on an editor `AppState` with `UiPlugin` mounted,
//! asserting Premiere / After Effects Title Case on every shipped label.
//!
//! What this does not catch: dynamic user-data rows whose labels come from
//! filesystem paths or user-chosen names (`file-recent:*`,
//! `file-import-recent:*`, `win-workspace-file:*`, `fx-apply-preset:*`), and
//! prose status messages that are not command, menu, or tool captions.

use crate::app::AppState;
use crate::chrome::menu_items::{DropEntry, drop_entries, row_caption};
use crate::commands::{BUILTIN_COMMANDS, BUILTIN_JOB_COMMANDS};
use crate::tool_catalog::tool_specs;
use crate::types::AppMenu;

const MINOR_WORDS: &[&str] = &[
    "a", "an", "the", "and", "but", "or", "nor", "for", "as", "at", "by", "in", "of", "off", "on",
    "per", "to", "up", "via", "vs", "with", "from", "into", "onto",
];

const EXACT_CASE_TOKENS: &[&str] = &[
    "EDL", "OTIO", "FCPXML", "AI", "LUT", "RGB", "YCbCr", "EQ", "PNG", "dB", "fx", "De-esser",
];

fn is_user_data_row(id: &str) -> bool {
    id.starts_with("file-recent:")
        || id.starts_with("file-import-recent:")
        || id.starts_with("win-workspace-file:")
        || id.starts_with("fx-apply-preset:")
}

/// The word in `token`, without the punctuation around it or a possessive `'s`.
fn trim_token_punctuation(token: &str) -> &str {
    let word = token
        .trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '/' && ch != '\'');
    word.strip_suffix("'s").unwrap_or(word)
}

/// Whether `words[index]` is the noun In of an In and Out pair, as in
/// `Clear In and Out`, rather than the preposition.
fn is_noun_in(words: &[&str], index: usize) -> bool {
    let Some(next) = words.get(index + 1).copied() else {
        return false;
    };
    let next_clean = trim_token_punctuation(next);
    matches!(next_clean, "to" | "and")
        && words
            .get(index + 2)
            .is_some_and(|after| trim_token_punctuation(after) == "Out")
}

fn check_subword_capitalized(segment: &str) -> Result<(), String> {
    if EXACT_CASE_TOKENS.contains(&segment) {
        return Ok(());
    }
    for part in segment.split('-') {
        if part.is_empty() || !part.chars().any(char::is_alphabetic) {
            continue;
        }
        let lower = part.to_ascii_lowercase();
        if MINOR_WORDS.contains(&lower.as_str()) {
            continue;
        }
        if !part
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
        {
            return Err(format!(
                "subword `{part}` in `{segment}` is not capitalized"
            ));
        }
    }
    Ok(())
}

/// Whether `label` is in Title Case, with the reason when it is not. Shared
/// with `chrome::dialog_tests` and `panels::install_dialog::tests`, which check
/// dialog titles.
pub(crate) fn check_title_case(label: &str) -> Result<(), String> {
    if label.is_empty() {
        return Err("label is empty".to_string());
    }
    if label.trim() != label {
        return Err(format!(
            "label `{label}` has leading or trailing whitespace"
        ));
    }
    let words: Vec<&str> = label
        .split_whitespace()
        .filter(|token| token.chars().any(char::is_alphabetic))
        .collect();
    if words.is_empty() {
        return Err(format!("label `{label}` contains no alphabetic words"));
    }
    let last_index = words.len() - 1;
    for (index, &raw_word) in words.iter().enumerate() {
        let trimmed = trim_token_punctuation(raw_word);
        if trimmed.is_empty() || !trimmed.chars().any(char::is_alphabetic) {
            continue;
        }
        if EXACT_CASE_TOKENS.contains(&trimmed) {
            continue;
        }
        if trimmed.contains('/') {
            for piece in trimmed.split('/') {
                if piece.chars().any(char::is_alphabetic) {
                    check_subword_capitalized(piece)?;
                }
            }
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if EXACT_CASE_TOKENS
            .iter()
            .any(|tok| tok.eq_ignore_ascii_case(trimmed))
        {
            return Err(format!(
                "token `{trimmed}` in `{label}` does not match required acronym/unit casing"
            ));
        }
        let is_edge = index == 0 || index == last_index;
        let is_minor = MINOR_WORDS.contains(&lower.as_str());
        if !is_edge && is_minor && !is_noun_in(&words, index) {
            if trimmed != lower {
                return Err(format!(
                    "interior minor word `{trimmed}` in `{label}` must be lowercase `{lower}`"
                ));
            }
            continue;
        }
        check_subword_capitalized(trimmed)
            .map_err(|reason| format!("{reason} (in label `{label}`)"))?;
    }
    Ok(())
}

#[test]
fn builtin_commands_and_jobs_obey_title_case() {
    let mut violations = Vec::new();
    for (id, label) in BUILTIN_COMMANDS
        .iter()
        .map(|cmd| (cmd.id, cmd.label))
        .chain(BUILTIN_JOB_COMMANDS.iter().map(|cmd| (cmd.id, cmd.label)))
    {
        if let Err(err) = check_title_case(label) {
            violations.push(format!("{id}: {err}"));
        }
    }
    assert!(
        violations.is_empty(),
        "command labels violating Title Case:\n{}",
        violations.join("\n")
    );
}

#[test]
fn tool_catalog_labels_obey_title_case() {
    let mut violations = Vec::new();
    for spec in tool_specs() {
        if let Err(err) = check_title_case(spec.label) {
            violations.push(format!("{}: {}", spec.id, err));
        }
    }
    assert!(
        violations.is_empty(),
        "tool labels violating Title Case:\n{}",
        violations.join("\n")
    );
}

#[test]
fn all_menus_and_submenus_obey_title_case() {
    let ctx = aria_plugin::PluginContext::new();
    ctx.mount(&crate::plugins::UiPlugin)
        .expect("mount ui plugin");
    let mut state =
        AppState::new(ctx, crate::scratch_pool::scratch("command-menu-casing")).expect("state");
    state.enter_editor();

    let mut violations = Vec::new();
    for menu in AppMenu::ALL {
        if let Err(err) = check_title_case(menu.label()) {
            violations.push(format!("menu {}: {}", menu.label(), err));
        }
        let items = state.menu_rows(menu);
        for (index, item) in items.iter().enumerate() {
            assert!(
                !item.id.is_empty(),
                "menu {} row {index} ({}) has an empty id",
                menu.label(),
                item.label
            );
        }
        for entry in drop_entries(&items) {
            match entry {
                DropEntry::Row(at) => {
                    let item = &items[at];
                    if is_user_data_row(item.id) {
                        continue;
                    }
                    if let Err(err) = check_title_case(item.label) {
                        violations.push(format!("{} > {}: {}", menu.label(), item.id, err));
                    }
                    let caption = row_caption(item, &state);
                    if let Err(err) = check_title_case(&caption) {
                        violations.push(format!(
                            "{} > {} (caption): {}",
                            menu.label(),
                            item.id,
                            err
                        ));
                    }
                }
                DropEntry::Sub { caption, rows } => {
                    if let Err(err) = check_title_case(caption) {
                        violations.push(format!("{} > submenu `{caption}`: {}", menu.label(), err));
                    }
                    for at in rows {
                        let item = &items[at];
                        if is_user_data_row(item.id) {
                            continue;
                        }
                        if let Err(err) = check_title_case(item.label) {
                            violations.push(format!(
                                "{} > {} > {}: {}",
                                menu.label(),
                                caption,
                                item.id,
                                err
                            ));
                        }
                        let rendered = row_caption(item, &state);
                        if let Err(err) = check_title_case(&rendered) {
                            violations.push(format!(
                                "{} > {} > {} (caption): {}",
                                menu.label(),
                                caption,
                                item.id,
                                err
                            ));
                        }
                    }
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "menu rows or submenu captions violating Title Case:\n{}",
        violations.join("\n")
    );
}

#[test]
fn title_case_validator_rejects_sentence_case_and_capitalized_minor_words() {
    assert!(check_title_case("Split at Playhead").is_ok());
    assert!(check_title_case("Mark In").is_ok());
    assert!(check_title_case("Go to In").is_ok());
    assert!(check_title_case("Render In to Out").is_ok());
    assert!(check_title_case("Clear In and Out").is_ok());
    assert!(check_title_case("Import Timeline (EDL / OTIO / FCPXML)…").is_ok());
    assert!(check_title_case("Nudge Selected Clip Gain +1 dB").is_ok());
    assert!(check_title_case("Toggle Rule-of-Thirds Grid").is_ok());
    assert!(check_title_case("Open One-Second Gap at Playhead").is_ok());

    assert!(check_title_case("Split at playhead").is_err());
    assert!(check_title_case("Shift lane one second later").is_err());
    assert!(check_title_case("Ripple trim").is_err());
    assert!(check_title_case("Paste To Fit").is_err());
    assert!(check_title_case("Place Nest At Playhead").is_err());
    assert!(check_title_case("Open One-second Gap at Playhead").is_err());
}
