//! Drift-detection suite for `plugin.toml` and data-tier package file reference
//! documentation (`docs/plugins/packages.md`, `docs/plugins/presets.md`,
//! `docs/plugins/workspaces.md`, `docs/plugins/effects.md`, and
//! `docs/theme-tokens.md`).
//!
//! # Why
//!
//! Closes the defect class where a field or table is added to, renamed in, or
//! removed from the `plugin.toml` parser (`PackageManifest::parse`), the
//! `.ariapreset` parser (`Preset::parse`), the workspace schema
//! (`WorkspaceSpec`), the WGSL pass header (`AriaPass` / `AriaTransition`), or
//! the theme token tables (`COLOR_TOKENS` / `FONT_TOKENS`) while the reference
//! documentation or its code examples stay on the old schema.
//!
//! Every schema member set is derived from the production parser or source of
//! truth at run time with no hardcoded field lists in this file:
//! - Private `#[serde(deny_unknown_fields)]` manifest tables in
//!   `crates/aria-plugin/src/package/{manifest.rs, manifest_passes.rs,
//!   manifest_clip_types.rs}` are introspected by probing
//!   `PackageManifest::parse` with an unknown sentinel key (`__probe__ = 1`),
//!   which triggers serde's generated `Visitor` to format its static `FIELDS`
//!   slice into the refusal message. Removing `#[serde(deny_unknown_fields)]`
//!   or adding a field without documenting it fails the suite immediately.
//! - Public `serde::Deserialize` structs (`EffectBounds`, `EffectGroupSpec`,
//!   `EffectParamSpec`, `EffectChoiceOption`, `EffectCurvePoint`,
//!   `EffectGradientStop`, `EffectVisibilityCondition`, `Preset`,
//!   `EffectInstance`, `AudioEffectInstance`, `PresetProperty`, `Keyframe`) are
//!   introspected through a zero-allocation `serde::Deserializer` that captures
//!   the static `FIELDS` slice passed to `deserialize_struct`.
//! - Every `toml`, `json`, and `wgsl` example block in the owning documentation
//!   chapters is parsed and validated by the production parsers.
//!
//! What this does not catch: prose descriptions that state an inaccurate range
//! while keeping the right key name and valid example values (covered by unit
//! tests in `package::tests`, `preset::tests`, and `theme_file_tests`).

mod effects_and_themes;
mod host_services;
mod native_sdk;
mod packages;
mod ui_and_basics;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn read_workspace_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

// ---------------------------------------------------------------------------
// Markdown table and code-block parsers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MarkdownTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or(trimmed);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut in_code = false;
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'|') {
            current.push('|');
            chars.next();
            continue;
        }
        if ch == '`' {
            in_code = !in_code;
            current.push(ch);
            continue;
        }
        if ch == '|' && !in_code {
            cells.push(current.trim().to_owned());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    cells.push(current.trim().to_owned());
    cells
}

fn is_separator_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let stripped = cell.trim_matches(':').trim();
            !stripped.is_empty() && stripped.chars().all(|c| c == '-')
        })
}

/// Parse all Markdown tables appearing in `section`.
fn parse_markdown_tables(section: &str) -> Vec<MarkdownTable> {
    let mut tables = Vec::new();
    let mut lines = section.lines().peekable();
    let mut in_fence = false;
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || !trimmed.starts_with('|') {
            continue;
        }
        let headers = split_table_row(trimmed);
        let Some(next_line) = lines.peek() else {
            continue;
        };
        if !next_line.trim_start().starts_with('|') {
            continue;
        }
        let sep = split_table_row(next_line.trim_start());
        if !is_separator_row(&sep) {
            continue;
        }
        lines.next();
        let mut rows = Vec::new();
        while let Some(row_line) = lines.peek() {
            let row_trimmed = row_line.trim_start();
            if !row_trimmed.starts_with('|') {
                break;
            }
            rows.push(split_table_row(row_trimmed));
            lines.next();
        }
        tables.push(MarkdownTable { headers, rows });
    }
    tables
}

