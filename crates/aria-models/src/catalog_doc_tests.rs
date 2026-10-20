//! Reference documentation coverage for every shipped catalog in `aria-models`.
//!
//! # Why
//!
//! `docs/reference/` documents the shipped video effects, generators,
//! transitions, audio effects, filters, color-grade sliders, animations, text
//! style presets, easings, speed-ramp presets, stickers, and masks. Hand-maintained
//! reference tables drift in three ways: a new catalog entry ships without a
//! section, a deleted or renamed entry stays in the chapter, or a parameter's
//! kind, range, or default changes in the TOML/Rust catalog while the table
//! still shows the old value.
//!
//! Each test below reads the chapter via `include_str!` and compares its tables
//! and per-entry sections against the runtime catalog in both directions, plus
//! each parameter's `(name, label, kind, min, max, default)`.
//!
//! Not caught: prose descriptions of shader or DSP math outside the tables.

use crate::{
    AudioTransition, BlendMode, ColorAdjustments, DEFAULT_TRANSITION_TICKS, EASING_PRESETS,
    EXPONENTIAL_FLOOR_DB, EffectParamKind, EffectParamSpec, MASK_PATH_VERSION, MAX_CLIP_MASKS,
    MAX_MASK_EXPANSION, MAX_MASK_PATH_POINTS, MaskKind, MaskMode, MaskParam, ParamAxis, ParamKind,
    ParamRange, TransitionAlignment, animation_catalog, filter_catalog, generator_specs,
    mask_catalog, piecewise_presets, shipped_audio_effect_catalog, shipped_effect_catalog,
    shipped_transition_catalog, speed_preset_catalog, sticker_catalog, text_effect_catalog,
};

const EFFECTS_DOC: &str = include_str!("../../../docs/reference/effects.md");
const GENERATORS_DOC: &str = include_str!("../../../docs/reference/generators.md");
const AUDIO_EFFECTS_DOC: &str = include_str!("../../../docs/reference/audio-effects.md");
const FILTERS_DOC: &str = include_str!("../../../docs/reference/filters.md");
const ANIMATIONS_DOC: &str = include_str!("../../../docs/reference/animations.md");
const STICKERS_MASKS_DOC: &str = include_str!("../../../docs/reference/stickers-and-masks.md");
const BLEND_COMPOSITE_WGSL: &str =
    include_str!("../../aria-compositor/shaders/blend_composite.wgsl");

/// Extracts the slice of `doc` under `heading` up to the next heading of the
/// same or outer level.
fn section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let level = heading.bytes().take_while(|&b| b == b'#').count();
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("missing heading {heading:?}"));
    let body = &doc[start + heading.len()..];
    let body = body.strip_prefix('\n').unwrap_or(body);
    let mut offset = 0usize;
    for line in body.lines() {
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if (1..=level).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
            return &body[..offset];
        }
        offset += line.len() + 1;
    }
    body
}

/// Parses Markdown table data rows (skipping the header and `---` separator
/// rows) into trimmed cell slices.
fn table_rows(text: &str) -> Vec<Vec<&str>> {
    let mut rows = Vec::new();
    let mut in_table = false;
    let mut past_sep = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            if in_table {
                in_table = false;
                past_sep = false;
            }
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if !in_table {
            in_table = true;
            continue;
        }
        if !past_sep {
            past_sep = true;
            continue;
        }
        rows.push(cells);
    }
    rows
}

/// Strips surrounding backticks from a Markdown inline-code cell.
fn unquote(cell: &str) -> &str {
    cell.trim().trim_start_matches('`').trim_end_matches('`')
}

/// Parses every `### <Label> (`<id>`)` subsection under `text`.
fn entry_sections(text: &str) -> Vec<(&str, &str, &str)> {
    let mut headers = Vec::new();
    let mut offset = 0usize;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("### ")
            && let Some((label, id_part)) = rest.rsplit_once(" (`")
            && let Some(id) = id_part.strip_suffix("`)")
        {
            headers.push((offset, offset + line.len(), id, label.trim()));
        }
        offset += line.len() + 1;
    }
    let mut out = Vec::with_capacity(headers.len());
    for (idx, &(_, line_end, id, label)) in headers.iter().enumerate() {
        let end = headers
            .get(idx + 1)
            .map(|&(next_start, ..)| next_start)
            .unwrap_or(text.len());
        let body = text.get(line_end..end).unwrap_or("");
        out.push((id, label, body));
    }
    out
}

/// Formats an [`EffectParamSpec`]'s default value the way `docs/reference/`
/// parameter tables display it.
fn parse_f32(cell: &str, context: &str) -> f32 {
    unquote(cell)
        .parse::<f32>()
        .unwrap_or_else(|e| panic!("{context}: invalid float {cell:?}: {e}"))
}

