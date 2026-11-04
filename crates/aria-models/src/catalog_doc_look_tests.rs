use crate::{
    BlendMode, ColorAdjustments, EASING_PRESETS, Easing, MASK_PATH_VERSION, MAX_CLIP_MASKS,
    MAX_MASK_EXPANSION, MAX_MASK_PATH_POINTS, MaskKind, MaskMode, MaskParam, ParamKind, ParamRange,
    animation_catalog, filter_catalog, mask_catalog, piecewise_presets, speed_preset_catalog,
    sticker_catalog, text_effect_catalog,
};

use super::*;

const FILTERS_DOC: &str = include_str!("../../../docs/reference/filters.md");
const ANIMATIONS_DOC: &str = include_str!("../../../docs/reference/animations.md");
const STICKERS_MASKS_DOC: &str = include_str!("../../../docs/reference/stickers-and-masks.md");
const BLEND_COMPOSITE_WGSL: &str =
    include_str!("../../aria-compositor/shaders/blend_composite.wgsl");

/// **Every shipped filter preset and `ColorAdjustments::SLIDERS` row matches `docs/reference/filters.md`.**
///
/// Closes: undocumented filter preset or grade slider, stale entry, or drifted
/// slider `(id, name, label, kind, min, max, default)`.
/// Not caught: prose descriptions of color math.
#[test]
fn filters_and_sliders_reference_matches_shipped_catalogs() {
    let filters = filter_catalog();
    let filter_sec = section(FILTERS_DOC, "## Filter presets (`filters.toml`)");
    let summary_rows = table_rows(filter_sec.split("\n### ").next().unwrap_or(filter_sec));
    let summary_ids: Vec<(&str, &str)> =
        summary_rows.iter().map(|r| (unquote(r[0]), r[1])).collect();
    let expected_filters: Vec<(&str, &str)> = filters
        .iter()
        .map(|f| (f.id.as_str(), f.label.as_str()))
        .collect();
    assert_eq!(
        summary_ids, expected_filters,
        "docs/reference/filters.md filter summary table does not match filter_catalog()"
    );

    let sections = entry_sections(filter_sec);
    let section_ids: Vec<(&str, &str)> =
        sections.iter().map(|&(id, label, _)| (id, label)).collect();
    assert_eq!(
        section_ids, expected_filters,
        "docs/reference/filters.md ### filter sections do not match filter_catalog()"
    );

    let sliders_sec = section(
        FILTERS_DOC,
        "## Color grading sliders (`ColorAdjustments::SLIDERS`)",
    );
    let slider_rows = table_rows(sliders_sec);
    let doc_ids: Vec<&str> = slider_rows.iter().map(|r| unquote(r[0])).collect();
    let expected_ids: Vec<&str> = ColorAdjustments::SLIDERS.iter().map(|s| s.id).collect();
    assert_eq!(
        doc_ids, expected_ids,
        "docs/reference/filters.md slider IDs do not match ColorAdjustments::SLIDERS"
    );

    for (row, slider) in slider_rows.iter().zip(ColorAdjustments::SLIDERS.iter()) {
        let ctx = format!("slider {}", slider.id);
        assert_eq!(unquote(row[0]), slider.id, "{ctx}: id mismatch");
        assert_eq!(unquote(row[1]), slider.name, "{ctx}: recipe key mismatch");
        assert_eq!(row[2], slider.label, "{ctx}: label mismatch");
        assert_eq!(unquote(row[3]), "scalar", "{ctx}: kind mismatch");
        let min = parse_f32(row[4], &ctx);
        let max = parse_f32(row[5], &ctx);
        let default = parse_f32(row[6], &ctx);
        assert!((min - slider.min).abs() <= 1e-5, "{ctx}: min mismatch");
        assert!((max - slider.max).abs() <= 1e-5, "{ctx}: max mismatch");
        assert!(
            (default - slider.neutral).abs() <= 1e-5,
            "{ctx}: default {default} != neutral {}",
            slider.neutral
        );
    }
}

