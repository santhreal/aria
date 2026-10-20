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

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use aria_compositor::{EffectDefinition, TransitionDefinition};
use aria_models::{
    AudioEffectInstance, EffectBounds, EffectChoiceOption, EffectCurvePoint, EffectGradientStop,
    EffectGroupSpec, EffectInstance, EffectParamSpec, EffectSpec, EffectVisibilityCondition,
    GraphPass, Keyframe, PassStage, Preset, PresetProperty, SpatialTangents, TransitionSpec,
};
use aria_plugin::PluginContext;
use aria_plugin::package::PackageManifest;
use serde::de::{self, Deserialize, Deserializer, Visitor};

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
// Runtime serde struct field introspection
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct FieldCaptureError(Option<&'static [&'static str]>);

impl fmt::Display for FieldCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "captured serde struct fields")
    }
}

impl std::error::Error for FieldCaptureError {}

impl de::Error for FieldCaptureError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        Self(None)
    }
}

struct StructFieldCapturer;

macro_rules! reject_non_struct {
    ($($method:ident ( $($arg:ident : $ty:ty),* ) ),* $(,)?) => {
        $(
            fn $method<V: Visitor<'de>>(self, $($arg: $ty,)* _visitor: V) -> Result<V::Value, Self::Error> {
                $(let _ = $arg;)*
                Err(FieldCaptureError(None))
            }
        )*
    };
}

impl<'de> Deserializer<'de> for StructFieldCapturer {
    type Error = FieldCaptureError;

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(FieldCaptureError(Some(fields)))
    }

    reject_non_struct! {
        deserialize_any(),
        deserialize_bool(),
        deserialize_i8(),
        deserialize_i16(),
        deserialize_i32(),
        deserialize_i64(),
        deserialize_u8(),
        deserialize_u16(),
        deserialize_u32(),
        deserialize_u64(),
        deserialize_f32(),
        deserialize_f64(),
        deserialize_char(),
        deserialize_str(),
        deserialize_string(),
        deserialize_bytes(),
        deserialize_byte_buf(),
        deserialize_option(),
        deserialize_unit(),
        deserialize_unit_struct(_name: &'static str),
        deserialize_newtype_struct(_name: &'static str),
        deserialize_seq(),
        deserialize_tuple(_len: usize),
        deserialize_tuple_struct(_name: &'static str, _len: usize),
        deserialize_map(),
        deserialize_enum(_name: &'static str, _variants: &'static [&'static str]),
        deserialize_identifier(),
        deserialize_ignored_any(),
    }
}

/// Extract the compile-time serde `FIELDS` slice from `T::deserialize`.
fn serde_struct_fields<'de, T: Deserialize<'de>>() -> Vec<String> {
    match T::deserialize(StructFieldCapturer) {
        Err(FieldCaptureError(Some(fields))) => {
            assert!(
                !fields.is_empty(),
                "serde struct {} reported zero fields",
                std::any::type_name::<T>()
            );
            fields.iter().map(|f| (*f).to_owned()).collect()
        }
        Err(FieldCaptureError(None)) => panic!(
            "{} did not call deserialize_struct",
            std::any::type_name::<T>()
        ),
        Ok(_) => panic!(
            "{} unexpectedly succeeded on StructFieldCapturer",
            std::any::type_name::<T>()
        ),
    }
}

/// Probe `PackageManifest::parse` with a manifest containing `__probe__ = 1`
/// inside a target table, and extract the expected field names from serde's
/// `unknown field \`__probe__\`, expected ...` error message.
fn probe_manifest_keys(label: &str, toml_with_probe: &str) -> Vec<String> {
    let Err(err) = PackageManifest::parse(toml_with_probe) else {
        panic!(
            "{label}: manifest with `__probe__ = 1` was accepted; is #[serde(deny_unknown_fields)] missing?"
        );
    };
    parse_unknown_field_error(label, &err)
}

fn parse_unknown_field_error(label: &str, err: &str) -> Vec<String> {
    let (_, after_probe) = err
        .split_once("unknown field `__probe__`, expected ")
        .unwrap_or_else(|| {
            panic!("{label}: expected serde unknown-field error for `__probe__`, got: {err}")
        });
    let expected_clause = after_probe.lines().next().unwrap_or(after_probe);
    let mut fields = Vec::new();
    let mut rest = expected_clause;
    while let Some((_, after_open)) = rest.split_once('`') {
        let Some((field, tail)) = after_open.split_once('`') else {
            break;
        };
        fields.push(field.to_owned());
        rest = tail;
    }
    assert!(
        !fields.is_empty(),
        "{label}: failed to extract any field names from error: {err}"
    );
    fields
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

// ---------------------------------------------------------------------------
// 1. `plugin.toml` reference in `docs/plugins/packages.md`
// ---------------------------------------------------------------------------

const MINIMAL_PLUGIN_HEADER: &str = "\
[plugin]
id = \"com.example.probe\"
name = \"Probe\"
version = \"0.1.0\"
api = 1
";

#[test]
fn plugin_toml_reference_covers_every_table_and_key_in_production_parser() {
    let packages_md = read_workspace_file("docs/plugins/packages.md");
    let ref_section = extract_heading_section(&packages_md, "## `plugin.toml` reference");

    // 1. Dynamically discover the top-level keys of `plugin.toml` (`RawFile`).
    let root_parser_keys = probe_manifest_keys(
        "plugin.toml root (RawFile)",
        &format!("__probe__ = 1\n{MINIMAL_PLUGIN_HEADER}"),
    );
    let ref_tables = parse_markdown_tables(ref_section);
    assert!(
        !ref_tables.is_empty(),
        "expected top-level table under ## `plugin.toml` reference"
    );
    let root_doc_keys =
        assert_key_type_default_description_table("plugin.toml root table", &ref_tables[0]);
    assert_key_parity(
        "plugin.toml root (RawFile)",
        &root_parser_keys,
        &root_doc_keys,
    );

    // Every top-level key in `RawFile` must also appear in a `###` heading
    // inside `## plugin.toml reference`.
    let subheadings: Vec<&str> = ref_section
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("### "))
        .collect();
    for top_key in &root_parser_keys {
        let single = format!("`[{top_key}]`");
        let array = format!("`[[{top_key}]]`");
        assert!(
            subheadings
                .iter()
                .any(|h| h.contains(&single) || h.contains(&array)),
            "top-level `plugin.toml` key `{top_key}` has no `### [{top_key}]` or `### [[{top_key}]]` heading in docs/plugins/packages.md (found headings: {subheadings:?})"
        );
    }

    // 2. Verify each section's table against the production parser.
    let section_probes: [(&str, String); 13] = [
        (
            "### `[plugin]`",
            format!("{MINIMAL_PLUGIN_HEADER}__probe__ = 1\n"),
        ),
        (
            "### `[permissions]`",
            format!("{MINIMAL_PLUGIN_HEADER}[permissions]\n__probe__ = 1\n"),
        ),
        (
            "### `[activation]`",
            format!("{MINIMAL_PLUGIN_HEADER}[activation]\n__probe__ = 1\n"),
        ),
        (
            "### `[[textures]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[textures]]\nid = \"t\"\nfile = \"t.png\"\n__probe__ = 1\n"
            ),
        ),
        (
            "### `[[effects]]` and `[[generators]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[effects]]\nid = \"e\"\nname = \"E\"\nshader = \"e.wgsl\"\n__probe__ = 1\n"
            ),
        ),
        (
            "### `[[transitions]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[transitions]]\nid = \"t\"\nname = \"T\"\nshader = \"t.wgsl\"\n__probe__ = 1\n"
            ),
        ),
        (
            "### `[[commands]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[commands]]\nid = \"c\"\ntitle = \"C\"\n__probe__ = 1\n"
            ),
        ),
        (
            "### `[[menus]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[menus]]\ncommand = \"c\"\nmenu = \"Clip\"\n__probe__ = 1\n"
            ),
        ),
        (
            "### `[[keys]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[keys]]\ncommand = \"c\"\nkey = \"Ctrl+K\"\n__probe__ = 1\n"
            ),
        ),
        (
            "### `[[panels]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[panels]]\nid = \"p\"\ntitle = \"P\"\nlocation = \"sidebar\"\n__probe__ = 1\n"
            ),
        ),
        (
            "### `[[settings]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[settings]]\nkey = \"k\"\nlabel = \"K\"\n__probe__ = 1\n"
            ),
        ),
        (
            "### `[[workspaces]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[workspaces]]\nid = \"w\"\nname = \"W\"\n__probe__ = 1\n"
            ),
        ),
        (
            "### `[[clip_types]]`",
            format!(
                "{MINIMAL_PLUGIN_HEADER}[[clip_types]]\nid = \"c\"\nname = \"C\"\ncolour = \"teal\"\nshader = \"c.wgsl\"\n__probe__ = 1\n"
            ),
        ),
    ];

    for (heading, probe_toml) in &section_probes {
        let sub = extract_heading_section(ref_section, heading);
        let tables = parse_markdown_tables(sub);
        assert!(
            !tables.is_empty(),
            "{heading}: missing `Key | Type | Default | Description` table"
        );
        let doc_keys = assert_key_type_default_description_table(heading, &tables[0]);
        let parser_keys = probe_manifest_keys(heading, probe_toml);
        assert_key_parity(heading, &parser_keys, &doc_keys);

        let examples = extract_code_blocks(sub, "toml");
        assert!(
            !examples.is_empty(),
            "{heading}: missing minimal valid TOML example"
        );
    }

    // Verify `[[generators]]` accepts the same keys as `[[effects]]`.
    let effects_keys = probe_manifest_keys(
        "[[effects]]",
        &format!(
            "{MINIMAL_PLUGIN_HEADER}[[effects]]\nid = \"e\"\nname = \"E\"\nshader = \"e.wgsl\"\n__probe__ = 1\n"
        ),
    );
    let generators_keys = probe_manifest_keys(
        "[[generators]]",
        &format!(
            "{MINIMAL_PLUGIN_HEADER}[[generators]]\nid = \"g\"\nname = \"G\"\nshader = \"g.wgsl\"\n__probe__ = 1\n"
        ),
    );
    assert_eq!(effects_keys, generators_keys);

    // 3. Verify `[[effects.passes]]`, `[[generators.passes]]`, `[[clip_types.passes]]`
    // and nested parameter tables (`EffectBounds`, `EffectGroupSpec`,
    // `EffectParamSpec`, `EffectChoiceOption`, `EffectCurvePoint`,
    // `EffectGradientStop`, `EffectVisibilityCondition`).
    let passes_heading = "### `[[effects.passes]]`, `[[generators.passes]]`, and parameter tables";
    let passes_sub = extract_heading_section(ref_section, passes_heading);
    let passes_tables = parse_markdown_tables(passes_sub);
    assert_eq!(
        passes_tables.len(),
        8,
        "{passes_heading}: expected 8 tables (passes, bounds, groups, params, choices, default_curve, default_gradient, visible_when)"
    );

    let effect_pass_keys = probe_manifest_keys(
        "[[effects.passes]]",
        &format!(
            "{MINIMAL_PLUGIN_HEADER}[[effects]]\nid = \"e\"\nname = \"E\"\n[[effects.passes]]\nfile = \"e.wgsl\"\n__probe__ = 1\n"
        ),
    );
    let generator_pass_keys = probe_manifest_keys(
        "[[generators.passes]]",
        &format!(
            "{MINIMAL_PLUGIN_HEADER}[[generators]]\nid = \"g\"\nname = \"G\"\n[[generators.passes]]\nfile = \"g.wgsl\"\n__probe__ = 1\n"
        ),
    );
    let clip_type_pass_keys = probe_manifest_keys(
        "[[clip_types.passes]]",
        &format!(
            "{MINIMAL_PLUGIN_HEADER}[[clip_types]]\nid = \"c\"\nname = \"C\"\ncolour = \"teal\"\n[[clip_types.passes]]\nfile = \"c.wgsl\"\n__probe__ = 1\n"
        ),
    );
    assert_eq!(effect_pass_keys, generator_pass_keys);
    assert_eq!(effect_pass_keys, clip_type_pass_keys);

    let pass_doc_keys =
        assert_key_type_default_description_table("[[effects.passes]]", &passes_tables[0]);
    assert_key_parity("[[effects.passes]]", &effect_pass_keys, &pass_doc_keys);

    let nested_schemas: [(&str, Vec<String>, &MarkdownTable); 7] = [
        (
            "bounds (EffectBounds)",
            serde_struct_fields::<EffectBounds>(),
            &passes_tables[1],
        ),
        (
            "groups (EffectGroupSpec)",
            serde_struct_fields::<EffectGroupSpec>(),
            &passes_tables[2],
        ),
        (
            "params (EffectParamSpec)",
            serde_struct_fields::<EffectParamSpec>(),
            &passes_tables[3],
        ),
        (
            "choices (EffectChoiceOption)",
            serde_struct_fields::<EffectChoiceOption>(),
            &passes_tables[4],
        ),
        (
            "default_curve (EffectCurvePoint)",
            serde_struct_fields::<EffectCurvePoint>(),
            &passes_tables[5],
        ),
        (
            "default_gradient (EffectGradientStop)",
            serde_struct_fields::<EffectGradientStop>(),
            &passes_tables[6],
        ),
        (
            "visible_when (EffectVisibilityCondition)",
            serde_struct_fields::<EffectVisibilityCondition>(),
            &passes_tables[7],
        ),
    ];
    for (label, parser_keys, table) in nested_schemas {
        let doc_keys = assert_key_type_default_description_table(label, table);
        assert_key_parity(label, &parser_keys, &doc_keys);
    }
}