fn parse_vec2(cell: &str, context: &str) -> [f32; 2] {
    let inner = unquote(cell).trim_start_matches('[').trim_end_matches(']');
    let mut parts = inner.split(',').map(str::trim);
    let x = parts
        .next()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or_else(|| panic!("{context}: invalid vec2 {cell:?}"));
    let y = parts
        .next()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or_else(|| panic!("{context}: invalid vec2 {cell:?}"));
    assert!(
        parts.next().is_none(),
        "{context}: extra component in {cell:?}"
    );
    [x, y]
}

fn parse_color(cell: &str, context: &str) -> [u8; 4] {
    let inner = unquote(cell).trim_start_matches('[').trim_end_matches(']');
    let parts: Vec<u8> = inner
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<u8>()
                .unwrap_or_else(|e| panic!("{context}: invalid color byte in {cell:?}: {e}"))
        })
        .collect();
    parts
        .try_into()
        .unwrap_or_else(|_| panic!("{context}: expected 4 RGBA bytes in {cell:?}"))
}

/// Verifies that `section_body` documents `params` bijectively with matching
/// `(name, label, kind, min, max, default)`.
fn assert_effect_params_match(entry_id: &str, section_body: &str, params: &[EffectParamSpec]) {
    let rows = table_rows(section_body);
    let doc_names: Vec<&str> = rows.iter().map(|r| unquote(r[0])).collect();
    let expected_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        doc_names, expected_names,
        "parameter list mismatch for entry `{entry_id}`"
    );

    for (row, spec) in rows.iter().zip(params.iter()) {
        let ctx = format!("{entry_id}.{}", spec.name);
        assert!(
            row.len() >= 7,
            "{ctx}: expected at least 7 columns, got {row:?}"
        );
        assert_eq!(unquote(row[0]), spec.name, "{ctx}: name mismatch");
        assert_eq!(row[1], spec.label, "{ctx}: label mismatch");
        assert_eq!(unquote(row[2]), spec.kind.as_str(), "{ctx}: kind mismatch");
        match spec.kind {
            EffectParamKind::Scalar => {
                let min = parse_f32(row[4], &ctx);
                let max = parse_f32(row[5], &ctx);
                let default = parse_f32(row[6], &ctx);
                assert!(
                    (min - spec.min).abs() <= 1e-5,
                    "{ctx}: documented min {min} != catalog min {}",
                    spec.min
                );
                assert!(
                    (max - spec.max).abs() <= 1e-5,
                    "{ctx}: documented max {max} != catalog max {}",
                    spec.max
                );
                assert!(
                    (default - spec.default).abs() <= 1e-5,
                    "{ctx}: documented default {default} != catalog default {}",
                    spec.default
                );
            }
            EffectParamKind::Vec2 => {
                let min = parse_f32(row[4], &ctx);
                let max = parse_f32(row[5], &ctx);
                let default = parse_vec2(row[6], &ctx);
                assert!(
                    (min - spec.min).abs() <= 1e-5,
                    "{ctx}: documented min {min} != catalog min {}",
                    spec.min
                );
                assert!(
                    (max - spec.max).abs() <= 1e-5,
                    "{ctx}: documented max {max} != catalog max {}",
                    spec.max
                );
                assert_eq!(
                    default, spec.default_vec2,
                    "{ctx}: documented default {default:?} != catalog default {:?}",
                    spec.default_vec2
                );
            }
            EffectParamKind::Color => {
                let min = parse_f32(row[4], &ctx);
                let max = parse_f32(row[5], &ctx);
                let default = parse_color(row[6], &ctx);
                assert_eq!(min, 0.0, "{ctx}: color min should be 0");
                assert_eq!(max, 255.0, "{ctx}: color max should be 255");
                assert_eq!(
                    default, spec.default_color,
                    "{ctx}: documented default {default:?} != catalog default {:?}",
                    spec.default_color
                );
            }
            other => panic!("{ctx}: unhandled parameter kind {other:?}"),
        }
    }
}

