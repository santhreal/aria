//! Verification tests for code-backed claims in `docs/architecture.md`,
//! `docs/features.md`, `docs/install.md`, `docs/mcp-design.md`, and
//! `docs/plugins/worked-example.md`.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use aria_core::{Rational, RationalTime};
use aria_models::{
    LookParam, MAX_CLIP_MASKS, MarkerColor, MarkerKind, MaskMode, MediaId, ShapeParam, StyleParam,
    TextParam, audio_effect_catalog, clip_param_catalog, effect_catalog, generator_specs,
    transition_catalog,
};
use aria_plugin::{PluginContext, SettingsSchema};
use aria_render::LayerSource;

use crate::layouts::WORKSPACES;
use crate::scopes::{TARGET_LEVELS, WaveformMode};
use crate::test_support::doc_tables::{section_slice, strip_ticks, table_rows};
use crate::transport::ShuttleSpeed;
use crate::types::AppMenu;

/// WHY: closes the defect class where `docs/architecture.md` drifts from the
/// workspace `Cargo.toml` crate list, built-in plugin count, `rfd` dependency
/// boundary, decoder capabilities, command reach census, menu count, property
/// registry groups, or `LayerSource` topology count.
/// Does not catch narrative phrasing changes that keep all crate names, counts,
/// and registry identifiers identical.
#[test]
fn architecture_doc_claims_match_workspace_and_registries() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc_path = repo_root.join("docs/architecture.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", doc_path.display()));

    // 1. Bidirectional workspace crate check (`## Crates` vs root `Cargo.toml`).
    let cargo_toml =
        std::fs::read_to_string(repo_root.join("Cargo.toml")).expect("read workspace Cargo.toml");
    let mut workspace_crates = BTreeSet::new();
    for line in cargo_toml.lines() {
        let trimmed = line.trim().trim_matches(',').trim_matches('"');
        if let Some(name) = trimmed.strip_prefix("crates/") {
            workspace_crates.insert(name.to_string());
        } else if trimmed == "apps/aria-desktop" {
            workspace_crates.insert("apps/aria-desktop".to_string());
        }
    }

    let crates_section = section_slice(&doc, "Crates");
    let crates_rows = table_rows(crates_section);
    let mut doc_crates = BTreeSet::new();
    for row in &crates_rows {
        for part in row[0].split('/') {
            let token = strip_ticks(part.trim());
            if token == "aria-desktop" {
                doc_crates.insert("apps/aria-desktop".to_string());
            } else if token == "apps" {
                continue;
            } else if !token.is_empty() {
                doc_crates.insert(token.to_string());
            }
        }
    }
    assert_eq!(
        doc_crates, workspace_crates,
        "docs/architecture.md `## Crates` table must match workspace crates in Cargo.toml"
    );

    // 2. Built-in plugin count (`mount_all (17 built-in plugins...)`).
    let builtin_count = crate::plugins::builtin(Path::new(".")).len();
    assert!(
        doc.contains(&format!("mount_all ({builtin_count} built-in plugins")),
        "docs/architecture.md must state `mount_all ({builtin_count} built-in plugins`"
    );

    // 3. `aria-dialog` is the only crate that depends on `rfd`.
    assert!(
        doc.contains("`aria-dialog` is the only crate that depends on `rfd`"),
        "docs/architecture.md must document the `aria-dialog` `rfd` boundary"
    );
    let mut rfd_dependents = BTreeSet::new();
    for crate_dir in ["crates", "apps"] {
        let dir_path = repo_root.join(crate_dir);
        for entry in std::fs::read_dir(&dir_path).expect("read crate dir") {
            let entry = entry.expect("dir entry");
            let manifest = entry.path().join("Cargo.toml");
            if manifest.is_file() {
                let text = std::fs::read_to_string(&manifest).expect("read Cargo.toml");
                for line in text.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("rfd ")
                        || trimmed.starts_with("rfd=")
                        || trimmed.starts_with("rfd.")
                    {
                        rfd_dependents.insert(entry.file_name().to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
    assert_eq!(
        rfd_dependents,
        BTreeSet::from(["aria-dialog".to_string()]),
        "only `aria-dialog` may depend on `rfd`"
    );

    // 4. Decoder output modes table and host `decoder_capability()`.
    let cap = aria_decoder::decoder_capability();
    assert!(
        !cap.backend.is_empty(),
        "aria_decoder::decoder_capability().backend must be non-empty"
    );
    for backend in [
        "AVFoundation",
        "Media Foundation",
        "GStreamer",
        "MediaCodec",
    ] {
        assert!(
            doc.contains(backend),
            "docs/architecture.md must list decoder backend `{backend}`"
        );
    }

    // 5. Command reach census and menu count.
    let edit_cmds = aria_commands::every_edit_command();
    let proj_cmds = aria_commands::every_project_command();
    assert!(!edit_cmds.is_empty() && !proj_cmds.is_empty());
    let menu_count = AppMenu::ALL.len();
    assert!(
        doc.contains(&format!("| Menu rows, all {menu_count} menus |")),
        "docs/architecture.md must state `| Menu rows, all {menu_count} menus |`"
    );
    for harness in [
        "app/reach_tests.rs",
        "app/menu_reach_tests.rs",
        "app/windowed_reach_tests.rs",
        "app/operator_surface_tests.rs",
    ] {
        assert!(
            doc.contains(harness),
            "docs/architecture.md must list reach harness `{harness}`"
        );
        let harness_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(harness);
        assert!(
            harness_path.is_file(),
            "reach harness `{}` listed in docs/architecture.md does not exist",
            harness_path.display()
        );
    }

    // 6. Property registry groups and param enums.
    assert!(!LookParam::ALL.is_empty());
    assert!(!StyleParam::ALL.is_empty());
    assert!(!TextParam::ALL.is_empty());
    assert!(!ShapeParam::ALL.is_empty());

    let catalog = clip_param_catalog();
    let mut static_groups: BTreeSet<String> = BTreeSet::new();
    for spec in catalog {
        static_groups.insert(spec.group.to_string());
    }
    for idx in 0..MAX_CLIP_MASKS {
        let row = aria_models::property_row(&format!("mask-{idx}-feather"))
            .expect("mask-N-feather property_row must resolve for 0..MAX_CLIP_MASKS");
        assert_eq!(row.group, format!("mask-{idx}"));
    }
    assert_eq!(MAX_CLIP_MASKS, 32);
    assert!(
        doc.contains("`mask-0`..`mask-31`"),
        "docs/architecture.md must state `mask-0`..`mask-31`"
    );
    let expected_static_groups: BTreeSet<String> = [
        "transform",
        "framing",
        "audio",
        "filter",
        "lut",
        "grade",
        "chroma",
        "matte",
        "shadow",
        "glow",
        "outline",
        "background",
        "text",
        "shape",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(
        static_groups, expected_static_groups,
        "static groups in clip_param_catalog() must match docs/architecture.md"
    );
    for grp in &expected_static_groups {
        assert!(
            doc.contains(&format!("`{grp}`")),
            "docs/architecture.md must list static property group `{grp}`"
        );
    }

    // 7. Exhaustive LayerSource topology count (`12 shipped topology IDs`).
    let dummy_layer = || aria_render::SceneLayer {
        clip: None,
        source: LayerSource::Solid([0, 0, 0, 0]),
        center: [0.0, 0.0],
        anchor_point: [0.5, 0.5],
        size: aria_render::SizeSpec::Fixed([0.0, 0.0]),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: Vec::new(),
        masks: Vec::new(),
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: aria_models::BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    };
    let sample_sources = [
        LayerSource::Media {
            media: MediaId::from_raw(0),
            source_time: RationalTime::new(0, Rational::FPS_30),
        },
        LayerSource::Still {
            media: MediaId::from_raw(0),
        },
        LayerSource::Sticker {
            asset: String::new(),
            local_time: 0.0,
        },
        LayerSource::Lottie {
            path: String::new(),
            local_time: 0.0,
        },
        LayerSource::Text {
            content: String::new(),
            style: aria_text::TextStyle::new(48.0),
            animation: None,
        },
        LayerSource::Solid([0, 0, 0, 255]),
        LayerSource::CanvasPass,
        LayerSource::Shape {
            params: aria_shapes::SdfParams::Ellipse,
            fill: [255, 255, 255, 255],
            stroke: None,
            pad: 0.0,
        },
        LayerSource::PathShape {
            path: aria_shapes::BezierPath {
                points: Vec::new(),
                closed: false,
            },
            fill: [255, 255, 255, 255],
            stroke: None,
            raster_scale: 1.0,
        },
        LayerSource::Transition {
            outgoing: Box::new(dummy_layer()),
            incoming: Box::new(dummy_layer()),
            transition_id: String::new(),
            progress: 0.5,
        },
        LayerSource::Matte {
            layers: Box::new([dummy_layer(), dummy_layer()]),
            read: aria_models::MatteSource::Alpha,
            invert: false,
            softness: 0.0,
        },
        LayerSource::Nest {
            scene: Box::new(aria_render::Scene::empty(1, 1, [0, 0, 0, 0])),
            frame: [0.0, 0.0, 1.0, 1.0],
        },
    ];
    let mut kinds = BTreeSet::new();
    for src in &sample_sources {
        // Exhaustive match guarantees a compile error if a LayerSource variant is added.
        match src {
            LayerSource::Media { .. }
            | LayerSource::Still { .. }
            | LayerSource::Sticker { .. }
            | LayerSource::Lottie { .. }
            | LayerSource::Text { .. }
            | LayerSource::Solid(_)
            | LayerSource::CanvasPass
            | LayerSource::Shape { .. }
            | LayerSource::PathShape { .. }
            | LayerSource::Transition { .. }
            | LayerSource::Matte { .. }
            | LayerSource::Nest { .. } => {}
        }
        kinds.insert(src.kind());
    }
    let topology_count = kinds.len();
    assert_eq!(topology_count, 12);
    assert!(
        doc.contains(&format!("{topology_count} shipped topology IDs")),
        "docs/architecture.md must state `{topology_count} shipped topology IDs` (scene_layers::SHIPPED)"
    );
    assert!(!aria_compositor::shipped_effect_descriptors().is_empty());
    assert!(!aria_compositor::shipped_transition_descriptors().is_empty());
}

/// WHY: closes the defect class where `docs/features.md` states stale catalog
/// counts, limits, scope targets, shuttle speeds, workspace layouts, menus, or
/// cross-linked documents.
/// Does not catch unquantified prose descriptions when counts and identifiers match.
#[test]
fn features_doc_claims_match_catalogs_and_capabilities() {
    let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let doc_path = docs_root.join("features.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", doc_path.display()));

    // Markers.
    assert_eq!(MarkerColor::ALL.len(), 8);
    assert!(doc.contains("eight colours"));
    let marker_kinds: Vec<&str> = MarkerKind::ALL.iter().map(|k| k.label()).collect();
    assert_eq!(marker_kinds, vec!["Comment", "Chapter", "Segmentation"]);
    assert!(doc.contains("Comment/Chapter/Segmentation kinds"));

    // Shuttle speed & scopes.
    assert_eq!(ShuttleSpeed::MAX_HUNDREDTHS, 800);
    assert!(doc.contains("J/K/L shuttle to 8x"));
    assert_eq!(TARGET_LEVELS, [191, 255]);
    assert!(doc.contains("75 % and 100 %"));
    assert_eq!(WaveformMode::ALL.len(), 3);
    assert!(doc.contains("luma, an RGB parade, or a YCbCr parade"));

    // Catalog counts.
    assert_eq!(effect_catalog().len(), 19);
    assert_eq!(transition_catalog().len(), 8);
    assert!(doc.contains("Nineteen effects and eight transitions"));

    assert_eq!(TextParam::ALL.len(), 11);
    assert!(doc.contains("the eleven text properties"));
    assert_eq!(MAX_CLIP_MASKS, 32);
    assert!(doc.contains("up to 32 masks"));
    let mask_mode_labels: Vec<&str> = MaskMode::ALL.iter().map(|m| m.label()).collect();
    assert_eq!(
        mask_mode_labels,
        vec![
            "None",
            "Add",
            "Subtract",
            "Intersect",
            "Lighten",
            "Darken",
            "Difference"
        ]
    );
    assert!(doc.contains("mode (None, Add, Subtract, Intersect, Lighten, Darken, Difference)"));
    let gen_ids: Vec<&str> = generator_specs().iter().map(|g| g.id).collect();
    assert_eq!(
        gen_ids,
        vec![
            "text",
            "solid",
            "shape",
            "sticker",
            "lottie",
            "effect",
            "filter",
            "adjustment"
        ]
    );
    assert!(doc.contains(
        "Shape generator: rounded rectangle, ellipse, polygon, star, line, arrow, and\n  heart"
    ));
    assert_eq!(audio_effect_catalog().len(), 14);
    assert!(doc.contains("Parametric EQ, Compressor with a sidechain key, Limiter, De-esser"));

    // Menus and workspaces.
    assert_eq!(AppMenu::ALL.len(), 10);
    for menu in AppMenu::ALL {
        assert!(
            doc.contains(menu.label()),
            "docs/features.md must list menu `{}`",
            menu.label()
        );
    }

    assert_eq!(WORKSPACES.len(), 5);
    let ws_labels: Vec<&str> = WORKSPACES.iter().map(|w| w.name).collect();
    assert_eq!(
        ws_labels,
        vec!["Editing", "Animation", "Color", "Audio", "Effects"]
    );
    assert!(doc.contains("Five shipped workspaces (Editing, Animation, Color, Audio, Effects)"));

    for rel in [
        "getting-started.md",
        "guide/README.md",
        "reference/README.md",
        "keyboard-shortcuts.md",
        "command-line.md",
        "architecture.md",
    ] {
        assert!(doc.contains(rel), "docs/features.md must link to `{rel}`");
        assert!(
            docs_root.join(rel).is_file(),
            "linked file `docs/{rel}` must exist on disk"
        );
    }
}

/// WHY: closes the defect class where `docs/install.md` drifts from
/// `packaging/linux/install.sh` package lists, GStreamer encoder element names
/// in `crates/aria-encoder/src/gst.rs`, or `rust-version` in root `Cargo.toml`.
#[test]
fn install_doc_claims_match_packaging_and_manifests() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc_path = repo_root.join("docs/install.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", doc_path.display()));

    assert!(doc.contains("~/.aria/config.toml"));
    assert!(doc.contains("`.aria`"));

    let install_sh = std::fs::read_to_string(repo_root.join("packaging/linux/install.sh"))
        .expect("read packaging/linux/install.sh");
    for token in ["./install.sh", "--prefix", "--uninstall", "~/.local"] {
        assert!(
            doc.contains(token),
            "docs/install.md must document install.sh token `{token}`"
        );
        assert!(
            install_sh.contains(token),
            "packaging/linux/install.sh must contain `{token}`"
        );
    }
    for artifact in [
        "aria-desktop",
        "aria.desktop",
        "aria.png",
        "aria.xml",
        "update-desktop-database",
        "update-mime-database",
        "gtk-update-icon-cache",
    ] {
        assert!(
            install_sh.contains(artifact),
            "packaging/linux/install.sh must manage `{artifact}`"
        );
    }

    for pkg in [
        "gstreamer1.0-plugins-base",
        "gstreamer1.0-plugins-good",
        "gstreamer1.0-plugins-ugly",
        "gstreamer1.0-libav",
    ] {
        assert!(
            doc.contains(pkg),
            "docs/install.md must list GStreamer package `{pkg}`"
        );
    }

    let gst_rs = std::fs::read_to_string(repo_root.join("crates/aria-encoder/src/gst.rs"))
        .expect("read crates/aria-encoder/src/gst.rs");
    for elem in ["x264enc", "x265enc"] {
        assert!(
            doc.contains(elem),
            "docs/install.md must document GStreamer software encoder `{elem}`"
        );
        assert!(
            gst_rs.contains(&format!("\"{elem}\"")),
            "crates/aria-encoder/src/gst.rs must contain encoder element `{elem}`"
        );
    }
    for hw_elem in ["nvh264enc", "vaapih264enc", "nvh265enc", "vaapih265enc"] {
        assert!(
            gst_rs.contains(&format!("\"{hw_elem}\"")),
            "crates/aria-encoder/src/gst.rs must contain hardware encoder `{hw_elem}`"
        );
    }

    let cargo_toml =
        std::fs::read_to_string(repo_root.join("Cargo.toml")).expect("read root Cargo.toml");
    assert!(
        doc.contains("cargo run -p aria-desktop") && cargo_toml.contains("apps/aria-desktop"),
        "docs/install.md `cargo run -p aria-desktop` must match workspace member `apps/aria-desktop`"
    );
    assert!(
        doc.contains("https://github.com/santhreal/santh-gpui")
            && cargo_toml.contains("https://github.com/santhreal/santh-gpui"),
        "docs/install.md santh-gpui link must match root Cargo.toml"
    );
}
/// `ActionLogEntry`, `AgentConfig` caps, or host-tool name validation).
/// Does not test unimplemented Workstream 11 MCP transport runtime.
#[test]
fn mcp_design_doc_claims_match_aria_ai() {
    let doc_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/mcp-design.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", doc_path.display()));

    const { assert!(aria_ai::TOOL_SCHEMA_VERSION >= 1) };
    assert!(doc.contains("`TOOL_SCHEMA_VERSION`"));

    let specs = aria_ai::wire::tool_specs();
    assert!(!specs.is_empty());
    assert!(doc.contains("`WireCommand`"));

    let step_type = std::any::type_name::<aria_ai::ActionLogEntry>();
    assert!(
        step_type.ends_with("ActionLogEntry"),
        "expected ActionLogEntry type name, got {step_type}"
    );
    assert!(
        doc.contains("`ActionLogEntry`"),
        "docs/mcp-design.md must reference `ActionLogEntry`"
    );

    let cfg = aria_ai::AgentConfig::default();
    assert_eq!(cfg.max_tool_calls, 1000);
    assert!(
        !aria_ai::tools::is_host_tool_name("mcp__server__tool")
            && aria_ai::tools::is_host_tool_name("plugin_lut_preview"),
        "mcp__<server>__<tool> is fenced outside existing host tool namespaces"
    );
    assert!(doc.contains("`mcp__<server>__<tool>`"));
}

/// WHY: closes the defect class where the expanded `plugin.toml` in
/// `docs/plugins/worked-example.md` fails `PackageManifest::parse` or fails to
/// register its deferred effect, command, menu placements, key chords, setting,
/// and sidebar panel via `PluginContext::defer_package`.
/// Does not compile external WASM/native crates (covered by `aria-cli` e2e test).
#[test]
fn worked_example_doc_manifest_mounts_deferred_contributions() {
    crate::test_support::catalogs::own_process(|| {
        let doc_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins/worked-example.md");
        let doc = std::fs::read_to_string(&doc_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", doc_path.display()));

        let mut toml_blocks = Vec::new();
        let mut wgsl_blocks = Vec::new();
        let mut in_toml = false;
        let mut in_wgsl = false;
        let mut buf = String::new();
        for line in doc.lines() {
            if let Some(tag) = line.strip_prefix("```") {
                if in_toml {
                    toml_blocks.push(std::mem::take(&mut buf));
                    in_toml = false;
                } else if in_wgsl {
                    wgsl_blocks.push(std::mem::take(&mut buf));
                    in_wgsl = false;
                } else {
                    match tag.trim() {
                        "toml" => in_toml = true,
                        "wgsl" => in_wgsl = true,
                        _ => {}
                    }
                    buf.clear();
                }
            } else if in_toml || in_wgsl {
                buf.push_str(line);
                buf.push('\n');
            }
        }
        assert_eq!(
            toml_blocks.len(),
            2,
            "expected 2 toml blocks in worked-example.md"
        );
        assert_eq!(
            wgsl_blocks.len(),
            1,
            "expected 1 wgsl block in worked-example.md"
        );

        let scratch_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.internal/agents/DocsEnum/scratch-worked-example");
        let _ = std::fs::remove_dir_all(&scratch_root);
        let pkg_dir = scratch_root.join("com.example.color-tools");
        std::fs::create_dir_all(pkg_dir.join("shaders")).expect("mkdir shaders");
        std::fs::write(pkg_dir.join("plugin.toml"), &toml_blocks[1]).expect("write plugin.toml");
        std::fs::write(pkg_dir.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("write plugin.wasm");
        std::fs::write(pkg_dir.join("shaders/color-tools.wgsl"), &wgsl_blocks[0])
            .expect("write shader");

        let pkg = aria_plugin::package::Package::open(&pkg_dir)
            .expect("Package::open on worked-example.md manifest and shader");
        let ctx = PluginContext::new();
        let schema = Arc::new(SettingsSchema::default());
        let _held_schema = ctx.register_arc(Arc::clone(&schema));
        ctx.defer_package(&pkg, |_pkg| {
            Err("worked-example deferred activation loader not invoked in declaration check".into())
        })
        .expect("defer_package must register declarations from worked-example.md");

        assert!(
            ctx.commands()
                .iter()
                .any(|c| c.id == "com.example.color-tools.run" && c.title == "Run Color Tools"),
            "deferred command `com.example.color-tools.run` must be registered"
        );
        assert_eq!(
            ctx.menu_entries().len(),
            2,
            "Sequence and Clip menu entries must be registered from worked-example.md"
        );
        assert_eq!(
            ctx.key_defaults().len(),
            2,
            "Ctrl+Alt+L and Ctrl+Shift+L key defaults must be registered from worked-example.md"
        );
        let section = schema
            .section_of("com.example.color-tools")
            .expect("settings section for com.example.color-tools");
        assert_eq!(section.fields.len(), 1);
        assert_eq!(
            section.fields[0].key,
            "com.example.color-tools.default_gain"
        );
        assert!(
            ctx.waiting_panels()
                .iter()
                .any(|p| p.spec.id == "com.example.color-tools.panel"),
            "waiting sidebar panel `com.example.color-tools.panel` must be registered"
        );
        let _ = std::fs::remove_dir_all(&scratch_root);
    });
}
