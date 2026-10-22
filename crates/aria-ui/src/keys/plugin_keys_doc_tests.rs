//! `docs/plugins/keys.md` and `docs/plugins/basics.md` coverage checks: the
//! built-in plugin mount order and the key binding types and fields a plugin
//! uses are compared against the `aria-ui` registries in both directions.

use std::collections::BTreeMap;
use std::path::Path;

use crate::test_support::doc_tables::{parse_table_rows, section_slice, strip_ticks};

#[test]
fn plugin_keys_and_basics_docs_match_ui_registries_bidirectionally() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let keys_plugin_doc =
        std::fs::read_to_string(root.join("docs/plugins/keys.md")).expect("read keys.md");
    let basics_plugin_doc =
        std::fs::read_to_string(root.join("docs/plugins/basics.md")).expect("read basics.md");
    const KEYS_MOD_SRC: &str = include_str!("mod.rs");
    const KEYS_OVERRIDES_SRC: &str = include_str!("overrides.rs");
    const KEYS_PLUGIN_KEYS_SRC: &str = include_str!("plugin_keys.rs");

    // 1. Verify `docs/plugins/basics.md` `## Mount order` matches `aria_ui::plugins::builtin(None)`.
    let mount_section = section_slice(&basics_plugin_doc, "Mount order");
    let list_paragraph = mount_section
        .split("\n\n")
        .nth(1)
        .expect("mount order plugin list paragraph");
    let mut doc_builtins = Vec::new();
    let mut rest = list_paragraph;
    while let Some((_, after_open)) = rest.split_once('`') {
        let Some((id, tail)) = after_open.split_once('`') else {
            break;
        };
        doc_builtins.push(id.to_owned());
        rest = tail;
    }
    let expected_builtins: Vec<String> = crate::plugins::builtin(std::path::Path::new(""))
        .iter()
        .map(|p| p.manifest().id.to_string())
        .collect();
    assert_eq!(
        doc_builtins, expected_builtins,
        "docs/plugins/basics.md ## Mount order drifted from aria_ui::plugins::builtin(data_dir)"
    );

    // 2. Verify `After::ALL` and `KeyHost` tables in `docs/plugins/keys.md`.
    let rows = parse_table_rows(&keys_plugin_doc);
    let mut type_members: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut key_host_doc_calls: Vec<String> = Vec::new();
    for row in &rows {
        if row.len() == 2 {
            let first = strip_ticks(row[0]);
            if first.contains('(') {
                key_host_doc_calls.push(first.split('(').next().unwrap_or(first).to_owned());
            } else {
                let mut members = Vec::new();
                let mut r: &str = row[1];
                while let Some((_, after_open)) = r.split_once('`') {
                    let Some((tok, tail)) = after_open.split_once('`') else {
                        break;
                    };
                    members.push(tok.to_owned());
                    r = tail;
                }
                type_members.insert(first.to_owned(), members);
            }
        }
    }

    let expected_after: Vec<String> = super::After::ALL
        .iter()
        .map(|after| match after {
            super::After::Nothing | super::After::Play | super::After::Preview => {
                format!("{after:?}")
            }
        })
        .collect();
    assert_eq!(
        type_members.get("After").expect("After row in keys.md"),
        &expected_after,
        "docs/plugins/keys.md After variants drifted from After::ALL"
    );

    // Exhaustive destructuring of Binding, KeyConflict, BindingRow and match on KeyBody / KeyOverrideError.
    let sample_binding = super::Binding {
        id: "sample",
        chord: super::Chord::plain("a"),
        route: super::KeyRoute::Editor,
        run: super::KeyBody::Command("sample"),
        after: super::After::Nothing,
    };
    let super::Binding {
        id: _,
        chord: _,
        route: _,
        run,
        after: _,
    } = sample_binding;
    match run {
        super::KeyBody::Plain(_) | super::KeyBody::Windowed(_) | super::KeyBody::Command(_) => {}
    }
    let sample_conflict = super::KeyConflict {
        held_by: "a",
        refused: "b",
        chord: super::Chord::plain("a"),
    };
    let super::KeyConflict {
        held_by: _,
        refused: _,
        chord: _,
    } = sample_conflict;
    let sample_row = super::BindingRow {
        binding: sample_binding,
        chord: None,
        overridden: false,
    };
    let super::BindingRow {
        binding: _,
        chord: _,
        overridden: _,
    } = sample_row;
    let sample_err = super::KeyOverrideError::Unknown;
    match sample_err {
        super::KeyOverrideError::Unknown | super::KeyOverrideError::Taken { .. } => {}
    }

    assert_eq!(
        type_members.get("KeyBody").expect("KeyBody row"),
        &vec![
            "Plain".to_string(),
            "Windowed".to_string(),
            "Command".to_string()
        ],
    );
    assert_eq!(
        type_members.get("Binding").expect("Binding row"),
        &vec![
            "id".to_string(),
            "chord".to_string(),
            "route".to_string(),
            "run".to_string(),
            "after".to_string(),
        ],
    );
    assert_eq!(
        type_members.get("KeyConflict").expect("KeyConflict row"),
        &vec![
            "held_by".to_string(),
            "refused".to_string(),
            "chord".to_string()
        ],
    );
    assert_eq!(
        type_members.get("BindingRow").expect("BindingRow row"),
        &vec![
            "binding".to_string(),
            "chord".to_string(),
            "overridden".to_string()
        ],
    );
    assert_eq!(
        type_members
            .get("KeyOverrideError")
            .expect("KeyOverrideError row"),
        &vec!["Unknown".to_string(), "Taken".to_string()],
    );

    let mut expected_key_host_calls = Vec::new();
    for src in [KEYS_MOD_SRC, KEYS_OVERRIDES_SRC, KEYS_PLUGIN_KEYS_SRC] {
        let mut in_key_host = false;
        let mut depth = 0usize;
        for line in src.lines() {
            let trimmed = line.trim();
            if !in_key_host {
                if line.starts_with("impl KeyHost {") {
                    in_key_host = true;
                } else {
                    continue;
                }
            }
            if depth == 1
                && let Some(rest) = trimmed.strip_prefix("pub fn ")
            {
                let ident: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                expected_key_host_calls.push(ident);
            }
            let code = trimmed.split("//").next().unwrap_or(trimmed);
            let opens = code.chars().filter(|&c| c == '{').count();
            let closes = code.chars().filter(|&c| c == '}').count();
            depth = depth + opens - closes;
            if in_key_host && depth == 0 {
                in_key_host = false;
            }
        }
    }
    assert_eq!(
        key_host_doc_calls, expected_key_host_calls,
        "docs/plugins/keys.md KeyHost calls drifted from impl KeyHost"
    );
}
