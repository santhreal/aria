//! Reference documentation checks for `docs/plugins/effects.md`,
//! `docs/plugins/audio-effects.md`, and `docs/theme-tokens.md`.

use aria_compositor::{EffectDefinition, TransitionDefinition};
use aria_models::{
    EffectParamKind, EffectParamSpec, EffectSpec, GraphPass, PassStage, TransitionSpec,
};
use aria_plugin::PluginContext;

use super::*;

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

/// `docs/plugins/audio-effects.md`: the `AudioBlock` call table lists each
/// `pub fn` of `aria_render::AudioBlock`, and its `param` row states each
/// `ParamSignal` variant and method.
#[test]
fn audio_effects_doc_matches_audio_block_and_param_signal() {
    let doc = read_workspace_file("docs/plugins/audio-effects.md");
    let src = read_workspace_file("crates/aria-render/src/audio_processor.rs");
    let tables = parse_markdown_tables(extract_heading_section(&doc, "## Processing"));
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].headers, ["`AudioBlock` call", "Result"]);
    assert_key_parity(
        "docs/plugins/audio-effects.md AudioBlock calls",
        &extract_impl_pub_fns(&src, "AudioBlock"),
        &extract_call_names_from_table(&tables[0]),
    );
    let param_row = tables[0]
        .rows
        .iter()
        .find(|row| row[0] == "`param(slot)`")
        .expect("`param(slot)` row");
    let stated = markdown_code_identifiers(&param_row[1]);
    let mut signal_members = extract_enum_variants(&src, "ParamSignal");
    signal_members.extend(extract_impl_pub_fns(&src, "ParamSignal"));
    let missing: Vec<&String> = signal_members
        .iter()
        .filter(|member| !stated.contains(*member))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/plugins/audio-effects.md `param(slot)` row omits ParamSignal {missing:?}"
    );
}

/// `docs/plugins/effects.md` states the `Curve` and `Gradient` lookup table
/// sizes, and `aria_plugin` and `aria_sdk` define the same sizes.
#[test]
fn effects_doc_states_lut_sample_counts_of_both_tiers() {
    let doc = read_workspace_file("docs/plugins/effects.md");
    let curve = aria_plugin::EFFECT_CURVE_LUT_SAMPLES;
    let gradient = aria_plugin::EFFECT_GRADIENT_LUT_SAMPLES;
    assert_eq!(aria_sdk::EFFECT_CURVE_LUT_SAMPLES, curve);
    assert_eq!(aria_sdk::EFFECT_GRADIENT_LUT_SAMPLES, gradient);
    for row in [
        format!("| `Curve` | `array<f32, {curve}>` ({curve}-sample LUT over `0..1`) | {curve} |"),
        format!(
            "| `Gradient` | `array<vec4<f32>, {gradient}>` ({gradient}-sample linear RGBA LUT over `0..1`) | {} |",
            gradient * 4
        ),
    ] {
        assert!(
            doc.contains(&row),
            "docs/plugins/effects.md has no parameter row `{row}`"
        );
    }
}

/// `docs/plugins/effects.md` states every `EffectParamKind` in its parameter
/// table once, and no other, with the storage floats `shader_floats` gives it.
#[test]
fn effects_doc_param_table_matches_effect_param_kinds_bidirectionally() {
    let doc = read_workspace_file("docs/plugins/effects.md");
    let tables = parse_markdown_tables(extract_heading_section(&doc, "## Shader source"));
    let table = tables
        .iter()
        .find(|table| {
            table
                .headers
                .first()
                .is_some_and(|header| header.contains("EffectParamKind"))
        })
        .expect("docs/plugins/effects.md ## Shader source has an `EffectParamKind` table");
    let mut stated = Vec::new();
    for row in &table.rows {
        let floats: usize = row[2].parse().unwrap_or_else(|_| {
            panic!(
                "docs/plugins/effects.md: storage floats `{}` of `{}` is not a count",
                row[2], row[0]
            )
        });
        stated.extend(
            extract_backticked_tokens(&row[0])
                .into_iter()
                .map(|name| (name, floats)),
        );
    }
    let shipped: Vec<(String, usize)> = EffectParamKind::ALL
        .iter()
        .map(|kind| (format!("{kind:?}"), kind.shader_floats()))
        .collect();
    let names = |pairs: &[(String, usize)]| -> Vec<String> {
        pairs.iter().map(|(name, _)| name.clone()).collect()
    };
    assert_key_parity(
        "docs/plugins/effects.md EffectParamKind",
        &names(&shipped),
        &names(&stated),
    );
    for (name, floats) in &shipped {
        let doc_floats = stated
            .iter()
            .find_map(|(doc_name, count)| (doc_name == name).then_some(*count))
            .expect("every kind is stated");
        assert_eq!(
            doc_floats, *floats,
            "docs/plugins/effects.md: `{name}` takes {floats} storage floats, the table states {doc_floats}"
        );
    }
}
