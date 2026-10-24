//! `docs/reference/panels.md` coverage checks.
//!
//! The panel, modal dialog, overlay, Project sidebar tab, and Home rail view
//! tables are compared against the runtime registries in `aria-ui` in both
//! directions: adding one without documenting it fails the suite, and
//! documenting a dead or renamed identifier fails the suite.
//!
//! Not covered: whether the Summary column describes the panel accurately.

use aria_plugin::PluginContext;

use crate::app::DialogKind;
use crate::app::analysis::AnalysisKind;
use crate::app::effects_browser::{EntryKind, Group};
use crate::app::generate::GenerateKind;
use crate::cloud::CloudFeed;
use crate::plugins::UiPlugin;
use crate::plugins::{overlay_host, panel_host};
use crate::test_support::doc_tables::{parse_table_rows, read_doc, section_slice, strip_ticks};
use crate::types::{
    ActivePanel, AgentEntryKind, HomeRail, MediaSort, OverlayMode, PoolView, SidebarGroup,
    SidebarTab,
};

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
            | DialogKind::MediaSupport
            | DialogKind::TimelineReport
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
