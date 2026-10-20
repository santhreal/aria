//! Reference documentation coverage checks for tools, workspaces, panels,
//! menus, palette commands, settings, and documentation cross-links.
//!
//! Each test compares the markdown tables in `docs/reference/` against the
//! runtime registries in `aria-ui` and `aria-settings` in both directions:
//! adding a tool, workspace, panel, overlay, sidebar tab, Home rail view,
//! menu item, palette command, or persisted setting without documenting it
//! fails the suite, and documenting a dead or renamed identifier fails the
//! suite.
//!
//! Not covered: whether the prose summary column in a table describes the
//! handler body accurately beyond its structured fields (ID, label, group,
//! shortcut, submenu, default value, and bounds).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use aria_models::{AudioLayout, CanvasAspect, CanvasSettings, TimeDisplay};
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::project_settings::CANVAS_BASELINES;
use crate::appearance::AppearanceMode;
use crate::chrome::menu_items::items_for;
use crate::cloud::BYOK_PROVIDERS;
use crate::commands::{BUILTIN_COMMANDS, BUILTIN_JOB_COMMANDS};
use crate::dock::DOCK_VERSION;
use crate::layouts::WORKSPACES;
use crate::motion::MotionPreference;
use crate::plugins::UiPlugin;
use crate::plugins::{overlay_host, panel_host};
use crate::resolution::{PausedResolution, PlaybackResolution};
use crate::scopes::WaveformMode;
use crate::settings::{
    INSPECTOR_MAX, INSPECTOR_MIN, OUTLINE_MAX, OUTLINE_MIN, SIDEBAR_MAX, SIDEBAR_MIN,
    TIMELINE_MIN_HEIGHT, UiSettings, WindowPlacement,
};
use crate::surfaces::settings_pick::RATES;
use crate::timeline_display::{Follow, Thumbnails, TimelineDisplay, WaveformScale};
use crate::tool_catalog::{ToolGroup, tool_specs};
use crate::types::{AppMenu, HomeRail, PenVariant, PoolView, SidebarTab};

const TOOLS_DOC: &str = include_str!("../../../../docs/reference/tools.md");
const WORKSPACES_DOC: &str = include_str!("../../../../docs/reference/workspaces.md");
const PANELS_DOC: &str = include_str!("../../../../docs/reference/panels.md");
const COMMANDS_DOC: &str = include_str!("../../../../docs/reference/commands.md");
const MENUS_DOC: &str = include_str!("../../../../docs/reference/menus.md");
const SETTINGS_DOC: &str = include_str!("../../../../docs/reference/settings.md");

fn strip_ticks(cell: &str) -> &str {
    cell.trim().trim_matches('`').trim()
}

fn section_slice<'a>(doc: &'a str, heading: &str) -> &'a str {
    let marker = format!("## {heading}\n");
    let start = doc
        .find(&marker)
        .unwrap_or_else(|| panic!("missing section `## {heading}`"));
    let rest = &doc[start + marker.len()..];
    match rest.find("\n## ") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

fn parse_table_rows<'a>(section: &'a str) -> Vec<Vec<&'a str>> {
    let mut rows = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.is_empty() {
            continue;
        }
        if cells
            .iter()
            .all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '))
        {
            continue;
        }
        if matches!(cells[0], "ID" | "Variant" | "Key" | "Shape" | "Shortcut") {
            continue;
        }
        rows.push(cells);
    }
    rows
}

