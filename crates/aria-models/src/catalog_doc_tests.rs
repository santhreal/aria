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
//! each parameter's `(name, label, kind, unit, min, max, default, animates)`.
//!
//! Not caught: prose descriptions of shader or DSP math outside the tables.

#[path = "catalog_doc_look_tests.rs"]
mod look;

use crate::{
    AudioTransition, DEFAULT_TRANSITION_TICKS, EXPONENTIAL_FLOOR_DB, EffectParamKind,
    EffectParamSpec, ParamAxis, TransitionAlignment, generator_specs, shipped_audio_effect_catalog,
    shipped_effect_catalog, shipped_transition_catalog,
};

const EFFECTS_DOC: &str = include_str!("../../../docs/reference/effects.md");
const GENERATORS_DOC: &str = include_str!("../../../docs/reference/generators.md");
const AUDIO_EFFECTS_DOC: &str = include_str!("../../../docs/reference/audio-effects.md");

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

/// Column headers of every effect, generator, transition, and audio effect
/// parameter table. [`assert_effect_params_match`] compares each column with
/// the catalog, so a column with no catalog field behind it fails the header
/// check instead of shipping unverified.
const PARAM_COLUMNS: [&str; 8] = [
    "Name", "Label", "Kind", "Unit", "Min", "Max", "Default", "Animates",
];

/// Verifies that `section_body` documents `params` bijectively with matching
/// `(name, label, kind, unit, min, max, default, animates)`, where `animates`
/// states whether the instance type stores the parameter as a keyframeable
/// `Param`.
fn assert_effect_params_match(
    entry_id: &str,
    section_body: &str,
    params: &[EffectParamSpec],
    animates: fn(&EffectParamSpec) -> bool,
) {
    for header in section_body
        .lines()
        .filter(|line| line.trim_start().starts_with("| Name |"))
    {
        let columns: Vec<&str> = header
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        assert_eq!(
            columns, PARAM_COLUMNS,
            "{entry_id}: parameter table columns"
        );
    }
    let rows = table_rows(section_body);
    let doc_names: Vec<&str> = rows.iter().map(|r| unquote(r[0])).collect();
    let expected_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        doc_names, expected_names,
        "parameter list mismatch for entry `{entry_id}`"
    );

    for (row, spec) in rows.iter().zip(params.iter()) {
        let ctx = format!("{entry_id}.{}", spec.name);
        assert_eq!(
            row.len(),
            PARAM_COLUMNS.len(),
            "{ctx}: expected {} columns, got {row:?}",
            PARAM_COLUMNS.len()
        );
        assert_eq!(unquote(row[0]), spec.name, "{ctx}: name mismatch");
        assert_eq!(row[1], spec.label, "{ctx}: label mismatch");
        assert_eq!(unquote(row[2]), spec.kind.as_str(), "{ctx}: kind mismatch");
        // A catalog unit is the suffix the inspector prints, so the table shows
        // it verbatim. With no catalog unit the cell describes the scale, and
        // `px` would claim canvas-pixel scaling the parameter does not get.
        match spec.unit.as_deref() {
            Some(unit) => assert_eq!(
                unquote(row[3]),
                unit,
                "{ctx}: documented unit disagrees with the catalog unit"
            ),
            None => assert_ne!(
                unquote(row[3]),
                crate::CANVAS_PX_UNIT,
                "{ctx}: documented as canvas pixels, but the catalog states no unit"
            ),
        }
        let documented_animates = match unquote(row[7]) {
            "yes" => true,
            "no" => false,
            other => panic!("{ctx}: Animates cell {other:?} is neither `yes` nor `no`"),
        };
        assert_eq!(
            documented_animates,
            animates(spec),
            "{ctx}: documented Animates `{}` disagrees with the instance storage",
            row[7]
        );
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

/// Effect, generator, and audio effect instances store every keyframeable kind
/// as a `Param` (`EffectInstance`, `AudioEffectInstance`), so those parameters
/// animate exactly when their kind is keyframeable.
fn keyframeable(spec: &EffectParamSpec) -> bool {
    spec.kind.is_keyframeable()
}

/// A `Transition` stores its parameters as constants (`Map<String, f32>` and
/// siblings), so no transition parameter animates.
fn constant(_: &EffectParamSpec) -> bool {
    false
}

/// **Every non-generator video effect and parameter in `effects.toml` matches `docs/reference/effects.md`.**
///
/// Closes: undocumented video effect, stale effect entry, or drifted parameter
/// `(kind, unit, min, max, default, animates)` in `docs/reference/effects.md`.
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
        assert_effect_params_match(id, body, &spec.params, keyframeable);
    }
}

/// **Every clip generator and GPU effect generator matches `docs/reference/generators.md`.**
///
/// Closes: undocumented `Generator` variant or GPU generator entry, stale
/// entry, or drifted generator parameter `(kind, unit, min, max, default, animates)`.
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
        assert_effect_params_match(id, body, &spec.params, keyframeable);
    }
}

/// **Every shipped transition and transition parameter matches `docs/reference/transitions.md`.**
///
/// Closes: undocumented transition, stale transition entry, or drifted
/// transition parameter `(kind, unit, min, max, default, animates)`.
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
        assert_effect_params_match(id, body, &spec.params, constant);
    }

    let align_sec = section(
        &transitions_doc,
        "## Transition cut alignment (`TransitionAlignment::ALL`)",
    );
    assert!(
        align_sec.contains(&format!(
            "DEFAULT_TRANSITION_TICKS = {DEFAULT_TRANSITION_TICKS}"
        )),
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
/// parameter `(kind, unit, min, max, default, animates)`, `latency`, or `sidechain`.
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
        assert_effect_params_match(id, body, &spec.params, keyframeable);
    }
}
