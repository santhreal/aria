//! `aria frame` — render one timeline instant to a PNG.

use std::path::PathBuf;

use aria_render::Renderer;

use crate::args::Args;
use crate::fail::Failure;
use crate::project;

pub const FLAGS: &[&str] = &["at", "fit"];
pub(crate) const DEFAULT_OUT: &str = "aria-frame.png";

pub fn run(args: &Args) -> Result<(), Failure> {
    let path = args.required_path(0, "a project file")?;
    let out = args
        .positional
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OUT));
    let at = args.seconds("at")?.unwrap_or(0.0);
    let fit = args.size("fit")?;

    let project = project::load(&path)?;
    let rate = project.timeline().frame_rate;
    let t = project::time_for_seconds(at, rate);
    let mut renderer = Renderer::new_headless().map_err(|err| err.to_string())?;
    let image = match fit {
        Some((width, height)) => renderer.render_frame_fit(&project, t, width, height),
        None => renderer.render_frame(&project, t),
    }
    .map_err(|err| err.to_string())?;
    project::warn_driver_faults(&project);

    crate::demo::write_png(&image, &out)?;
    println!(
        "wrote {}x{} at {} (tick {}) to {}",
        image.width,
        image.height,
        project::clock(t.seconds()),
        t.value,
        out.display()
    );
    Ok(())
}