/// Extract the text under a specific Markdown heading (`## ...` or `### ...`)
/// up to the next heading of the same or higher level.
fn extract_heading_section<'a>(markdown: &'a str, heading: &str) -> &'a str {
    let level = heading.chars().take_while(|&c| c == '#').count();
    assert!(level > 0, "heading must start with `#`: {heading}");
    let mut in_fence = false;
    let mut start_offset = None;
    let mut offset = 0;
    for line in markdown.lines() {
        let line_len = line.len() + 1;
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence {
            if let Some(start) = start_offset {
                let hashes = trimmed.chars().take_while(|&c| c == '#').count();
                if hashes > 0 && hashes <= level && trimmed.as_bytes().get(hashes) == Some(&b' ') {
                    return &markdown[start..offset];
                }
            } else if trimmed.trim_end() == heading {
                start_offset = Some(offset + line.len());
            }
        }
        offset += line_len.min(markdown.len().saturating_sub(offset));
    }
    let start = start_offset.unwrap_or_else(|| panic!("heading `{heading}` not found"));
    &markdown[start..]
}

/// Extract backticked identifiers from the first cell of each row in `table`,
/// verifying `Key | Type | Default | Description` headers and non-empty cells.
fn assert_key_type_default_description_table(label: &str, table: &MarkdownTable) -> Vec<String> {
    assert_eq!(
        table.headers,
        ["Key", "Type", "Default", "Description"],
        "{label}: table headers must be `Key | Type | Default | Description`"
    );
    let mut keys = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        assert_eq!(row.len(), 4, "{label}: row {row:?} does not have 4 columns");
        for (col_idx, cell) in row.iter().enumerate() {
            assert!(
                !cell.is_empty(),
                "{label}: row {row:?} has an empty cell in column {}",
                table.headers[col_idx]
            );
        }
        let key = row[0]
            .strip_prefix('`')
            .and_then(|s| s.strip_suffix('`'))
            .unwrap_or_else(|| {
                panic!(
                    "{label}: key cell `{}` must be a single backticked identifier",
                    row[0]
                )
            });
        keys.push(key.to_owned());
    }
    keys
}

/// Assert 1:1 set equality and absence of duplicate rows between parser keys
/// and documented table keys.
fn assert_key_parity(label: &str, parser_keys: &[String], doc_keys: &[String]) {
    let mut seen = BTreeSet::new();
    for key in doc_keys {
        assert!(
            seen.insert(key.as_str()),
            "{label}: duplicate key `{key}` in documentation table"
        );
    }
    let parser_set: BTreeSet<&str> = parser_keys.iter().map(String::as_str).collect();
    let doc_set: BTreeSet<&str> = doc_keys.iter().map(String::as_str).collect();
    let missing: Vec<&&str> = parser_set.difference(&doc_set).collect();
    let extra: Vec<&&str> = doc_set.difference(&parser_set).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{label}: schema drift between production parser and documentation:\n  undocumented parser keys: {missing:?}\n  stale documented keys: {extra:?}"
    );
}

/// Extract all fenced code blocks with language tag `lang` from `markdown`.
fn extract_code_blocks(markdown: &str, lang: &str) -> Vec<String> {
    let fence_open = format!("```{lang}");
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if !in_block {
            if trimmed.trim_end() == fence_open {
                in_block = true;
                current.clear();
            }
        } else if trimmed.trim_end() == "```" {
            in_block = false;
            blocks.push(current.clone());
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    assert!(!in_block, "unclosed ```{lang} fence in markdown");
    blocks
}

const MINIMAL_PLUGIN_HEADER: &str = "\
[plugin]
id = \"com.example.probe\"
name = \"Probe\"
version = \"0.1.0\"
api = 1
";

// ---------------------------------------------------------------------------
// 6. Plugin API & subsystem reference chapters (`docs/plugins/*.md`)
// ---------------------------------------------------------------------------

fn extract_backticked_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some((_, after_open)) = rest.split_once('`') {
        let Some((token, tail)) = after_open.split_once('`') else {
            break;
        };
        out.push(token.to_owned());
        rest = tail;
    }
    out
}

