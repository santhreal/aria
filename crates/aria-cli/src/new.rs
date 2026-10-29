//! `aria new` — build a project file from a list of media, end to end.
//!
//! The assembly a rough cut starts from: import each file, lay it after the
//! previous one, save. It runs through the engine rather than writing JSON, so a
//! project written here obeys every invariant the editor enforces — lane kinds,
//! no overlaps, a probed pool — and opens in the editor with no repair.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_core::count::counted;
use aria_core::{Rational, RationalTime, TimeRange, resample};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{TrackId, TrackKind};

use crate::args::Args;
use crate::fail::Failure;
use crate::project::clock;

pub(crate) const FLAGS: &[&str] = &["fps"];

/// Frame rates a `--fps` value may name, in the order a picker would offer them.
///
/// A timeline rate is not a free number: 23.976 and 29.97 are `24000/1001` and
/// `30000/1001`, and a project written at a rounded `24/1` drifts against the
/// footage. A caller names a rate that exists or is told which ones do.
pub(crate) const RATES: [(&str, Rational); 7] = [
    ("23.976", Rational::FPS_23_976),
    ("24", Rational::FPS_24),
    ("25", Rational::FPS_25),
    ("29.97", Rational::FPS_29_97),
    ("30", Rational::FPS_30),
    ("50", Rational::new(50, 1)),
    ("60", Rational::new(60, 1)),
];

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let out = args.required_path(0, "an output project file")?;
    let media: Vec<PathBuf> = args.positional[1..].iter().map(PathBuf::from).collect();
    let rate = rate_from(args)?;

    let mut engine = Engine::new(EngineConfig::default()).map_err(|err| err.to_string())?;
    // Clips are placed in ticks of `rate`, so the timeline counts in it first.
    if engine.project().timeline().frame_rate != rate {
        engine
            .apply(Command::Edit(EditCommand::SetSequenceRate {
                frame_rate: rate,
            }))
            .map_err(|err| err.to_string())?;
    }
    engine
        .apply(Command::Edit(EditCommand::AddTrack {
            kind: TrackKind::Video,
            name: "V1".into(),
            index: None,
            pinned: true,
        }))
        .map_err(|err| err.to_string())?;

    let mut video_cursor = 0_i64;
    let mut audio_cursor = 0_i64;
    for path in &media {
        let path = path
            .canonicalize()
            .map_err(|err| format!("{}: {err}", path.display()))?;
        let ApplyOutcome::Imported { media } = engine
            .apply(Command::Project(ProjectCommand::Import {
                path: path.clone(),
            }))
            .map_err(|err| err.to_string())?
        else {
            return Err(Failure::Failed(format!(
                "{}: the engine did not report an import",
                path.display()
            )));
        };
        let entry = engine
            .project()
            .media(media)
            .ok_or_else(|| format!("{}: imported but absent from the pool", path.display()))?;
        let source = TimeRange::at_rate(0, entry.duration.value, entry.frame_rate);
        let length = resample(entry.duration, rate).value.max(1);
        // A file with no picture is audio: it belongs on an audio lane, and
        // putting it on V1 would place a clip that composites nothing.
        let kind = if entry.width == 0 || entry.height == 0 {
            TrackKind::Audio
        } else {
            TrackKind::Video
        };
        // Which cursor the clip advances is the lane's own `is_audio` column,
        // not a list of kinds kept here.
        let cursor = if kind.is_audio() {
            &mut audio_cursor
        } else {
            &mut video_cursor
        };
        let start = RationalTime::new(*cursor, rate);
        let track = lane_for(&mut engine, kind)?;
        engine
            .apply(Command::Edit(EditCommand::AddClip {
                track,
                media,
                source,
                start,
            }))
            .map_err(|err| err.to_string())?;
        *cursor += length;
        println!(
            "placed {} at {} on {:?}",
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string()),
            clock(start.seconds()),
            kind
        );
    }

    engine
        .apply(Command::Project(ProjectCommand::Save { path: out.clone() }))
        .map_err(|err| err.to_string())?;
    let duration = engine.project().timeline().duration();
    println!(
        "wrote {} — {} at {}/{}, {}",
        out.display(),
        clock(duration.seconds()),
        rate.num,
        rate.den,
        counted(media.len(), "clip", "clips")
    );
    Ok(())
}

