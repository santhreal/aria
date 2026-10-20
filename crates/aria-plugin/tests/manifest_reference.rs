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