/// **Every non-generator video effect and parameter in `effects.toml` matches `docs/reference/effects.md`.**
///
/// Closes: undocumented video effect, stale effect entry, or drifted parameter
/// `(kind, min, max, default)` in `docs/reference/effects.md`.
/// Not caught: prose descriptions of WGSL formulas.
#[test]
fn video_effects_reference_matches_shipped_catalog() {
    let catalog: Vec<_> = shipped_effect_catalog()
        .iter()
        .filter(|spec| !spec.generator)
        .collect();
    let entries_sec = section(EFFECTS_DOC, "## Catalog entries");
    let summary_rows = table_rows(entries_sec.split("\n### ").next().unwrap_or(entries_sec));
    let summary_ids: Vec<(&str, &str, &str)> = summary_rows
        .iter()
        .map(|r| (unquote(r[0]), r[1], r[2]))
        .collect();
    let expected_summary: Vec<(&str, &str, &str)> = catalog
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str(), s.category()))
        .collect();
    assert_eq!(
        summary_ids, expected_summary,
        "docs/reference/effects.md summary table does not match shipped_effect_catalog()"
    );

    let sections = entry_sections(entries_sec);
    let section_ids: Vec<(&str, &str)> =
        sections.iter().map(|&(id, label, _)| (id, label)).collect();
    let expected_ids: Vec<(&str, &str)> = catalog
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str()))
        .collect();
    assert_eq!(
        section_ids, expected_ids,
        "docs/reference/effects.md ### sections do not match shipped_effect_catalog()"
    );

    for ((id, _, body), spec) in sections.iter().zip(catalog.iter()) {
        assert_effect_params_match(id, body, &spec.params);
    }
}

/// **Every clip generator and GPU effect generator matches `docs/reference/generators.md`.**
///
/// Closes: undocumented `Generator` variant or GPU generator entry, stale
/// entry, or drifted generator parameter `(kind, min, max, default)`.
/// Not caught: prose descriptions of generator shader internals.
#[test]
fn generators_reference_matches_shipped_catalogs() {
    let clip_gen_sec = section(GENERATORS_DOC, "## Clip content generators");
    let clip_rows = table_rows(clip_gen_sec);
    let doc_clip_gens: Vec<(&str, &str, &str, bool, bool)> = clip_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]),
                r[1],
                unquote(r[2]),
                unquote(r[4]) == "yes",
                unquote(r[6]) == "yes",
            )
        })
        .collect();
    let expected_clip_gens: Vec<(&str, &str, &str, bool, bool)> = generator_specs()
        .iter()
        .map(|g| (g.id, g.label, g.lane.id(), g.paints, g.animatable))
        .collect();
    assert_eq!(
        doc_clip_gens, expected_clip_gens,
        "docs/reference/generators.md clip content generators table does not match generator_specs()"
    );

    let gpu_gen_sec = section(GENERATORS_DOC, "## GPU effect generators");
    let gpu_catalog: Vec<_> = shipped_effect_catalog()
        .iter()
        .filter(|spec| spec.generator)
        .collect();
    let summary_rows = table_rows(gpu_gen_sec.split("\n### ").next().unwrap_or(gpu_gen_sec));
    let summary_ids: Vec<(&str, &str, &str)> = summary_rows
        .iter()
        .map(|r| (unquote(r[0]), r[1], r[2]))
        .collect();
    let expected_summary: Vec<(&str, &str, &str)> = gpu_catalog
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str(), s.category()))
        .collect();
    assert_eq!(
        summary_ids, expected_summary,
        "docs/reference/generators.md GPU summary table does not match generator effects"
    );

    let sections = entry_sections(gpu_gen_sec);
    let section_ids: Vec<(&str, &str)> =
        sections.iter().map(|&(id, label, _)| (id, label)).collect();
    let expected_ids: Vec<(&str, &str)> = gpu_catalog
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str()))
        .collect();
    assert_eq!(
        section_ids, expected_ids,
        "docs/reference/generators.md ### sections do not match generator effects"
    );

    for ((id, _, body), spec) in sections.iter().zip(gpu_catalog.iter()) {
        assert_effect_params_match(id, body, &spec.params);
    }
}

