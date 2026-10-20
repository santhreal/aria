//! Bidirectional reference documentation checks for `docs/reference/clip-properties.md`.
//!
//! # Why
//!
//! Closes the defect class where a `ClipParam` variant (`ClipParam::CENSUS`),
//! a row in `clip_param_catalog()` (`LookParam::ALL`, `StyleParam::ALL`,
//! `TextParam::ALL`, `ShapeParam::ALL`), `ClipTransform::IDENTITY`,
//! `CropRect::FULL` / `CROP_EDGES`, `MotionBlur::default()`,
//! `AlignTo::ALL` / `AlignEdge::ALL` / `AlignAxis::ALL`,
//! `TextCase::ALL` / `TextAlignH::ALL` / `TextAlignV::ALL` / `TextStyle`, or
//! `generator_specs()` changes without updating
//! `docs/reference/clip-properties.md`.
//!
//! Not caught: prose descriptions outside the structured tables and constants.

use aria_models::{
    ClipParam, ClipParamClass, ClipTransform, CropRect, LookParam, MAX_SHAPE_DIM, MAX_SPEED,
    MAX_STAR_POINTS, MAX_STROKE_WIDTH, MAX_TEXT_RUNS, MIN_CROP_FRACTION, MIN_SPEED, MotionBlur,
    ParamRange, SPEED_CURVE_SCALE, Scale2, ShapeParam, ShapeStroke, StyleParam, TEXT_RUN_VERSION,
    TextAlignH, TextAlignV, TextBackground, TextCase, TextParam, TextShadow, TextStroke,
    TextStyle, clip_param_catalog, generator_specs,
};

use crate::app::align::{AlignAxis, AlignEdge, AlignTo, DISTRIBUTE_MIN};
use crate::app::clip_frame::CROP_EDGES;


fn unquote(cell: &str) -> &str {
    cell.trim().trim_start_matches('`').trim_end_matches('`')
}

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

fn format_param_variant(p: ClipParam) -> String {
    match p {
        ClipParam::Position => "Position".into(),
        ClipParam::AnchorPoint => "AnchorPoint".into(),
        ClipParam::Scale => "Scale".into(),
        ClipParam::Rotation => "Rotation".into(),
        ClipParam::Opacity => "Opacity".into(),
        ClipParam::Crop => "Crop".into(),
        ClipParam::Speed => "Speed".into(),
        ClipParam::Volume => "Volume".into(),
        ClipParam::Pan => "Pan".into(),
        ClipParam::Effect { .. } => "Effect".into(),
        ClipParam::Shape { param } => format!("Shape({param:?})"),
        ClipParam::Text { param } => format!("Text({param:?})"),
        ClipParam::Look { param } => format!("Look({param:?})"),
        ClipParam::Mask { .. } => "Mask".into(),
        ClipParam::Style { param } => format!("Style({param:?})"),
    }
}

fn format_range(r: ParamRange) -> String {
    let fmt_bound = |v: f32| -> String {
        if v == f32::EPSILON {
            "> 0".to_string()
        } else if v.fract() == 0.0 {
            format!("{}", v as i64)
        } else {
            format!("{v}")
        }
    };
    match (r.min, r.max) {
        (None, None) => "any".into(),
        (Some(min), None) if min == f32::EPSILON => "> 0".into(),
        (Some(min), None) => format!(">= {}", fmt_bound(min)),
        (None, Some(max)) => format!("<= {}", fmt_bound(max)),
        (Some(min), Some(max)) if min == f32::EPSILON => format!("> 0..={}", fmt_bound(max)),
        (Some(min), Some(max)) => format!("{}..={}", fmt_bound(min), fmt_bound(max)),
    }
}