fn extract_call_names_from_table(table: &MarkdownTable) -> Vec<String> {
    table
        .rows
        .iter()
        .map(|row| {
            let tokens = extract_backticked_tokens(&row[0]);
            let first = tokens
                .first()
                .unwrap_or_else(|| panic!("expected backticked call in {:?}", row[0]));
            let before_paren = first.split('(').next().unwrap_or(first);
            let after_colon = before_paren.rsplit("::").next().unwrap_or(before_paren);
            after_colon
                .strip_prefix("ctx.")
                .unwrap_or(after_colon)
                .to_owned()
        })
        .collect()
}

fn extract_type_members_map(
    table: &MarkdownTable,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut map = std::collections::BTreeMap::new();
    for row in &table.rows {
        let ty = extract_backticked_tokens(&row[0])
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("expected backticked type name in {:?}", row[0]));
        let members = extract_backticked_tokens(&row[1]);
        assert!(
            map.insert(ty.clone(), members).is_none(),
            "duplicate type row `{ty}`"
        );
    }
    map
}

fn count_braces_outside_comments(line: &str) -> (usize, usize) {
    let code = line.split("//").next().unwrap_or(line);
    let opens = code.chars().filter(|&c| c == '{').count();
    let closes = code.chars().filter(|&c| c == '}').count();
    (opens, closes)
}

/// The `pub` fields of `pub struct struct_name` in `src`, empty when `src`
/// does not define it with named fields.
fn struct_pub_fields(src: &str, struct_name: &str) -> Vec<String> {
    let prefix = format!("pub struct {struct_name}");
    let mut in_struct = false;
    let mut depth = 0usize;
    let mut fields = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if !in_struct {
            if let Some(rest) = trimmed.strip_prefix(&prefix)
                && rest
                    .chars()
                    .next()
                    .is_none_or(|c| matches!(c, '<' | '{' | ' '))
            {
                in_struct = true;
            } else {
                continue;
            }
        }
        if depth == 1
            && let Some(after_pub) = trimmed.strip_prefix("pub ")
            && !after_pub.starts_with('(')
            && let Some((field, _)) = after_pub.split_once(':')
        {
            fields.push(field.trim().to_owned());
        }
        let (opens, closes) = count_braces_outside_comments(trimmed);
        depth = depth + opens - closes;
        if in_struct && depth == 0 && opens + closes > 0 {
            break;
        }
    }
    fields
}

fn extract_struct_pub_fields(src: &str, struct_name: &str) -> Vec<String> {
    let fields = struct_pub_fields(src, struct_name);
    assert!(
        !fields.is_empty(),
        "failed to extract pub fields for `pub struct {struct_name}`"
    );
    fields
}

/// The variants of `pub enum enum_name` in `src`, empty when `src` does not
/// define it.
fn enum_variants(src: &str, enum_name: &str) -> Vec<String> {
    let prefix = format!("pub enum {enum_name}");
    let mut in_enum = false;
    let mut depth = 0usize;
    let mut variants = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if !in_enum {
            if let Some(rest) = trimmed.strip_prefix(&prefix)
                && rest
                    .chars()
                    .next()
                    .is_none_or(|c| matches!(c, '<' | '{' | ' '))
            {
                in_enum = true;
            } else {
                continue;
            }
        }
        if depth == 1
            && !trimmed.is_empty()
            && !trimmed.starts_with("//")
            && !trimmed.starts_with('#')
        {
            let ident: String = trimmed
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                variants.push(ident);
            }
        }
        let (opens, closes) = count_braces_outside_comments(trimmed);
        depth = depth + opens - closes;
        if in_enum && depth == 0 && opens + closes > 0 {
            break;
        }
    }
    variants
}

