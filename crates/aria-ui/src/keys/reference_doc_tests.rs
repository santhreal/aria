//! Reference documentation coverage checks for tools, workspaces, menus, and
//! palette commands.
//!
//! Each test compares the markdown tables in `docs/reference/` against the
//! runtime registries in `aria-ui` in both directions: adding a tool,
//! workspace, menu item, or palette command without documenting it fails the
//! suite, and documenting a dead or renamed identifier fails the suite.
//!
//! Not covered: whether the prose summary column in a table describes the
//! handler body accurately beyond its structured fields (ID, label, group,
//! shortcut, and submenu).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use aria_models::MatteSource;
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::audio_channels::AudioChannels;
use crate::app::clip_ops::AudioGainMode;
use crate::app::decline::Decline;
use crate::chrome::menu_items::items_for;
use crate::commands::{BUILTIN_COMMANDS, BUILTIN_JOB_COMMANDS};
use crate::dock::DOCK_VERSION;
use crate::layouts::WORKSPACES;
use crate::surfaces::context_menu::{
    BADGE_ITEMS, BadgeMenu, CLIP_ITEMS, ClipMenu, EFFECT_ITEMS, EffectMenu, MenuItem, POOL_ITEMS,
    PREVIEW_ITEMS, PoolMenu, RECENT_ITEMS, TRACK_ITEMS, TRANSITION_ITEMS, TrackMenu,
};
use crate::test_support::doc_tables::{
    parse_table_rows, read_doc, section_slice, strip_ticks, table_rows,
};
use crate::tool_catalog::{ToolGroup, tool_specs};
use crate::types::{AppMenu, PenVariant};

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

/// Closes the class where a shipped workspace, its place in the
/// `Shift+Alt+<n>` slots, or its pane arrangement changes in `layouts.rs`, or
/// `DOCK_VERSION` changes in `dock/mod.rs`, without updating
/// `docs/reference/workspaces.md`.
/// Does not catch prose edits in the worked example section.
#[test]
fn workspaces_doc_matches_registry() {
    let rows = parse_table_rows(section_slice(WORKSPACES_DOC, "Shipped workspaces"));
    let expected: Vec<Vec<String>> = WORKSPACES
        .iter()
        .enumerate()
        .map(|(slot, ws)| {
            vec![
                ws.id.to_string(),
                ws.name.to_string(),
                format!("Shift+Alt+{}", slot + 1),
                format!("{}", ws.panes.sidebar_width as u32),
                format!("{}", ws.panes.sidebar_collapsed),
                format!("{}", ws.panes.inspector_width as u32),
                format!("{}", ws.panes.inspector_collapsed),
                format!("{}", ws.panes.timeline_height as u32),
                format!("{}", ws.panes.scopes_open),
                format!("{}", ws.panes.graph_open),
                format!("{:?}", ws.panes.panel),
                format!("{:?}", ws.panes.route),
            ]
        })
        .collect();
    let documented: Vec<Vec<String>> = rows
        .iter()
        .map(|cells| {
            cells
                .iter()
                .map(|cell| strip_ticks(cell).to_string())
                .collect()
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
