//! `aria frame` — render one timeline instant to a PNG.

use std::path::PathBuf;

use aria_render::{EmptyExport, Renderer};

use crate::args::Args;
use crate::fail::Failure;
use crate::project;

pub(crate) const FLAGS: &[&str] = &["at", "fit"];
pub(crate) const DEFAULT_OUT: &str = "aria-frame.png";

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let path = args.required_path(0, "a project file")?;
    let out = args
        .positional
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OUT));
    let at = args.seconds("at")?.unwrap_or(0.0);
    let fit = args.size("fit")?;

    let project = project::load(&path)?;
    aria_render::check_export_destination(&project, &out, Some(&path))
        .map_err(|err| Failure::Usage(err.to_string()))?;
    let rate = project.timeline().frame_rate;
    let duration = project.timeline().duration();
    if duration.value <= 0 {
        return Err(Failure::Failed(format!(
            "nothing to render: {}",
            EmptyExport::Sequence
        )));
    }
    let t = project::instant_before_end(at, duration, rate).map_err(Failure::Usage)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::RationalTime;

    fn new_project(dir: &std::path::Path, name: &str, media: &[&str]) -> PathBuf {
        let out = dir.join(name);
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        let media: Vec<String> = media
            .iter()
            .map(|file| fixtures.join(file).to_string_lossy().into_owned())
            .collect();
        let mut argv = vec!["new".to_owned(), out.to_string_lossy().into_owned()];
        argv.extend(media);
        let args = Args::parse(argv.iter().map(String::as_str)).expect("arguments parse");
        if let Err(err) = crate::new::run(&args) {
            panic!("aria new failed: {}", err.message());
        }
        out
    }

    fn frame(
        project: &std::path::Path,
        png: &std::path::Path,
        at: Option<&str>,
    ) -> Result<(), Failure> {
        let mut argv = vec![
            "frame".to_owned(),
            project.to_string_lossy().into_owned(),
            png.to_string_lossy().into_owned(),
        ];
        if let Some(at) = at {
            argv.extend(["--at".to_owned(), at.to_owned()]);
        }
        run(&Args::parse(argv.iter().map(String::as_str)).expect("arguments parse"))
    }

    /// WHY: `aria frame cut.aria --at 99` on a 3 s cut wrote a black PNG and
    /// exited 0, and a frame of an empty timeline did the same, so a script
    /// could not tell a rendered picture from no picture. An instant at or past
    /// the last frame is a usage error (exit 2) with the same words `export`
    /// prints, an empty timeline is a failure (exit 1) naming the reason, and
    /// neither writes the PNG. Both refusals come before a renderer opens, so
    /// this runs without a GPU. Not caught: a picture that is wrong inside the
    /// span.
    #[test]
    fn an_instant_with_no_frame_is_refused_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("scratch directory");
        let png = dir.path().join("out.png");

        let empty = new_project(dir.path(), "empty.aria", &[]);
        let err = frame(&empty, &png, None).expect_err("an empty timeline rendered a frame");
        assert_eq!(
            err,
            Failure::Failed(format!("nothing to render: {}", EmptyExport::Sequence))
        );
        assert_eq!(err.code(), 1);
        assert!(!png.exists(), "an empty timeline wrote {}", png.display());

        // 3 s of picture on a 24 fps timeline: ticks 0..72. 2.98 s rounds to
        // tick 72, the first instant with no frame.
        let cut = new_project(dir.path(), "cut.aria", &["fixture-640x360-30fps-3s.mp4"]);
        for (at, shown) in [
            ("99", "0:01:39.000"),
            ("3", "0:00:03.000"),
            ("2.98", "0:00:03.000"),
        ] {
            let err = frame(&cut, &png, Some(at)).expect_err("an instant past the end rendered");
            assert_eq!(
                err,
                Failure::Usage(format!(
                    "--at ({shown}) must be before project duration (0:00:03.000)"
                )),
                "--at {at}"
            );
            assert_eq!(err.code(), 2, "--at {at}");
            assert!(!png.exists(), "--at {at} wrote {}", png.display());
        }
    }

    /// WHY: `aria frame p.aria p.aria` and `aria export p.aria p.aria`
    /// replaced the project with a PNG or an MP4 and exited 0, and naming a
    /// file in the pool as the output replaced the footage. Both verbs fail
    /// with a usage error (exit 2) before a renderer opens, and the input keeps
    /// its bytes. Not caught: `demo`, which reads no project.
    #[test]
    fn an_output_that_is_an_input_is_a_usage_error_for_every_verb_that_writes() {
        let dir = tempfile::tempdir().expect("scratch directory");
        let media = dir.path().join("clip.mp4");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4"),
            &media,
        )
        .expect("copy the fixture");
        let project = dir.path().join("p.aria");
        let new = [
            "new".to_owned(),
            project.to_string_lossy().into_owned(),
            media.to_string_lossy().into_owned(),
        ];
        crate::new::run(&Args::parse(new.iter().map(String::as_str)).expect("parse"))
            .expect("aria new");
        let inputs = [
            (project.clone(), "the project file"),
            (media.clone(), "media this project uses"),
        ];
        let before: Vec<Vec<u8>> = inputs
            .iter()
            .map(|(path, _)| std::fs::read(path).expect("read input"))
            .collect();
        for verb in ["frame", "export"] {
            for (output, what) in &inputs {
                let argv = [
                    verb.to_owned(),
                    project.to_string_lossy().into_owned(),
                    output.to_string_lossy().into_owned(),
                ];
                let args = Args::parse(argv.iter().map(String::as_str)).expect("parse");
                let run = if verb == "frame" {
                    run
                } else {
                    crate::export::run
                };
                let err = run(&args).expect_err("an output over an input was written");
                assert_eq!(
                    err,
                    Failure::Usage(format!(
                        "{} is {what}; export to another file",
                        output.display()
                    )),
                    "{verb} over {what}"
                );
            }
        }
        for ((path, _), bytes) in inputs.iter().zip(&before) {
            assert_eq!(
                &std::fs::read(path).expect("read input"),
                bytes,
                "{}",
                path.display()
            );
        }
    }

    /// The last frame of a span is an instant `frame` renders and the next one
    /// is not, on every rate `aria new` offers. A check written in seconds
    /// rather than ticks drifts on the 1001 rates.
    #[test]
    fn the_last_frame_is_inside_the_span_and_the_next_is_past_it_at_every_rate() {
        for (key, rate) in crate::new::RATES {
            let duration = RationalTime::new(10, rate);
            let last = RationalTime::new(9, rate).seconds();
            let past = duration.seconds();
            assert_eq!(
                project::instant_before_end(last, duration, rate).map(|t| t.value),
                Ok(9),
                "{key} fps"
            );
            assert!(
                project::instant_before_end(past, duration, rate).is_err(),
                "{key} fps accepted the instant after the last frame"
            );
        }
    }
}