/// Closes the class where a timeline tool, toolbar group, or Pen vertex mode is
/// added, renamed, or re-bound in `tool_catalog.rs` / `types_editor.rs` without
/// updating `docs/reference/tools.md`, or where `tools.md` lists a tool ID or
/// attribute that no longer exists.
/// Does not catch inaccurate prose in the Summary column.
#[test]
fn tools_doc_matches_registry() {
    let built_in_rows = parse_table_rows(section_slice(TOOLS_DOC, "Built-in tools"));
    let expected_tools: Vec<(String, String, String, String, String, String, String)> =
        tool_specs()
            .iter()
            .map(|spec| {
                (
                    spec.id.to_string(),
                    spec.label.to_string(),
                    spec.key.to_string(),
                    format!("{:?}", spec.group),
                    spec.body
                        .map(|b| format!("{b:?}"))
                        .unwrap_or_else(|| "none".to_string()),
                    spec.edge
                        .map(|e| format!("{e:?}"))
                        .unwrap_or_else(|| "none".to_string()),
                    spec.track_select
                        .map(|t| format!("{t:?}"))
                        .unwrap_or_else(|| "none".to_string()),
                )
            })
            .collect();
    let documented_tools: Vec<(String, String, String, String, String, String, String)> =
        built_in_rows
            .iter()
            .map(|cells| {
                assert!(
                    cells.len() >= 8,
                    "malformed row in Built-in tools table: {cells:?}"
                );
                (
                    strip_ticks(cells[0]).to_string(),
                    cells[1].to_string(),
                    strip_ticks(cells[2]).to_string(),
                    strip_ticks(cells[3]).to_string(),
                    strip_ticks(cells[4]).to_string(),
                    strip_ticks(cells[5]).to_string(),
                    strip_ticks(cells[6]).to_string(),
                )
            })
            .collect();
    assert_eq!(
        documented_tools, expected_tools,
        "docs/reference/tools.md Built-in tools table drifted from tool_catalog::SHIPPED"
    );

    let group_section = section_slice(TOOLS_DOC, "Tool bar groups");
    for group in ToolGroup::ALL {
        let ids: Vec<&str> = group
            .tools()
            .map(|t| crate::tool_catalog::tool_spec(t).id)
            .collect();
        let line = format!(
            "- `{:?}`: {}",
            group,
            ids.iter()
                .map(|id| format!("`{id}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        assert!(
            group_section.contains(&line),
            "docs/reference/tools.md Tool bar groups missing `{line}`"
        );
    }

    let pen_rows = parse_table_rows(section_slice(TOOLS_DOC, "Pen tool vertex modes"));
    let expected_pen: Vec<(String, String)> = PenVariant::ALL
        .iter()
        .map(|v| (format!("{v:?}"), v.label().to_string()))
        .collect();
    let documented_pen: Vec<(String, String)> = pen_rows
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 3, "malformed PenVariant row: {cells:?}");
            (strip_ticks(cells[0]).to_string(), cells[1].to_string())
        })
        .collect();
    assert_eq!(
        documented_pen, expected_pen,
        "docs/reference/tools.md Pen tool vertex modes drifted from PenVariant::ALL"
    );
}

/// Closes the class where a shipped workspace or its pane geometry changes in
/// `layouts.rs` or `DOCK_VERSION` changes in `dock/mod.rs` without updating
/// `docs/reference/workspaces.md`.
/// Does not catch prose edits in the worked example section.
#[test]
fn workspaces_doc_matches_registry() {
    let rows = parse_table_rows(section_slice(WORKSPACES_DOC, "Shipped workspaces"));
    let expected: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    )> = WORKSPACES
        .iter()
        .map(|ws| {
            (
                ws.id.to_string(),
                ws.name.to_string(),
                format!("{}", ws.panes.sidebar_width as u32),
                format!("{}", ws.panes.sidebar_collapsed),
                format!("{}", ws.panes.inspector_width as u32),
                format!("{}", ws.panes.inspector_collapsed),
                format!("{}", ws.panes.timeline_height as u32),
                format!("{}", ws.panes.scopes_open),
                format!("{:?}", ws.panes.panel),
                format!("{:?}", ws.panes.route),
            )
        })
        .collect();
    let documented: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    )> = rows
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 11, "malformed workspace row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                cells[1].to_string(),
                strip_ticks(cells[3]).to_string(),
                strip_ticks(cells[4]).to_string(),
                strip_ticks(cells[5]).to_string(),
                strip_ticks(cells[6]).to_string(),
                strip_ticks(cells[7]).to_string(),
                strip_ticks(cells[8]).to_string(),
                strip_ticks(cells[9]).to_string(),
                strip_ticks(cells[10]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented, expected,
        "docs/reference/workspaces.md Shipped workspaces table drifted from WORKSPACES"
    );
    assert!(
        WORKSPACES_DOC.contains(&format!("DOCK_VERSION = {DOCK_VERSION}")),
        "docs/reference/workspaces.md must state DOCK_VERSION = {DOCK_VERSION}"
    );
}

