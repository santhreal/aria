//! `aria export` — encode a span of the timeline to a video file.
//!
//! The command refuses before it renders. A codec this machine cannot write, or
//! a bitrate its encoder ignores, is named and rejected rather than silently
//! swapped for something that happens to work: a script that asked for HEVC and
//! got H.264 has no way to notice. The flags become one
//! [`aria_core::ExportFormat`], and every refusal in it is
//! [`ExportSettings::resolved`]'s — the same one the app reports — so this file
//! only owns what is a *usage* error: a rate of zero, or a `--to` at or before
//! its `--at`.

use aria_core::count::counted;
use aria_core::{ExportFormat, FpsError, VideoCodec};
use aria_render::{
    ExportSettings, MAX_FPS, RenderError, Renderer, export_to_file_with_encoder_observed,
};

use crate::args::Args;
use crate::fail::Failure;
use crate::project;

pub(crate) const FLAGS: &[&str] = &[
    "at", "to", "size", "fps", "codec", "bitrate", "no-audio", "quiet",
];

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let source = args.required_path(0, "a project file")?;
    let out = args.required_path(1, "an output file")?;
    let at = args.seconds("at")?;
    let to = args.seconds("to")?;
    let size = args.size("size")?;
    let fps = rate_from(args)?;
    let bitrate = args.number("bitrate")?;
    let codec = codec_from(args)?;
    let progress = shows_progress(
        args.present("quiet"),
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
    );

    let project = project::load(&source)?;
    aria_render::check_export_destination(&project, &out, Some(&source))
        .map_err(|err| Failure::Usage(err.to_string()))?;
    let rate = project.timeline().frame_rate;
    let duration = project.timeline().duration();
    let range = match resolve_export_range(at, to, duration, rate) {
        Ok(range) => range,
        Err(err) => return Err(Failure::Usage(err)),
    };

    let format = ExportFormat {
        size,
        frame_rate: fps,
        video_codec: codec,
        bitrate_kbps: bitrate,
        audio: args.present("no-audio").then_some(false),
        range,
        ..ExportFormat::PROJECT
    };
    let settings = ExportSettings::resolved(&project, format)
        .map_err(|err| Failure::Failed(err.to_string()))?;
    project::warn_missing_plugins(&project);

    let mut renderer = Renderer::new_headless().map_err(|err| err.to_string())?;
    // A signal stops the export between frames, so the export discards its
    // staging directory instead of leaving it, with a partial file, beside
    // `out`.
    let interrupt = crate::interrupt::Interrupt::install()?;
    let mut last_percent = u64::MAX;
    let mut observer = |done: u64, total: u64| {
        if progress && total > 0 {
            let percent = done * 100 / total;
            if percent != last_percent {
                last_percent = percent;
                eprint!("\rencoding {percent}% ({done}/{total})");
            }
        }
        interrupt.status().is_none()
    };
    let exported = export_to_file_with_encoder_observed(
        &mut renderer,
        &project,
        &out,
        settings,
        &mut aria_encoder::open_encoder,
        &mut observer as _,
    );
    // Ends the progress line before a failure as well as before the receipt.
    if progress {
        eprintln!();
    }
    let exported = exported.map_err(|err| match (err, interrupt.status()) {
        (RenderError::Cancelled, Some(code)) => Failure::Exit {
            code,
            message: format!("interrupted; nothing was written to {}", out.display()),
        },
        (err, _) => Failure::Failed(err.to_string()),
    })?;
    project::warn_driver_faults(&project);
    println!(
        "wrote {} of {}x{} {} to {}",
        counted(exported.frames, "frame", "frames"),
        settings.size.0,
        settings.size.1,
        aria_encoder::capability().label_for(settings.video_codec, exported.audio),
        out.display()
    );
    // The encoder line, then one `skipped` line per encoder that did not open.
    println!("{}", exported.encoder);
    Ok(())
}

/// Whether the export prints its `\rencoding N%` line: only to a terminal,
/// where each update overwrites the last, and never with `--quiet`. Sent to a
/// file or a pipe, the carriage returns piled one update per percent into a
/// single log line.
fn shows_progress(quiet: bool, stderr_is_terminal: bool) -> bool {
    !quiet && stderr_is_terminal
}

