//! SDF shape layers against the CPU reference raster: shape.wgsl and
//! aria-shapes/src/sdf.rs implement the same math, so the two canvases must
//! agree within rounding tolerance.

use super::*;

#[test]
fn sdf_shapes_match_cpu_reference() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);

    let cases: Vec<(&str, SdfParams, [f32; 2])> = vec![
        ("rect", SdfParams::RoundedRect { radius: 0.0 }, [40.0, 25.0]),
        (
            "rounded_rect",
            SdfParams::RoundedRect { radius: 12.0 },
            [40.0, 25.0],
        ),
        ("ellipse", SdfParams::Ellipse, [35.0, 22.0]),
        ("triangle", SdfParams::polygon(3, 0.0), [40.0, 40.0]),
        ("hexagon", SdfParams::polygon(6, 4.0), [40.0, 40.0]),
        (
            "star5",
            SdfParams::Star {
                points: 5,
                inner: 0.5,
                round: 0.0,
            },
            [40.0, 40.0],
        ),
        ("line", SdfParams::Line, [45.0, 5.0]),
        ("arrow", SdfParams::Arrow, [45.0, 20.0]),
        ("heart", SdfParams::Heart, [38.0, 36.0]),
    ];
    for (name, params, half) in cases {
        let (gpu_img, cpu_img) = render_shape_pair(
            &gpu,
            &mut comp,
            params,
            half,
            fill_only([230, 80, 40, 255]),
            1.0,
        );
        assert_images_agree(&gpu_img, &cpu_img, name, 4);
    }
}

#[test]
fn sdf_stroke_and_translucency_match_cpu_reference() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);

    // Fill + stroke.
    let stroked = ShapeStyle {
        fill: Some([20, 120, 240, 255]),
        stroke: Some(Stroke {
            rgba: [255, 230, 0, 255],
            width: 8.0,
        }),
    };
    let (g, c) = render_shape_pair(
        &gpu,
        &mut comp,
        SdfParams::Ellipse,
        [30.0, 30.0],
        stroked,
        1.0,
    );
    assert_images_agree(&g, &c, "stroked ellipse", 4);

    // Stroke-only (fill alpha 0) with a translucent stroke.
    let outline = ShapeStyle {
        fill: None,
        stroke: Some(Stroke {
            rgba: [255, 255, 255, 128],
            width: 5.0,
        }),
    };
    let (g, c) = render_shape_pair(
        &gpu,
        &mut comp,
        SdfParams::RoundedRect { radius: 10.0 },
        [30.0, 20.0],
        outline,
        1.0,
    );
    assert_images_agree(&g, &c, "outline rect", 4);

    // Translucent fill under layer opacity.
    let (g, c) = render_shape_pair(
        &gpu,
        &mut comp,
        SdfParams::polygon(5, 0.0),
        [30.0, 30.0],
        fill_only([200, 40, 200, 180]),
        0.6,
    );
    assert_images_agree(&g, &c, "translucent pentagon at 0.6 opacity", 4);
}

#[test]
fn sdf_quad_padding_keeps_stroke_unclipped() {
    // The stroke overhangs the shape edge by width/2; the CPU raster (and the
    // resolver) pad the quad accordingly. Verify ink reaches beyond the shape
    // box but stays inside the padded quad.
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let style = ShapeStyle {
        fill: Some([255, 0, 0, 255]),
        stroke: Some(Stroke {
            rgba: [0, 255, 0, 255],
            width: 10.0,
        }),
    };
    let (g, _) = render_shape_pair(
        &gpu,
        &mut comp,
        SdfParams::RoundedRect { radius: 0.0 },
        [20.0, 15.0],
        style,
        1.0,
    );
    let (cx, cy) = (g.width / 2, g.height / 2);
    // 23px right of center: inside the stroke ring (edge at 20 + 5 overhang).
    assert_px(&g, cx + 23, cy, [0, 255, 0, 255], 3);
    // At the canvas corner: untouched background.
    assert_px(&g, 0, 0, [40, 60, 80, 255], 1);
}
