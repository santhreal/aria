//! Bidirectional reference documentation checks for `docs/reference/color.md`.
//!
//! # Why
//!
//! Closes the defect class where a `ColorPrimaries`, `TransferFunction`,
//! `MatrixCoefficients`, `ColorRange`, `ColorSpace` preset, `StarterLutEntry`,
//! or `WheelControl` variant changes in `aria-core`, `aria-render`, or
//! `aria-ui` without updating `docs/reference/color.md`. The Color panel's
//! sections and the wheel radius are held to the same page by the panel's
//! own tests.
//!
//! Not caught: prose descriptions in the Summary column.

use aria_core::{ColorPrimaries, ColorRange, ColorSpace, MatrixCoefficients, TransferFunction};
use aria_render::{STARTER_LUT_SIZE, SWATCH_REFERENCE, starter_lut_entries};

use crate::app::grade_controls::WheelControl;
use crate::test_support::doc_tables::{heading_body, strip_ticks, table_rows};

#[test]
fn color_doc_matches_registries() {
    let doc_buf = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/color.md"),
    )
    .expect("read docs/reference/color.md");
    #[allow(non_snake_case)]
    let COLOR_DOC = doc_buf.as_str();
    let all_primaries = [
        ColorPrimaries::Bt709,
        ColorPrimaries::Bt601_525,
        ColorPrimaries::Bt601_625,
        ColorPrimaries::Bt2020,
        ColorPrimaries::DisplayP3,
        ColorPrimaries::Unspecified,
    ];
    for p in all_primaries {
        match p {
            ColorPrimaries::Bt709
            | ColorPrimaries::Bt601_525
            | ColorPrimaries::Bt601_625
            | ColorPrimaries::Bt2020
            | ColorPrimaries::DisplayP3
            | ColorPrimaries::Unspecified => {}
        }
    }
    let primaries_rows = table_rows(heading_body(
        COLOR_DOC,
        "### Color primaries (`ColorPrimaries`)",
    ));
    let doc_primaries: Vec<(String, bool, bool, String)> = primaries_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed ColorPrimaries row: {r:?}");
            (
                strip_ticks(r[0]).to_string(),
                strip_ticks(r[1]) == "yes",
                strip_ticks(r[2]) == "yes",
                strip_ticks(r[3]).to_string(),
            )
        })
        .collect();
    let expected_primaries: Vec<(String, bool, bool, String)> = all_primaries
        .iter()
        .map(|&p| {
            let cs = ColorSpace {
                primaries: p,
                ..ColorSpace::BT709
            };
            (
                format!("{p:?}"),
                p == ColorPrimaries::default(),
                cs.is_wide_gamut(),
                format!("{:?}", cs.resolved().primaries),
            )
        })
        .collect();
    assert_eq!(
        doc_primaries, expected_primaries,
        "docs/reference/color.md ColorPrimaries table drifted"
    );

    let all_transfers = [
        TransferFunction::Bt709,
        TransferFunction::Srgb,
        TransferFunction::Pq,
        TransferFunction::Hlg,
        TransferFunction::Unspecified,
    ];
    for t in all_transfers {
        match t {
            TransferFunction::Bt709
            | TransferFunction::Srgb
            | TransferFunction::Pq
            | TransferFunction::Hlg
            | TransferFunction::Unspecified => {}
        }
    }
    let transfer_rows = table_rows(heading_body(
        COLOR_DOC,
        "### Transfer functions (`TransferFunction`)",
    ));
    let doc_transfers: Vec<(String, bool, bool, String)> = transfer_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed TransferFunction row: {r:?}");
            (
                strip_ticks(r[0]).to_string(),
                strip_ticks(r[1]) == "yes",
                strip_ticks(r[2]) == "yes",
                strip_ticks(r[3]).to_string(),
            )
        })
        .collect();
    let expected_transfers: Vec<(String, bool, bool, String)> = all_transfers
        .iter()
        .map(|&t| {
            let cs = ColorSpace {
                transfer: t,
                ..ColorSpace::BT709
            };
            (
                format!("{t:?}"),
                t == TransferFunction::default(),
                cs.is_hdr(),
                format!("{:?}", cs.resolved().transfer),
            )
        })
        .collect();
    assert_eq!(
        doc_transfers, expected_transfers,
        "docs/reference/color.md TransferFunction table drifted"
    );

    let all_matrices = [
        MatrixCoefficients::Bt709,
        MatrixCoefficients::Bt601,
        MatrixCoefficients::Bt2020Ncl,
        MatrixCoefficients::Identity,
        MatrixCoefficients::Unspecified,
    ];
    for m in all_matrices {
        match m {
            MatrixCoefficients::Bt709
            | MatrixCoefficients::Bt601
            | MatrixCoefficients::Bt2020Ncl
            | MatrixCoefficients::Identity
            | MatrixCoefficients::Unspecified => {}
        }
    }
    let matrix_rows = table_rows(heading_body(
        COLOR_DOC,
        "### YUV-to-RGB matrix coefficients (`MatrixCoefficients`)",
    ));
    let doc_matrices: Vec<(String, bool, String)> = matrix_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed MatrixCoefficients row: {r:?}");
            (
                strip_ticks(r[0]).to_string(),
                strip_ticks(r[1]) == "yes",
                strip_ticks(r[2]).to_string(),
            )
        })
        .collect();
    let expected_matrices: Vec<(String, bool, String)> = all_matrices
        .iter()
        .map(|&m| {
            let cs = ColorSpace {
                matrix: m,
                ..ColorSpace::BT709
            };
            (
                format!("{m:?}"),
                m == MatrixCoefficients::default(),
                format!("{:?}", cs.resolved().matrix),
            )
        })
        .collect();
    assert_eq!(
        doc_matrices, expected_matrices,
        "docs/reference/color.md MatrixCoefficients table drifted"
    );

    let all_ranges = [ColorRange::Limited, ColorRange::Full];
    for r in all_ranges {
        match r {
            ColorRange::Limited | ColorRange::Full => {}
        }
    }
    let range_rows = table_rows(heading_body(COLOR_DOC, "### Sample range (`ColorRange`)"));
    let doc_ranges: Vec<(String, bool, bool)> = range_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 4, "malformed ColorRange row: {r:?}");
            (
                strip_ticks(r[0]).to_string(),
                strip_ticks(r[1]) == "yes",
                strip_ticks(r[2]) == "true",
            )
        })
        .collect();
    let expected_ranges: Vec<(String, bool, bool)> = all_ranges
        .iter()
        .map(|&r| (format!("{r:?}"), r == ColorRange::default(), r.is_full()))
        .collect();
    assert_eq!(
        doc_ranges, expected_ranges,
        "docs/reference/color.md ColorRange table drifted"
    );

    let presets = [
        ("BT709", ColorSpace::BT709),
        ("BT601", ColorSpace::BT601),
        ("BT2020_PQ", ColorSpace::BT2020_PQ),
        ("SRGB", ColorSpace::SRGB),
    ];
    let preset_rows = table_rows(heading_body(COLOR_DOC, "### Preset `ColorSpace` constants"));
    let doc_presets: Vec<(String, String, String, String, String, bool, bool, bool)> = preset_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 8, "malformed ColorSpace preset row: {r:?}");
            (
                strip_ticks(r[0]).to_string(),
                strip_ticks(r[1]).to_string(),
                strip_ticks(r[2]).to_string(),
                strip_ticks(r[3]).to_string(),
                strip_ticks(r[4]).to_string(),
                strip_ticks(r[5]) == "true",
                strip_ticks(r[6]) == "true",
                strip_ticks(r[7]) == "yes",
            )
        })
        .collect();
    let expected_presets: Vec<(String, String, String, String, String, bool, bool, bool)> = presets
        .iter()
        .map(|&(name, cs)| {
            (
                name.to_string(),
                format!("{:?}", cs.primaries),
                format!("{:?}", cs.transfer),
                format!("{:?}", cs.matrix),
                format!("{:?}", cs.range),
                cs.is_hdr(),
                cs.is_wide_gamut(),
                cs == ColorSpace::default(),
            )
        })
        .collect();
    assert_eq!(
        doc_presets, expected_presets,
        "docs/reference/color.md Preset ColorSpace constants table drifted"
    );

    // Starter LUT pack.
    let lut_sec = heading_body(
        COLOR_DOC,
        "## Starter LUT pack (`StarterLutEntry` and `StarterLut`)",
    );
    let lut_rows = table_rows(lut_sec);
    let doc_luts: Vec<(String, String, String, u32)> = lut_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 4, "malformed StarterLutEntry row: {r:?}");
            (
                strip_ticks(r[0]).to_string(),
                r[1].to_string(),
                strip_ticks(r[2]).to_string(),
                strip_ticks(r[3]).parse::<u32>().expect("grid size"),
            )
        })
        .collect();
    let expected_luts: Vec<(String, String, String, u32)> = starter_lut_entries()
        .into_iter()
        .map(|e| (e.id, e.label, e.filter_id, STARTER_LUT_SIZE))
        .collect();
    assert_eq!(
        doc_luts, expected_luts,
        "docs/reference/color.md Starter LUT pack table drifted from starter_lut_entries()"
    );
    for patch in SWATCH_REFERENCE {
        let token = format!("`[{}, {}, {}]`", patch[0], patch[1], patch[2]);
        assert!(
            lut_sec.contains(&token),
            "color.md Starter LUT section missing SWATCH_REFERENCE patch {token}"
        );
    }

    // WheelControl::ALL.
    let wheel_sec = heading_body(COLOR_DOC, "### Grading wheels (`WheelControl`)");
    let wheel_rows = table_rows(wheel_sec);
    let doc_wheels: Vec<(
        String,
        String,
        String,
        f32,
        f32,
        String,
        String,
        String,
        String,
    )> = wheel_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 9, "malformed WheelControl row: {r:?}");
            (
                strip_ticks(r[0]).to_string(),
                strip_ticks(r[1]).to_string(),
                r[2].to_string(),
                strip_ticks(r[3]).parse::<f32>().expect("neutral master"),
                strip_ticks(r[4]).parse::<f32>().expect("reach"),
                strip_ticks(r[5]).to_string(),
                strip_ticks(r[6]).to_string(),
                r[7].to_string(),
                r[8].to_string(),
            )
        })
        .collect();
    let expected_wheels: Vec<(
        String,
        String,
        String,
        f32,
        f32,
        String,
        String,
        String,
        String,
    )> = WheelControl::ALL
        .iter()
        .map(|&w| {
            let [pr, pg, pb] = w.rgb_params();
            (
                format!("{w:?}"),
                w.id().to_string(),
                w.label().to_string(),
                w.neutral_master(),
                w.reach(),
                format!("{:?}", w.master_param()),
                format!("{pr:?}, {pg:?}, {pb:?}"),
                w.drag_label().to_string(),
                w.reset_label().to_string(),
            )
        })
        .collect();
    assert_eq!(
        doc_wheels, expected_wheels,
        "docs/reference/color.md WheelControl table drifted from WheelControl::ALL"
    );
}