#[test]
fn every_toml_example_in_packages_and_workspaces_docs_parses_through_package_manifest() {
    for rel in ["docs/plugins/packages.md", "docs/plugins/workspaces.md"] {
        let markdown = read_workspace_file(rel);
        let blocks = extract_code_blocks(&markdown, "toml");
        assert!(
            !blocks.is_empty(),
            "{rel}: expected at least one ```toml example block"
        );
        for (idx, block) in blocks.iter().enumerate() {
            PackageManifest::parse(block).unwrap_or_else(|err| {
                panic!(
                    "{rel} ```toml block #{idx} failed PackageManifest::parse: {err}\n---\n{block}"
                )
            });
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Data-tier schemas: `presets.md`, `workspaces.md`, `effects.md`, `theme-tokens.md`
// ---------------------------------------------------------------------------

#[test]
fn presets_doc_covers_every_preset_serde_field_and_validates_all_json_examples() {
    let presets_md = read_workspace_file("docs/plugins/presets.md");
    let pkg_section = extract_heading_section(&presets_md, "## Package files");
    let tables = parse_markdown_tables(pkg_section);
    assert!(
        tables.len() >= 7,
        "docs/plugins/presets.md: expected at least 7 schema tables under ## Package files, found {}",
        tables.len()
    );

    let preset_schemas: [(&str, Vec<String>, &MarkdownTable); 6] = [
        ("Preset root", serde_struct_fields::<Preset>(), &tables[0]),
        (
            "EffectInstance (effects[])",
            serde_struct_fields::<EffectInstance>(),
            &tables[1],
        ),
        (
            "AudioEffectInstance (audio_effects[])",
            serde_struct_fields::<AudioEffectInstance>(),
            &tables[2],
        ),
        (
            "PresetProperty (properties[])",
            serde_struct_fields::<PresetProperty>(),
            &tables[3],
        ),
        (
            "Keyframe<T>",
            serde_struct_fields::<Keyframe<f32>>(),
            &tables[5],
        ),
        (
            "SpatialTangents (s)",
            serde_struct_fields::<SpatialTangents>(),
            &tables[6],
        ),
    ];
    for (label, parser_keys, table) in preset_schemas {
        let doc_keys = assert_key_type_default_description_table(label, table);
        assert_key_parity(label, &parser_keys, &doc_keys);
    }

    // Also verify `Preset` and `PresetProperty` reject unknown fields via
    // `Preset::parse`.
    let root_probe = br#"{"version": 1, "name": "Probe", "__probe__": 1}"#;
    let root_err = Preset::parse(root_probe)
        .expect_err("Preset must reject unknown root fields")
        .to_string();
    assert_eq!(
        parse_unknown_field_error("Preset::parse root", &root_err),
        serde_struct_fields::<Preset>()
    );

    let prop_probe = br#"{"version": 1, "name": "Probe", "properties": [{"id": "opacity", "curve": {"scalar": 1.0}, "__probe__": 1}]}"#;
    let prop_err = Preset::parse(prop_probe)
        .expect_err("PresetProperty must reject unknown fields")
        .to_string();
    assert_eq!(
        parse_unknown_field_error("PresetProperty", &prop_err),
        serde_struct_fields::<PresetProperty>()
    );

    // Every ```json block in `docs/plugins/presets.md` must pass `Preset::parse`
    // (which runs `Preset::validate`).
    let json_blocks = extract_code_blocks(&presets_md, "json");
    assert!(
        json_blocks.len() >= 2,
        "docs/plugins/presets.md: expected at least 2 ```json example blocks"
    );
    for (idx, block) in json_blocks.iter().enumerate() {
        Preset::parse(block.as_bytes()).unwrap_or_else(|err| {
            panic!(
                "docs/plugins/presets.md ```json block #{idx} failed Preset::parse: {err}\n---\n{block}"
            )
        });
    }
}

#[test]
fn workspaces_doc_table_matches_manifest_workspace_parser() {
    let workspaces_md = read_workspace_file("docs/plugins/workspaces.md");
    let tables = parse_markdown_tables(&workspaces_md);
    assert_eq!(
        tables.len(),
        1,
        "docs/plugins/workspaces.md: expected 1 schema table"
    );
    let doc_keys =
        assert_key_type_default_description_table("docs/plugins/workspaces.md", &tables[0]);
    let parser_keys = probe_manifest_keys(
        "[[workspaces]]",
        &format!(
            "{MINIMAL_PLUGIN_HEADER}[[workspaces]]\nid = \"w\"\nname = \"W\"\n__probe__ = 1\n"
        ),
    );
    assert_key_parity("docs/plugins/workspaces.md", &parser_keys, &doc_keys);
}

/// Extract the field names of a WGSL struct (`struct Name { ... }`) from
/// `registered_graph.rs`.
fn extract_wgsl_struct_fields(source: &str, struct_name: &str) -> Vec<String> {
    let header = format!("struct {struct_name}");
    let (_, after) = source
        .split_once(&header)
        .unwrap_or_else(|| panic!("WGSL struct `{struct_name}` not found"));
    let (_, body_and_rest) = after.split_once('{').expect("opening brace");
    let (body, _) = body_and_rest.split_once('}').expect("closing brace");
    let mut fields = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some((name, _)) = trimmed.split_once(':') {
            fields.push(name.trim().to_owned());
        }
    }
    assert!(!fields.is_empty(), "no fields found in {struct_name}");
    fields
}

#[test]
fn effects_doc_matches_wgsl_structs_and_validates_all_wgsl_examples() {
    let effects_md = read_workspace_file("docs/plugins/effects.md");
    let registered_graph_rs = read_workspace_file("crates/aria-compositor/src/registered_graph.rs");

    let shader_section = extract_heading_section(&effects_md, "## Shader source");
    let tables = parse_markdown_tables(shader_section);
    assert!(
        tables.len() >= 5,
        "docs/plugins/effects.md ## Shader source: expected at least 5 tables, found {}",
        tables.len()
    );

    // Verify `AriaPass` and `AriaTransition` field tables against `registered_graph.rs`.
    for (struct_name, table) in [("AriaPass", &tables[1]), ("AriaTransition", &tables[4])] {
        let source_fields = extract_wgsl_struct_fields(&registered_graph_rs, struct_name);
        let mut doc_fields = Vec::new();
        for row in &table.rows {
            for part in row[0].split(',') {
                let field = part.trim().trim_matches('`');
                if !field.is_empty() {
                    doc_fields.push(field.to_owned());
                }
            }
        }
        assert_key_parity(struct_name, &source_fields, &doc_fields);
    }

    // Validate every ```wgsl code block in `docs/plugins/effects.md` through the
    // compositor's real naga validator (`register_effect` / `register_transition`).
    let wgsl_blocks = extract_code_blocks(&effects_md, "wgsl");
    assert!(
        wgsl_blocks.len() >= 3,
        "docs/plugins/effects.md: expected at least 3 ```wgsl example blocks"
    );
    let ctx = PluginContext::new();
    for (idx, wgsl) in wgsl_blocks.iter().enumerate() {
        if wgsl.contains("outgoing_tex") || wgsl.contains("incoming_tex") {
            let id = format!("com.example.doccheck.transition_{idx}");
            ctx.register_transition(TransitionDefinition {
                spec: TransitionSpec {
                    id,
                    label: format!("Doc Transition {idx}"),
                    params: vec![EffectParamSpec::scalar(
                        "softness", "Softness", 0.25, 0.0, 1.0,
                    )],
                },
                shader: wgsl.clone(),
            })
            .unwrap_or_else(|err| {
                panic!("docs/plugins/effects.md ```wgsl transition #{idx} failed validation: {err}\n---\n{wgsl}")
            });
        } else {
            let id = format!("com.example.doccheck.effect_{idx}");
            let pass = if wgsl.contains("@compute") {
                GraphPass {
                    name: "half".into(),
                    stage: PassStage::Compute { workgroup: [8, 8] },
                    inputs: Vec::new(),
                    output: aria_models::OutputSize::Same,
                    source: wgsl.clone(),
                }
            } else {
                GraphPass::fragment(wgsl.clone())
            };
            ctx.register_effect(EffectDefinition {
                spec: EffectSpec {
                    id,
                    label: format!("Doc Effect {idx}"),
                    params: vec![EffectParamSpec::scalar("gain", "Gain", 1.0, 0.0, 2.0)],
                    ..Default::default()
                },
                passes: vec![pass],
            })
            .unwrap_or_else(|err| {
                panic!("docs/plugins/effects.md ```wgsl effect #{idx} failed validation: {err}\n---\n{wgsl}")
            });
        }
    }
}

/// Extract token names from `slots!(Ty; ...)` in `crates/aria-ui/src/theme_file.rs`.
fn extract_theme_slots(theme_file_rs: &str, macro_prefix: &str) -> Vec<String> {
    let (_, after) = theme_file_rs
        .split_once(macro_prefix)
        .unwrap_or_else(|| panic!("`{macro_prefix}` not found in theme_file.rs"));
    let (body, _) = after.split_once(");").expect("closing `);` of slots!");
    body.lines()
        .map(|line| line.trim().trim_end_matches(',').trim())
        .filter(|token| !token.is_empty() && !token.starts_with("//"))
        .map(str::to_owned)
        .collect()
}

/// Extract `pub <field>:` declarations from `pub struct Theme` in
/// `crates/aria-ui/src/theme.rs`.
fn extract_theme_struct_fields(theme_rs: &str) -> Vec<String> {
    let (_, after) = theme_rs
        .split_once("pub struct Theme {")
        .expect("`pub struct Theme` in theme.rs");
    let (body, _) = after.split_once('}').expect("closing brace of Theme");
    let mut fields = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("pub ")
            && let Some((name, _)) = rest.split_once(':')
        {
            let name = name.trim();
            if name != "appearance" {
                fields.push(name.to_owned());
            }
        }
    }
    fields
}

/// Extract the top-level TOML keys accepted by `ThemeFile::parse` in
/// `crates/aria-ui/src/theme_file.rs`.
fn extract_theme_file_top_keys(theme_file_rs: &str) -> Vec<String> {
    let (_, after_parse) = theme_file_rs
        .split_once("pub fn parse(")
        .expect("`ThemeFile::parse` in theme_file.rs");
    let (_, after_match) = after_parse
        .split_once("match key.as_str()")
        .expect("`match key.as_str()` in ThemeFile::parse");
    let (match_body, _) = after_match
        .split_once("other =>")
        .expect("`other =>` arm in ThemeFile::parse");
    let mut keys = Vec::new();
    for line in match_body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('"')
            && let Some((key, tail)) = rest.split_once('"')
            && tail.trim_start().starts_with("=>")
        {
            keys.push(key.to_owned());
        }
    }
    assert!(
        !keys.is_empty(),
        "failed to extract top-level keys from ThemeFile::parse"
    );
    keys
}

