//! Reference documentation checks for `docs/plugins/basics.md`,
//! `docs/plugins/keys.md`, `docs/plugins/commands.md`, `docs/plugins/menus.md`,
//! `docs/plugins/analyzers.md`, `docs/plugins/panels.md`, and
//! `docs/plugins/settings.md`.

use aria_plugin::package::PackageManifest;

use super::*;

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

/// The backticked tokens of the text in `markdown` that follows `intro` and
/// ends at `end`, with line breaks read as spaces.
fn listed_after(markdown: &str, intro: &str, end: &str) -> Vec<String> {
    let flat = markdown.split_whitespace().collect::<Vec<_>>().join(" ");
    let (_, after) = flat
        .split_once(intro)
        .unwrap_or_else(|| panic!("no text `{intro}` in the chapter"));
    let (list, _) = after
        .split_once(end)
        .unwrap_or_else(|| panic!("no `{end}` after `{intro}` in the chapter"));
    extract_backticked_tokens(list)
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
    let scope_tables = parse_markdown_tables(extract_heading_section(
        &analyzers_doc,
        "## Scope and menus",
    ));
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

/// `docs/plugins/panels.md` states that the `aria-sdk` `UiNode`, `PanelEvent`,
/// and `Tone` have the variants of the in-process types, lists the SDK
/// `PanelLocation` variants, and lists every `Tone`.
#[test]
fn panels_doc_states_sdk_panel_variants_and_tones() {
    let doc = read_workspace_file("docs/plugins/panels.md");
    let sdk_src = read_workspace_file("crates/aria-sdk/src/panels.rs");
    let host_src = read_workspace_file("crates/aria-plugin/src/ui_tree.rs");
    for ty in ["UiNode", "PanelEvent", "Tone"] {
        assert_key_parity(
            &format!("aria_sdk::{ty} against aria_plugin::{ty}"),
            &extract_enum_variants(&host_src, ty),
            &extract_enum_variants(&sdk_src, ty),
        );
    }
    assert_key_parity(
        "docs/plugins/panels.md SDK PanelLocation",
        &extract_enum_variants(&sdk_src, "PanelLocation"),
        &listed_after(&doc, "the SDK `PanelLocation` is ", "."),
    );
    assert_key_parity(
        "docs/plugins/panels.md Tone",
        &extract_enum_variants(&host_src, "Tone"),
        &listed_after(&doc, "A `Tone` is ", ", drawn"),
    );
}

/// `docs/plugins/settings.md`: the kind table lists each `SettingKind`
/// variant, and each `plugin.toml` `type` value the chapter lists parses
/// through `PackageManifest::parse` to a distinct variant that
/// `SettingKind::label` spells the same way. A new variant fails until the
/// chapter lists its row and its `type` spelling.
#[test]
fn settings_doc_matches_setting_kind_variants_and_plugin_toml_types() {
    let doc = read_workspace_file("docs/plugins/settings.md");
    let src = read_workspace_file("crates/aria-plugin/src/settings_schema.rs");
    let variants = extract_enum_variants(&src, "SettingKind");
    let tables = parse_markdown_tables(&doc);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].headers, ["Kind", "Row", "Stored as"]);
    let doc_kinds: Vec<String> = tables[0]
        .rows
        .iter()
        .map(|row| leading_ident(&extract_backticked_tokens(&row[0]).remove(0)))
        .collect();
    assert_key_parity(
        "docs/plugins/settings.md SettingKind rows",
        &variants,
        &doc_kinds,
    );
    let parsed_variants: Vec<String> = listed_after(&doc, "`type` is ", ".")
        .into_iter()
        .map(|type_name| {
            let options = if type_name == "choice" {
                "options = [\"a\"]\n"
            } else {
                ""
            };
            let manifest = format!(
                "{MINIMAL_PLUGIN_HEADER}\n[[settings]]\nkey = \"k\"\nlabel = \"K\"\ntype = \"{type_name}\"\n{options}"
            );
            let parsed = PackageManifest::parse(&manifest).unwrap_or_else(|err| {
                panic!("docs/plugins/settings.md `type = \"{type_name}\"` failed to parse: {err}")
            });
            let [setting] = parsed.settings.as_slice() else {
                panic!("`type = \"{type_name}\"` parsed to {} settings", parsed.settings.len());
            };
            assert_eq!(
                setting.kind.label(),
                type_name,
                "SettingKind::label spells `type = \"{type_name}\"` differently"
            );
            leading_ident(&format!("{:?}", setting.kind))
        })
        .collect();
    assert_key_parity(
        "docs/plugins/settings.md plugin.toml `type` values",
        &variants,
        &parsed_variants,
    );
}