/// Closes the class where a built-in panel, modal dialog, overlay, Project
/// sidebar tab, or Home rail view is added or modified without updating
/// `docs/reference/panels.md`.
/// Does not catch prose descriptions in the Summary column.
#[test]
fn panels_doc_matches_registry() {
    let ctx = PluginContext::new();
    ctx.mount(&UiPlugin).expect("mount UiPlugin");
    let panels = panel_host::host_of(&ctx).expect("PanelHost mounted");
    let overlays = overlay_host::host_of(&ctx).expect("OverlayHost mounted");

    let expected_panels: Vec<(String, String, String, String, String, String, String)> = panels
        .ids()
        .into_iter()
        .map(|id| {
            let entry = panels.get(id).expect("panel entry exists");
            let (frame, min_w, min_h, focus) = match entry.dock {
                Some(spec) => (
                    format!("{:?}", spec.default_frame),
                    format!("{}", spec.min_size.0 as u32),
                    format!("{}", spec.min_size.1 as u32),
                    format!("{:?}", spec.focus),
                ),
                None => (
                    "none".to_string(),
                    "none".to_string(),
                    "none".to_string(),
                    "none".to_string(),
                ),
            };
            (
                entry.id.to_string(),
                entry.title.to_string(),
                format!("{:?}", entry.location),
                frame,
                min_w,
                min_h,
                focus,
            )
        })
        .collect();

    let documented_panels: Vec<(String, String, String, String, String, String, String)> =
        parse_table_rows(section_slice(PANELS_DOC, "Built-in panels"))
            .iter()
            .map(|cells| {
                assert!(cells.len() >= 7, "malformed panel row: {cells:?}");
                (
                    strip_ticks(cells[0]).to_string(),
                    cells[1].to_string(),
                    strip_ticks(cells[2]).to_string(),
                    strip_ticks(cells[3]).to_string(),
                    strip_ticks(cells[4]).to_string(),
                    strip_ticks(cells[5]).to_string(),
                    strip_ticks(cells[6]).to_string(),
                )
            })
            .collect();
    assert_eq!(
        documented_panels, expected_panels,
        "docs/reference/panels.md Built-in panels table drifted from PanelHost"
    );

    let expected_overlays: Vec<String> = overlays.ids().into_iter().map(str::to_string).collect();
    let documented_overlays: Vec<String> =
        parse_table_rows(section_slice(PANELS_DOC, "Built-in dialogs and overlays"))
            .iter()
            .map(|cells| strip_ticks(cells[0]).to_string())
            .collect();
    assert_eq!(
        documented_overlays, expected_overlays,
        "docs/reference/panels.md Built-in dialogs and overlays drifted from OverlayHost"
    );

    let expected_tabs: Vec<(String, String, String)> = SidebarTab::ALL
        .iter()
        .map(|tab| {
            (
                format!("{tab:?}"),
                tab.group().label().to_string(),
                tab.label().to_string(),
            )
        })
        .collect();
    let documented_tabs: Vec<(String, String, String)> =
        parse_table_rows(section_slice(PANELS_DOC, "Project panel tabs"))
            .iter()
            .map(|cells| {
                assert!(cells.len() >= 4, "malformed SidebarTab row: {cells:?}");
                (
                    strip_ticks(cells[0]).to_string(),
                    strip_ticks(cells[1]).to_string(),
                    cells[2].to_string(),
                )
            })
            .collect();
    assert_eq!(
        documented_tabs, expected_tabs,
        "docs/reference/panels.md Project panel tabs drifted from SidebarTab::ALL"
    );

    let all_rails = [
        HomeRail::New,
        HomeRail::Open,
        HomeRail::Recents,
        HomeRail::Samples,
        HomeRail::Learn,
    ];
    for rail in all_rails {
        match rail {
            HomeRail::New
            | HomeRail::Open
            | HomeRail::Recents
            | HomeRail::Samples
            | HomeRail::Learn => {}
        }
    }
    let expected_rails: Vec<(String, String)> = all_rails
        .iter()
        .map(|rail| (format!("{rail:?}"), rail.label().to_string()))
        .collect();
    let documented_rails: Vec<(String, String)> =
        parse_table_rows(section_slice(PANELS_DOC, "Home rail items"))
            .iter()
            .map(|cells| {
                assert!(cells.len() >= 3, "malformed HomeRail row: {cells:?}");
                (strip_ticks(cells[0]).to_string(), cells[1].to_string())
            })
            .collect();
    assert_eq!(
        documented_rails, expected_rails,
        "docs/reference/panels.md Home rail items drifted from HomeRail"
    );
}

/// Closes the class where a state command (`BUILTIN_COMMANDS`) or background
/// job command (`BUILTIN_JOB_COMMANDS`) is added, removed, or relabelled
/// without updating `docs/reference/commands.md`.
/// Does not catch prose descriptions in the Action column.
#[test]
fn commands_doc_matches_registry() {
    let expected_state: Vec<(String, String)> = BUILTIN_COMMANDS
        .iter()
        .map(|entry| (entry.id.to_string(), entry.label.to_string()))
        .collect();
    let documented_state: Vec<(String, String)> =
        parse_table_rows(section_slice(COMMANDS_DOC, "State commands"))
            .iter()
            .map(|cells| {
                assert!(cells.len() >= 5, "malformed state command row: {cells:?}");
                (strip_ticks(cells[0]).to_string(), cells[1].to_string())
            })
            .collect();
    assert_eq!(
        documented_state, expected_state,
        "docs/reference/commands.md State commands drifted from BUILTIN_COMMANDS"
    );

    let expected_jobs: Vec<(String, String)> = BUILTIN_JOB_COMMANDS
        .iter()
        .map(|entry| (entry.id.to_string(), entry.label.to_string()))
        .collect();
    let documented_jobs: Vec<(String, String)> =
        parse_table_rows(section_slice(COMMANDS_DOC, "Job commands"))
            .iter()
            .map(|cells| {
                assert!(cells.len() >= 5, "malformed job command row: {cells:?}");
                (strip_ticks(cells[0]).to_string(), cells[1].to_string())
            })
            .collect();
    assert_eq!(
        documented_jobs, expected_jobs,
        "docs/reference/commands.md Background job commands drifted from BUILTIN_JOB_COMMANDS"
    );
}

