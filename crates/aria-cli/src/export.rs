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

use aria_core::{ExportFormat, VideoCodec};
use aria_render::{ExportSettings, Renderer, export_to_file_observed};

use crate::args::Args;
use crate::fail::Failure;
use crate::project;

pub const FLAGS: &[&str] = &[
    "at", "to", "size", "fps", "codec", "bitrate", "no-audio", "quiet",
];

pub fn run(args: &Args) -> Result<(), Failure> {
    let source = args.required_path(0, "a project file")?;
    let out = args.required_path(1, "an output file")?;
    let at = args.seconds("at")?;
    let to = args.seconds("to")?;
    let size = args.size("size")?;
    let fps = args.number("fps")?;
    let bitrate = args.number("bitrate")?;
    let codec = codec_from(args)?;
    let quiet = args.present("quiet");

    if fps == Some(0) {
        return Err(Failure::Usage("--fps wants a rate above zero".into()));
    }

    let project = project::load(&source)?;
    let rate = project.timeline().frame_rate;
    let duration = project.timeline().duration();
    let range = match resolve_export_range(at, to, duration, rate) {
        Ok(range) => range,
        Err(err) => return Err(Failure::Usage(err)),
    };

    let format = ExportFormat {
        size,
        frame_rate: fps
            .map(|fps| aria_core::Rational::new(i32::try_from(fps).unwrap_or(i32::MAX), 1)),
        video_codec: codec,
        bitrate_kbps: bitrate,
        audio: args.present("no-audio").then_some(false),
        range,
        ..ExportFormat::PROJECT
    };
    let settings = ExportSettings::resolved(&project, format)
        .map_err(|err| Failure::Failed(err.to_string()))?;

    let mut renderer = Renderer::new_headless().map_err(|err| err.to_string())?;
    let mut last_percent = u64::MAX;
    let mut observer = |done: u64, total: u64| {
        if !quiet && total > 0 {
            let percent = done * 100 / total;
            if percent != last_percent {
                last_percent = percent;
                eprint!("\rencoding {percent}% ({done}/{total})");
            }
        }
        true
    };
    let frames =
        export_to_file_observed(&mut renderer, &project, &out, settings, &mut observer as _)
            .map_err(|err| err.to_string())?;
    if !quiet {
        eprintln!();
    }
    println!(
        "wrote {frames} frames of {}x{} {} to {}",
        settings.size.0,
        settings.size.1,
        aria_encoder::capability().label_for(settings.video_codec),
        out.display()
    );
    Ok(())
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

pub(crate) fn resolve_export_range(
    at: Option<f64>,
    to: Option<f64>,
    duration: aria_core::RationalTime,
    rate: aria_core::Rational,
) -> Result<Option<(aria_core::RationalTime, aria_core::RationalTime)>, String> {
    if at.is_none() && to.is_none() {
        return Ok(None);
    }
    if let Some(at_val) = at {
        if at_val < 0.0 {
            return Err("--at cannot be negative".into());
        }
    }
    if let Some(to_val) = to {
        if to_val < 0.0 {
            return Err("--to cannot be negative".into());
        }
    }
    let from = project::time_for_seconds(at.unwrap_or(0.0), rate);
    let end = match to {
        Some(to) => project::time_for_seconds(to, rate),
        None => duration,
    };
    if from.value >= duration.value {
        return Err(format!(
            "--at ({}) must be before project duration ({})",
            project::clock(from.seconds()),
            project::clock(duration.seconds())
        ));
    }
    if end.value <= from.value {
        return Err(format!(
            "--to ({}) must be after --at ({})",
            project::clock(end.seconds()),
            project::clock(from.seconds())
        ));
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

        // Valid range
        let res = resolve_export_range(Some(1.0), Some(5.0), duration, rate).unwrap();
        assert!(res.is_some());

        // at >= duration
        let err = resolve_export_range(Some(10.0), None, duration, rate).unwrap_err();
        assert!(err.contains("must be before project duration"), "{err}");

        // at >= to
        let err = resolve_export_range(Some(6.0), Some(4.0), duration, rate).unwrap_err();
        assert!(err.contains("must be after --at"), "{err}");

        // negative at
        let err = resolve_export_range(Some(-1.0), Some(4.0), duration, rate).unwrap_err();
        assert!(err.contains("cannot be negative"), "{err}");
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
}
