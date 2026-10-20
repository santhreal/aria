# Reference Manual

Complete reference for Aria's built-in catalogs, user interface controls, menus, palette commands, workspaces, panels, and persisted settings. Every chapter is checked against the source registries by automated tests.

## Chapters

| Chapter | Scope | Enforcing test suite |
| --- | --- | --- |
| [effects.md](effects.md) | 12 GPU video effects (`crates/aria-models/data/effects.toml` and `crates/aria-compositor/shaders/`) | `aria_models::catalog_doc_tests::video_effects_reference_matches_shipped_catalog` |
| [generators.md](generators.md) | 8 clip content generators and 7 GPU effect generators (`crates/aria-models/src/clip/generator_catalog.rs` and `effects.toml`) | `aria_models::catalog_doc_tests::generators_reference_matches_shipped_catalogs` |
| [transitions.md](transitions.md) | 8 video transitions and 3 audio crossfades (`crates/aria-models/data/transitions.toml` and `AudioCrossfadeKind::ALL`) | `aria_models::catalog_doc_tests::transitions_reference_matches_shipped_catalog` |
| [audio-effects.md](audio-effects.md) | 14 built-in audio DSP effects (`crates/aria-models/data/audio_effects.toml`) | `aria_models::catalog_doc_tests::audio_effects_reference_matches_shipped_catalog` |
| [filters.md](filters.md) | 16 colour filter presets and 62 colour grading sliders (`crates/aria-models/data/filters.toml` and `ColorAdjustments::SLIDERS`) | `aria_models::catalog_doc_tests::filters_and_sliders_reference_matches_shipped_catalogs` |
| [animations.md](animations.md) | 17 animations, 6 text style presets, 17 easing presets, and 6 speed-ramp presets (`crates/aria-models/data/`) | `aria_models::catalog_doc_tests::animations_text_effects_easings_and_speed_presets_match_catalogs` |
| [stickers-and-masks.md](stickers-and-masks.md) | 8 stickers, 7 mask shapes, 7 mask stack modes, and 7 animatable mask parameters (`stickers.toml` and `masks.toml`) | `aria_models::catalog_doc_tests::stickers_and_masks_reference_matches_catalogs` |
| [tools.md](tools.md) | 13 editor tools (`crates/aria-ui/src/tool_catalog.rs`) | `aria_ui::keys::reference_doc_tests::tools_doc_matches_registry` |
| [workspaces.md](workspaces.md) | 5 shipped workspaces and dock trees (`crates/aria-ui/src/layouts.rs`) | `aria_ui::keys::reference_doc_tests::workspaces_doc_matches_registry` |
| [panels.md](panels.md) | 24 panels, 15 dialogs/overlays, 15 Project tabs, and 5 Home rail views (`crates/aria-ui/src/plugins/ui.rs`) | `aria_ui::keys::reference_doc_tests::panels_doc_matches_registry` |
| [menus.md](menus.md) | 10 application menus and all menu rows (`crates/aria-ui/src/chrome/menu_items.rs`) | `aria_ui::keys::reference_doc_tests::menus_doc_matches_registry` |
| [commands.md](commands.md) | 149 state commands and 10 job commands (`crates/aria-ui/src/commands/`) | `aria_ui::keys::reference_doc_tests::commands_doc_matches_registry` |
| [settings.md](settings.md) | `config.toml` keys, `[schema]` versions, `ui-settings.json` fields, and project canvas settings (`crates/aria-settings/` and `crates/aria-ui/src/settings.rs`) | `aria_ui::keys::reference_doc_tests::settings_doc_matches_registries` |
| [../keyboard-shortcuts.md](../keyboard-shortcuts.md) | All shipped keyboard shortcuts (`SHIPPED_BINDINGS`) | `aria_ui::keys::doc_tests` |
| [../catalogs.md](../catalogs.md) | TOML catalog override file format and `FilterGrade` recipe keys | `aria_models::look::filter_grade_tests` |
| [../plugins/README.md](../plugins/README.md) | Plugin manifest (`plugin.toml`), WIT, C ABI, and Rust SDK reference | `aria_ui::test_support` |