/// Closes the class where an application menu or menu item in `items_for` is
/// added, removed, moved to a different submenu, or relabelled without updating
/// `docs/reference/menus.md`.
/// Does not catch prose descriptions in the Action column.
#[test]
fn menus_doc_matches_registry() {
    let dir = crate::scratch_pool::scratch("reference-menus-doc");
    let state = AppState::new(PluginContext::new(), dir).expect("build headless AppState");

    for menu in AppMenu::ALL {
        let heading = format!("{} menu", menu.label());
        let section = section_slice(MENUS_DOC, &heading);
        let documented: Vec<(String, String, String)> = parse_table_rows(section)
            .iter()
            .map(|cells| {
                assert!(
                    cells.len() >= 6,
                    "malformed menu row in {heading}: {cells:?}"
                );
                (
                    strip_ticks(cells[0]).to_string(),
                    strip_ticks(cells[1]).to_string(),
                    cells[2].to_string(),
                )
            })
            .collect();

        let expected: Vec<(String, String, String)> = items_for(menu, &state)
            .iter()
            .filter(|item| {
                !(menu == AppMenu::Effect
                    && (item.id.starts_with("fx-apply:") || item.id.starts_with("fx-apply-audio:")))
            })
            .map(|item| {
                (
                    item.id.to_string(),
                    item.under.unwrap_or("none").to_string(),
                    item.label.to_string(),
                )
            })
            .collect();

        assert_eq!(
            documented, expected,
            "docs/reference/menus.md `{heading}` drifted from items_for({menu:?}, &state)"
        );

        if menu == AppMenu::Effect {
            assert!(
                section.contains("`fx-apply:<id>`") && section.contains("`fx-apply-audio:<id>`"),
                "Effect menu section must document dynamic `fx-apply:<id>` and `fx-apply-audio:<id>` rows"
            );
        }
    }
}