/// **Every animation, text style preset, easing preset, and speed preset matches `docs/reference/animations.md`.**
///
/// Closes: undocumented or stale entry in `animations.toml`, `text_effects.toml`,
/// `easings.toml`, `PiecewiseEasingPreset`, or `speed_presets.toml`.
/// Not caught: prose descriptions of curve formulas.
#[test]
fn animations_text_effects_easings_and_speed_presets_match_catalogs() {
    let whole_sec = section(
        ANIMATIONS_DOC,
        "### Whole-layer animations (`text_only = false`, `crates/aria-render/src/animation.rs`)",
    );
    let char_sec = section(
        ANIMATIONS_DOC,
        "### Per-character text animations (`text_only = true`, `crates/aria-render/src/render/text_anim.rs`)",
    );
    let mut doc_anims: Vec<(&str, &str, &str, bool)> = table_rows(whole_sec)
        .into_iter()
        .map(|r| (unquote(r[0]), r[1], unquote(r[2]), false))
        .collect();
    doc_anims.extend(
        table_rows(char_sec)
            .into_iter()
            .map(|r| (unquote(r[0]), r[1], unquote(r[2]), true)),
    );
    let expected_anims: Vec<(&str, &str, &str, bool)> = animation_catalog()
        .iter()
        .map(|a| (a.id.as_str(), a.label.as_str(), a.slot.id(), a.text_only))
        .collect();
    assert_eq!(
        doc_anims, expected_anims,
        "docs/reference/animations.md animation tables do not match animation_catalog()"
    );

    let text_fx_sec = section(
        ANIMATIONS_DOC,
        "## Text style presets (`text_effects.toml`)",
    );
    let doc_text_fx: Vec<(&str, &str)> = table_rows(text_fx_sec)
        .into_iter()
        .map(|r| (unquote(r[0]), r[1]))
        .collect();
    let expected_text_fx: Vec<(&str, &str)> = text_effect_catalog()
        .iter()
        .map(|t| (t.id.as_str(), t.label.as_str()))
        .collect();
    assert_eq!(
        doc_text_fx, expected_text_fx,
        "docs/reference/animations.md text style presets table does not match text_effect_catalog()"
    );

    let bezier_sec = section(
        ANIMATIONS_DOC,
        "### Named cubic-bezier presets (`easings.toml`)",
    );
    let doc_beziers: Vec<(&str, &str)> = table_rows(bezier_sec)
        .into_iter()
        .map(|r| (unquote(r[0]), r[1]))
        .collect();
    let expected_beziers: Vec<(&str, &str)> = EASING_PRESETS
        .iter()
        .map(|e| (e.id.as_str(), e.label.as_str()))
        .collect();
    assert_eq!(
        doc_beziers, expected_beziers,
        "docs/reference/animations.md cubic-bezier presets table does not match EASING_PRESETS"
    );

    let easing_sec = section(
        ANIMATIONS_DOC,
        "### Built-in `Easing` interpolation modes (`crates/aria-models/src/param/easing.rs`)",
    );
    let doc_easings: Vec<(&str, &str)> = table_rows(easing_sec)
        .into_iter()
        .map(|r| (unquote(r[0]), unquote(r[1])))
        .collect();
    let easing_census = [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::Hold,
        Easing::Bezier {
            points: [0.0, 0.0, 1.0, 1.0],
        },
        Easing::Temporal {
            in_ease: None,
            out_ease: None,
            roving: false,
            hold: false,
            points: [0.0, 0.0, 1.0, 1.0],
        },
    ];
    let expected_easings: Vec<(&str, &str)> = easing_census
        .iter()
        .map(|e| match e {
            Easing::Linear => ("linear", "Easing::Linear"),
            Easing::EaseIn => ("ease_in", "Easing::EaseIn"),
            Easing::EaseOut => ("ease_out", "Easing::EaseOut"),
            Easing::EaseInOut => ("ease_in_out", "Easing::EaseInOut"),
            Easing::Hold => ("hold", "Easing::Hold"),
            Easing::Bezier { .. } => ("bezier", "Easing::Bezier { points }"),
            Easing::Temporal { .. } => (
                "temporal",
                "Easing::Temporal { in_ease, out_ease, roving, hold, points }",
            ),
        })
        .collect();
    assert_eq!(
        doc_easings, expected_easings,
        "docs/reference/animations.md built-in Easing modes table does not match Easing variants"
    );

    let pw_sec = section(
        ANIMATIONS_DOC,
        "### Multi-keyframe piecewise easing presets (`crates/aria-models/src/param/preset_catalog.rs`)",
    );
    let doc_pw: Vec<(&str, &str)> = table_rows(pw_sec)
        .into_iter()
        .map(|r| (unquote(r[0]), r[1]))
        .collect();
    let expected_pw: Vec<(&str, &str)> = piecewise_presets()
        .iter()
        .map(|p| (p.id, p.label))
        .collect();
    assert_eq!(
        doc_pw, expected_pw,
        "docs/reference/animations.md piecewise easing presets table does not match piecewise_presets()"
    );

    let speed_sec = section(
        ANIMATIONS_DOC,
        "## Speed-ramp presets (`speed_presets.toml`)",
    );
    let doc_speed: Vec<(&str, &str)> = table_rows(speed_sec)
        .into_iter()
        .map(|r| (unquote(r[0]), r[1]))
        .collect();
    let expected_speed: Vec<(&str, &str)> = speed_preset_catalog()
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str()))
        .collect();
    assert_eq!(
        doc_speed, expected_speed,
        "docs/reference/animations.md speed-ramp presets table does not match speed_preset_catalog()"
    );
}