#[test]
fn clip_properties_doc_matches_registries() {
    let doc_buf = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/reference/clip-properties.md"),
    )
    .expect("read docs/reference/clip-properties.md");
    #[allow(non_snake_case)]
    let DOC = doc_buf.as_str();
    // 1. ClipParam::CENSUS and ClipParamClass.
    let census_sec = section(
        DOC,
        "## `ClipParam` variants and classification (`ClipParam::CENSUS` and `ClipParamClass`)",
    );
    assert!(
        census_sec.contains(&format!("SPEED_CURVE_SCALE = {SPEED_CURVE_SCALE}"))
            && census_sec.contains(&format!("MIN_SPEED = {MIN_SPEED}"))
            && census_sec.contains(&format!("MAX_SPEED = {MAX_SPEED:.1}")),
        "clip-properties.md missing speed curve constants"
    );
    let census_rows = table_rows(census_sec);
    let doc_census: Vec<(String, String, String, String, bool)> = census_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed ClipParam::CENSUS row: {r:?}");
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]).to_string(),
                unquote(r[4]) == "true",
            )
        })
        .collect();
    let expected_census: Vec<(String, String, String, String, bool)> = ClipParam::CENSUS
        .iter()
        .map(|&p| {
            let v_name = match p {
                ClipParam::Shape { .. } => "Shape".to_string(),
                ClipParam::Text { .. } => "Text".to_string(),
                ClipParam::Look { .. } => "Look".to_string(),
                ClipParam::Style { .. } => "Style".to_string(),
                other => format_param_variant(other),
            };
            let class_str = match p.class() {
                ClipParamClass::Direct(id) => format!("Direct({id:?})"),
                ClipParamClass::Delegated(axis) => format!("Delegated({axis:?})"),
                ClipParamClass::Excluded(ex) => format!("Excluded({ex:?})"),
            };
            let kind_str = p
                .kind()
                .map(|k| format!("{k:?}"))
                .unwrap_or_else(|| "None".to_string());
            (
                v_name,
                class_str,
                format!("{:?}", p.scope()),
                kind_str,
                p.supports_easing_preset(),
            )
        })
        .collect();
    assert_eq!(
        doc_census, expected_census,
        "docs/reference/clip-properties.md ClipParam::CENSUS table drifted"
    );

    // 2. clip_param_catalog() (all 104 rows) and sub-enums.
    let catalog_rows = table_rows(section(
        DOC,
        "## Static animatable clip parameter catalog (`clip_param_catalog()`)",
    ));
    let doc_catalog: Vec<(String, String, String, String, String, String, String)> = catalog_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 7, "malformed clip_param_catalog row: {r:?}");
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]).to_string(),
                unquote(r[4]).to_string(),
                unquote(r[5]).to_string(),
                unquote(r[6]).to_string(),
            )
        })
        .collect();
    let catalog = clip_param_catalog();
    let expected_catalog: Vec<(String, String, String, String, String, String, String)> = catalog
        .iter()
        .map(|s| {
            (
                s.id.to_string(),
                s.label.to_string(),
                s.group.to_string(),
                format_param_variant(s.param),
                format!("{:?}", s.kind),
                format!("{:?}", s.scope),
                format_range(s.range),
            )
        })
        .collect();
    assert_eq!(
        doc_catalog, expected_catalog,
        "docs/reference/clip-properties.md clip_param_catalog() table drifted"
    );
    assert_eq!(
        LookParam::ALL.len() + StyleParam::ALL.len() + TextParam::ALL.len() + ShapeParam::ALL.len() + 8,
        catalog.len(),
        "sub-enum variant counts must sum to clip_param_catalog().len()"
    );

    // 3. ClipTransform::IDENTITY, Scale2::ONE, CropRect::FULL, CROP_EDGES, MotionBlur::default().
    let tf_sec = section(
        DOC,
        "## Clip transform, framing, and motion blur (`ClipTransform`, `CropRect`, `MotionBlur`)",
    );
    let id_tf = ClipTransform::IDENTITY;
    assert_eq!(id_tf.scale, Scale2::ONE);
    assert!(
        tf_sec.contains(&format!("position = {:?}", id_tf.position))
            && tf_sec.contains(&format!("anchor_point = {:?}", id_tf.anchor_point))
            && tf_sec.contains(&format!("rotation = {:.1}", id_tf.rotation))
            && tf_sec.contains(&format!("opacity = {:.1}", id_tf.opacity))
            && tf_sec.contains(&format!("MIN_CROP_FRACTION = {MIN_CROP_FRACTION}")),
        "clip-properties.md missing ClipTransform::IDENTITY or MIN_CROP_FRACTION"
    );
    assert_eq!(
        CropRect::FULL,
        CropRect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0
        }
    );
    let crop_rows = table_rows(section(DOC, "### `CropRect` and `CROP_EDGES`"));
    let doc_edges: Vec<(String, String)> = crop_rows
        .iter()
        .map(|r| (unquote(r[0]).to_string(), r[1].to_string()))
        .collect();
    let expected_edges: Vec<(String, String)> = CROP_EDGES
        .iter()
        .map(|&(id, label, _, _)| (id.to_string(), label.to_string()))
        .collect();
    assert_eq!(
        doc_edges, expected_edges,
        "docs/reference/clip-properties.md CROP_EDGES table drifted"
    );
    let mb = MotionBlur::default();
    let mb_sec = section(DOC, "### `MotionBlur`");
    assert!(
        mb_sec.contains(&format!("default `{}`", mb.enabled))
            && mb_sec.contains(&format!("default `{:.1}`", mb.shutter_deg))
            && mb_sec.contains(&format!("default `{}`", mb.samples)),
        "clip-properties.md missing MotionBlur defaults"
    );

    // 4. AlignTo::ALL, AlignEdge::ALL, AlignAxis::ALL, DISTRIBUTE_MIN.
    let align_sec = section(
        DOC,
        "## Alignment and distribution (`AlignTo`, `AlignEdge`, `AlignAxis`)",
    );
    assert!(
        align_sec.contains(&format!("DISTRIBUTE_MIN = {DISTRIBUTE_MIN}")),
        "clip-properties.md missing DISTRIBUTE_MIN"
    );
    let align_to_rows = table_rows(section(DOC, "### Alignment reference (`AlignTo::ALL`)"));
    let doc_align_to: Vec<(String, String, bool)> = align_to_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]) == "yes",
            )
        })
        .collect();
    let expected_align_to: Vec<(String, String, bool)> = AlignTo::ALL
        .iter()
        .map(|&a| (format!("{a:?}"), a.label().to_string(), a == AlignTo::default()))
        .collect();
    assert_eq!(doc_align_to, expected_align_to, "AlignTo::ALL table drifted");

    let align_edge_rows = table_rows(section(DOC, "### Alignment edges (`AlignEdge::ALL`)"));
    let doc_align_edge: Vec<(String, String)> = align_edge_rows
        .iter()
        .map(|r| (unquote(r[0]).to_string(), r[1].to_string()))
        .collect();
    let expected_align_edge: Vec<(String, String)> = AlignEdge::ALL
        .iter()
        .map(|&e| (format!("{e:?}"), e.label().to_string()))
        .collect();
    assert_eq!(
        doc_align_edge, expected_align_edge,
        "AlignEdge::ALL table drifted"
    );

    let align_axis_rows = table_rows(section(DOC, "### Distribution axes (`AlignAxis::ALL`)"));
    let doc_align_axis: Vec<(String, String)> = align_axis_rows
        .iter()
        .map(|r| (unquote(r[0]).to_string(), r[1].to_string()))
        .collect();
    let expected_align_axis: Vec<(String, String)> = AlignAxis::ALL
        .iter()
        .map(|&a| (format!("{a:?}"), a.label().to_string()))
        .collect();
    assert_eq!(
        doc_align_axis, expected_align_axis,
        "AlignAxis::ALL table drifted"
    );

    // 5. TextStyle, TextCase::ALL, TextAlignH::ALL, TextAlignV::ALL.
    let text_sec = section(
        DOC,
        "## Text and title styling (`TextStyle`, `TextCase`, `TextAlignH`, `TextAlignV`)",
    );
    let ts = TextStyle::default();
    let t_stroke = TextStroke::default();
    let t_bg = TextBackground::default();
    let t_shadow = TextShadow::default();
    assert!(
        text_sec.contains(&format!("default `{:.1}`", ts.size.sample(0)))
            && text_sec.contains(&format!("default `{:?}`", ts.fill.sample(0)))
            && text_sec.contains(&format!("default `{:.1}`", ts.line_spacing.sample(0)))
            && text_sec.contains(&format!("width = {:.1}", t_stroke.width.sample(0)))
            && text_sec.contains(&format!("radius = {:.1}", t_bg.radius.sample(0)))
            && text_sec.contains(&format!("blur = {:.2}", t_shadow.blur.sample(0)))
            && text_sec.contains(&format!("distance = {:.1}", t_shadow.distance.sample(0)))
            && text_sec.contains(&format!("MAX_TEXT_RUNS = {MAX_TEXT_RUNS}"))
            && text_sec.contains(&format!("TEXT_RUN_VERSION = {TEXT_RUN_VERSION}")),
        "clip-properties.md missing TextStyle defaults or run constants"
    );

    let case_rows = table_rows(section(DOC, "### Text case (`TextCase::ALL`)"));
    let doc_cases: Vec<(String, String, bool)> = case_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]) == "yes",
            )
        })
        .collect();
    let expected_cases: Vec<(String, String, bool)> = TextCase::ALL
        .iter()
        .map(|&c| (format!("{c:?}"), c.label().to_string(), c == TextCase::default()))
        .collect();
    assert_eq!(doc_cases, expected_cases, "TextCase::ALL table drifted");

    let align_h_rows = table_rows(section(
        DOC,
        "### Horizontal text alignment (`TextAlignH::ALL`)",
    ));
    let doc_align_h: Vec<(String, String, bool)> = align_h_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]) == "yes",
            )
        })
        .collect();
    let expected_align_h: Vec<(String, String, bool)> = TextAlignH::ALL
        .iter()
        .map(|&a| (format!("{a:?}"), a.label().to_string(), a == TextAlignH::default()))
        .collect();
    assert_eq!(
        doc_align_h, expected_align_h,
        "TextAlignH::ALL table drifted"
    );

    let align_v_rows = table_rows(section(
        DOC,
        "### Vertical text alignment (`TextAlignV::ALL`)",
    ));
    let doc_align_v: Vec<(String, String, bool)> = align_v_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]) == "yes",
            )
        })
        .collect();
    let expected_align_v: Vec<(String, String, bool)> = TextAlignV::ALL
        .iter()
        .map(|&a| (format!("{a:?}"), a.label().to_string(), a == TextAlignV::default()))
        .collect();
    assert_eq!(
        doc_align_v, expected_align_v,
        "TextAlignV::ALL table drifted"
    );

    // 6. generator_specs() and Shape constants.
    let gen_sec = section(
        DOC,
        "## Generators and vector shapes (`generator_specs()` and `Shape`)",
    );
    let s_stroke = ShapeStroke::default();
    assert!(
        gen_sec.contains(&format!("MAX_SHAPE_DIM = {MAX_SHAPE_DIM:.1}"))
            && gen_sec.contains(&format!("MAX_STAR_POINTS = {MAX_STAR_POINTS}"))
            && gen_sec.contains(&format!("MAX_STROKE_WIDTH = {MAX_STROKE_WIDTH:.1}"))
            && gen_sec.contains(&format!("width = {:.1}", s_stroke.width.sample(0))),
        "clip-properties.md missing shape constants"
    );
    let gen_rows = table_rows(gen_sec);
    #[allow(clippy::type_complexity)]
    let doc_gens: Vec<(
        String,
        String,
        String,
        String,
        bool,
        bool,
        bool,
        bool,
        String,
        bool,
    )> = gen_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 10, "malformed generator_specs() row: {r:?}");
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]).to_string(),
                unquote(r[4]) == "true",
                unquote(r[5]) == "true",
                unquote(r[6]) == "true",
                unquote(r[7]) == "true",
                unquote(r[8]).to_string(),
                unquote(r[9]) == "true",
            )
        })
        .collect();
    #[allow(clippy::type_complexity)]
    let expected_gens: Vec<(
        String,
        String,
        String,
        String,
        bool,
        bool,
        bool,
        bool,
        String,
        bool,
    )> = generator_specs()
        .iter()
        .map(|g| {
            (
                g.id.to_string(),
                g.label.to_string(),
                format!("{:?}", g.lane),
                g.renders_with.join(", "),
                g.has_text,
                g.has_shape,
                g.needs_asset,
                g.paints,
                format!("{:?}", g.param_time),
                g.animatable,
            )
        })
        .collect();
    assert_eq!(
        doc_gens, expected_gens,
        "docs/reference/clip-properties.md generator_specs() table drifted"
    );
}