/// Closes the class where a `config.toml` key, `[schema]` version,
/// `ui-settings.json` field, or project canvas setting changes in
/// `aria-settings` or `aria-ui` without updating `docs/reference/settings.md`.
/// Does not catch prose wording in the Effect column.
#[test]
fn settings_doc_matches_registries() {
    let defaults = aria_settings::Settings::default();
    let aria_settings::Settings {
        appearance: aria_settings::AppearanceSettings { theme },
        ai:
            aria_settings::AiSettings {
                source,
                base_url,
                model,
                api_protocol,
                reasoning_summary,
                api_key,
                api_key_env,
                autonomy,
            },
        providers: _,
        cloud: aria_settings::CloudSettings {
            base_url: cloud_base_url,
        },
        storage:
            aria_settings::StorageSettings {
                root,
                download_quota_mib,
                paths: _,
            },
        transcription:
            aria_settings::TranscriptionSettings {
                connect_timeout_secs,
                read_timeout_secs,
                total_timeout_secs,
            },
        plugin_limits:
            aria_settings::PluginLimitSettings {
                wasm_fuel_per_call,
                wasm_memory_mib,
            },
        plugins: _,
        rejected: _,
    } = defaults;

    let mut expected_toml_keys: Vec<(String, String)> = vec![
        ("appearance.theme".into(), format!("\"{}\"", theme.key())),
        ("ai.source".into(), format!("\"{}\"", source.key())),
        ("ai.base_url".into(), format!("\"{base_url}\"")),
        ("ai.model".into(), format!("\"{model}\"")),
        (
            "ai.api_protocol".into(),
            format!("\"{}\"", api_protocol.key()),
        ),
        (
            "ai.reasoning_summary".into(),
            format!("\"{}\"", reasoning_summary.key()),
        ),
        (
            "ai.api_key".into(),
            api_key.unwrap_or_else(|| "none".into()),
        ),
        (
            "ai.api_key_env".into(),
            api_key_env.unwrap_or_else(|| "none".into()),
        ),
        ("ai.autonomy".into(), format!("\"{}\"", autonomy.key())),
        ("cloud.base_url".into(), format!("\"{cloud_base_url}\"")),
        (
            "storage.root".into(),
            root.map(|p| p.display().to_string())
                .unwrap_or_else(|| "none".into()),
        ),
        (
            "storage.download_quota_mib".into(),
            download_quota_mib.to_string(),
        ),
    ];
    for key in aria_settings::StoragePathOverrides::KEYS {
        expected_toml_keys.push((format!("storage.paths.{key}"), "none".into()));
    }
    expected_toml_keys.extend([
        (
            "transcription.connect_timeout_secs".into(),
            connect_timeout_secs.to_string(),
        ),
        (
            "transcription.read_timeout_secs".into(),
            read_timeout_secs.to_string(),
        ),
        (
            "transcription.total_timeout_secs".into(),
            total_timeout_secs.to_string(),
        ),
        (
            "plugin_limits.wasm_fuel_per_call".into(),
            wasm_fuel_per_call.to_string(),
        ),
        (
            "plugin_limits.wasm_memory_mib".into(),
            wasm_memory_mib.to_string(),
        ),
    ]);
    for (provider, _) in BYOK_PROVIDERS {
        expected_toml_keys.push((format!("providers.{provider}"), "unconfigured".into()));
    }
    expected_toml_keys.push(("plugins.<key>".into(), "unset".into()));

    let toml_section = section_slice(SETTINGS_DOC, "Configuration file keys (`config.toml`)");
    let toml_rows = parse_table_rows(toml_section);
    let documented_toml: Vec<(String, String)> = toml_rows
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 6, "malformed config.toml row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[2]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented_toml, expected_toml_keys,
        "docs/reference/settings.md config.toml keys or defaults drifted from aria_settings::Settings"
    );

    // Check documented numeric bounds for config.toml keys.
    let toml_bounds: BTreeMap<String, String> = toml_rows
        .iter()
        .map(|cells| (strip_ticks(cells[0]).to_string(), cells[3].to_string()))
        .collect();
    assert!(
        toml_bounds["storage.download_quota_mib"].contains(&format!(
            "`{}` to `{}`",
            aria_settings::MIN_DOWNLOAD_QUOTA_MIB,
            aria_settings::MAX_DOWNLOAD_QUOTA_MIB
        )),
        "storage.download_quota_mib bounds drifted"
    );
    assert!(
        toml_bounds["transcription.connect_timeout_secs"].contains(&format!(
            "`1` to `{}`",
            aria_settings::MAX_DOWNLOAD_TIMEOUT_SECS
        )),
        "transcription.connect_timeout_secs bounds drifted"
    );
    assert!(
        toml_bounds["plugin_limits.wasm_fuel_per_call"].contains(&format!(
            "`{}` to `{}`",
            aria_settings::MIN_WASM_FUEL_PER_CALL,
            aria_settings::MAX_WASM_FUEL_PER_CALL
        )),
        "plugin_limits.wasm_fuel_per_call bounds drifted"
    );
    assert!(
        toml_bounds["plugin_limits.wasm_memory_mib"].contains(&format!(
            "`{}` to `{}`",
            aria_settings::MIN_WASM_MEMORY_MIB,
            aria_settings::MAX_WASM_MEMORY_MIB
        )),
        "plugin_limits.wasm_memory_mib bounds drifted"
    );

    // Schema versions table.
    let schema_rows = parse_table_rows(section_slice(
        SETTINGS_DOC,
        "Schema version table (`[schema]`)",
    ));
    let expected_schema: Vec<(String, String, String)> =
        aria_settings::schema::PersistedShape::all()
            .map(|shape| {
                (
                    shape.table().to_string(),
                    shape.current_version().to_string(),
                    shape.oldest_migratable_version().to_string(),
                )
            })
            .collect();
    let documented_schema: Vec<(String, String, String)> = schema_rows
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 3, "malformed schema row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[1]).to_string(),
                strip_ticks(cells[2]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented_schema, expected_schema,
        "docs/reference/settings.md Schema version table drifted from PersistedShape::all()"
    );

    // Exhaustive construction of UiSettings and TimelineDisplay so adding any
    // field fails compilation until accounted for here and in settings.md.
    let ui_default = UiSettings::default();
    let UiSettings {
        sidebar_width,
        sidebar_collapsed,
        inspector_width,
        inspector_collapsed,
        timeline_height,
        outline_width,
        appearance,
        home_rail,
        motion,
        ref keys,
        ref keymap,
        ref workspace,
        ref dock_tree,
        ref workspace_trees,
        ref active_workspace,
        pool_view,
        log_dropped_frames,
        ref theme_file,
        playback_resolution,
        paused_resolution,
        use_proxies,
        transparency_grid,
        audio_scrubbing,
        timeline_display:
            TimelineDisplay {
                thumbnails,
                waveform: td_waveform,
                follow,
            },
        waveform,
        ref favourite_effects,
        ref window_placements,
        ref last_display_arrangement,
    } = ui_default;
    let _ = (
        keys,
        keymap,
        workspace,
        dock_tree,
        workspace_trees,
        active_workspace,
        theme_file,
        favourite_effects,
        window_placements,
        last_display_arrangement,
    );

    let populated = UiSettings {
        sidebar_width,
        sidebar_collapsed,
        inspector_width,
        inspector_collapsed,
        timeline_height,
        outline_width,
        appearance,
        home_rail,
        motion,
        keys: BTreeMap::new(),
        keymap: Some("default".into()),
        workspace: None,
        dock_tree: Some(crate::dock::DockTree::editing()),
        workspace_trees: BTreeMap::from([("editing".into(), crate::dock::DockTree::editing())]),
        active_workspace: Some("editing".into()),
        pool_view,
        log_dropped_frames,
        theme_file: None,
        playback_resolution,
        paused_resolution,
        use_proxies,
        transparency_grid,
        audio_scrubbing,
        timeline_display: TimelineDisplay {
            thumbnails,
            waveform: td_waveform,
            follow,
        },
        waveform,
        favourite_effects: Vec::new(),
        window_placements: BTreeMap::from([("primary".into(), WindowPlacement::default())]),
        last_display_arrangement: Some("primary".into()),
    };
    let serialized = serde_json::to_value(&populated).expect("serialize UiSettings");
    let obj = serialized
        .as_object()
        .expect("UiSettings serializes as object");
    let mut expected_ui_keys: BTreeSet<String> = obj.keys().cloned().collect();
    let td_obj = obj["timelineDisplay"]
        .as_object()
        .expect("timelineDisplay serializes as object");
    for sub in td_obj.keys() {
        expected_ui_keys.insert(format!("timelineDisplay.{sub}"));
    }

    let ui_rows = parse_table_rows(section_slice(
        SETTINGS_DOC,
        "UI settings (`ui-settings.json`)",
    ));
    let documented_ui_keys: BTreeSet<String> = ui_rows
        .iter()
        .map(|cells| strip_ticks(cells[0]).to_string())
        .collect();
    assert_eq!(
        documented_ui_keys, expected_ui_keys,
        "docs/reference/settings.md UI settings keys drifted from UiSettings"
    );

    let ui_defaults: BTreeMap<String, String> = ui_rows
        .iter()
        .map(|cells| {
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[2]).to_string(),
            )
        })
        .collect();
    let ui_ranges: BTreeMap<String, String> = ui_rows
        .iter()
        .map(|cells| (strip_ticks(cells[0]).to_string(), cells[3].to_string()))
        .collect();

    assert_eq!(ui_defaults["sidebarWidth"], format!("{sidebar_width:.1}"));
    assert!(
        ui_ranges["sidebarWidth"].contains(&format!("`{SIDEBAR_MIN:.1}` to `{SIDEBAR_MAX:.1}`"))
    );
    assert_eq!(
        ui_defaults["inspectorWidth"],
        format!("{inspector_width:.1}")
    );
    assert!(
        ui_ranges["inspectorWidth"]
            .contains(&format!("`{INSPECTOR_MIN:.1}` to `{INSPECTOR_MAX:.1}`"))
    );
    assert_eq!(
        ui_defaults["timelineHeight"],
        format!("{timeline_height:.1}")
    );
    assert!(ui_ranges["timelineHeight"].contains(&format!("`>= {TIMELINE_MIN_HEIGHT:.1}`")));
    assert_eq!(ui_defaults["outlineWidth"], format!("{outline_width:.1}"));
    assert!(
        ui_ranges["outlineWidth"].contains(&format!("`{OUTLINE_MIN:.1}` to `{OUTLINE_MAX:.1}`"))
    );
    assert_eq!(
        ui_defaults["appearance"],
        serde_json::to_string(&appearance).unwrap()
    );
    assert_eq!(
        ui_defaults["homeRail"],
        serde_json::to_string(&home_rail).unwrap()
    );
    assert_eq!(
        ui_defaults["motion"],
        serde_json::to_string(&motion).unwrap()
    );
    assert_eq!(
        ui_defaults["poolView"],
        serde_json::to_string(&pool_view).unwrap()
    );
    assert_eq!(
        ui_defaults["playbackResolution"],
        serde_json::to_string(&playback_resolution).unwrap()
    );
    assert_eq!(
        ui_defaults["pausedResolution"],
        serde_json::to_string(&paused_resolution).unwrap()
    );
    assert_eq!(ui_defaults["useProxies"], use_proxies.to_string());
    assert_eq!(
        ui_defaults["transparencyGrid"],
        transparency_grid.to_string()
    );
    assert_eq!(ui_defaults["audioScrubbing"], audio_scrubbing.to_string());
    assert_eq!(
        ui_defaults["timelineDisplay.thumbnails"],
        serde_json::to_string(&thumbnails).unwrap()
    );
    assert_eq!(
        ui_defaults["timelineDisplay.waveform"],
        serde_json::to_string(&td_waveform).unwrap()
    );
    assert_eq!(
        ui_defaults["timelineDisplay.follow"],
        serde_json::to_string(&follow).unwrap()
    );
    assert_eq!(
        ui_defaults["waveform"],
        serde_json::to_string(&waveform).unwrap()
    );

    for v in AppearanceMode::ALL {
        let s = serde_json::to_string(&v).unwrap();
        assert!(
            ui_ranges["appearance"].contains(&s),
            "missing appearance {s}"
        );
    }
    for v in MotionPreference::ALL {
        let s = serde_json::to_string(&v).unwrap();
        assert!(ui_ranges["motion"].contains(&s), "missing motion {s}");
    }
    for v in PoolView::ALL {
        let s = serde_json::to_string(v).unwrap();
        assert!(ui_ranges["poolView"].contains(&s), "missing poolView {s}");
    }
    for v in PlaybackResolution::ALL {
        let s = serde_json::to_string(&v).unwrap();
        assert!(
            ui_ranges["playbackResolution"].contains(&s),
            "missing playbackResolution {s}"
        );
    }
    for v in PausedResolution::ALL {
        let s = serde_json::to_string(&v).unwrap();
        assert!(
            ui_ranges["pausedResolution"].contains(&s),
            "missing pausedResolution {s}"
        );
    }
    for v in WaveformMode::ALL {
        let s = serde_json::to_string(&v).unwrap();
        assert!(ui_ranges["waveform"].contains(&s), "missing waveform {s}");
    }
    for v in [
        Thumbnails::Head,
        Thumbnails::HeadAndTail,
        Thumbnails::Continuous,
    ] {
        let s = serde_json::to_string(&v).unwrap();
        assert!(
            ui_ranges["timelineDisplay.thumbnails"].contains(&s),
            "missing thumbnails {s}"
        );
    }
    for v in [
        WaveformScale::Logarithmic,
        WaveformScale::Linear,
        WaveformScale::Hidden,
    ] {
        let s = serde_json::to_string(&v).unwrap();
        assert!(
            ui_ranges["timelineDisplay.waveform"].contains(&s),
            "missing timelineDisplay.waveform {s}"
        );
    }
    for v in [Follow::Page, Follow::Smooth] {
        let s = serde_json::to_string(&v).unwrap();
        assert!(
            ui_ranges["timelineDisplay.follow"].contains(&s),
            "missing timelineDisplay.follow {s}"
        );
    }

    // Project canvas settings section.
    let canvas_section = section_slice(SETTINGS_DOC, "Project canvas settings");
    for aspect in CanvasAspect::ALL {
        assert!(
            canvas_section.contains(&format!("`{}`", aspect.name())),
            "Project canvas settings missing aspect `{}`",
            aspect.name()
        );
    }
    for baseline in CANVAS_BASELINES {
        assert!(
            canvas_section.contains(&format!("`{baseline}`")),
            "Project canvas settings missing baseline `{baseline}`"
        );
    }
    assert!(
        canvas_section.contains(&format!("`{}`", CanvasSettings::MIN_DIM))
            && canvas_section.contains(&format!("`{}`", CanvasSettings::MAX_DIM)),
        "Project canvas settings missing MIN_DIM / MAX_DIM"
    );
    for rate in RATES {
        let ratio = format!("`{}/{}`", rate.num, rate.den);
        assert!(
            canvas_section.contains(&ratio),
            "Project canvas settings missing frame rate {ratio}"
        );
    }
    for layout in AudioLayout::ALL {
        assert!(
            canvas_section.contains(&format!("`{}`", layout.name()))
                && canvas_section.contains(&format!("`{}`", layout.label())),
            "Project canvas settings missing audio layout `{}`",
            layout.name()
        );
    }
    for display in TimeDisplay::ALL {
        assert!(
            canvas_section.contains(&format!("`{}`", display.name()))
                && canvas_section.contains(&format!("`{}`", display.label())),
            "Project canvas settings missing time display `{}`",
            display.name()
        );
    }
}

fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"));
    let mut paths: Vec<PathBuf> = entries.map(|e| e.unwrap().path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_md_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

fn gfm_slug(heading: &str) -> String {
    let mut stripped = String::new();
    let mut chars = heading.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '`' {
            continue;
        }
        if ch == '[' {
            let mut label = String::new();
            for c in chars.by_ref() {
                if c == ']' {
                    break;
                }
                label.push(c);
            }
            if chars.peek() == Some(&'(') {
                chars.next();
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            stripped.push_str(&label);
            continue;
        }
        stripped.push(ch);
    }
    let mut slug = String::new();
    for ch in stripped.trim().to_lowercase().chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch);
        } else if ch == ' ' {
            slug.push('-');
        }
    }
    slug
}

fn markdown_headings(md_path: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(md_path)
        .unwrap_or_else(|e| panic!("failed to read {md_path:?}: {e}"));
    let mut in_fence = false;
    let mut slugs = BTreeSet::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) {
            let after = &trimmed[hashes..];
            if let Some(title) = after.strip_prefix(' ') {
                let base = gfm_slug(title);
                let count = counts.entry(base.clone()).or_insert(0);
                let slug = if *count == 0 {
                    base
                } else {
                    format!("{base}-{count}")
                };
                *count += 1;
                slugs.insert(slug);
            }
        }
    }
    slugs
}