fn extract_enum_variants(src: &str, enum_name: &str) -> Vec<String> {
    let variants = enum_variants(src, enum_name);
    assert!(
        !variants.is_empty(),
        "failed to extract variants for `pub enum {enum_name}`"
    );
    variants
}

fn is_inherent_impl_for(line: &str, type_name: &str) -> bool {
    if !line.starts_with("impl") || line.contains(" for ") {
        return false;
    }
    let before_brace = line.split('{').next().unwrap_or(line).trim();
    let last_token = before_brace.split_whitespace().last().unwrap_or("");
    let base = last_token.split('<').next().unwrap_or(last_token);
    base == type_name
}

/// The `pub fn` and `pub const fn` names, and with `with_consts` the
/// `pub const` names, of every inherent `impl type_name` block in `src`.
fn impl_pub_items(src: &str, type_name: &str, with_consts: bool) -> Vec<String> {
    let mut in_impl = false;
    let mut depth = 0usize;
    let mut items = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if !in_impl {
            if is_inherent_impl_for(line, type_name) {
                in_impl = true;
            } else {
                continue;
            }
        }
        if depth == 1 {
            let after_pub = trimmed
                .strip_prefix("pub const fn ")
                .or_else(|| trimmed.strip_prefix("pub fn "))
                .or_else(|| {
                    with_consts
                        .then(|| trimmed.strip_prefix("pub const "))
                        .flatten()
                });
            if let Some(rest) = after_pub {
                let ident: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() {
                    items.push(ident);
                }
            }
        }
        let (opens, closes) = count_braces_outside_comments(trimmed);
        depth = depth + opens - closes;
        if in_impl && depth == 0 {
            in_impl = false;
        }
    }
    items
}

fn extract_impl_pub_fns(src: &str, type_name: &str) -> Vec<String> {
    let fns = impl_pub_items(src, type_name, false);
    assert!(
        !fns.is_empty(),
        "failed to extract inherent pub fns for `impl {type_name}`"
    );
    fns
}

/// The method names of `pub trait trait_name` in `src`, empty when `src` does
/// not define it.
fn trait_fns(src: &str, trait_name: &str) -> Vec<String> {
    let prefix = format!("pub trait {trait_name}");
    let mut in_trait = false;
    let mut depth = 0usize;
    let mut fns = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if !in_trait {
            if let Some(rest) = line.strip_prefix(&prefix)
                && rest
                    .chars()
                    .next()
                    .is_none_or(|c| matches!(c, ':' | '<' | '{' | ' '))
            {
                in_trait = true;
            } else {
                continue;
            }
        }
        if depth == 1
            && let Some(rest) = trimmed.strip_prefix("fn ")
        {
            let ident: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                fns.push(ident);
            }
        }
        let (opens, closes) = count_braces_outside_comments(trimmed);
        depth = depth + opens - closes;
        if in_trait && depth == 0 && opens + closes > 0 {
            break;
        }
    }
    fns
}

fn extract_trait_fns(src: &str, trait_name: &str) -> Vec<String> {
    let fns = trait_fns(src, trait_name);
    assert!(
        !fns.is_empty(),
        "failed to extract trait fns for `pub trait {trait_name}`"
    );
    fns
}

fn leading_ident(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// Every identifier inside an inline code span or a fenced code block of
/// `markdown`.
fn markdown_code_identifiers(markdown: &str) -> BTreeSet<String> {
    let mut code = String::new();
    let mut prose = String::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        let target = if in_fence { &mut code } else { &mut prose };
        target.push_str(line);
        target.push('\n');
    }
    for token in extract_backticked_tokens(&prose) {
        code.push_str(&token);
        code.push('\n');
    }
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}
