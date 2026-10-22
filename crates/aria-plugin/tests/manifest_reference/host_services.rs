//! Reference documentation checks for `docs/plugins/activation.md`,
//! `docs/plugins/ai-tools.md`, `docs/plugins/jobs.md`, `docs/plugins/scopes.md`,
//! `docs/plugins/interchange.md`, and `docs/plugins/project.md`.

use super::*;

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
    assert_eq!(dec_tables.len(), 3);

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

    // Native decoders: the `FrameFormat` table lists every SDK pixel layout.
    let sdk_decoders_src = read_workspace_file("crates/aria-sdk/src/decoders.rs");
    let doc_formats: Vec<String> = dec_tables[2]
        .rows
        .iter()
        .map(|row| extract_backticked_tokens(&row[0]).remove(0))
        .collect();
    assert_key_parity(
        "interchange.md aria_sdk::FrameFormat variants",
        &extract_enum_variants(&sdk_decoders_src, "FrameFormat"),
        &doc_formats,
    );
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