#[test]
fn theme_tokens_doc_covers_every_theme_token_and_validates_toml_examples() {
    let theme_tokens_md = read_workspace_file("docs/theme-tokens.md");
    let theme_file_rs = read_workspace_file("crates/aria-ui/src/theme_file.rs");
    let theme_rs = read_workspace_file("crates/aria-ui/src/theme.rs");

    let color_tokens = extract_theme_slots(&theme_file_rs, "slots!(Hsla;");
    let font_tokens = extract_theme_slots(&theme_file_rs, "slots!(SharedString;");
    let mut parser_tokens: Vec<String> = color_tokens
        .iter()
        .chain(font_tokens.iter())
        .cloned()
        .collect();
    let struct_fields = extract_theme_struct_fields(&theme_rs);
    assert_key_parity(
        "Theme struct fields vs COLOR_TOKENS + FONT_TOKENS",
        &struct_fields,
        &parser_tokens,
    );

    // Collect every token documented across the token tables in `docs/theme-tokens.md`
    // (`## Surfaces`, `## Lines`, `## Text`, `## Accents`, `## Editor tokens`).
    let mut doc_tokens = Vec::new();
    for heading in [
        "## Surfaces",
        "## Lines",
        "## Text",
        "## Accents",
        "## Editor tokens",
    ] {
        let section = extract_heading_section(&theme_tokens_md, heading);
        for table in parse_markdown_tables(section) {
            if table.headers.first().map(String::as_str) != Some("Token") {
                continue;
            }
            for row in &table.rows {
                let token = row[0]
                    .strip_prefix('`')
                    .and_then(|s| s.strip_suffix('`'))
                    .unwrap_or_else(|| {
                        panic!(
                            "{heading}: token cell `{}` must be a backticked identifier",
                            row[0]
                        )
                    });
                doc_tokens.push(token.to_owned());
            }
        }
    }
    parser_tokens.dedup();
    assert_key_parity(
        "docs/theme-tokens.md token tables",
        &parser_tokens,
        &doc_tokens,
    );

    // Verify the `## Theme files` top-level key table (`name`, `dark`, `light`).
    let theme_files_section = extract_heading_section(&theme_tokens_md, "## Theme files");
    let theme_file_tables = parse_markdown_tables(theme_files_section);
    assert_eq!(
        theme_file_tables.len(),
        1,
        "docs/theme-tokens.md ## Theme files: expected 1 schema table"
    );
    let top_doc_keys = assert_key_type_default_description_table(
        "docs/theme-tokens.md ## Theme files",
        &theme_file_tables[0],
    );
    let expected_top_keys = extract_theme_file_top_keys(&theme_file_rs);
    assert_key_parity(
        "ThemeFile top-level keys",
        &expected_top_keys,
        &top_doc_keys,
    );

    // Validate every ```toml example in `docs/theme-tokens.md`.
    let token_set: BTreeSet<&str> = parser_tokens.iter().map(String::as_str).collect();
    let toml_blocks = extract_code_blocks(&theme_tokens_md, "toml");
    assert!(
        !toml_blocks.is_empty(),
        "docs/theme-tokens.md: expected at least 1 ```toml example"
    );
    for (idx, block) in toml_blocks.iter().enumerate() {
        let table: toml::Table = block.parse().unwrap_or_else(|err| {
            panic!("docs/theme-tokens.md ```toml block #{idx} is not valid TOML: {err}")
        });
        let mut saw_override = false;
        for (key, value) in &table {
            match key.as_str() {
                "name" => {
                    assert!(
                        value.as_str().is_some_and(|s| !s.trim().is_empty()),
                        "docs/theme-tokens.md ```toml block #{idx}: `name` must be a non-empty string"
                    );
                }
                "dark" | "light" => {
                    let sub = value.as_table().unwrap_or_else(|| {
                        panic!(
                            "docs/theme-tokens.md ```toml block #{idx}: `[{key}]` must be a table"
                        )
                    });
                    for (token, val) in sub {
                        assert!(
                            token_set.contains(token.as_str()),
                            "docs/theme-tokens.md ```toml block #{idx}: unknown token `{token}` in `[{key}]`"
                        );
                        assert!(
                            val.as_str().is_some_and(|s| !s.trim().is_empty()),
                            "docs/theme-tokens.md ```toml block #{idx}: `[{key}].{token}` must be a non-empty string"
                        );
                        saw_override = true;
                    }
                }
                other => panic!(
                    "docs/theme-tokens.md ```toml block #{idx}: unknown top-level key `{other}`"
                ),
            }
        }
        assert!(
            saw_override,
            "docs/theme-tokens.md ```toml block #{idx} sets no tokens"
        );
    }
}

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