fn strip_inline_code(line: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for ch in line.chars() {
        if ch == '`' {
            in_code = !in_code;
        } else if !in_code {
            out.push(ch);
        }
    }
    out
}

fn extract_markdown_links(line: &str) -> Vec<String> {
    let cleaned = strip_inline_code(line);
    let bytes = cleaned.as_bytes();
    let mut links = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b']' {
                j += 1;
            }
            if j + 1 < bytes.len() && bytes[j] == b']' && bytes[j + 1] == b'(' {
                let mut k = j + 2;
                while k < bytes.len() && bytes[k] != b')' {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b')' {
                    let target = cleaned[j + 2..k].trim();
                    if !target.is_empty() {
                        links.push(target.to_string());
                    }
                    i = k + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    links
}

/// Closes the class where a documentation edit introduces a broken relative
/// file link or a broken `#anchor` heading fragment anywhere under `docs/`.
/// Does not check reachability of external `http://` or `https://` URLs.
#[test]
fn all_markdown_links_and_anchors_in_docs_resolve() {
    let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut md_files = Vec::new();
    collect_md_files(&docs_root, &mut md_files);
    assert!(
        md_files.len() >= 20,
        "expected at least 20 markdown files under docs/, found {}",
        md_files.len()
    );

    let mut heading_cache: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut broken: Vec<String> = Vec::new();
    let mut checked_links = 0usize;

    for md_file in &md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        let mut in_fence = false;
        for (idx, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            for target in extract_markdown_links(line) {
                if target.starts_with("http://")
                    || target.starts_with("https://")
                    || target.starts_with("mailto:")
                {
                    continue;
                }
                checked_links += 1;
                let (path_part, anchor) = match target.split_once('#') {
                    Some((p, a)) => (p, Some(a)),
                    None => (target.as_str(), None),
                };
                let target_path = if path_part.is_empty() {
                    md_file.clone()
                } else {
                    md_file.parent().unwrap().join(path_part)
                };
                let Ok(canonical) = std::fs::canonicalize(&target_path) else {
                    broken.push(format!(
                        "{}:{}: broken link `{target}` (target file does not exist)",
                        md_file.display(),
                        idx + 1
                    ));
                    continue;
                };
                if let Some(anchor_slug) = anchor
                    && canonical.extension().and_then(|s| s.to_str()) == Some("md")
                {
                    let slugs = heading_cache
                        .entry(canonical.clone())
                        .or_insert_with(|| markdown_headings(&canonical));
                    if !slugs.contains(anchor_slug) {
                        broken.push(format!(
                            "{}:{}: broken anchor `#{anchor_slug}` in `{target}`",
                            md_file.display(),
                            idx + 1
                        ));
                    }
                }
            }
        }
    }

    assert!(
        checked_links >= 200,
        "expected at least 200 internal links across docs/, checked {checked_links}"
    );
    assert!(
        broken.is_empty(),
        "broken relative links or #anchors in docs/:\n{}",
        broken.join("\n")
    );
}

/// WHY: a fenced code block closes only on a fence line with no info string.
/// A block left open swallows every following line, so a ```` ```rust ````
/// meant to open the next example renders as code text, the prose between
/// the two examples renders as code, and the link check above, which toggles
/// on every fence line, reads code as prose and prose as code. This walks each
/// file under `docs/` with the closing rule a renderer uses and fails on a
/// fence that opens inside an open block or a block still open at the end of
/// the file. Not caught: a bare fence meant to open a block that instead
/// closes the one before it.
#[test]
fn every_fenced_code_block_in_docs_closes() {
    let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut md_files = Vec::new();
    collect_md_files(&docs_root, &mut md_files);
    let mut fences = 0usize;
    let mut unclosed: Vec<String> = Vec::new();
    for md_file in &md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        let mut open: Option<(usize, char, usize)> = None;
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start_matches(' ');
            if line.len() - trimmed.len() > 3 {
                continue;
            }
            let Some(mark) = trimmed.chars().next().filter(|c| *c == '`' || *c == '~') else {
                continue;
            };
            let run = trimmed.chars().take_while(|c| *c == mark).count();
            if run < 3 {
                continue;
            }
            let info = trimmed[run..].trim();
            match open {
                None => {
                    open = Some((idx + 1, mark, run));
                    fences += 1;
                }
                Some((_, open_mark, open_run))
                    if mark == open_mark && run >= open_run && info.is_empty() =>
                {
                    open = None;
                }
                Some((opened, _, _)) if !info.is_empty() => unclosed.push(format!(
                    "{}:{}: the fence line {:?} opens inside the block opened at line {opened}",
                    md_file.display(),
                    idx + 1,
                    trimmed.trim_end()
                )),
                Some(_) => {}
            }
        }
        if let Some((opened, _, _)) = open {
            unclosed.push(format!(
                "{}:{opened}: fenced block never closes",
                md_file.display()
            ));
        }
    }
    assert!(
        fences >= 100,
        "expected at least 100 fenced blocks across docs/, found {fences}"
    );
    assert!(
        unclosed.is_empty(),
        "unbalanced code fences in docs/:\n{}",
        unclosed.join("\n")
    );
}