/// The lowest lane of `kind`, creating one when the project has none.
fn lane_for(engine: &mut Engine, kind: TrackKind) -> Result<TrackId, Failure> {
    if let Some(track) = engine
        .project()
        .timeline()
        .tracks_ordered()
        .find(|track| track.kind == kind)
    {
        return Ok(track.id);
    }
    let name = kind.default_name();
    engine
        .apply(Command::Edit(EditCommand::AddTrack {
            kind,
            name: name.into(),
            index: None,
            pinned: true,
        }))
        .map_err(|err| err.to_string())?;
    engine
        .project()
        .timeline()
        .tracks_ordered()
        .find(|track| track.kind == kind)
        .map(|track| track.id)
        .ok_or_else(|| Failure::Failed(format!("the engine did not add a {kind:?} lane")))
}

/// The `--fps` flag as an exact [`Rational`].
fn rate_from(args: &Args) -> Result<Rational, Failure> {
    let Some(raw) = args.value("fps")? else {
        return Ok(Rational::FPS_24);
    };
    RATES
        .iter()
        .find(|(key, _)| *key == raw)
        .map(|(_, rate)| *rate)
        .ok_or_else(|| {
            Failure::Usage(format!(
                "--fps wants one of {}, got \"{raw}\"",
                RATES
                    .iter()
                    .map(|(key, _)| *key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class closed here is "a rounded rate silently replaces a broadcast
    /// one", which drifts a whole project against its footage. Every offered
    /// rate is exact, and a rate nobody offers is refused with the list.
    #[test]
    fn a_frame_rate_is_exact_or_refused_with_the_alternatives() {
        let args = Args::parse(["new", "out.aria", "--fps", "23.976"]).unwrap();
        assert_eq!(rate_from(&args).unwrap(), Rational::FPS_23_976);

        let args = Args::parse(["new", "out.aria", "--fps", "29.97"]).unwrap();
        assert_eq!(rate_from(&args).unwrap(), Rational::FPS_29_97);

        let args = Args::parse(["new", "out.aria"]).unwrap();
        assert_eq!(rate_from(&args).unwrap(), Rational::FPS_24);

        let args = Args::parse(["new", "out.aria", "--fps", "23.98"]).unwrap();
        let err = rate_from(&args).expect_err("23.98 was accepted");
        assert!(err.message().contains("23.976"), "{}", err.message());
        assert_eq!(err.code(), 2);
    }

    /// Every offered rate is a valid, reduced rational — a typo in the table
    /// would otherwise ship as a project rate nothing can play.
    #[test]
    fn every_offered_rate_is_valid() {
        for (key, rate) in RATES {
            assert!(rate.is_valid(), "{key} is not a valid rate");
            assert!(rate.as_f64() > 0.0, "{key} is not positive");
        }
    }

    /// WHY: `aria new out.aria clip.mp4 --fps 60` failed with "rate mismatch:
    /// expected 24/1, got 60/1": the timeline stayed at the default rate while
    /// the clips were placed in ticks of the requested one. The written project
    /// counts in the requested rate and its clip spans the file at that rate.
    #[test]
    fn a_project_with_media_is_written_at_the_requested_rate() {
        let media = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4");
        let dir = std::env::temp_dir().join("aria-cli-new-rate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        let out = dir.join("rate.aria");
        let args = Args::parse([
            "new",
            out.to_str().expect("utf-8 path"),
            media.to_str().expect("utf-8 path"),
            "--fps",
            "60",
        ])
        .expect("arguments parse");
        let result = run(&args);
        let project = result.map(|()| aria_models::Project::load_from_file(&out));
        let _ = std::fs::remove_dir_all(&dir);
        let project = project
            .unwrap_or_else(|err| panic!("aria new failed: {}", err.message()))
            .expect("the written project loads");
        let timeline = project.timeline();
        assert_eq!(timeline.frame_rate, Rational::new(60, 1));
        let clip = timeline
            .tracks_ordered()
            .flat_map(|track| track.clips_ordered())
            .next()
            .expect("the clip was placed");
        assert_eq!(clip.timeline.start.value, 0);
        assert_eq!(
            clip.timeline.duration,
            RationalTime::new(180, Rational::new(60, 1))
        );
    }
}