fn extract_struct_pub_fields(src: &str, struct_name: &str) -> Vec<String> {
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
    assert!(
        !fields.is_empty(),
        "failed to extract pub fields for `pub struct {struct_name}`"
    );
    fields
}

fn extract_enum_variants(src: &str, enum_name: &str) -> Vec<String> {
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

fn extract_impl_pub_fns(src: &str, type_name: &str) -> Vec<String> {
    let mut in_impl = false;
    let mut depth = 0usize;
    let mut fns = Vec::new();
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
                .or_else(|| trimmed.strip_prefix("pub fn "));
            if let Some(rest) = after_pub {
                let ident: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() {
                    fns.push(ident);
                }
            }
        }
        let (opens, closes) = count_braces_outside_comments(trimmed);
        depth = depth + opens - closes;
        if in_impl && depth == 0 {
            in_impl = false;
        }
    }
    assert!(
        !fns.is_empty(),
        "failed to extract inherent pub fns for `impl {type_name}`"
    );
    fns
}

fn extract_trait_fns(src: &str, trait_name: &str) -> Vec<String> {
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
    assert!(
        !fns.is_empty(),
        "failed to extract trait fns for `pub trait {trait_name}`"
    );
    fns
}

fn extract_pub_traits(src: &str) -> Vec<String> {
    let mut traits = Vec::new();
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("pub trait ") {
            let ident: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                traits.push(ident);
            }
        }
    }
    traits
}

