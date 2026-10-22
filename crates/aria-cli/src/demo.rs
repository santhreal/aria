//! `aria demo` — composite a scene on a headless GPU and write the canvas.
//!
//! The one command that needs no project: it proves the GPU, the text
//! rasterizer, and the readback path on a machine that has never opened the
//! editor.

use std::path::{Path, PathBuf};

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, GpuContext, LayerPlacement, RgbaImage,
};
use aria_text::{TextRenderer, TextStyle};

pub const FLAGS: &[&str] = &[];
pub(crate) const DEFAULT_OUT: &str = "aria-demo.png";
pub(crate) const WIDTH: u32 = 1920;
pub(crate) const HEIGHT: u32 = 1080;
const BACKGROUND_COLOR: [u8; 4] = [20, 20, 30, 255];
const TEXT_COLOR: [u8; 4] = [255, 255, 255, 255];
const TEXT_SIZE: f32 = 240.0;

pub fn run(out: Option<PathBuf>) -> Result<(), String> {
    let out_path = out.unwrap_or_else(|| PathBuf::from(DEFAULT_OUT));

    let gpu = GpuContext::new_headless_blocking().map_err(|err| err.to_string())?;
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(WIDTH, HEIGHT).with_background(BACKGROUND_COLOR);

    let mut text = TextRenderer::new();
    let title = text.rasterize("Aria", &TextStyle::new(TEXT_SIZE).with_color(TEXT_COLOR));

    let mut layers = vec![CompositeLayer::solid(
        BACKGROUND_COLOR,
        LayerPlacement::full_canvas(&config),
    )];
    // A fontless host rasterizes to a 0×0 bitmap, which the compositor rejects;
    // only stack the text layer when there are actually glyphs to place.
    if title.width > 0 && title.height > 0 {
        let (tw, th) = (title.width as f32, title.height as f32);
        // Snap the top-left corner to an integer pixel. This is an unscaled 1:1
        // blit, so a half-pixel corner makes every destination pixel sample
        // halfway between two texels and the linear sampler smears the whole
        // run by half a texel.
        let left = (WIDTH as f32 / 2.0 - tw / 2.0).round();
        let top = (HEIGHT as f32 / 2.0 - th / 2.0).round();
        layers.push(CompositeLayer::cutout(
            &title,
            LayerPlacement {
                center: [left + tw / 2.0, top + th / 2.0],
                size: [tw, th],
                rotation: 0.0,
                opacity: 1.0,
            },
        ));
    } else {
        eprintln!("note: no fonts available; rendering the background only");
    }

    let image = comp
        .render(&gpu, &config, &layers)
        .map_err(|err| err.to_string())?;
    write_png(&image, &out_path)?;
    println!(
        "wrote {}x{} canvas to {}",
        image.width,
        image.height,
        out_path.display()
    );
    Ok(())
}

/// Write a straight-alpha [`RgbaImage`] to an 8-bit RGBA PNG.
pub fn write_png(image: &RgbaImage, path: &Path) -> Result<(), String> {
    let bytes = aria_render::encode_png(image).map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| format!("{}: {err}", path.display()))
}