/// **Every shipped transition and transition parameter matches `docs/reference/transitions.md`.**
///
/// Closes: undocumented transition, stale transition entry, or drifted
/// transition parameter `(kind, min, max, default)`.
/// Not caught: prose descriptions of transition shader math.
#[test]
fn transitions_reference_matches_shipped_catalog() {
    let transitions_doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/reference/transitions.md"
    ))
    .expect("read docs/reference/transitions.md");
    let catalog = shipped_transition_catalog();
    let sec = section(
        &transitions_doc,
        "## Video transition catalog (`transitions.toml`)",
    );
    let summary_rows = table_rows(sec.split("\n### ").next().unwrap_or(sec));
    let summary_ids: Vec<(&str, &str)> =
        summary_rows.iter().map(|r| (unquote(r[0]), r[1])).collect();
    let expected_ids: Vec<(&str, &str)> = catalog
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str()))
        .collect();
    assert_eq!(
        summary_ids, expected_ids,
        "docs/reference/transitions.md summary table does not match shipped_transition_catalog()"
    );

    let sections = entry_sections(sec);
    let section_ids: Vec<(&str, &str)> =
        sections.iter().map(|&(id, label, _)| (id, label)).collect();
    assert_eq!(
        section_ids, expected_ids,
        "docs/reference/transitions.md ### sections do not match shipped_transition_catalog()"
    );

    for ((id, _, body), spec) in sections.iter().zip(catalog.iter()) {
        assert_effect_params_match(id, body, &spec.params);
    }

    let align_sec = section(
        &transitions_doc,
        "## Transition cut alignment (`TransitionAlignment::ALL`)",
    );
    assert!(
        align_sec.contains(&format!("DEFAULT_TRANSITION_TICKS = {DEFAULT_TRANSITION_TICKS}")),
        "transitions.md must document DEFAULT_TRANSITION_TICKS"
    );
    let align_rows = table_rows(align_sec);
    let doc_align: Vec<(&str, &str, String, bool)> = align_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed TransitionAlignment row: {r:?}");
            (
                unquote(r[0]),
                r[1],
                unquote(r[2]).to_string(),
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let expected_align: Vec<(&str, &str, String, bool)> = TransitionAlignment::ALL
        .iter()
        .map(|&a| {
            (
                a.id(),
                a.label(),
                format!("{a:?}"),
                a == TransitionAlignment::default(),
            )
        })
        .collect();
    assert_eq!(
        doc_align, expected_align,
        "docs/reference/transitions.md TransitionAlignment table does not match TransitionAlignment::ALL"
    );

    let audio_tr_sec = section(
        &transitions_doc,
        "## Audio transitions (`AudioTransition::ALL`)",
    );
    assert!(
        audio_tr_sec.contains(&format!("EXPONENTIAL_FLOOR_DB = {EXPONENTIAL_FLOOR_DB:.1}")),
        "transitions.md must document EXPONENTIAL_FLOOR_DB"
    );
    let audio_tr_rows = table_rows(audio_tr_sec);
    let doc_audio_tr: Vec<(&str, &str, String, bool)> = audio_tr_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed AudioTransition row: {r:?}");
            (
                unquote(r[0]),
                r[1],
                unquote(r[2]).to_string(),
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let expected_audio_tr: Vec<(&str, &str, String, bool)> = AudioTransition::ALL
        .iter()
        .map(|&t| {
            (
                t.id(),
                t.label(),
                format!("{t:?}"),
                t == AudioTransition::DEFAULT,
            )
        })
        .collect();
    assert_eq!(
        doc_audio_tr, expected_audio_tr,
        "docs/reference/transitions.md AudioTransition table does not match AudioTransition::ALL"
    );

    assert_eq!(ParamAxis::ALL.len(), 4);
}

/// **Every shipped audio effect and parameter matches `docs/reference/audio-effects.md`.**
///
/// Closes: undocumented audio effect, stale audio effect entry, or drifted
/// parameter `(kind, min, max, default)`, `latency`, or `sidechain`.
/// Not caught: prose descriptions of DSP algorithms.
#[test]
fn audio_effects_reference_matches_shipped_catalog() {
    let catalog = shipped_audio_effect_catalog();
    let sec = section(
        AUDIO_EFFECTS_DOC,
        "## Catalog entries (`audio_effects.toml`)",
    );
    let summary_rows = table_rows(sec.split("\n### ").next().unwrap_or(sec));
    let summary_entries: Vec<(&str, &str, &str, u32, bool)> = summary_rows
        .iter()
        .map(|r| {
            let latency_cell = r[3].split_whitespace().next().unwrap_or(r[3]);
            let latency: u32 = unquote(latency_cell)
                .parse()
                .unwrap_or_else(|e| panic!("invalid latency in {r:?}: {e}"));
            let sidechain = unquote(r[4]) == "yes";
            (unquote(r[0]), r[1], r[2], latency, sidechain)
        })
        .collect();
    let expected_summary: Vec<(&str, &str, &str, u32, bool)> = catalog
        .iter()
        .map(|s| {
            (
                s.id.as_str(),
                s.label.as_str(),
                s.category(),
                s.latency,
                s.sidechain,
            )
        })
        .collect();
    assert_eq!(
        summary_entries, expected_summary,
        "docs/reference/audio-effects.md summary table does not match shipped_audio_effect_catalog()"
    );

    let sections = entry_sections(sec);
    let section_ids: Vec<(&str, &str)> =
        sections.iter().map(|&(id, label, _)| (id, label)).collect();
    let expected_ids: Vec<(&str, &str)> = catalog
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str()))
        .collect();
    assert_eq!(
        section_ids, expected_ids,
        "docs/reference/audio-effects.md ### sections do not match shipped_audio_effect_catalog()"
    );

    for ((id, _, body), spec) in sections.iter().zip(catalog.iter()) {
        assert_effect_params_match(id, body, &spec.params);
    }
}

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

    let shape_sec = section(&blend_modes_doc, "## Mask shapes (`MaskKind` and `masks.toml`)");
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