#[test]
fn activation_doc_matches_activation_registries_bidirectionally() {
    let doc = read_workspace_file("docs/plugins/activation.md");
    let src = read_workspace_file("crates/aria-plugin/src/activation.rs");

    // 1. ActivationKind::ALL spellings vs the top-level activation events table.
    let all_tables = parse_markdown_tables(&doc);
    assert!(!all_tables.is_empty());
    let events_tables = &all_tables[..1];
    let mut doc_spellings = Vec::new();
    for row in &events_tables[0].rows {
        for token in extract_backticked_tokens(&row[0]) {
            let prefix = token
                .split_once('<')
                .map(|(head, _)| head)
                .unwrap_or(token.as_str());
            doc_spellings.push(prefix.to_owned());
        }
    }
    let expected_spellings: Vec<String> = aria_plugin::ActivationKind::ALL
        .iter()
        .map(|kind| {
            // Exhaustive match on ActivationKind
            match kind {
                aria_plugin::ActivationKind::Startup
                | aria_plugin::ActivationKind::Command
                | aria_plugin::ActivationKind::File
                | aria_plugin::ActivationKind::Effect
                | aria_plugin::ActivationKind::Transition
                | aria_plugin::ActivationKind::Generator
                | aria_plugin::ActivationKind::Panel
                | aria_plugin::ActivationKind::Setting => kind.spelling().to_owned(),
            }
        })
        .collect();
    assert_key_parity(
        "docs/plugins/activation.md ## Activation events",
        &expected_spellings,
        &doc_spellings,
    );

    // 2. Host calls and types under `## Host calls`.
    let host_section = extract_heading_section(&doc, "## Host calls");
    let host_tables = parse_markdown_tables(host_section);
    assert_eq!(host_tables.len(), 2);
    let doc_calls = extract_call_names_from_table(&host_tables[0]);
    let expected_calls = extract_impl_pub_fns(&src, "PluginContext");
    assert_key_parity(
        "docs/plugins/activation.md ## Host PluginContext calls",
        &expected_calls,
        &doc_calls,
    );

    let type_map = extract_type_members_map(&host_tables[1]);
    for ty in ["WaitingPlugin", "WaitingPanel", "Activated"] {
        let expected_fields = extract_struct_pub_fields(&src, ty);
        let doc_fields = type_map
            .get(ty)
            .unwrap_or_else(|| panic!("missing `{ty}` row in activation.md"));
        assert_key_parity(
            &format!("docs/plugins/activation.md {ty} fields"),
            &expected_fields,
            doc_fields,
        );
    }
}

#[test]
fn ai_tools_doc_matches_ai_tool_registries_bidirectionally() {
    let doc = read_workspace_file("docs/plugins/ai-tools.md");
    let src = read_workspace_file("crates/aria-plugin/src/ai_tools.rs");

    assert!(doc.contains(&format!("`{}`", aria_plugin::AI_TOOL_PREFIX)));
    assert!(doc.contains(&format!("`{}`", aria_plugin::MAX_AI_TOOL_NAME)));
    assert!(doc.contains(&format!("`{}`", aria_plugin::MAX_AI_TOOL_SPEC)));
    assert!(doc.contains(&format!("`{}`", aria_plugin::MAX_AI_TOOL_IMAGE_SIDE)));

    // 1. AiToolSpec fields under `## Names and schemas`.
    let schema_tables =
        parse_markdown_tables(extract_heading_section(&doc, "## Names and schemas"));
    assert_eq!(schema_tables.len(), 1);
    let doc_spec_fields: Vec<String> = schema_tables[0]
        .rows
        .iter()
        .map(|row| extract_backticked_tokens(&row[0]).remove(0))
        .collect();
    let expected_spec_fields = extract_struct_pub_fields(&src, "AiToolSpec");
    assert_key_parity(
        "docs/plugins/ai-tools.md AiToolSpec fields",
        &expected_spec_fields,
        &doc_spec_fields,
    );

    // 2. AiToolTier::ALL under `## Tiers`.
    let tier_tables = parse_markdown_tables(extract_heading_section(&doc, "## Tiers"));
    assert_eq!(tier_tables.len(), 1);
    let doc_tiers: Vec<String> = tier_tables[0]
        .rows
        .iter()
        .map(|row| extract_backticked_tokens(&row[0]).remove(0))
        .collect();
    let expected_tiers: Vec<String> = aria_plugin::AiToolTier::ALL
        .iter()
        .map(|tier| match tier {
            aria_plugin::AiToolTier::ReadOnly
            | aria_plugin::AiToolTier::Workspace
            | aria_plugin::AiToolTier::System => format!("{tier:?}"),
        })
        .collect();
    assert_key_parity(
        "docs/plugins/ai-tools.md AiToolTier::ALL",
        &expected_tiers,
        &doc_tiers,
    );

    // 3. AiToolOutput and AiToolImage under `## Output`.
    let output_tables = parse_markdown_tables(extract_heading_section(&doc, "## Output"));
    assert_eq!(output_tables.len(), 1);
    let output_map = extract_type_members_map(&output_tables[0]);
    for ty in ["AiToolOutput", "AiToolImage"] {
        let expected = extract_struct_pub_fields(&src, ty);
        let documented = output_map
            .get(ty)
            .unwrap_or_else(|| panic!("missing `{ty}` in ai-tools.md"));
        assert_key_parity(&format!("ai-tools.md {ty}"), &expected, documented);
    }

    // 4. PluginContext AI tool calls and PluginAiTool / AiToolError under `## Listing`.
    let listing_tables = parse_markdown_tables(extract_heading_section(&doc, "## Listing"));
    assert_eq!(listing_tables.len(), 2);
    let doc_calls = extract_call_names_from_table(&listing_tables[0]);
    let expected_calls = extract_impl_pub_fns(&src, "PluginContext");
    assert_key_parity(
        "docs/plugins/ai-tools.md PluginContext calls",
        &expected_calls,
        &doc_calls,
    );
    let listing_map = extract_type_members_map(&listing_tables[1]);
    assert_key_parity(
        "ai-tools.md PluginAiTool fields",
        &extract_struct_pub_fields(&src, "PluginAiTool"),
        listing_map.get("PluginAiTool").expect("PluginAiTool row"),
    );
    assert_key_parity(
        "ai-tools.md AiToolError variants",
        &extract_enum_variants(&src, "AiToolError"),
        listing_map.get("AiToolError").expect("AiToolError row"),
    );
}

#[test]
fn keys_doc_matches_plugin_key_registries_bidirectionally() {
    let doc = read_workspace_file("docs/plugins/keys.md");
    let src = read_workspace_file("crates/aria-plugin/src/keys.rs");

    assert!(doc.contains(&format!("`{}`", aria_plugin::keys::BINDABLE_KEYS.len())));
    assert!(doc.contains(&format!("`{}`", aria_plugin::keys::SHIFTED_SYMBOLS.len())));
    for &(symbol, key) in aria_plugin::keys::SHIFTED_SYMBOLS {
        assert!(
            doc.contains(symbol) && doc.contains(key),
            "docs/plugins/keys.md missing SHIFTED_SYMBOLS pair ({symbol:?}, {key:?})"
        );
    }

    let tables = parse_markdown_tables(&doc);
    assert!(
        tables.len() >= 3,
        "expected at least 3 tables in docs/plugins/keys.md"
    );

    // Table 0: KeyRoute::ALL (`Route | KeyRoute::label() | Scope`).
    let expected_routes: Vec<(String, String)> = aria_plugin::KeyRoute::ALL
        .iter()
        .map(|route| match route {
            aria_plugin::KeyRoute::Editor
            | aria_plugin::KeyRoute::Timeline
            | aria_plugin::KeyRoute::Viewer
            | aria_plugin::KeyRoute::Home => (format!("{route:?}"), format!("{:?}", route.label())),
        })
        .collect();
    let doc_routes: Vec<(String, String)> = tables[0]
        .rows
        .iter()
        .map(|row| {
            (
                extract_backticked_tokens(&row[0]).remove(0),
                extract_backticked_tokens(&row[1]).remove(0),
            )
        })
        .collect();
    assert_eq!(
        doc_routes, expected_routes,
        "docs/plugins/keys.md KeyRoute table drifted from KeyRoute::ALL"
    );

    // Table 1: Chord and KeyDefault fields.
    let type_map = extract_type_members_map(&tables[1]);
    for ty in ["Chord", "KeyDefault"] {
        assert_key_parity(
            &format!("docs/plugins/keys.md {ty} fields"),
            &extract_struct_pub_fields(&src, ty),
            type_map
                .get(ty)
                .unwrap_or_else(|| panic!("missing `{ty}` row")),
        );
    }
}

