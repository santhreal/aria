//! `docs/reference/settings.md` coverage checks.
//!
//! The `config.toml` keys, `[schema]` versions, `ui-settings.json` fields, and
//! project canvas settings are compared against `aria-settings` and `aria-ui`
//! in both directions: a persisted setting added without a row fails the
//! suite, and a row for a dead or renamed setting fails the suite.
//!
//! Not covered: whether the Effect column describes the setting accurately
//! beyond its default value and bounds.

use std::collections::{BTreeMap, BTreeSet};

use aria_models::{AudioLayout, CanvasAspect, CanvasSettings, TimeDisplay};
use aria_settings::{AiApiProtocol, AiSource, Autonomy, ReasoningSummary, ThemeChoice};

use crate::app::models::TimeoutField;
use crate::app::plugin_install::WasmLimit;
use crate::app::project_settings::CANVAS_BASELINES;
use crate::app::settings_form::SettingsField;
use crate::appearance::AppearanceMode;
use crate::cloud::BYOK_PROVIDERS;
use crate::motion::MotionPreference;
use crate::resolution::{PausedResolution, PlaybackResolution, Resolution};
use crate::scopes::WaveformMode;
use crate::settings::{
    INSPECTOR_MAX, INSPECTOR_MIN, OUTLINE_MAX, OUTLINE_MIN, SIDEBAR_MAX, SIDEBAR_MIN,
    TIMELINE_MIN_HEIGHT, UiSettings, WindowPlacement,
};
use crate::surfaces::settings_pick::RATES;
use crate::test_support::doc_tables::{parse_table_rows, read_doc, section_slice, strip_ticks};
use crate::timeline_display::{Follow, Thumbnails, TimelineDisplay, WaveformScale, ZoomBarKnobs};
use crate::types::PoolView;

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
        zoom_bar_knobs,
        waveform,
        ref favorite_effects,
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
        favorite_effects,
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
        zoom_bar_knobs,
        waveform,
        favorite_effects: Vec::new(),
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
        ui_defaults["zoomBarKnobs"],
        serde_json::to_string(&zoom_bar_knobs).unwrap()
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
    for v in ZoomBarKnobs::ALL {
        let s = serde_json::to_string(&v).unwrap();
        assert!(
            ui_ranges["zoomBarKnobs"].contains(&s),
            "missing zoomBarKnobs {s}"
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
