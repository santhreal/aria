//! Glyph atlas + instanced draw parity: compositing shaped clusters through
//! the GPU glyph pipeline must match CPU `rasterize` of the same run drawn as
//! one cutout bitmap, wherever the run lands and at any scale.
//!
//! The two draws sample different textures: each glyph sits in the atlas
//! inside transparent padding, and the run's bitmap holds every glyph. A
//! quad drawn to its texture rect's edges clips the bilinear footprint, which
//! reaches half a texel past the edge texels, so at a quad edge between
//! pixels the glyph quads and the bitmap quad dropped different texels, and a
//! bitmap sampled past its edge repeated the edge texel where the atlas read
//! transparent. The sweep crosses sub-pixel origins with scales.
//!
//! What this does not catch: glyphs closer than a texel, where two glyph
//! quads blend where the bitmap interpolates, and rotation.

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, GlyphsLayer, GpuContext, LayerPlacement,
    RgbaImage, identity_instances,
};
use aria_text::{TextRenderer, TextStyle};

fn try_gpu() -> Option<GpuContext> {
    match GpuContext::new_headless_blocking() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping glyph compositor test: no GPU adapter ({e})");
            None
        }
    }
}

macro_rules! gpu_or_skip {
    () => {
        match try_gpu() {
            Some(g) => g,
            None => return,
        }
    };
}

/// Bundled OFL face so CI shapes real glyphs without system fonts.
const TEST_FONT: &[u8] = include_bytes!("../../aria-text/assets/Micro5-Regular.ttf");

fn test_renderer() -> TextRenderer {
    let mut r = TextRenderer::new();
    assert!(r.load_font(TEST_FONT.to_vec()) > 0);
    r
}

fn assert_images_agree(gpu_img: &RgbaImage, cpu_img: &RgbaImage, what: &str, tol: i32) {
    assert_eq!(
        (gpu_img.width, gpu_img.height),
        (cpu_img.width, cpu_img.height),
        "{what}: size mismatch"
    );
    let mut worst = 0i32;
    let mut worst_at = (0u32, 0u32);
    for y in 0..gpu_img.height {
        for x in 0..gpu_img.width {
            let a = gpu_img.pixel(x, y);
            let b = cpu_img.pixel(x, y);
            for c in 0..4 {
                let d = (i32::from(a[c]) - i32::from(b[c])).abs();
                if d > worst {
                    worst = d;
                    worst_at = (x, y);
                }
            }
        }
    }
    assert!(
        worst <= tol,
        "{what}: GPU and CPU disagree by {worst} at {worst_at:?} \
         (gpu {:?} vs cpu {:?}, tol {tol})",
        gpu_img.pixel(worst_at.0, worst_at.1),
        cpu_img.pixel(worst_at.0, worst_at.1),
    );
}

/// Origins of the run on the canvas, in pixels: on the grid, halfway between
/// pixels, and a quarter off on each axis.
const ORIGINS: [[f32; 2]; 3] = [[8.0, 8.0], [8.5, 8.5], [8.25, 8.75]];

/// Run scales: drawn at its texels, larger, and smaller.
const SCALES: [f32; 3] = [1.0, 1.5, 0.75];

#[test]
fn glyph_instances_match_the_cutout_bitmap_at_any_placement() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let mut text = test_renderer();
    let style = TextStyle::new(48.0).with_color([255, 220, 0, 255]);
    let content = "Hi";

    let shaped = text.shape(content, &style);
    assert!(shaped.has_ink());
    // CPU rasterize of the same run — the shape() docs guarantee compositing
    // clusters reproduces this.
    let cpu = text.rasterize(content, &style);
    assert!(cpu.width > 0 && cpu.height > 0);

    let glyphs: Vec<RgbaImage> = shaped.clusters.iter().map(|c| c.image.clone()).collect();
    let offsets: Vec<[f32; 2]> = shaped.clusters.iter().map(|c| c.offset).collect();

    // Opaque background so fringe premultiply differences don't dominate,
    // with room around the largest run for the quad edges to land on.
    let config = CompositorConfig::new(cpu.width * 2 + 16, cpu.height * 2 + 16)
        .with_background([0, 0, 0, 255]);
    let tol = if gpu.is_software() { 8 } else { 4 };
    for scale in SCALES {
        for origin in ORIGINS {
            let instances = identity_instances(&glyphs, &offsets, origin, scale);
            let size = [cpu.width as f32 * scale, cpu.height as f32 * scale];
            let placement = LayerPlacement {
                center: [origin[0] + size[0] * 0.5, origin[1] + size[1] * 0.5],
                size,
                rotation: 0.0,
                opacity: 1.0,
            };
            let glyph_layer = CompositeLayer::glyphs(
                GlyphsLayer {
                    atlas_key: 1,
                    glyphs: &glyphs,
                    instances: &instances,
                },
                placement,
            );
            let glyph_img = comp
                .render(&gpu, &config, &[glyph_layer])
                .expect("render glyphs");
            let cutout_img = comp
                .render(&gpu, &config, &[CompositeLayer::cutout(&cpu, placement)])
                .expect("render cutout");
            assert_images_agree(
                &glyph_img,
                &cutout_img,
                &format!("glyphs vs cutout at scale {scale}, origin {origin:?}"),
                tol,
            );
        }
    }
}

#[test]
fn glyph_atlas_reused_across_frames() {
    let gpu = gpu_or_skip!();
    let mut comp = Compositor::new(&gpu);
    let mut text = test_renderer();
    let style = TextStyle::new(40.0);
    let shaped = text.shape("AB", &style);
    let glyphs: Vec<RgbaImage> = shaped.clusters.iter().map(|c| c.image.clone()).collect();
    let offsets: Vec<[f32; 2]> = shaped.clusters.iter().map(|c| c.offset).collect();

    let config = CompositorConfig::new(200, 80).with_background([0, 0, 0, 255]);
    for frame in 0..3 {
        let origin = [10.0 + frame as f32 * 4.0, 10.0];
        let instances = identity_instances(&glyphs, &offsets, origin, 1.0);
        let layer = CompositeLayer::glyphs(
            GlyphsLayer {
                atlas_key: 42,
                glyphs: &glyphs,
                instances: &instances,
            },
            LayerPlacement {
                center: [100.0, 40.0],
                size: [200.0, 80.0],
                rotation: 0.0,
                opacity: 1.0,
            },
        );
        let img = comp.render(&gpu, &config, &[layer]).expect("render");
        // Some glyph coverage should land on the canvas.
        let lit = img
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0)
            .count();
        assert!(lit > 0, "frame {frame} drew no glyph coverage");
    }
}