#[test]
fn jobs_doc_matches_job_registries_bidirectionally() {
    let doc = read_workspace_file("docs/plugins/jobs.md");
    let jobs_src = read_workspace_file("crates/aria-plugin/src/jobs.rs");
    let aria_jobs_src = read_workspace_file("crates/aria-jobs/src/lib.rs");

    let in_proc_tables =
        parse_markdown_tables(extract_heading_section(&doc, "## In-process plugins"));
    assert_eq!(in_proc_tables.len(), 3);

    // Table 0: PluginContext job methods.
    let doc_ctx_calls = extract_call_names_from_table(&in_proc_tables[0]);
    let expected_ctx_calls = extract_impl_pub_fns(&jobs_src, "PluginContext");
    assert_key_parity(
        "docs/plugins/jobs.md PluginContext job calls",
        &expected_ctx_calls,
        &doc_ctx_calls,
    );

    // Table 1: JobContext methods.
    let doc_job_calls = extract_call_names_from_table(&in_proc_tables[1]);
    let expected_job_calls = extract_impl_pub_fns(&aria_jobs_src, "JobContext");
    assert_key_parity(
        "docs/plugins/jobs.md JobContext calls",
        &expected_job_calls,
        &doc_job_calls,
    );

    // Table 2: JobState variants.
    let doc_states: Vec<String> = in_proc_tables[2]
        .rows
        .iter()
        .map(|row| extract_backticked_tokens(&row[0]).remove(0))
        .collect();
    let expected_states = extract_enum_variants(&aria_jobs_src, "JobState");
    assert_key_parity(
        "docs/plugins/jobs.md JobState variants",
        &expected_states,
        &doc_states,
    );

    // `## Stepped jobs: WASM and native plugins` table 0: JobStep fields.
    let stepped_tables = parse_markdown_tables(extract_heading_section(
        &doc,
        "## Stepped jobs: WASM and native plugins",
    ));
    assert_eq!(stepped_tables.len(), 2);
    let doc_step_fields: Vec<String> = stepped_tables[0]
        .rows
        .iter()
        .map(|row| extract_backticked_tokens(&row[0]).remove(0))
        .collect();
    let expected_step_fields = extract_struct_pub_fields(&jobs_src, "JobStep");
    assert_key_parity(
        "docs/plugins/jobs.md JobStep fields",
        &expected_step_fields,
        &doc_step_fields,
    );
}

#[test]
fn scopes_doc_matches_scope_registries_bidirectionally() {
    let doc = read_workspace_file("docs/plugins/scopes.md");
    let src = read_workspace_file("crates/aria-plugin/src/scopes.rs");

    assert!(doc.contains(&format!("`{}`", aria_plugin::MAX_SCOPE_SIDE)));
    assert!(doc.contains(&format!("`{}`", aria_plugin::MAX_SCOPE_READOUT)));

    let frame_tables = parse_markdown_tables(extract_heading_section(&doc, "## The frame"));
    assert_eq!(frame_tables.len(), 1);
    assert_key_parity(
        "docs/plugins/scopes.md ScopeFrame calls",
        &extract_impl_pub_fns(&src, "ScopeFrame"),
        &extract_call_names_from_table(&frame_tables[0]),
    );

    let image_tables = parse_markdown_tables(extract_heading_section(&doc, "## The image"));
    assert_eq!(image_tables.len(), 1);
    assert_key_parity(
        "docs/plugins/scopes.md ScopeImage calls",
        &extract_impl_pub_fns(&src, "ScopeImage"),
        &extract_call_names_from_table(&image_tables[0]),
    );

    let failure_tables = parse_markdown_tables(extract_heading_section(&doc, "## Failure"));
    assert_eq!(failure_tables.len(), 3);
    let type_map = extract_type_members_map(&failure_tables[1]);
    for ty in ["ScopeSpec", "PluginScope"] {
        assert_key_parity(
            &format!("docs/plugins/scopes.md {ty} fields"),
            &extract_struct_pub_fields(&src, ty),
            type_map
                .get(ty)
                .unwrap_or_else(|| panic!("missing `{ty}` row")),
        );
    }
    assert_key_parity(
        "docs/plugins/scopes.md ScopeError variants",
        &extract_enum_variants(&src, "ScopeError"),
        type_map.get("ScopeError").expect("ScopeError row"),
    );
    assert_key_parity(
        "docs/plugins/scopes.md PluginContext calls",
        &extract_impl_pub_fns(&src, "PluginContext"),
        &extract_call_names_from_table(&failure_tables[2]),
    );
}

#[test]
fn interchange_doc_matches_interchange_encoder_decoder_registries_bidirectionally() {
    let doc = read_workspace_file("docs/plugins/interchange.md");
    let interchange_src = read_workspace_file("crates/aria-plugin/src/interchange.rs");
    let decoders_src = read_workspace_file("crates/aria-plugin/src/decoders.rs");
    let core_traits_src = read_workspace_file("crates/aria-core/src/encode.rs");
    let decoder_lib_src = read_workspace_file("crates/aria-decoder/src/external.rs");

    let imp_tables =
        parse_markdown_tables(extract_heading_section(&doc, "## Importers and exporters"));
    assert_eq!(imp_tables.len(), 2);
    let enc_tables = parse_markdown_tables(extract_heading_section(&doc, "## Encoders"));
    assert_eq!(enc_tables.len(), 2);
    let dec_tables = parse_markdown_tables(extract_heading_section(&doc, "## Decoders"));
    assert_eq!(dec_tables.len(), 2);

    // Combined PluginContext calls in interchange.rs (importers/exporters + encoders).
    let mut doc_interchange_calls = extract_call_names_from_table(&imp_tables[0]);
    doc_interchange_calls.extend(extract_call_names_from_table(&enc_tables[0]));
    assert_key_parity(
        "docs/plugins/interchange.md PluginContext interchange+encoder calls",
        &extract_impl_pub_fns(&interchange_src, "PluginContext"),
        &doc_interchange_calls,
    );

    let imp_types = extract_type_members_map(&imp_tables[1]);
    for ty in ["ImporterSpec", "ExporterSpec", "InterchangeReport"] {
        assert_key_parity(
            &format!("interchange.md {ty} fields"),
            &extract_struct_pub_fields(&interchange_src, ty),
            imp_types
                .get(ty)
                .unwrap_or_else(|| panic!("missing `{ty}` row")),
        );
    }
    assert_key_parity(
        "interchange.md InterchangeError variants",
        &extract_enum_variants(&interchange_src, "InterchangeError"),
        imp_types
            .get("InterchangeError")
            .expect("InterchangeError row"),
    );

    let enc_types = extract_type_members_map(&enc_tables[1]);
    for ty in ["EncoderSpec", "EncoderFrame"] {
        assert_key_parity(
            &format!("interchange.md {ty} fields"),
            &extract_struct_pub_fields(&interchange_src, ty),
            enc_types
                .get(ty)
                .unwrap_or_else(|| panic!("missing `{ty}` row")),
        );
    }
    assert_key_parity(
        "interchange.md Encoder trait methods",
        &extract_trait_fns(&interchange_src, "Encoder"),
        enc_types.get("Encoder").expect("Encoder row"),
    );
    assert_key_parity(
        "interchange.md VideoEncoder trait methods",
        &extract_trait_fns(&core_traits_src, "VideoEncoder"),
        enc_types.get("VideoEncoder").expect("VideoEncoder row"),
    );

    // Decoders: ExternalDecoder trait methods + PluginContext decoder methods.
    let mut expected_dec_calls = extract_trait_fns(&decoder_lib_src, "ExternalDecoder");
    expected_dec_calls.extend(extract_impl_pub_fns(&decoders_src, "PluginContext"));
    assert_key_parity(
        "docs/plugins/interchange.md Decoders calls",
        &expected_dec_calls,
        &extract_call_names_from_table(&dec_tables[0]),
    );
    let dec_types = extract_type_members_map(&dec_tables[1]);
    for ty in ["DecoderSpec", "PluginDecoder"] {
        assert_key_parity(
            &format!("interchange.md {ty} fields"),
            &extract_struct_pub_fields(&decoders_src, ty),
            dec_types
                .get(ty)
                .unwrap_or_else(|| panic!("missing `{ty}` row")),
        );
    }
}

