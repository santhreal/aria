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

use aria_models::{AudioLayout, CanvasAspect, CanvasSettings, MatteSource, TimeDisplay};
use aria_plugin::PluginContext;
use aria_settings::{AiApiProtocol, AiSource, Autonomy, ReasoningSummary, ThemeChoice};

use crate::app::analysis::AnalysisKind;
use crate::app::audio_channels::AudioChannels;
use crate::app::clip_ops::AudioGainMode;
use crate::app::decline::Decline;
use crate::app::effects_browser::{EntryKind, Group};
use crate::app::generate::GenerateKind;
use crate::app::models::TimeoutField;
use crate::app::plugin_install::WasmLimit;
use crate::app::project_settings::CANVAS_BASELINES;
use crate::app::settings_form::SettingsField;
use crate::app::{AppState, DialogKind};
use crate::appearance::AppearanceMode;
use crate::chrome::menu_items::items_for;
use crate::cloud::{BYOK_PROVIDERS, CloudFeed};
use crate::commands::{BUILTIN_COMMANDS, BUILTIN_JOB_COMMANDS};
use crate::dock::DOCK_VERSION;
use crate::layouts::WORKSPACES;
use crate::motion::MotionPreference;
use crate::plugins::UiPlugin;
use crate::plugins::{overlay_host, panel_host};
use crate::resolution::{PausedResolution, PlaybackResolution, Resolution};
use crate::scopes::WaveformMode;
use crate::settings::{
    INSPECTOR_MAX, INSPECTOR_MIN, OUTLINE_MAX, OUTLINE_MIN, SIDEBAR_MAX, SIDEBAR_MIN,
    TIMELINE_MIN_HEIGHT, UiSettings, WindowPlacement,
};
use crate::surfaces::context_menu::{
    BADGE_ITEMS, BadgeMenu, CLIP_ITEMS, ClipMenu, EFFECT_ITEMS, EffectMenu, MenuItem, POOL_ITEMS,
    PREVIEW_ITEMS, PoolMenu, RECENT_ITEMS, TRACK_ITEMS, TRANSITION_ITEMS, TrackMenu,
};
use crate::surfaces::settings_pick::RATES;
use crate::test_support::doc_tables::{
    parse_table_rows, read_doc, section_slice, strip_ticks, table_rows,
};
use crate::timeline_display::{Follow, Thumbnails, TimelineDisplay, WaveformScale};
use crate::tool_catalog::{ToolGroup, tool_specs};
use crate::types::{
    ActivePanel, AgentEntryKind, AppMenu, HomeRail, MediaSort, OverlayMode, PenVariant, PoolView,
    SidebarGroup, SidebarTab,
};