/// The `--codec` flag as a [`VideoCodec`], refusing a name no codec answers to.
fn codec_from(args: &Args) -> Result<Option<VideoCodec>, Failure> {
    let Some(raw) = args.value("codec")? else {
        return Ok(None);
    };
    VideoCodec::ALL
        .iter()
        .find(|codec| codec.key() == raw)
        .copied()
        .map(Some)
        .ok_or_else(|| {
            Failure::Usage(format!(
                "--codec wants one of {}, got \"{raw}\"",
                VideoCodec::ALL
                    .iter()
                    .map(|codec| codec.key())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

/// The `--fps` flag: a rate [`aria_core::NAMED_RATES`] holds, kept exact, so
/// `29.97` is `30000/1001` as it is for `aria new`, or a whole number of frames
/// a second from 1 to [`aria_render::MAX_FPS`].
fn rate_from(args: &Args) -> Result<Option<aria_core::Rational>, Failure> {
    let Some(raw) = args.value("fps")? else {
        return Ok(None);
    };
    aria_core::Rational::from_fps(raw, MAX_FPS)
        .map(Some)
        .map_err(|err| match err {
            FpsError::NotPositive => Failure::Usage("--fps wants a rate above zero".into()),
            FpsError::AboveMax => {
                Failure::Usage(format!("--fps wants a rate up to {MAX_FPS}, got {raw}"))
            }
            FpsError::NotARate => Failure::Usage(format!(
                "--fps wants a whole number or one of {}, got \"{raw}\"",
                aria_core::NAMED_RATES
                    .iter()
                    .map(|(key, _)| *key)
                    .filter(|key| key.contains('.'))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        })
}

pub(crate) fn resolve_export_range(
    at: Option<f64>,
    to: Option<f64>,
    duration: aria_core::RationalTime,
    rate: aria_core::Rational,
) -> Result<Option<(aria_core::RationalTime, aria_core::RationalTime)>, String> {
    if at.is_none() && to.is_none() {
        return Ok(None);
    }
    if let Some(at_val) = at
        && at_val < 0.0
    {
        return Err("--at cannot be negative".into());
    }
    if let Some(to_val) = to
        && to_val < 0.0
    {
        return Err("--to cannot be negative".into());
    }
    let from = project::instant_before_end(at.unwrap_or(0.0), duration, rate)?;
    let end = match to {
        Some(to) => project::time_for_seconds(to, rate),
        None => duration,
    };
    if end.value <= from.value {
        // `--to` past `--at` that still rounds onto the same frame holds no
        // frame; printing both rounded instants would show two equal times.
        let at_seconds = at.unwrap_or(0.0);
        let to_seconds = to.unwrap_or(0.0);
        let start = match at {
            Some(_) => format!("--at ({})", project::clock(at_seconds)),
            None => format!("the start ({})", project::clock(0.0)),
        };
        return Err(if to_seconds > at_seconds {
            format!(
                "{start} and --to ({}) land on the same frame; make the span at least one frame ({}) long",
                project::clock(to_seconds),
                project::clock(f64::from(rate.den) / f64::from(rate.num))
            )
        } else {
            format!(
                "--to ({}) must be after {start}",
                project::clock(to_seconds)
            )
        });
    }
    Ok(Some((from, end)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::Args;

    #[test]
    fn export_range_validation_catches_invalid_spans() {
        let rate = aria_core::Rational::FPS_24;
        let duration = aria_core::RationalTime::new(240, rate); // 10s

        assert_eq!(resolve_export_range(None, None, duration, rate), Ok(None));
        assert_eq!(
            resolve_export_range(Some(1.0), Some(5.0), duration, rate),
            Ok(Some((
                aria_core::RationalTime::new(24, rate),
                aria_core::RationalTime::new(120, rate),
            )))
        );
        assert_eq!(
            resolve_export_range(Some(2.0), None, duration, rate),
            Ok(Some((aria_core::RationalTime::new(48, rate), duration)))
        );
        assert_eq!(
            resolve_export_range(None, Some(5.0), duration, rate),
            Ok(Some((
                aria_core::RationalTime::new(0, rate),
                aria_core::RationalTime::new(120, rate),
            )))
        );

        // at >= duration, and an instant in the last half frame
        let err = resolve_export_range(Some(10.0), None, duration, rate).unwrap_err();
        assert_eq!(
            err,
            "--at must be before the end of the project (0:00:10.000)"
        );
        assert_eq!(
            resolve_export_range(Some(9.99), None, duration, rate),
            Ok(Some((aria_core::RationalTime::new(239, rate), duration)))
        );

        // at >= to
        assert_eq!(
            resolve_export_range(Some(6.0), Some(4.0), duration, rate),
            Err("--to (0:00:04.000) must be after --at (0:00:06.000)".to_owned())
        );
        assert_eq!(
            resolve_export_range(Some(1.0), Some(1.0), duration, rate),
            Err("--to (0:00:01.000) must be after --at (0:00:01.000)".to_owned())
        );
        assert_eq!(
            resolve_export_range(None, Some(0.0), duration, rate),
            Err("--to (0:00:00.000) must be after the start (0:00:00.000)".to_owned())
        );

        // negative at / to
        assert_eq!(
            resolve_export_range(Some(-1.0), Some(4.0), duration, rate),
            Err("--at cannot be negative".to_owned())
        );
        assert_eq!(
            resolve_export_range(Some(1.0), Some(-4.0), duration, rate),
            Err("--to cannot be negative".to_owned())
        );
    }

    /// The class closed here is "a span shorter than a frame is refused with
    /// two equal times". `--to` after `--at` that rounds onto the same frame
    /// prints the instants as typed and the length of one frame, so the
    /// refusal states what to widen. A span that reaches the next frame
    /// exports that one frame.
    #[test]
    fn a_span_inside_one_frame_is_refused_with_the_frame_length() {
        let rate = aria_core::Rational::FPS_24;
        let duration = aria_core::RationalTime::new(240, rate);
        for (at, to, typed) in [
            (
                Some(0.01),
                0.02,
                "--at (0:00:00.010) and --to (0:00:00.020)",
            ),
            (Some(1.0), 1.01, "--at (0:00:01.000) and --to (0:00:01.010)"),
            (
                Some(1.03),
                1.04,
                "--at (0:00:01.030) and --to (0:00:01.040)",
            ),
            (None, 0.01, "the start (0:00:00.000) and --to (0:00:00.010)"),
        ] {
            assert_eq!(
                resolve_export_range(at, Some(to), duration, rate),
                Err(format!(
                    "{typed} land on the same frame; make the span at least one frame (0:00:00.042) long"
                )),
                "--at {at:?} --to {to}"
            );
        }
        assert_eq!(
            resolve_export_range(Some(1.0), Some(1.03), duration, rate),
            Ok(Some((
                aria_core::RationalTime::new(24, rate),
                aria_core::RationalTime::new(25, rate),
            )))
        );
    }

    /// The class closed here is "a codec name nobody writes is silently
    /// replaced". Every shipped codec key parses, and anything else is refused
    /// with the list of what would work.
    #[test]
    fn a_codec_name_is_parsed_or_refused_with_the_alternatives() {
        for codec in VideoCodec::ALL {
            let args = Args::parse(["export", "a.aria", "b.mp4", "--codec", codec.key()]).unwrap();
            assert_eq!(codec_from(&args).unwrap(), Some(codec));
        }

        let args = Args::parse(["export", "a.aria", "b.mp4", "--codec", "prores"]).unwrap();
        let err = codec_from(&args).expect_err("prores was accepted");
        assert!(err.message().contains("h264"), "{}", err.message());
        assert_eq!(err.code(), 2);

        let args = Args::parse(["export", "a.aria", "b.mp4"]).unwrap();
        assert_eq!(codec_from(&args).unwrap(), None);
    }

    /// WHY: export progress went to stderr whether or not it was a terminal,
    /// so `aria export … 2> log` filled the log with a hundred carriage-return
    /// updates. Every combination of `--quiet` and a terminal is listed. Not
    /// caught: `run` passing something other than stderr's terminal state.
    #[test]
    fn progress_prints_only_to_a_terminal_and_never_with_quiet() {
        for (quiet, terminal, shown) in [
            (false, true, true),
            (false, false, false),
            (true, true, false),
            (true, false, false),
        ] {
            assert_eq!(
                shows_progress(quiet, terminal),
                shown,
                "--quiet {quiet}, stderr a terminal {terminal}"
            );
        }
    }

    /// WHY: `aria export --fps` took only whole numbers, so a timeline could
    /// not be exported at 23.976 or 29.97, rates `aria new` offers. Then it took
    /// every whole number up to `i32::MAX`, and above 1000 fps `nvh264enc` held
    /// the last frame for 1/29.97 s, so the file ran past the span. Every rate
    /// `aria new` offers parses to the same exact rate here, a whole number up
    /// to [`MAX_FPS`] parses to itself, and zero, a negative, a rounded
    /// broadcast rate, a word, one past the ceiling, and a number past `i32` are
    /// usage errors. Not caught: an encoder that rounds a rate at or below the
    /// ceiling.
    #[test]
    fn every_rate_new_offers_exports_exactly_and_a_bad_rate_is_a_usage_error() {
        let parse = |raw: &str| {
            rate_from(&Args::parse(["export", "a.aria", "b.mp4", "--fps", raw]).unwrap())
        };
        for (key, rate) in aria_core::NAMED_RATES {
            assert_eq!(parse(key), Ok(Some(rate)), "{key}");
        }
        for fps in [1, 12, MAX_FPS] {
            assert_eq!(
                parse(&fps.to_string()),
                Ok(Some(aria_core::Rational::new(fps, 1)))
            );
        }
        for (raw, says) in [
            ("0", "--fps wants a rate above zero"),
            ("-5", "--fps wants a rate above zero"),
            ("23.98", "one of 23.976, 29.97,"),
            ("fast", "whole number"),
            ("1001", "--fps wants a rate up to 1000, got 1001"),
            ("5000", "--fps wants a rate up to 1000, got 5000"),
            (
                "2147483648",
                "--fps wants a rate up to 1000, got 2147483648",
            ),
        ] {
            let err = parse(raw).expect_err(raw);
            assert!(err.message().contains(says), "{raw}: {}", err.message());
            assert_eq!(err.code(), 2, "{raw}");
        }
        let args = Args::parse(["export", "a.aria", "b.mp4"]).unwrap();
        assert_eq!(rate_from(&args), Ok(None));
    }

    /// WHY: `--fps 24.0` and `--fps 29.970` were usage errors ("wants a whole
    /// number"), though each names a rate the flag takes; a script printing a
    /// float writes those spellings. Every rate `aria new` offers and every
    /// whole number, spelled with zeros after its decimal point, parses as
    /// written without them, and a fraction that is not zero is still refused.
    /// Not caught: a spelling with a leading `+` or zero, which stays refused.
    #[test]
    fn a_rate_spelled_with_trailing_zeros_is_the_rate() {
        let parse = |raw: &str| {
            rate_from(&Args::parse(["export", "a.aria", "b.mp4", "--fps", raw]).unwrap())
        };
        let whole = ["1", "12", "1000"];
        let keys = aria_core::NAMED_RATES.iter().map(|(key, _)| *key);
        for key in keys.chain(whole) {
            let want = parse(key).expect(key);
            let point = if key.contains('.') { "" } else { "." };
            for zeros in ["0", "000"] {
                let raw = format!("{key}{point}{zeros}");
                assert_eq!(parse(&raw), Ok(want), "{raw}");
            }
            if !key.contains('.') {
                assert_eq!(parse(&format!("{key}.")), Ok(want), "{key}.");
            }
        }
        for (raw, says) in [
            ("24.50", "whole number"),
            ("0.0", "above zero"),
            ("1001.0", "up to 1000"),
        ] {
            let err = parse(raw).expect_err(raw);
            assert!(err.message().contains(says), "{raw}: {}", err.message());
        }
    }
}