/// **Every sticker, mask shape, mask mode, and mask parameter matches `docs/reference/stickers-and-masks.md`.**
///
/// Closes: undocumented or stale entry in `stickers.toml`, `masks.toml`,
/// `MaskMode::ALL`, or `MaskParam::ALL`, or drifted mask parameter bounds.
/// Not caught: prose descriptions of stencil shaders.
#[test]
fn stickers_and_masks_reference_matches_catalogs() {
    let sticker_sec = section(STICKERS_MASKS_DOC, "## Stickers (`stickers.toml`)");
    let doc_stickers: Vec<(&str, &str, String, bool)> = table_rows(sticker_sec)
        .into_iter()
        .map(|r| {
            (
                unquote(r[0]),
                r[1],
                unquote(r[2]).to_string(),
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let expected_stickers: Vec<(&str, &str, String, bool)> = sticker_catalog()
        .iter()
        .map(|s| {
            (
                s.id.as_str(),
                s.label.as_str(),
                format!("{} x {}", s.width, s.height),
                s.animated,
            )
        })
        .collect();
    assert_eq!(
        doc_stickers, expected_stickers,
        "docs/reference/stickers-and-masks.md stickers table does not match sticker_catalog()"
    );

    let mask_sec = section(STICKERS_MASKS_DOC, "## Mask shapes (`masks.toml`)");
    let mask_summary = mask_sec.split("\n### ").next().unwrap_or(mask_sec);
    let doc_masks: Vec<(&str, &str)> = table_rows(mask_summary)
        .into_iter()
        .map(|r| (unquote(r[0]), r[1]))
        .collect();
    let expected_masks: Vec<(&str, &str)> = mask_catalog()
        .iter()
        .map(|m| (m.kind.id(), m.label.as_str()))
        .collect();
    assert_eq!(
        doc_masks, expected_masks,
        "docs/reference/stickers-and-masks.md mask shapes table does not match mask_catalog()"
    );

    let mode_sec = section(
        STICKERS_MASKS_DOC,
        "### Multi-mask stack modes (`MaskMode` in `crates/aria-models/src/look/mask.rs`)",
    );
    let doc_modes: Vec<(&str, &str)> = table_rows(mode_sec)
        .into_iter()
        .map(|r| (unquote(r[0]), r[1]))
        .collect();
    let expected_modes: Vec<(&str, &str)> =
        MaskMode::ALL.iter().map(|m| (m.id(), m.label())).collect();
    assert_eq!(
        doc_modes, expected_modes,
        "docs/reference/stickers-and-masks.md mask modes table does not match MaskMode::ALL"
    );

    let param_sec = section(
        STICKERS_MASKS_DOC,
        "### Animatable mask parameters (`MaskParam` / `mask_rows.rs`)",
    );
    let param_rows = table_rows(param_sec);
    let doc_param_ids: Vec<&str> = param_rows.iter().map(|r| unquote(r[0])).collect();
    let expected_param_ids: Vec<&str> = MaskParam::ALL.iter().map(|p| p.id()).collect();
    assert_eq!(
        doc_param_ids, expected_param_ids,
        "docs/reference/stickers-and-masks.md mask parameters do not match MaskParam::ALL"
    );

    for (row, &param) in param_rows.iter().zip(MaskParam::ALL.iter()) {
        let ctx = format!("mask param {}", param.id());
        assert_eq!(row[2], param.label(), "{ctx}: label mismatch");
        let expected_kind = match param.kind() {
            ParamKind::Scalar => "scalar",
            ParamKind::Vec2 => "vec2",
            other => panic!("{ctx}: unexpected ParamKind {other:?}"),
        };
        assert_eq!(unquote(row[3]), expected_kind, "{ctx}: kind mismatch");
        match param.range() {
            ParamRange {
                min: Some(min),
                max: Some(max),
            } => {
                let doc_min = parse_f32(row[5], &ctx);
                let doc_max = parse_f32(row[6], &ctx);
                assert!((doc_min - min).abs() <= 1e-5, "{ctx}: min mismatch");
                assert!((doc_max - max).abs() <= 1e-5, "{ctx}: max mismatch");
            }
            ParamRange {
                min: Some(_),
                max: None,
            } => {
                assert_eq!(unquote(row[5]), "> 0.0", "{ctx}: min mismatch");
                assert_eq!(unquote(row[6]), "f32::MAX", "{ctx}: max mismatch");
            }
            other => panic!("{ctx}: unexpected mask param range {other:?}"),
        }
    }
}

/// **Every layer blend mode, mask combination mode, and mask shape matches `docs/reference/blend-modes.md`.**
///
/// Closes: undocumented or stale `BlendMode`, `MaskMode`, or `MaskKind` variant,
/// drifted `masks.toml` label, or mismatched `blend_composite.wgsl` shader ID / formula.
/// Not caught: prose descriptions in the Summary column.
#[test]
fn blend_modes_and_masks_reference_matches_catalog() {
    let blend_modes_doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/reference/blend-modes.md"
    ))
    .expect("read docs/reference/blend-modes.md");
    let blend_sec = section(&blend_modes_doc, "## Layer blend modes (`BlendMode`)");
    let blend_rows = table_rows(blend_sec);
    let doc_blends: Vec<(&str, &str, &str, usize, bool, &str)> = blend_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 7, "malformed BlendMode row: {r:?}");
            let shader_id: usize = unquote(r[3])
                .parse()
                .unwrap_or_else(|e| panic!("invalid shader_id in {r:?}: {e}"));
            (
                unquote(r[0]),
                r[1],
                unquote(r[2]),
                shader_id,
                unquote(r[4]) == "yes",
                unquote(r[5]),
            )
        })
        .collect();
    let expected_blends: Vec<(&str, &str, String, usize, bool)> = BlendMode::ALL
        .iter()
        .enumerate()
        .map(|(idx, &m)| {
            (
                m.id(),
                m.label(),
                format!("{m:?}"),
                idx,
                m == BlendMode::default(),
            )
        })
        .collect();
    let doc_blend_meta: Vec<(&str, &str, String, usize, bool)> = doc_blends
        .iter()
        .map(|&(id, label, var, sid, is_def, _)| (id, label, var.to_string(), sid, is_def))
        .collect();
    assert_eq!(
        doc_blend_meta, expected_blends,
        "docs/reference/blend-modes.md BlendMode table does not match BlendMode::ALL"
    );
    for &(id, _, variant, shader_id, _, formula) in &doc_blends {
        if shader_id == 0 {
            assert_eq!(formula, "cs", "Normal blend mode formula must be `cs`");
        } else {
            let case_header = format!("case {shader_id}u:");
            assert!(
                BLEND_COMPOSITE_WGSL.contains(&case_header),
                "blend_composite.wgsl missing `{case_header}` for `{id}` ({variant})"
            );
            assert!(
                BLEND_COMPOSITE_WGSL.contains(formula),
                "blend_composite.wgsl missing documented formula `{formula}` for `{id}`"
            );
        }
    }

    let mode_sec = section(
        &blend_modes_doc,
        "## Mask combination modes (`MaskMode` and `MaskOp`)",
    );
    let mode_rows = table_rows(mode_sec);
    let doc_modes: Vec<(&str, &str, String, bool, &str, &str)> = mode_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 7, "malformed MaskMode row: {r:?}");
            (
                unquote(r[0]),
                r[1],
                unquote(r[2]).to_string(),
                unquote(r[3]) == "yes",
                unquote(r[4]),
                unquote(r[5]),
            )
        })
        .collect();
    let expected_modes: Vec<(&str, &str, String, bool, &str, &str)> = MaskMode::ALL
        .iter()
        .map(|&m| {
            let (op_id, start) = match m {
                MaskMode::None => ("none", "none"),
                MaskMode::Add => ("0", "0.0"),
                MaskMode::Subtract => ("1", "1.0"),
                MaskMode::Intersect => ("2", "1.0"),
                MaskMode::Lighten => ("3", "0.0"),
                MaskMode::Darken => ("4", "1.0"),
                MaskMode::Difference => ("5", "0.0"),
            };
            (
                m.id(),
                m.label(),
                format!("{m:?}"),
                m == MaskMode::default(),
                op_id,
                start,
            )
        })
        .collect();
    assert_eq!(
        doc_modes, expected_modes,
        "docs/reference/blend-modes.md MaskMode table does not match MaskMode::ALL"
    );

    let shape_sec = section(
        &blend_modes_doc,
        "## Mask shapes (`MaskKind` and `masks.toml`)",
    );
    let shape_rows = table_rows(shape_sec);
    let doc_shapes: Vec<(&str, &str, String, &str, bool, bool)> = shape_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 6, "malformed MaskKind row: {r:?}");
            (
                unquote(r[0]),
                r[1],
                unquote(r[2]).to_string(),
                unquote(r[3]),
                unquote(r[4]) == "yes",
                unquote(r[5]) == "yes",
            )
        })
        .collect();
    assert_eq!(
        mask_catalog()
            .iter()
            .map(|s| s.kind)
            .collect::<Vec<MaskKind>>(),
        MaskKind::ALL.to_vec(),
        "mask_catalog() order must match MaskKind::ALL"
    );
    let expected_shapes: Vec<(&str, &str, String, &str, bool, bool)> = mask_catalog()
        .iter()
        .map(|spec| {
            let shader_kind = match spec.kind {
                MaskKind::Linear => "0",
                MaskKind::Mirror => "1",
                MaskKind::Circle => "2",
                MaskKind::Rectangle => "3",
                MaskKind::Heart => "4",
                MaskKind::Star => "5",
                MaskKind::Path => "cpu",
            };
            (
                spec.kind.id(),
                spec.label.as_str(),
                format!("{:?}", spec.kind),
                shader_kind,
                MaskParam::Size.applies_to(spec.kind),
                MaskParam::Roundness.applies_to(spec.kind),
            )
        })
        .collect();
    assert_eq!(
        doc_shapes, expected_shapes,
        "docs/reference/blend-modes.md MaskKind table does not match mask_catalog()"
    );
    assert!(
        blend_modes_doc.contains(&format!("MAX_CLIP_MASKS = {MAX_CLIP_MASKS}")),
        "blend-modes.md must document MAX_CLIP_MASKS = {MAX_CLIP_MASKS}"
    );
    assert!(
        blend_modes_doc.contains(&format!("MAX_MASK_EXPANSION = {MAX_MASK_EXPANSION:.1}")),
        "blend-modes.md must document MAX_MASK_EXPANSION = {MAX_MASK_EXPANSION:.1}"
    );
    assert!(
        blend_modes_doc.contains(&format!("MASK_PATH_VERSION = {MASK_PATH_VERSION}")),
        "blend-modes.md must document MASK_PATH_VERSION = {MASK_PATH_VERSION}"
    );
    assert!(
        blend_modes_doc.contains(&format!("MAX_MASK_PATH_POINTS = {MAX_MASK_PATH_POINTS}")),
        "blend-modes.md must document MAX_MASK_PATH_POINTS = {MAX_MASK_PATH_POINTS}"
    );
}