#[test]
fn project_doc_matches_project_render_audio_host_registries_bidirectionally() {
    let doc = read_workspace_file("docs/plugins/project.md");
    let project_host_src = read_workspace_file("crates/aria-plugin/src/project_host.rs");
    let queries_src = read_workspace_file("crates/aria-plugin/src/queries.rs");
    let render_host_src = read_workspace_file("crates/aria-plugin/src/render_host.rs");
    let audio_host_src = read_workspace_file("crates/aria-plugin/src/audio_host.rs");

    assert!(doc.contains(&format!(
        "({} ms)",
        aria_plugin::ProjectHost::CONTENDED_WAIT.as_millis()
    )));
    assert!(doc.contains(&format!("({})", aria_plugin::EXPORT_AUDIO_RATE)));
    assert!(doc.contains(&format!("({})", aria_plugin::AUDIO_HOST_BLOCK)));

    // 1. `## Project`
    let proj_tables = parse_markdown_tables(extract_heading_section(&doc, "## Project"));
    assert_eq!(proj_tables.len(), 2);
    assert_key_parity(
        "docs/plugins/project.md ProjectHost calls",
        &extract_impl_pub_fns(&project_host_src, "ProjectHost"),
        &extract_call_names_from_table(&proj_tables[0]),
    );
    let proj_types = extract_type_members_map(&proj_tables[1]);
    for enum_ty in ["Origin", "TransactError"] {
        assert_key_parity(
            &format!("project.md {enum_ty} variants"),
            &extract_enum_variants(&project_host_src, enum_ty),
            proj_types
                .get(enum_ty)
                .unwrap_or_else(|| panic!("missing `{enum_ty}`")),
        );
    }
    for struct_ty in ["CommandApplied", "Receipt"] {
        assert_key_parity(
            &format!("project.md {struct_ty} fields"),
            &extract_struct_pub_fields(&project_host_src, struct_ty),
            proj_types
                .get(struct_ty)
                .unwrap_or_else(|| panic!("missing `{struct_ty}`")),
        );
    }
    for impl_ty in ["EditGroup", "Transaction"] {
        assert_key_parity(
            &format!("project.md {impl_ty} methods"),
            &extract_impl_pub_fns(&project_host_src, impl_ty),
            proj_types
                .get(impl_ty)
                .unwrap_or_else(|| panic!("missing `{impl_ty}`")),
        );
    }
    for query_ty in ["ClipRef", "InOut"] {
        assert_key_parity(
            &format!("project.md {query_ty} fields"),
            &extract_struct_pub_fields(&queries_src, query_ty),
            proj_types
                .get(query_ty)
                .unwrap_or_else(|| panic!("missing `{query_ty}`")),
        );
    }

    // 2. `## Frames`
    let frame_tables = parse_markdown_tables(extract_heading_section(&doc, "## Frames"));
    assert_eq!(frame_tables.len(), 2);
    assert_key_parity(
        "docs/plugins/project.md RenderHost calls",
        &extract_impl_pub_fns(&render_host_src, "RenderHost"),
        &extract_call_names_from_table(&frame_tables[0]),
    );
    let doc_frame_fields: Vec<String> = frame_tables[1]
        .rows
        .iter()
        .map(|row| extract_backticked_tokens(&row[0]).remove(0))
        .collect();
    assert_key_parity(
        "docs/plugins/project.md FrameRequest fields",
        &extract_struct_pub_fields(&render_host_src, "FrameRequest"),
        &doc_frame_fields,
    );

    // 3. `## Sound`
    let sound_tables = parse_markdown_tables(extract_heading_section(&doc, "## Sound"));
    assert_eq!(sound_tables.len(), 1);
    assert_key_parity(
        "docs/plugins/project.md AudioHost calls",
        &extract_impl_pub_fns(&audio_host_src, "AudioHost"),
        &extract_call_names_from_table(&sound_tables[0]),
    );
}

