//! Every `const ALL` registry in the workspace has a recorded documentation
//! decision: a reference table with a bidirectional doc test, or an
//! internal-only rationale.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"));
    let mut paths: Vec<PathBuf> = entries.map(|e| e.unwrap().path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn is_const_all_decl(trimmed: &str) -> bool {
    let rest = trimmed
        .strip_prefix("pub const ALL")
        .or_else(|| trimmed.strip_prefix("pub(crate) const ALL"));
    matches!(rest.and_then(|s| s.chars().next()), Some(':' | ' ' | '\t'))
}

fn extract_impl_type_name(trimmed: &str) -> Option<&str> {
    if !(trimmed.starts_with("impl ") || trimmed.starts_with("impl<")) || !trimmed.ends_with('{') {
        return None;
    }
    let before_brace = trimmed[..trimmed.len() - 1].trim();
    let last = before_brace.split_whitespace().last()?;
    Some(last.split('<').next().unwrap_or(last))
}

fn scan_workspace_const_all_registries(root: &Path) -> Vec<(String, usize, String)> {
    let mut rs_files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut rs_files);
    let mut results = Vec::new();
    for rs in rs_files {
        let rel = rs
            .strip_prefix(root)
            .unwrap_or(&rs)
            .to_string_lossy()
            .replace('\\', "/");
        if !rel.contains("/src/") {
            continue;
        }
        let fname = rs.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if fname == "tests.rs" || fname.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&rs).unwrap_or_else(|e| panic!("read {rs:?}: {e}"));
        let lines: Vec<&str> = text.lines().collect();
        let mut in_param_enum_call = false;
        for (idx0, &line) in lines.iter().enumerate() {
            let line_no = idx0 + 1;
            if line.starts_with("param_enum! {") {
                in_param_enum_call = true;
                continue;
            }
            if in_param_enum_call {
                if line.starts_with('}') {
                    in_param_enum_call = false;
                } else {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("pub enum ")
                        && let Some(name) = rest.strip_suffix('{')
                    {
                        results.push((rel.clone(), line_no, name.trim().to_string()));
                    }
                }
                continue;
            }
            let trimmed = line.trim();
            if is_const_all_decl(trimmed) {
                let mut enclosing = None;
                for back in (0..idx0).rev() {
                    if let Some(ty) = extract_impl_type_name(lines[back].trim()) {
                        enclosing = Some(ty);
                        break;
                    }
                }
                if let Some(ty) = enclosing
                    && !ty.starts_with('$')
                {
                    results.push((rel.clone(), line_no, ty.to_string()));
                }
            }
        }
    }
    results
}

fn collect_workspace_test_fn_bodies(root: &Path) -> BTreeMap<String, Vec<String>> {
    let mut rs_files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut rs_files);
    let mut bodies: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rs in rs_files {
        let text = std::fs::read_to_string(&rs).unwrap_or_else(|e| panic!("read {rs:?}: {e}"));
        let lines: Vec<&str> = text.lines().collect();
        let mut test_fn_starts: Vec<(usize, String)> = Vec::new();
        let mut saw_test_attr = false;
        for (i, &line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("#[test]") {
                saw_test_attr = true;
                continue;
            }
            if saw_test_attr {
                if trimmed.starts_with("#[") || trimmed.starts_with("//") || trimmed.is_empty() {
                    continue;
                }
                if let Some((_, after_fn)) = trimmed.split_once("fn ")
                    && let Some((fn_name, _)) = after_fn.split_once('(')
                {
                    test_fn_starts.push((i, fn_name.trim().to_string()));
                }
                saw_test_attr = false;
            }
        }
        for (idx, (start_line, fn_name)) in test_fn_starts.iter().enumerate() {
            let end_line = test_fn_starts
                .get(idx + 1)
                .map(|(next_start, _)| *next_start)
                .unwrap_or(lines.len());
            let body = lines[*start_line..end_line].join("\n");
            bodies.entry(fn_name.clone()).or_default().push(body);
        }
    }
    bodies
}

