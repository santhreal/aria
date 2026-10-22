//! Reference documentation checks for `docs/plugins/packages.md`,
//! `docs/plugins/presets.md`, and `docs/plugins/workspaces.md`.

use std::fmt;

use aria_models::{
    AudioEffectInstance, EffectBounds, EffectChoiceOption, EffectCurvePoint, EffectGradientStop,
    EffectGroupSpec, EffectInstance, EffectParamSpec, EffectVisibilityCondition, Keyframe, Preset,
    PresetProperty, SpatialTangents,
};
use aria_plugin::package::PackageManifest;
use serde::de::{self, Deserialize, Deserializer, Visitor};

use super::*;

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
// 1. `plugin.toml` reference in `docs/plugins/packages.md`
// ---------------------------------------------------------------------------

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