const TOOLS_DOC: &str = include_str!("../../../../docs/reference/tools.md");
const WORKSPACES_DOC: &str = include_str!("../../../../docs/reference/workspaces.md");
const COMMANDS_DOC: &str = include_str!("../../../../docs/reference/commands.md");

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
/// `docs/reference/panels.md`. The Effect Controls inspector tabs are held
/// to the same page by `panels::inspector::tab_host`.
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

    let panels_doc_buf = read_doc("reference/panels.md");
    #[allow(non_snake_case)]
    let PANELS_DOC = panels_doc_buf.as_str();
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

    for kind in overlays.dialog_kinds() {
        match kind {
            DialogKind::None
            | DialogKind::Export
            | DialogKind::Settings
            | DialogKind::Palette
            | DialogKind::Jobs
            | DialogKind::ProjectReport
            | DialogKind::UnsavedChanges
            | DialogKind::InstallPlugin
            | DialogKind::SpeedDuration
            | DialogKind::AudioGain
            | DialogKind::About
            | DialogKind::RemoveAttributes
            | DialogKind::KeyframeVelocity
            | DialogKind::MarkerEdit
            | DialogKind::Plugin(_) => {}
        }
    }
    let expected_overlays: Vec<(String, String)> = overlays
        .entries_meta()
        .into_iter()
        .map(|(id, when)| (id.to_string(), format!("{when:?}")))
        .collect();
    let documented_overlays: Vec<(String, String)> =
        parse_table_rows(section_slice(PANELS_DOC, "Built-in dialogs and overlays"))
            .iter()
            .map(|cells| {
                assert!(cells.len() >= 2, "malformed overlay row: {cells:?}");
                (
                    strip_ticks(cells[0]).to_string(),
                    strip_ticks(cells[1]).to_string(),
                )
            })
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
    let expected_sidebar_groups: Vec<String> = SidebarGroup::ALL
        .iter()
        .map(|g| {
            match g {
                SidebarGroup::Media
                | SidebarGroup::Graphics
                | SidebarGroup::Effects
                | SidebarGroup::Library
                | SidebarGroup::Analyze => {}
            }
            g.label().to_string()
        })
        .collect();
    let mut documented_sidebar_groups: Vec<String> = Vec::new();
    for (_, group_label, _) in &documented_tabs {
        if !documented_sidebar_groups.contains(group_label) {
            documented_sidebar_groups.push(group_label.clone());
        }
    }
    assert_eq!(
        documented_sidebar_groups, expected_sidebar_groups,
        "docs/reference/panels.md Project panel tabs groups drifted from SidebarGroup::ALL"
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

    let expected_pool_views: Vec<(
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
        bool,
    )> = PoolView::ALL
        .iter()
        .map(|&v| {
            match v {
                PoolView::Tiles | PoolView::List | PoolView::Icons | PoolView::Freeform => {}
            }
            let serde_s = serde_json::to_value(v)
                .expect("serialize PoolView")
                .as_str()
                .expect("string")
                .to_string();
            let width_s = v
                .width()
                .map_or_else(|| "none".to_string(), |w| format!("{w:.1}"));
            (
                format!("{v:?}"),
                serde_s,
                v.label().to_string(),
                v.picture().to_string(),
                v.facts().to_string(),
                v.inline().to_string(),
                v.wraps().to_string(),
                v.freeform().to_string(),
                width_s,
                format!("{:?}", v.layout()),
                v == PoolView::default(),
            )
        })
        .collect();
    let documented_pool_views: Vec<(
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
        bool,
    )> = parse_table_rows(section_slice(
        PANELS_DOC,
        "Media pool views and sort orders",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 11, "malformed PoolView row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            cells[2].to_string(),
            strip_ticks(cells[3]).to_string(),
            strip_ticks(cells[4]).to_string(),
            strip_ticks(cells[5]).to_string(),
            strip_ticks(cells[6]).to_string(),
            strip_ticks(cells[7]).to_string(),
            strip_ticks(cells[8]).to_string(),
            strip_ticks(cells[9]).to_string(),
            strip_ticks(cells[10]) == "yes",
        )
    })
    .collect();
    assert_eq!(
        documented_pool_views, expected_pool_views,
        "docs/reference/panels.md Media pool views drifted from PoolView::ALL"
    );

    let expected_sorts: Vec<(String, String, bool)> = MediaSort::ALL
        .iter()
        .map(|&s| {
            match s {
                MediaSort::Name
                | MediaSort::Duration
                | MediaSort::Resolution
                | MediaSort::DateAdded => {}
            }
            (
                format!("{s:?}"),
                s.label().to_string(),
                s == MediaSort::default(),
            )
        })
        .collect();
    let documented_sorts: Vec<(String, String, bool)> =
        parse_table_rows(section_slice(PANELS_DOC, "Media pool sort orders"))
            .iter()
            .map(|cells| {
                assert!(cells.len() >= 3, "malformed MediaSort row: {cells:?}");
                (
                    strip_ticks(cells[0]).to_string(),
                    cells[1].to_string(),
                    strip_ticks(cells[2]) == "yes",
                )
            })
            .collect();
    assert_eq!(
        documented_sorts, expected_sorts,
        "docs/reference/panels.md Media pool sort orders drifted from MediaSort::ALL"
    );

    let expected_overlay_modes: Vec<(String, String, String, bool)> = OverlayMode::ALL
        .iter()
        .map(|&m| {
            match m {
                OverlayMode::Transform | OverlayMode::Crop | OverlayMode::Mask => {}
            }
            (
                format!("{m:?}"),
                m.label().to_string(),
                format!("{:?}", m.next()),
                m == OverlayMode::default(),
            )
        })
        .collect();
    let documented_overlay_modes: Vec<(String, String, String, bool)> =
        parse_table_rows(section_slice(PANELS_DOC, "Viewer overlay modes"))
            .iter()
            .map(|cells| {
                assert!(cells.len() >= 4, "malformed OverlayMode row: {cells:?}");
                (
                    strip_ticks(cells[0]).to_string(),
                    cells[1].to_string(),
                    strip_ticks(cells[2]).to_string(),
                    strip_ticks(cells[3]) == "yes",
                )
            })
            .collect();
    assert_eq!(
        documented_overlay_modes, expected_overlay_modes,
        "docs/reference/panels.md Viewer overlay modes drifted from OverlayMode::ALL"
    );

    let expected_focus: Vec<(String, String, String)> = ActivePanel::ALL
        .iter()
        .map(|&p| {
            match p {
                ActivePanel::Media
                | ActivePanel::Preview
                | ActivePanel::Timeline
                | ActivePanel::Inspector
                | ActivePanel::Ai
                | ActivePanel::Mixer => {}
            }
            let serde_s = serde_json::to_value(p)
                .expect("serialize ActivePanel")
                .as_str()
                .expect("string")
                .to_string();
            (format!("{p:?}"), serde_s, p.panel_id().to_string())
        })
        .collect();
    let documented_focus: Vec<(String, String, String)> =
        parse_table_rows(section_slice(PANELS_DOC, "Keyboard focus targets"))
            .iter()
            .map(|cells| {
                assert!(cells.len() >= 3, "malformed ActivePanel row: {cells:?}");
                (
                    strip_ticks(cells[0]).to_string(),
                    strip_ticks(cells[1]).to_string(),
                    strip_ticks(cells[2]).to_string(),
                )
            })
            .collect();
    assert_eq!(
        documented_focus, expected_focus,
        "docs/reference/panels.md Keyboard focus targets drifted from ActivePanel::ALL"
    );

    let expected_agent_kinds: Vec<(String, String, String)> = AgentEntryKind::ALL
        .iter()
        .map(|&k| {
            match k {
                AgentEntryKind::You
                | AgentEntryKind::Assistant
                | AgentEntryKind::Reasoning
                | AgentEntryKind::Action
                | AgentEntryKind::Error => {}
            }
            assert_eq!(AgentEntryKind::from_key(k.key()), Some(k));
            (format!("{k:?}"), k.label().to_string(), k.key().to_string())
        })
        .collect();
    let documented_agent_kinds: Vec<(String, String, String)> = parse_table_rows(section_slice(
        PANELS_DOC,
        "Assistant transcript entry kinds",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 3, "malformed AgentEntryKind row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            strip_ticks(cells[2]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_agent_kinds, expected_agent_kinds,
        "docs/reference/panels.md Assistant transcript entry kinds drifted from AgentEntryKind::ALL"
    );

    let expected_cloud_feeds: Vec<(String, String, bool, String)> = CloudFeed::ALL
        .iter()
        .map(|&f| {
            match f {
                CloudFeed::Stock
                | CloudFeed::Templates
                | CloudFeed::Sfx
                | CloudFeed::Luts
                | CloudFeed::TextPresets
                | CloudFeed::Lottie
                | CloudFeed::Skills => {}
            }
            (
                format!("{f:?}"),
                f.label().to_string(),
                f.is_search(),
                f.asset_kind()
                    .map_or_else(|| "none".to_string(), |k| format!("{k:?}")),
            )
        })
        .collect();
    let documented_cloud_feeds: Vec<(String, String, bool, String)> = parse_table_rows(
        section_slice(PANELS_DOC, "Cloud library feeds (`CloudFeed::ALL`)"),
    )
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 4, "malformed CloudFeed row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            strip_ticks(cells[2]) == "true",
            strip_ticks(cells[3]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_cloud_feeds, expected_cloud_feeds,
        "docs/reference/panels.md Cloud library feeds drifted from CloudFeed::ALL"
    );

    let expected_generate_kinds: Vec<(String, String, String)> = GenerateKind::ALL
        .iter()
        .map(|&(k, label, ext)| {
            match k {
                GenerateKind::Image | GenerateKind::Video | GenerateKind::Tts => {}
            }
            assert_eq!(k.label(), label);
            assert_eq!(k.fallback_extension(), ext);
            (format!("{k:?}"), label.to_string(), ext.to_string())
        })
        .collect();
    let documented_generate_kinds: Vec<(String, String, String)> = parse_table_rows(section_slice(
        PANELS_DOC,
        "Generate panel media kinds (`GenerateKind::ALL`)",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 3, "malformed GenerateKind row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            strip_ticks(cells[2]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_generate_kinds, expected_generate_kinds,
        "docs/reference/panels.md Generate panel media kinds drifted from GenerateKind::ALL"
    );

    let expected_analysis_kinds: Vec<(String, String, String)> = AnalysisKind::ALL
        .iter()
        .map(|&k| {
            match k {
                AnalysisKind::Shots | AnalysisKind::Silence | AnalysisKind::Beats => {}
            }
            (format!("{k:?}"), k.id().to_string(), k.label().to_string())
        })
        .collect();
    let documented_analysis_kinds: Vec<(String, String, String)> = parse_table_rows(section_slice(
        PANELS_DOC,
        "Analysis panel detector kinds (`AnalysisKind::ALL`)",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 3, "malformed AnalysisKind row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            strip_ticks(cells[2]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_analysis_kinds, expected_analysis_kinds,
        "docs/reference/panels.md Analysis panel detector kinds drifted from AnalysisKind::ALL"
    );

    let expected_entry_kinds: Vec<(String, String, String, String)> = EntryKind::ALL
        .iter()
        .map(|&k| {
            match k {
                EntryKind::VideoEffect
                | EntryKind::AudioEffect
                | EntryKind::Transition
                | EntryKind::AudioTransition
                | EntryKind::Generator
                | EntryKind::ClipType
                | EntryKind::Preset => {}
            }
            (
                format!("{k:?}"),
                k.group().label().to_string(),
                k.tag().to_string(),
                format!("{:?}", k.drag()),
            )
        })
        .collect();
    let documented_entry_kinds: Vec<(String, String, String, String)> = parse_table_rows(
        section_slice(PANELS_DOC, "Effects browser entry kinds (`EntryKind::ALL`)"),
    )
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 4, "malformed EntryKind row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            strip_ticks(cells[2]).to_string(),
            strip_ticks(cells[3]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_entry_kinds, expected_entry_kinds,
        "docs/reference/panels.md Effects browser entry kinds drifted from EntryKind::ALL"
    );

    let expected_browser_groups: Vec<(String, String, String)> = Group::ALL
        .iter()
        .map(|&g| {
            match g {
                Group::Favourites
                | Group::VideoEffects
                | Group::AudioEffects
                | Group::Transitions
                | Group::AudioTransitions
                | Group::Generators
                | Group::Presets
                | Group::Failed => {}
            }
            (
                format!("{g:?}"),
                g.label().to_string(),
                g.only_label().to_string(),
            )
        })
        .collect();
    let documented_browser_groups: Vec<(String, String, String)> = parse_table_rows(section_slice(
        PANELS_DOC,
        "Effects browser folder groups (`Group::ALL`)",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 3, "malformed Group row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            strip_ticks(cells[2]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_browser_groups, expected_browser_groups,
        "docs/reference/panels.md Effects browser folder groups drifted from Group::ALL"
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
    let menus_doc_buf = read_doc("reference/menus.md");
    #[allow(non_snake_case)]
    let MENUS_DOC = menus_doc_buf.as_str();

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

    let expected_gain_modes: Vec<(String, String, bool)> = AudioGainMode::ALL
        .iter()
        .map(|&m| {
            match m {
                AudioGainMode::SetGain
                | AudioGainMode::AdjustGain
                | AudioGainMode::NormalizeMaxPeak
                | AudioGainMode::NormalizeAllPeaks => {}
            }
            (
                format!("{m:?}"),
                m.label().to_string(),
                m == AudioGainMode::default(),
            )
        })
        .collect();
    let documented_gain_modes: Vec<(String, String, bool)> = parse_table_rows(section_slice(
        MENUS_DOC,
        "Audio Gain dialog modes (`AudioGainMode::ALL`)",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 4, "malformed AudioGainMode row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            strip_ticks(cells[2]) == "yes",
        )
    })
    .collect();
    assert_eq!(
        documented_gain_modes, expected_gain_modes,
        "docs/reference/menus.md Audio Gain dialog modes drifted from AudioGainMode::ALL"
    );

    let expected_audio_channels: Vec<(String, String, String, String)> = AudioChannels::ALL
        .iter()
        .map(|&c| {
            match c {
                AudioChannels::Stereo
                | AudioChannels::Swap
                | AudioChannels::LeftToBoth
                | AudioChannels::RightToBoth
                | AudioChannels::Mono => {}
            }
            (
                format!("{c:?}"),
                c.row().to_string(),
                c.label().to_string(),
                c.tool().unwrap_or("none").to_string(),
            )
        })
        .collect();
    let documented_audio_channels: Vec<(String, String, String, String)> =
        parse_table_rows(section_slice(
            MENUS_DOC,
            "Audio Channels routing presets (`AudioChannels::ALL`)",
        ))
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 4, "malformed AudioChannels row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[1]).to_string(),
                strip_ticks(cells[2]).to_string(),
                strip_ticks(cells[3]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented_audio_channels, expected_audio_channels,
        "docs/reference/menus.md Audio Channels routing presets drifted from AudioChannels::ALL"
    );

    let expected_matte_sources: Vec<(String, String, String)> = MatteSource::ALL
        .iter()
        .map(|&m| {
            match m {
                MatteSource::Alpha | MatteSource::Luma => {}
            }
            (format!("{m:?}"), m.id().to_string(), m.label().to_string())
        })
        .collect();
    let documented_matte_sources: Vec<(String, String, String)> = parse_table_rows(section_slice(
        MENUS_DOC,
        "Track matte sources (`MatteSource::ALL`)",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 3, "malformed MatteSource row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            strip_ticks(cells[2]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_matte_sources, expected_matte_sources,
        "docs/reference/menus.md Track matte sources drifted from MatteSource::ALL"
    );

    let expected_declines: Vec<(String, String)> = Decline::ALL
        .iter()
        .map(|&d| {
            match d {
                Decline::NoSelection
                | Decline::NoAbuttingCut
                | Decline::UnknownTransition
                | Decline::NoTransitionAtCut
                | Decline::PictureCut
                | Decline::SoundCut
                | Decline::OneAngleOnly
                | Decline::AnglesStillDecoding
                | Decline::NoAngleSound
                | Decline::AnglesDoNotMatch
                | Decline::AnglesRepeat => {}
            }
            (format!("{d:?}"), d.reason().to_string())
        })
        .collect();
    let documented_declines: Vec<(String, String)> = parse_table_rows(section_slice(
        MENUS_DOC,
        "Action decline reasons (`Decline::ALL`)",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 2, "malformed Decline row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_declines, expected_declines,
        "docs/reference/menus.md Action decline reasons drifted from Decline::ALL"
    );

    fn collect_ctx_items<F>(
        slice_name: &str,
        items: &[MenuItem<F>],
        facts: &F,
        out: &mut Vec<(String, String, String, String)>,
    ) {
        for item in items {
            let (kind, id, label) = match item {
                MenuItem::Row(row) => ("Row", row.id.to_string(), (row.label)(facts).to_string()),
                MenuItem::Outside(row) => (
                    "Outside",
                    row.id.to_string(),
                    (row.label)(facts).to_string(),
                ),
                MenuItem::Labels => ("Labels", "none".to_string(), "none".to_string()),
                MenuItem::Blends => ("Blends", "none".to_string(), "none".to_string()),
                MenuItem::Properties => ("Properties", "none".to_string(), "none".to_string()),
            };
            out.push((slice_name.to_string(), kind.to_string(), id, label));
        }
    }
    let clip_facts = ClipMenu {
        clip: 0,
        multi: false,
        linked: false,
        grouped: false,
        enabled: true,
        can_time_remap: false,
        has_audio: false,
        can_extract_audio: false,
        can_freeze_frame: false,
        parented: false,
        transition_alignment: None,
    };
    let track_facts = TrackMenu {
        track: 0,
        muted: false,
        audio: false,
        row: 0,
        lanes: 1,
        lane: aria_models::TrackDisplay::default(),
    };
    let pool_facts = PoolMenu {
        media: 0,
        in_use: false,
        has_proxy: false,
    };
    let recent_facts = PathBuf::new();
    let effect_facts = EffectMenu {
        clip: 0,
        index: 0,
        copied: false,
    };
    let badge_facts = BadgeMenu {
        clip: 0,
        track: 0,
        audio: false,
        drawn: None,
        chosen: false,
        properties: std::sync::Arc::from([]),
    };
    let mut expected_ctx_items = Vec::new();
    collect_ctx_items("PREVIEW_ITEMS", PREVIEW_ITEMS, &(), &mut expected_ctx_items);
    collect_ctx_items(
        "CLIP_ITEMS",
        CLIP_ITEMS,
        &clip_facts,
        &mut expected_ctx_items,
    );
    collect_ctx_items(
        "TRANSITION_ITEMS",
        TRANSITION_ITEMS,
        &clip_facts,
        &mut expected_ctx_items,
    );
    collect_ctx_items(
        "TRACK_ITEMS",
        TRACK_ITEMS,
        &track_facts,
        &mut expected_ctx_items,
    );
    collect_ctx_items(
        "POOL_ITEMS",
        POOL_ITEMS,
        &pool_facts,
        &mut expected_ctx_items,
    );
    collect_ctx_items(
        "RECENT_ITEMS",
        RECENT_ITEMS,
        &recent_facts,
        &mut expected_ctx_items,
    );
    collect_ctx_items(
        "EFFECT_ITEMS",
        EFFECT_ITEMS,
        &effect_facts,
        &mut expected_ctx_items,
    );
    collect_ctx_items(
        "BADGE_ITEMS",
        BADGE_ITEMS,
        &badge_facts,
        &mut expected_ctx_items,
    );
    let documented_ctx_items: Vec<(String, String, String, String)> = table_rows(section_slice(
        MENUS_DOC,
        "Context menu item tables (`PREVIEW_ITEMS`, `CLIP_ITEMS`, `TRANSITION_ITEMS`, `TRACK_ITEMS`, `POOL_ITEMS`, `RECENT_ITEMS`, `EFFECT_ITEMS`, `BADGE_ITEMS`)",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 4, "malformed context menu row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
            strip_ticks(cells[2]).to_string(),
            strip_ticks(cells[3]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_ctx_items, expected_ctx_items,
        "docs/reference/menus.md Context menu item tables drifted from context_menu slices"
    );
}

/// Every valid menu path (`Menu > Item`, `Menu > Submenu > Item`, and `Menu > Submenu`)
/// produced by `items_for(menu, &state)` across `AppMenu::ALL`.
pub(super) fn shipped_menu_paths() -> BTreeSet<String> {
    let dir = crate::scratch_pool::scratch("guide-menus-doc");
    let state = AppState::new(PluginContext::new(), dir).expect("build headless AppState");
    let mut paths = BTreeSet::new();
    for menu in AppMenu::ALL {
        let menu_name = menu.label();
        for item in items_for(menu, &state).iter() {
            match item.under {
                Some(sub) => {
                    paths.insert(format!("{menu_name} > {sub}"));
                    paths.insert(format!("{menu_name} > {sub} > {}", item.label));
                }
                None => {
                    paths.insert(format!("{menu_name} > {}", item.label));
                }
            }
        }
    }
    paths
}

/// Every registered Command Palette command id (`pal-*`) mapped to its display label.
pub(super) fn shipped_palette_commands() -> BTreeMap<&'static str, &'static str> {
    BUILTIN_COMMANDS
        .iter()
        .map(|entry| (entry.id, entry.label))
        .chain(
            BUILTIN_JOB_COMMANDS
                .iter()
                .map(|entry| (entry.id, entry.label)),
        )
        .collect()
}

/// Closes the class where a `config.toml` key, `[schema]` version,
/// `ui-settings.json` field, or project canvas setting changes in
/// `aria-settings` or `aria-ui` without updating `docs/reference/settings.md`.
/// Does not catch prose wording in the Effect column.
#[test]
fn settings_doc_matches_registries() {
    let settings_doc_buf = read_doc("reference/settings.md");
    #[allow(non_snake_case)]
    let SETTINGS_DOC = settings_doc_buf.as_str();
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
    for v in ThemeChoice::ALL {
        let s = format!("\"{}\"", v.key());
        assert!(
            toml_bounds["appearance.theme"].contains(&s),
            "appearance.theme bounds missing ThemeChoice {s}"
        );
    }
    for v in AiSource::ALL {
        let s = format!("\"{}\"", v.key());
        assert!(
            toml_bounds["ai.source"].contains(&s),
            "ai.source bounds missing AiSource {s}"
        );
    }
    for v in AiApiProtocol::ALL {
        let s = format!("\"{}\"", v.key());
        assert!(
            toml_bounds["ai.api_protocol"].contains(&s),
            "ai.api_protocol bounds missing AiApiProtocol {s}"
        );
    }
    for v in ReasoningSummary::ALL {
        let s = format!("\"{}\"", v.key());
        assert!(
            toml_bounds["ai.reasoning_summary"].contains(&s),
            "ai.reasoning_summary bounds missing ReasoningSummary {s}"
        );
    }
    for v in Autonomy::ALL {
        let s = format!("\"{}\"", v.key());
        assert!(
            toml_bounds["ai.autonomy"].contains(&s),
            "ai.autonomy bounds missing Autonomy {s}"
        );
    }

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

    // Config refusal reasons (`Refusal::ALL`).
    let expected_refusals: Vec<(String, String)> = aria_settings::Refusal::ALL
        .iter()
        .map(|&r| {
            match r {
                aria_settings::Refusal::WrongType
                | aria_settings::Refusal::NotOneOf
                | aria_settings::Refusal::NotAbsolute
                | aria_settings::Refusal::OutOfRange => {}
            }
            (format!("{r:?}"), r.reason().to_string())
        })
        .collect();
    let documented_refusals: Vec<(String, String)> = parse_table_rows(section_slice(
        SETTINGS_DOC,
        "Config refusal reasons (`Refusal::ALL`)",
    ))
    .iter()
    .map(|cells| {
        assert!(cells.len() >= 2, "malformed Refusal row: {cells:?}");
        (
            strip_ticks(cells[0]).to_string(),
            strip_ticks(cells[1]).to_string(),
        )
    })
    .collect();
    assert_eq!(
        documented_refusals, expected_refusals,
        "docs/reference/settings.md Config refusal reasons drifted from aria_settings::Refusal::ALL"
    );

    // Storage cache registry (`CacheId::ALL`).
    let expected_caches: Vec<(String, String, String, String, String, String)> =
        aria_storage::CacheId::ALL
            .iter()
            .map(|&id| {
                let d = id.descriptor();
                (
                    format!("{id:?}"),
                    id.as_str().to_string(),
                    d.label.to_string(),
                    format!("{:?}", d.kind),
                    format!("{:?}", d.tier),
                    d.default_relative.unwrap_or("none").to_string(),
                )
            })
            .collect();
    let documented_caches: Vec<(String, String, String, String, String, String)> =
        parse_table_rows(section_slice(
            SETTINGS_DOC,
            "Storage cache registry (`CacheId::ALL`)",
        ))
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 6, "malformed CacheId row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[1]).to_string(),
                strip_ticks(cells[2]).to_string(),
                strip_ticks(cells[3]).to_string(),
                strip_ticks(cells[4]).to_string(),
                strip_ticks(cells[5]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented_caches, expected_caches,
        "docs/reference/settings.md Storage cache registry drifted from aria_storage::CacheId::ALL"
    );

    // Settings dialog text fields and numeric limits (`SettingsField::ALL`, `TimeoutField::ALL`, `WasmLimit::ALL`).
    let fields_section = section_slice(
        SETTINGS_DOC,
        "Settings dialog text fields and numeric limits (`SettingsField::ALL`, `TimeoutField::ALL`, `WasmLimit::ALL`)",
    );
    let field_rows = parse_table_rows(fields_section);
    assert_eq!(
        field_rows.len(),
        SettingsField::ALL.len() + TimeoutField::ALL.len() + WasmLimit::ALL.len(),
        "Settings dialog text fields and numeric limits row count drifted"
    );
    let expected_settings_fields: Vec<(String, String)> = SettingsField::ALL
        .iter()
        .map(|&f| {
            match f {
                SettingsField::AiBaseUrl
                | SettingsField::AiModel
                | SettingsField::AiKey
                | SettingsField::AiKeyEnv
                | SettingsField::CloudBaseUrl
                | SettingsField::KeymapName
                | SettingsField::Filter
                | SettingsField::Plugin(_) => {}
            }
            (format!("{f:?}"), f.id().to_string())
        })
        .collect();
    let documented_settings_fields: Vec<(String, String)> = field_rows[..SettingsField::ALL.len()]
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 3, "malformed SettingsField row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[1]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented_settings_fields, expected_settings_fields,
        "docs/reference/settings.md SettingsField table drifted from SettingsField::ALL"
    );

    let expected_timeout_fields: Vec<(String, String, String)> = TimeoutField::ALL
        .iter()
        .map(|&(f, id, label)| {
            match f {
                TimeoutField::Connect | TimeoutField::Read | TimeoutField::Total => {}
            }
            (format!("{f:?}"), id.to_string(), label.to_string())
        })
        .collect();
    let timeout_start = SettingsField::ALL.len();
    let timeout_end = timeout_start + TimeoutField::ALL.len();
    let documented_timeout_fields: Vec<(String, String, String)> = field_rows
        [timeout_start..timeout_end]
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 3, "malformed TimeoutField row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[1]).to_string(),
                strip_ticks(cells[2]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented_timeout_fields, expected_timeout_fields,
        "docs/reference/settings.md TimeoutField table drifted from TimeoutField::ALL"
    );

    let expected_wasm_limits: Vec<(String, String, String)> = WasmLimit::ALL
        .iter()
        .map(|&(l, id, label)| {
            match l {
                WasmLimit::Fuel | WasmLimit::Memory => {}
            }
            (format!("{l:?}"), id.to_string(), label.to_string())
        })
        .collect();
    let documented_wasm_limits: Vec<(String, String, String)> = field_rows[timeout_end..]
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 3, "malformed WasmLimit row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[1]).to_string(),
                strip_ticks(cells[2]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented_wasm_limits, expected_wasm_limits,
        "docs/reference/settings.md WasmLimit table drifted from WasmLimit::ALL"
    );

    // Playback drop causes and decoder output modes (`DropCause::ALL` and `OutputMode::ALL`).
    let drop_output_section = section_slice(
        SETTINGS_DOC,
        "Playback drop causes and decoder output modes (`DropCause::ALL` and `OutputMode::ALL`)",
    );
    let drop_output_rows = parse_table_rows(drop_output_section);
    assert_eq!(
        drop_output_rows.len(),
        aria_render::DropCause::ALL.len() + aria_decoder::OutputMode::ALL.len(),
        "Playback drop causes and decoder output modes row count drifted"
    );
    let expected_drop_causes: Vec<(String, String)> = aria_render::DropCause::ALL
        .iter()
        .map(|&c| {
            match c {
                aria_render::DropCause::DecodeLate
                | aria_render::DropCause::RenderLate
                | aria_render::DropCause::PresentLate => {}
            }
            (format!("{c:?}"), c.label().to_string())
        })
        .collect();
    let documented_drop_causes: Vec<(String, String)> = drop_output_rows
        [..aria_render::DropCause::ALL.len()]
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 2, "malformed DropCause row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[1]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented_drop_causes, expected_drop_causes,
        "docs/reference/settings.md DropCause table drifted from aria_render::DropCause::ALL"
    );

    let expected_output_modes: Vec<(String, String)> = aria_decoder::OutputMode::ALL
        .iter()
        .map(|&m| {
            match m {
                aria_decoder::OutputMode::Gpu | aria_decoder::OutputMode::Cpu => {}
            }
            (format!("{m:?}"), m.label().to_string())
        })
        .collect();
    let documented_output_modes: Vec<(String, String)> = drop_output_rows
        [aria_render::DropCause::ALL.len()..]
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 2, "malformed OutputMode row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[1]).to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented_output_modes, expected_output_modes,
        "docs/reference/settings.md OutputMode table drifted from aria_decoder::OutputMode::ALL"
    );

    let expected_resolutions: Vec<(String, String, String, String, String, bool)> = Resolution::ALL
        .iter()
        .map(|&r| {
            match r {
                Resolution::Full | Resolution::Half | Resolution::Quarter | Resolution::Eighth => {}
            }
            (
                format!("{r:?}"),
                format!("{}", r.divisor()),
                r.label().to_string(),
                format!("{:?}", r.lower()),
                format!("{:?}", r.higher()),
                r == Resolution::default(),
            )
        })
        .collect();
    let documented_resolutions: Vec<(String, String, String, String, String, bool)> =
        parse_table_rows(section_slice(
            SETTINGS_DOC,
            "Render resolution levels (`Resolution::ALL`)",
        ))
        .iter()
        .map(|cells| {
            assert!(cells.len() >= 6, "malformed Resolution row: {cells:?}");
            (
                strip_ticks(cells[0]).to_string(),
                strip_ticks(cells[1]).to_string(),
                strip_ticks(cells[2]).to_string(),
                strip_ticks(cells[3]).to_string(),
                strip_ticks(cells[4]).to_string(),
                strip_ticks(cells[5]) == "yes",
            )
        })
        .collect();
    assert_eq!(
        documented_resolutions, expected_resolutions,
        "docs/reference/settings.md Render resolution levels drifted from Resolution::ALL"
    );
}

pub(super) fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>) {
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

pub(super) fn strip_inline_code(line: &str) -> String {
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

pub(super) fn extract_markdown_links(line: &str) -> Vec<String> {
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

pub(super) fn check_markdown_links_in_files(md_files: &[PathBuf]) -> (usize, Vec<String>) {
    let mut heading_cache: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut broken: Vec<String> = Vec::new();
    let mut checked_links = 0usize;

    for md_file in md_files {
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

    (checked_links, broken)
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

    let (checked_links, broken) = check_markdown_links_in_files(&md_files);

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

/// WHY: a pipe table continues to the first blank line or block start, so a
/// paragraph written on the line after the last row renders as one more row
/// of the table, its text in the first column. This walks each file under
/// `docs/` outside fenced blocks and fails on a table followed by a line that
/// is neither a row, a blank line, nor the start of another block. Not caught:
/// a row whose cell count differs from the header's.
#[test]
fn every_table_in_docs_ends_at_a_blank_line() {
    let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut md_files = Vec::new();
    collect_md_files(&docs_root, &mut md_files);
    let starts_block = |line: &str| {
        let t = line.trim_start();
        let ordered = t
            .split_once(". ")
            .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        ["#", "```", "~~~", ">", "- ", "* ", "+ ", "<"]
            .iter()
            .any(|p| t.starts_with(p))
            || ordered
    };
    let mut tables = 0usize;
    let mut runons: Vec<String> = Vec::new();
    for md_file in &md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        let lines: Vec<&str> = text.lines().collect();
        let mut in_fence = false;
        let mut in_table = false;
        for (idx, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                in_table = false;
                continue;
            }
            if in_fence {
                continue;
            }
            let row = line.trim_start().starts_with('|');
            if in_table {
                if row {
                    continue;
                }
                in_table = false;
                if !line.trim().is_empty() && !starts_block(line) {
                    runons.push(format!(
                        "{}:{}: {:?} continues the table above it",
                        md_file.display(),
                        idx + 1,
                        line.trim()
                    ));
                }
                continue;
            }
            let delimiter = lines.get(idx + 1).is_some_and(|next| {
                let next = next.trim();
                next.starts_with('|')
                    && next.contains('-')
                    && next.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
            });
            if row && delimiter {
                in_table = true;
                tables += 1;
            }
        }
    }
    assert!(
        tables >= 100,
        "expected at least 100 tables across docs/, found {tables}"
    );
    assert!(
        runons.is_empty(),
        "table rows run into the next paragraph in docs/:\n{}",
        runons.join("\n")
    );
}

/// WHY: the documentation register states the control, setting, or file
/// that acts, in flat sentences, so an em dash and the persona nouns "the
/// user" and "the operator" are out of the register. Rewriting the instances
/// leaves the class open, so this walks the repository README and every file
/// under `docs/` outside fenced blocks and inline code spans and fails on an
/// em dash or one of those phrases. Not caught: the other banned filler and
/// metaphor words, and a persona phrased another way, such as "users".
#[test]
fn docs_prose_has_no_em_dash_or_persona_noun() {
    const BANNED: [&str; 4] = ["\u{2014}", "the user", "the operator", "an operator"];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut md_files = vec![root.join("README.md")];
    collect_md_files(&root.join("docs"), &mut md_files);
    let mut prose_lines = 0usize;
    let mut found: Vec<String> = Vec::new();
    for md_file in &md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        let mut fence: Option<&str> = None;
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            match fence {
                Some(mark) if trimmed.starts_with(mark) => {
                    fence = None;
                    continue;
                }
                Some(_) => continue,
                None => {
                    if let Some(mark) = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m))
                    {
                        fence = Some(mark);
                        continue;
                    }
                }
            }
            prose_lines += 1;
            let prose = strip_inline_code(line).to_lowercase();
            for banned in BANNED.iter().filter(|b| prose.contains(*b)) {
                found.push(format!(
                    "{}:{}: {banned:?} in {:?}",
                    md_file.display(),
                    idx + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        prose_lines >= 10_000,
        "expected at least 10000 prose lines across docs/ and README.md, read {prose_lines}"
    );
    assert!(
        found.is_empty(),
        "docs prose outside the register:\n{}",
        found.join("\n")
    );
}

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