/// Closes the defect class where a `pub const ALL` or `pub(crate) const ALL`
/// registry is added to `crates/*/src` without either a reference documentation
/// table + bidirectional doc test (`DOCUMENTED`) or an explicit internal-only
/// rationale (`INTERNAL`), or where an entry in either table goes stale after a
/// registry is renamed or removed.
///
/// Does not catch registries that do not use the `const ALL` naming convention.
#[test]
fn const_all_registries_have_recorded_documentation_decisions() {
    const DOCUMENTED: &[(&str, &str, &str)] = &[
        // CLI
        (
            "Verb",
            "docs/command-line.md",
            "command_line_doc_and_usage_match_cli_registries_bidirectionally",
        ),
        (
            "Subcommand",
            "docs/command-line.md",
            "command_line_doc_and_usage_match_cli_registries_bidirectionally",
        ),
        (
            "NewTier",
            "docs/command-line.md",
            "command_line_doc_and_usage_match_cli_registries_bidirectionally",
        ),
        // Plugins
        (
            "ActivationKind",
            "docs/plugins/activation.md",
            "activation_doc_matches_activation_registries_bidirectionally",
        ),
        (
            "AiToolTier",
            "docs/plugins/ai-tools.md",
            "ai_tools_doc_matches_ai_tool_registries_bidirectionally",
        ),
        (
            "AnalyzerScope",
            "docs/plugins/analyzers.md",
            "commands_menus_and_analyzers_docs_match_context_key_menu_location_and_analyzer_scope",
        ),
        (
            "ContextKey",
            "docs/plugins/commands.md",
            "commands_menus_and_analyzers_docs_match_context_key_menu_location_and_analyzer_scope",
        ),
        (
            "WireKind",
            "docs/plugins/events.md",
            "the_events_chapter_lists_every_kind_with_its_payload_fields",
        ),
        (
            "KeyRoute",
            "docs/plugins/keys.md",
            "keys_doc_matches_plugin_key_registries_bidirectionally",
        ),
        (
            "After",
            "docs/plugins/keys.md",
            "plugin_keys_and_basics_docs_match_ui_registries_bidirectionally",
        ),
        (
            "MenuLocation",
            "docs/plugins/menus.md",
            "commands_menus_and_analyzers_docs_match_context_key_menu_location_and_analyzer_scope",
        ),
        (
            "LogLevel",
            "docs/plugins/basics.md",
            "basics_doc_matches_plugin_basics_registries_bidirectionally",
        ),
        (
            "ContributionKind",
            "docs/plugins/basics.md",
            "basics_doc_matches_plugin_basics_registries_bidirectionally",
        ),
        // Reference: Scopes, Color, Transitions
        (
            "WaveformTrace",
            "docs/reference/scopes.md",
            "scopes_doc_matches_registries",
        ),
        (
            "YuvMatrix",
            "docs/reference/color.md",
            "color_doc_yuv_matrices_match_coefficients",
        ),
        (
            "WheelControl",
            "docs/reference/color.md",
            "color_doc_matches_registries",
        ),
        (
            "AudioTransition",
            "docs/reference/transitions.md",
            "transitions_reference_matches_shipped_catalog",
        ),
        (
            "TransitionAlignment",
            "docs/reference/transitions.md",
            "transitions_reference_matches_shipped_catalog",
        ),
        // Reference: Blend modes, Masks, Clip properties
        (
            "BlendMode",
            "docs/reference/blend-modes.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "MaskKind",
            "docs/reference/blend-modes.md",
            "blend_modes_and_masks_reference_matches_catalog",
        ),
        (
            "MaskMode",
            "docs/reference/blend-modes.md",
            "stickers_and_masks_reference_matches_catalogs",
        ),
        (
            "MaskOp",
            "docs/reference/blend-modes.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "MaskHandle",
            "docs/reference/blend-modes.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "MaskParam",
            "docs/reference/stickers-and-masks.md",
            "stickers_and_masks_reference_matches_catalogs",
        ),
        (
            "ShapeParam",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "TextParam",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "LookParam",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "StyleParam",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "ParamAxis",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "ClipAttribute",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "TextCase",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "TextAlignH",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "TextAlignV",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "StabilizeLevel",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "AlignTo",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "AlignEdge",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "AlignAxis",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "CropHandle",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        (
            "TransformHandle",
            "docs/reference/clip-properties.md",
            "clip_properties_doc_matches_registries",
        ),
        // Reference: Keyframes and Graph
        (
            "PathParam",
            "docs/reference/keyframes-and-graph.md",
            "keyframes_and_graph_doc_matches_registries",
        ),
        (
            "TangentSide",
            "docs/reference/keyframes-and-graph.md",
            "keyframes_and_graph_doc_matches_registries",
        ),
        (
            "Group",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "PropertyGroup",
            "docs/reference/keyframes-and-graph.md",
            "keyframes_and_graph_doc_matches_registries",
        ),
        // Reference: Project Settings
        (
            "LabelColor",
            "docs/reference/project-settings.md",
            "project_settings_doc_matches_registries",
        ),
        (
            "AudioRole",
            "docs/reference/project-settings.md",
            "project_settings_doc_matches_registries",
        ),
        (
            "SourceChannel",
            "docs/reference/project-settings.md",
            "project_settings_doc_matches_registries",
        ),
        (
            "MarkerColor",
            "docs/reference/project-settings.md",
            "project_settings_doc_matches_registries",
        ),
        (
            "CanvasAspect",
            "docs/reference/project-settings.md",
            "project_settings_doc_matches_registries",
        ),
        (
            "AudioLayout",
            "docs/reference/project-settings.md",
            "project_settings_doc_matches_registries",
        ),
        (
            "TimeDisplay",
            "docs/reference/project-settings.md",
            "project_settings_doc_matches_registries",
        ),
        (
            "MarkerKind",
            "docs/reference/project-settings.md",
            "project_settings_doc_matches_registries",
        ),
        (
            "AutomationMode",
            "docs/reference/project-settings.md",
            "project_settings_doc_matches_registries",
        ),
        // Reference: Export and Subtitles
        (
            "VideoCodec",
            "docs/reference/export-and-subtitles.md",
            "export_and_subtitles_doc_matches_registries",
        ),
        (
            "EmptyExport",
            "docs/reference/export-and-subtitles.md",
            "export_and_subtitles_doc_matches_registries",
        ),
        (
            "CaptionCommit",
            "docs/reference/export-and-subtitles.md",
            "export_and_subtitles_doc_matches_registries",
        ),
        (
            "ExportScale",
            "docs/reference/export-and-subtitles.md",
            "export_and_subtitles_doc_matches_registries",
        ),
        (
            "ExportRate",
            "docs/reference/export-and-subtitles.md",
            "export_and_subtitles_doc_matches_registries",
        ),
        (
            "ExportGop",
            "docs/reference/export-and-subtitles.md",
            "export_and_subtitles_doc_matches_registries",
        ),
        (
            "ExportBitrate",
            "docs/reference/export-and-subtitles.md",
            "export_and_subtitles_doc_matches_registries",
        ),
        // Reference: Tools, Panels, Menus, Settings
        (
            "ToolGroup",
            "docs/reference/tools.md",
            "tools_doc_matches_registry",
        ),
        (
            "PenVariant",
            "docs/reference/tools.md",
            "tools_doc_matches_registry",
        ),
        (
            "SidebarTab",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "SidebarGroup",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "UiClipKind",
            "docs/reference/panels.md",
            "the_panels_reference_lists_every_tab_with_its_clip_kinds",
        ),
        (
            "MediaSort",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "OverlayMode",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "ActivePanel",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "AgentEntryKind",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "EntryKind",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "CloudFeed",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "GenerateKind",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "AnalysisKind",
            "docs/reference/panels.md",
            "panels_doc_matches_registry",
        ),
        (
            "AppMenu",
            "docs/reference/menus.md",
            "menus_doc_matches_registry",
        ),
        (
            "AudioGainMode",
            "docs/reference/menus.md",
            "menus_doc_matches_registry",
        ),
        (
            "AudioChannels",
            "docs/reference/menus.md",
            "menus_doc_matches_registry",
        ),
        (
            "MatteSource",
            "docs/reference/menus.md",
            "menus_doc_matches_registry",
        ),
        (
            "Decline",
            "docs/reference/menus.md",
            "menus_doc_matches_registry",
        ),
        (
            "AppearanceMode",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "MotionPreference",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "PoolView",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "Resolution",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "PlaybackResolution",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "PausedResolution",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "WaveformMode",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "ThemeChoice",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "AiSource",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "AiApiProtocol",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "ReasoningSummary",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "Autonomy",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "Refusal",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "CacheId",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "SettingsField",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "TimeoutField",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "WasmLimit",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "DropCause",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
        (
            "OutputMode",
            "docs/reference/settings.md",
            "settings_doc_matches_registries",
        ),
    ];

    const INTERNAL: &[(&str, &str)] = &[
        (
            "FixtureId",
            "benchmark fixture identifier internal to aria-bench",
        ),
        (
            "ScenarioId",
            "benchmark scenario identifier internal to aria-bench",
        ),
        (
            "HttpCaller",
            "internal HTTP timeout caller tag in aria-cloud",
        ),
        (
            "CoverageMix",
            "internal test-only coverage generator mix in aria-media",
        ),
        (
            "Rotation",
            "container display-matrix orientation applied automatically on decode",
        ),
        (
            "WindowReason",
            "internal whisper windowing split reason in aria-speech",
        ),
        (
            "ChipSize",
            "internal UI widget size token in aria-ui::components",
        ),
        (
            "LabelTarget",
            "internal context-menu colour-label dispatch target in aria-ui::app",
        ),
        (
            "TextSwitch",
            "internal boolean toggle tag for inspector text property buttons",
        ),
        (
            "CommandKind",
            "internal discriminator between state and job commands in aria-ui::commands",
        ),
        (
            "Modifier",
            "internal keyboard modifier bitmask order for chord formatting in aria-ui::keys",
        ),
        (
            "Zone",
            "internal dock tab drag drop-target quadrant in aria-ui::dock",
        ),
        (
            "Moved",
            "internal shell redraw invalidation tag in aria-ui::shell",
        ),
        (
            "SnapKind",
            "internal magnetic snap candidate tie-break rank in aria-ui::snap",
        ),
    ];

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root");
    let discovered = scan_workspace_const_all_registries(&root);
    assert!(
        discovered.len() >= 110,
        "expected at least 110 `const ALL` declarations across crates/*/src, found {}",
        discovered.len()
    );

    let mut documented_map: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for &(ty, doc_file, test_fn) in DOCUMENTED {
        assert!(
            documented_map.insert(ty, (doc_file, test_fn)).is_none(),
            "duplicate entry `{ty}` in DOCUMENTED table"
        );
    }

    let mut internal_map: BTreeMap<&str, &str> = BTreeMap::new();
    for &(ty, reason) in INTERNAL {
        assert!(
            !reason.trim().is_empty(),
            "INTERNAL entry `{ty}` must state a non-empty rationale"
        );
        assert!(
            !documented_map.contains_key(ty),
            "type `{ty}` appears in both DOCUMENTED and INTERNAL"
        );
        assert!(
            internal_map.insert(ty, reason).is_none(),
            "duplicate entry `{ty}` in INTERNAL table"
        );
    }

    let mut undecided = Vec::new();
    let mut discovered_types = BTreeSet::new();
    for (file_rel, line_no, ty) in &discovered {
        discovered_types.insert(ty.as_str());
        if !documented_map.contains_key(ty.as_str()) && !internal_map.contains_key(ty.as_str()) {
            undecided.push(format!("{file_rel}:{line_no} ({ty}::ALL)"));
        }
    }
    assert!(
        undecided.is_empty(),
        "undecided `const ALL` registries found in crates/*/src (record each in DOCUMENTED with a reference doc + bidirectional doc test, or in INTERNAL with a rationale):\n  {}",
        undecided.join("\n  ")
    );

    let mut stale = Vec::new();
    for &ty in documented_map.keys().chain(internal_map.keys()) {
        if !discovered_types.contains(ty) {
            stale.push(ty);
        }
    }
    assert!(
        stale.is_empty(),
        "stale entries in DOCUMENTED / INTERNAL no longer match any `const ALL` in crates/*/src: {stale:?}"
    );

    let test_fn_bodies = collect_workspace_test_fn_bodies(&root);
    let mut doc_cache: BTreeMap<&str, String> = BTreeMap::new();
    let mut verification_errors = Vec::new();
    for &(ty, doc_file, test_fn) in DOCUMENTED {
        let doc_text = doc_cache.entry(doc_file).or_insert_with(|| {
            let p = root.join(doc_file);
            std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("DOCUMENTED[{ty}] missing doc file {p:?}: {e}"))
        });
        if standalone(doc_text, ty).next().is_none() {
            verification_errors.push(format!(
                "DOCUMENTED[{ty}]: `{doc_file}` does not mention `{ty}` as a standalone identifier"
            ));
        }
        let needle = format!("{ty}::ALL");
        match test_fn_bodies.get(test_fn) {
            None => verification_errors.push(format!(
                "DOCUMENTED[{ty}]: no #[test] fn `{test_fn}` found in crates/"
            )),
            Some(bodies)
                if !bodies
                    .iter()
                    .any(|b| standalone(b, &needle).any(|end| !b[end..].starts_with(".len()"))) =>
            {
                verification_errors.push(format!(
                    "DOCUMENTED[{ty}]: #[test] fn `{test_fn}` does not iterate or match `{needle}` (a standalone `{needle}` reference not followed by `.len()` is required)"
                ));
            }
            Some(_) => {}
        }
    }
    assert!(
        verification_errors.is_empty(),
        "DOCUMENTED registry verification failed:\n  {}",
        verification_errors.join("\n  ")
    );
}

/// The end of each occurrence of `ident` in `haystack` that is not part of a
/// longer identifier.
fn standalone<'a>(haystack: &'a str, ident: &'a str) -> impl Iterator<Item = usize> + 'a {
    let bytes = haystack.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    haystack.match_indices(ident).filter_map(move |(at, _)| {
        let end = at + ident.len();
        let alone = (at == 0 || !word(bytes[at - 1])) && bytes.get(end).is_none_or(|&b| !word(b));
        alone.then_some(end)
    })
}