/// **Every workspace path and `(`Symbol` in `crates/...`)` citation in the look reference docs resolves.**
///
/// Closes: renamed or moved source files (`crates/...`, `assets/...`) or renamed
/// catalog specification structs in `docs/reference/{animations,filters,stickers-and-masks,blend-modes}.md`.
/// Not caught: prose behavior descriptions.
#[test]
fn reference_docs_cited_paths_and_defined_in_symbols_resolve() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let blend_modes_doc = std::fs::read_to_string(root.join("docs/reference/blend-modes.md"))
        .expect("read docs/reference/blend-modes.md");
    for (name, doc) in [
        ("docs/reference/animations.md", ANIMATIONS_DOC),
        ("docs/reference/filters.md", FILTERS_DOC),
        ("docs/reference/stickers-and-masks.md", STICKERS_MASKS_DOC),
        ("docs/reference/blend-modes.md", blend_modes_doc.as_str()),
    ] {
        let mut rest = doc;
        while let Some(open) = rest.find('`') {
            let after_open = &rest[open + 1..];
            let Some(close) = after_open.find('`') else {
                break;
            };
            let span = &after_open[..close];
            if (span.starts_with("crates/") || span.starts_with("assets/")) && !span.contains(' ') {
                let full = root.join(span.trim_end_matches('/'));
                assert!(
                    full.exists(),
                    "{name} cites missing workspace path `{span}`"
                );
            }
            let tail = &after_open[close + 1..];
            if let Some(after_in) = tail.strip_prefix(" in `")
                && let Some(path_close) = after_in.find('`')
            {
                let path = &after_in[..path_close];
                if path.starts_with("crates/") {
                    let full = root.join(path);
                    let source = std::fs::read_to_string(&full).unwrap_or_else(|e| {
                        panic!("{name} cites unreadable `{path}` for `{span}`: {e}")
                    });
                    let ident = span
                        .split("::")
                        .last()
                        .unwrap_or(span)
                        .trim_end_matches("()");
                    assert!(
                        source.contains(ident),
                        "{name} cites `{span}` in `{path}`, but `{ident}` is not in that file"
                    );
                }
            }
            rest = tail;
        }
    }
}
