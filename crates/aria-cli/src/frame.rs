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
    project::warn_missing_plugins(&project);
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
    /// the end of the span is a usage error (exit 2) with the same words `export`
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

        // 3 s of picture on a 24 fps timeline: ticks 0..72.
        let cut = new_project(dir.path(), "cut.aria", &["fixture-640x360-30fps-3s.mp4"]);
        for at in ["99", "3", "1e300"] {
            let err = frame(&cut, &png, Some(at)).expect_err("an instant past the end rendered");
            assert_eq!(
                err,
                Failure::Usage(
                    "--at must be before the end of the project (0:00:03.000)".to_owned()
                ),
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

    /// The last frame of a span is an instant `frame` renders and the end is
    /// not, on every rate `aria new` offers and every span up to 200 frames,
    /// and an instant in the last half frame, which rounds to the end, lands on
    /// the last frame. The end read back from seconds falls a hair short of its
    /// tick on several of these spans, so a check without slack accepts it. The
    /// last half frame was once refused as "--at (0:00:03.000) must be before
    /// project duration (0:00:03.000)" for `--at 2.98` on a 3 s span.
    #[test]
    fn the_last_frame_is_inside_the_span_and_the_end_is_past_it_at_every_rate() {
        for (key, rate) in crate::new::RATES {
            let tick = 1.0 / rate.as_f64();
            for frames in 2..=200 {
                let duration = RationalTime::new(frames, rate);
                let last = frames - 1;
                let start = RationalTime::new(last, rate).seconds();
                for (into, lands) in [(0.0, last), (0.49, last), (0.75, last), (0.999, last)]
                    .into_iter()
                    .chain([(-0.75, last - 1)])
                {
                    assert_eq!(
                        project::instant_before_end(start + into * tick, duration, rate)
                            .map(|t| t.value),
                        Ok(lands),
                        "{key} fps, {frames} frames, {into} of a frame into the last"
                    );
                }
                assert!(
                    project::instant_before_end(duration.seconds(), duration, rate).is_err(),
                    "{key} fps accepted the end of a {frames}-frame span"
                );
            }
        }
    }

    /// Every `.rs` file in `crates/aria-cli/src` whose production code calls
    /// `write_png(`.
    fn png_writers_in_cli_source() -> std::collections::BTreeSet<String> {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut writers = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir(&src).expect("read crates/aria-cli/src") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("file name")
                .to_owned();
            if name.ends_with("_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read source file");
            let prod = text.split("#[cfg(test)]").next().unwrap_or(&text);
            if prod.contains("write_png(") {
                assert!(
                    !prod.contains("std::fs::write(") && !prod.contains("File::create("),
                    "{name} writes directly to disk instead of routing through atomic write_png"
                );
                writers.insert(name);
            }
        }
        writers
    }

    /// WHY: `aria frame` and `aria demo` encoded a PNG and passed it to
    /// `std::fs::write`, which opens the destination with `O_TRUNC` in place
    /// and follows symlinks: an interrupted or failed write left a truncated
    /// file over the prior PNG, and a symlink destination overwrote its target.
    /// Every PNG writer in `crates/aria-cli/src` is derived from source at run
    /// time and driven over a pre-populated destination observed through a hard
    /// link (proving replacement by rename rather than in-place truncation), a
    /// failing write, and a symlink destination. Not caught: power loss when
    /// the filesystem ignores `sync_all`.
    #[test]
    fn every_cli_png_writer_publishes_atomically_and_preserves_prior_destination_on_failure() {
        use aria_core::RgbaImage;
        use std::collections::BTreeSet;

        let derived = png_writers_in_cli_source();
        type Writer = (
            &'static str,
            Box<dyn Fn(&std::path::Path) -> Result<(), String>>,
        );
        let dir = tempfile::tempdir().expect("scratch directory");
        let cut = new_project(dir.path(), "cut.aria", &["fixture-640x360-30fps-3s.mp4"]);
        let writers: Vec<Writer> = vec![
            (
                "demo.rs",
                Box::new(|out| {
                    let image = RgbaImage::new(
                        2,
                        2,
                        vec![
                            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
                        ],
                    );
                    crate::demo::write_png(&image, out)
                }),
            ),
            (
                "frame.rs",
                Box::new({
                    let cut = cut.clone();
                    move |out| frame(&cut, out, None).map_err(|err| err.message().to_owned())
                }),
            ),
        ];
        let covered: BTreeSet<String> =
            writers.iter().map(|(name, _)| (*name).to_owned()).collect();
        assert_eq!(
            covered, derived,
            "every PNG writer in crates/aria-cli/src must be exercised in this table"
        );

        let old_bytes = b"prior PNG bytes that an interrupted or failed write must not truncate";
        for (name, write_ok) in &writers {
            let sub = dir.path().join(name.trim_end_matches(".rs"));
            std::fs::create_dir_all(&sub).expect("create writer scratch");
            let out = sub.join("out.png");
            let prior_inode = sub.join("prior-inode.png");
            std::fs::write(&out, old_bytes).expect("seed prior destination");
            std::fs::hard_link(&out, &prior_inode).expect("hard-link prior inode");

            write_ok(&out).unwrap_or_else(|err| panic!("{name} failed on valid output: {err}"));

            assert_eq!(
                std::fs::read(&prior_inode).expect("read prior inode"),
                old_bytes,
                "{name} truncated the destination inode in place instead of replacing by rename"
            );
            let written = std::fs::read(&out).expect("read published PNG");
            let decoded = aria_render::decode_png(&written)
                .unwrap_or_else(|err| panic!("{name} did not write a valid PNG: {err}"));
            assert!(decoded.width > 0 && decoded.height > 0, "{name}");

            let leftover: Vec<_> = std::fs::read_dir(&sub)
                .expect("read writer dir")
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|entry| entry.starts_with(".aria-export-"))
                .collect();
            assert!(
                leftover.is_empty(),
                "{name} left staging directories behind: {leftover:?}"
            );

            #[cfg(unix)]
            {
                let target = sub.join("symlink-target.png");
                let link = sub.join("symlink-out.png");
                std::fs::write(&target, old_bytes).expect("seed symlink target");
                std::os::unix::fs::symlink(&target, &link).expect("create symlink");
                let err = write_ok(&link)
                    .expect_err(&format!("{name} followed a symlink at the destination"));
                assert!(
                    err.contains(&format!("{} is a symbolic link", link.display())),
                    "{name} symlink refusal did not name the symlink: {err}"
                );
                assert_eq!(
                    std::fs::read(&target).expect("read symlink target"),
                    old_bytes,
                    "{name} overwrote the symlink target"
                );
            }
        }

        // A failed encode or missing parent directory leaves an existing file
        // untouched and leaves no staging directory behind.
        let guarded = dir.path().join("guarded.png");
        std::fs::write(&guarded, old_bytes).expect("seed guarded destination");
        let malformed = RgbaImage::new(4, 4, vec![0u8; 4]);
        assert!(
            crate::demo::write_png(&malformed, &guarded).is_err(),
            "malformed RGBA buffer must fail encode_png"
        );
        assert_eq!(
            std::fs::read(&guarded).expect("read guarded destination"),
            old_bytes,
            "failed encode overwrote existing destination"
        );
    }
}