#[test]
fn basics_doc_matches_plugin_basics_registries_bidirectionally() {
    let doc = read_workspace_file("docs/plugins/basics.md");
    let plugin_src = read_workspace_file("crates/aria-plugin/src/plugin.rs");
    let context_src = read_workspace_file("crates/aria-plugin/src/context.rs");
    let reg_src = read_workspace_file("crates/aria-plugin/src/registration.rs");
    let error_src = read_workspace_file("crates/aria-plugin/src/error.rs");
    let service_src = read_workspace_file("crates/aria-plugin/src/service.rs");
    let log_src = read_workspace_file("crates/aria-plugin/src/plugin_log.rs");
    let cost_src = read_workspace_file("crates/aria-plugin/src/cost.rs");

    assert!(doc.contains(&format!("`{}`", aria_plugin::LOG_LINES_PER_PLUGIN)));
    assert!(doc.contains(&format!("`{}`", aria_plugin::LOG_LINE_MAX_BYTES)));
    assert!(doc.contains(&format!("`{}`", aria_plugin::LOG_LINE_TRUNCATED)));

    // 1. `## Plugin trait` tables: types, ContributionKind::ALL, PluginError variants.
    let trait_tables = parse_markdown_tables(extract_heading_section(&doc, "## Plugin trait"));
    assert_eq!(trait_tables.len(), 3);
    let type_map = extract_type_members_map(&trait_tables[0]);
    assert_key_parity(
        "basics.md PluginManifest fields",
        &extract_struct_pub_fields(&plugin_src, "PluginManifest"),
        type_map.get("PluginManifest").expect("PluginManifest row"),
    );
    for ty in [
        "MountedPluginInfo",
        "MountFailure",
        "CapabilityDenial",
        "PluginFault",
    ] {
        assert_key_parity(
            &format!("basics.md {ty} fields"),
            &extract_struct_pub_fields(&context_src, ty),
            type_map
                .get(ty)
                .unwrap_or_else(|| panic!("missing `{ty}` row")),
        );
    }
    assert_key_parity(
        "basics.md Contribution fields",
        &extract_struct_pub_fields(&reg_src, "Contribution"),
        type_map.get("Contribution").expect("Contribution row"),
    );
    for ty in ["CostMetric", "PluginCost"] {
        assert_key_parity(
            &format!("basics.md {ty} fields"),
            &extract_struct_pub_fields(&cost_src, ty),
            type_map
                .get(ty)
                .unwrap_or_else(|| panic!("missing `{ty}` row")),
        );
    }

    let expected_kinds: Vec<(String, String)> = aria_plugin::ContributionKind::ALL
        .iter()
        .map(|kind| (format!("{kind:?}"), format!("{:?}", kind.label())))
        .collect();
    let doc_kinds: Vec<(String, String)> = trait_tables[1]
        .rows
        .iter()
        .map(|row| {
            (
                extract_backticked_tokens(&row[0]).remove(0),
                extract_backticked_tokens(&row[1]).remove(0),
            )
        })
        .collect();
    assert_eq!(
        doc_kinds, expected_kinds,
        "docs/plugins/basics.md ContributionKind table drifted from ContributionKind::ALL"
    );

    let doc_errors: Vec<String> = trait_tables[2]
        .rows
        .iter()
        .map(|row| extract_backticked_tokens(&row[0]).remove(0))
        .collect();
    assert_key_parity(
        "docs/plugins/basics.md PluginError variants",
        &extract_enum_variants(&error_src, "PluginError"),
        &doc_errors,
    );

    // 2. `## Services` table vs `pub trait` in `service.rs` (excluding root `Service`).
    let service_tables = parse_markdown_tables(extract_heading_section(&doc, "## Services"));
    assert_eq!(service_tables.len(), 1);
    let doc_services: Vec<String> = service_tables[0]
        .rows
        .iter()
        .map(|row| extract_backticked_tokens(&row[0]).remove(0))
        .collect();
    let expected_services: Vec<String> = extract_pub_traits(&service_src)
        .into_iter()
        .filter(|name| name != "Service")
        .collect();
    assert_key_parity(
        "docs/plugins/basics.md Service traits",
        &expected_services,
        &doc_services,
    );

    // 3. `## Logging` tables: LogLevel::ALL and LogLine fields.
    let log_tables = parse_markdown_tables(extract_heading_section(&doc, "## Logging"));
    assert_eq!(log_tables.len(), 3);
    let expected_levels: Vec<(String, String)> = aria_plugin::LogLevel::ALL
        .iter()
        .map(|lvl| match lvl {
            aria_plugin::LogLevel::Trace
            | aria_plugin::LogLevel::Debug
            | aria_plugin::LogLevel::Info
            | aria_plugin::LogLevel::Warn
            | aria_plugin::LogLevel::Error => (format!("{lvl:?}"), format!("{:?}", lvl.as_str())),
        })
        .collect();
    let doc_levels: Vec<(String, String)> = log_tables[1]
        .rows
        .iter()
        .map(|row| {
            (
                extract_backticked_tokens(&row[0]).remove(0),
                extract_backticked_tokens(&row[1]).remove(0),
            )
        })
        .collect();
    assert_eq!(
        doc_levels, expected_levels,
        "docs/plugins/basics.md LogLevel table drifted from LogLevel::ALL"
    );
    let log_type_map = extract_type_members_map(&log_tables[2]);
    assert_key_parity(
        "docs/plugins/basics.md LogLine fields",
        &extract_struct_pub_fields(&log_src, "LogLine"),
        log_type_map.get("LogLine").expect("LogLine row"),
    );
}

#[test]
fn commands_menus_and_analyzers_docs_match_context_key_menu_location_and_analyzer_scope() {
    // 1. `ContextKey::ALL` in `docs/plugins/commands.md` `## Enablement`.
    let commands_doc = read_workspace_file("docs/plugins/commands.md");
    let enablement_tables =
        parse_markdown_tables(extract_heading_section(&commands_doc, "## Enablement"));
    assert_eq!(enablement_tables.len(), 1);
    let doc_context_keys: Vec<String> = enablement_tables[0]
        .rows
        .iter()
        .map(|row| extract_backticked_tokens(&row[0]).remove(0))
        .collect();
    let expected_context_keys: Vec<String> = aria_plugin::ContextKey::ALL
        .iter()
        .map(|key| match key {
            aria_plugin::ContextKey::Clips
            | aria_plugin::ContextKey::Selection
            | aria_plugin::ContextKey::SingleSelection
            | aria_plugin::ContextKey::MarkIn
            | aria_plugin::ContextKey::MarkOut
            | aria_plugin::ContextKey::InOut
            | aria_plugin::ContextKey::Playing
            | aria_plugin::ContextKey::FocusMedia
            | aria_plugin::ContextKey::FocusViewer
            | aria_plugin::ContextKey::FocusTimeline
            | aria_plugin::ContextKey::FocusInspector
            | aria_plugin::ContextKey::FocusAssistant
            | aria_plugin::ContextKey::FocusMixer => key.name().to_owned(),
        })
        .collect();
    assert_eq!(
        doc_context_keys, expected_context_keys,
        "docs/plugins/commands.md Enablement table drifted from ContextKey::ALL"
    );

    // 2. `MenuLocation::ALL` in `docs/plugins/menus.md`.
    let menus_doc = read_workspace_file("docs/plugins/menus.md");
    let (_, after_starts) = menus_doc
        .split_once("A path starts with a menu-bar menu (")
        .expect("menu-bar menu list in docs/plugins/menus.md");
    let (bar_part, after_bar) = after_starts
        .split_once(") or a context menu (")
        .expect("context menu list in docs/plugins/menus.md");
    let (ctx_part, _) = after_bar
        .split_once(") from")
        .expect("closing paren of context menu list in docs/plugins/menus.md");
    let doc_bar_menus = extract_backticked_tokens(bar_part);
    let doc_ctx_menus = extract_backticked_tokens(ctx_part);
    let expected_bar_menus: Vec<String> = aria_plugin::MenuLocation::ALL
        .iter()
        .filter(|loc| loc.in_menu_bar())
        .map(|loc| loc.label().to_owned())
        .collect();
    let expected_ctx_menus: Vec<String> = aria_plugin::MenuLocation::ALL
        .iter()
        .filter(|loc| !loc.in_menu_bar())
        .map(|loc| loc.label().to_owned())
        .collect();
    assert_eq!(
        doc_bar_menus, expected_bar_menus,
        "docs/plugins/menus.md menu-bar locations drifted from MenuLocation::ALL"
    );
    assert_eq!(
        doc_ctx_menus, expected_ctx_menus,
        "docs/plugins/menus.md context menu locations drifted from MenuLocation::ALL"
    );

    // 3. `AnalyzerScope::ALL` in `docs/plugins/analyzers.md` `## Scope and menus`.
    let analyzers_doc = read_workspace_file("docs/plugins/analyzers.md");
    let scope_tables =
        parse_markdown_tables(extract_heading_section(&analyzers_doc, "## Scope and menus"));
    assert_eq!(scope_tables.len(), 1);
    let doc_scopes: Vec<(String, String)> = scope_tables[0]
        .rows
        .iter()
        .map(|row| {
            (
                extract_backticked_tokens(&row[0]).remove(0),
                row[1].trim().to_owned(),
            )
        })
        .collect();
    let expected_scopes: Vec<(String, String)> = aria_plugin::AnalyzerScope::ALL
        .iter()
        .map(|scope| {
            let menu = match scope {
                aria_plugin::AnalyzerScope::Clip => "Clip > Analyze",
                aria_plugin::AnalyzerScope::Sequence | aria_plugin::AnalyzerScope::InOut => {
                    "Sequence > Analyze"
                }
            };
            (format!("{scope:?}"), menu.to_owned())
        })
        .collect();
    assert_eq!(
        doc_scopes, expected_scopes,
        "docs/plugins/analyzers.md Scope and menus table drifted from AnalyzerScope::ALL"
    );
}
