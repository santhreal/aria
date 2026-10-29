//! `aria new` — build a project file from a list of media, end to end.
//!
//! The assembly a rough cut starts from: import each file, lay it after the
//! previous one, save. Each file is one `Source edit`, so its picture lands on
//! `V1` and its sound on `A1`, linked, as a drop in the editor places them. It
//! runs through the engine rather than writing JSON, so a project written here
//! obeys every invariant the editor enforces — lane kinds, no overlaps, a
//! probed pool — and opens in the editor with no repair.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, ProjectCommand, SoundPlacement, SourceChannel};
use aria_core::count::counted;
use aria_core::{Rational, RationalTime, resample};
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
        let source = entry.full_range();
        let length = resample(entry.duration, rate).value.max(1);
        let channels = (
            entry.has_channel(SourceChannel::Video),
            entry.has_channel(SourceChannel::Audio),
        );
        let track = if channels.0 {
            Some(lane_for(&mut engine, TrackKind::Video)?)
        } else {
            None
        };
        let sound = if channels.1 {
            SoundPlacement::Lane(lane_for(&mut engine, TrackKind::Audio)?)
        } else {
            SoundPlacement::Off
        };
        // The file starts where every lane it lands on is free.
        let at = match channels {
            (true, true) => video_cursor.max(audio_cursor),
            (true, false) => video_cursor,
            (false, true) => audio_cursor,
            (false, false) => {
                return Err(Failure::Failed(format!(
                    "{}: the file has no picture and no sound",
                    path.display()
                )));
            }
        };
        let start = RationalTime::new(at, rate);
        engine
            .apply(Command::Edit(EditCommand::SourceEdit {
                media,
                source,
                at: start,
                fill: None,
                ripple: false,
                track,
                sound,
            }))
            .map_err(|err| err.to_string())?;
        if channels.0 {
            video_cursor = at + length;
        }
        if channels.1 {
            audio_cursor = at + length;
        }
        println!(
            "placed {} at {} on {}",
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string()),
            clock(start.seconds()),
            match channels {
                (true, true) => "V1 and A1",
                (true, false) => "V1",
                _ => "A1",
            }
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
        counted(media.len(), "file", "files")
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

    /// WHY: `aria new` placed a file with sound as one clip on `V1` with the
    /// sound inside it, where a drop in the editor lands the sound on `A1`
    /// linked to the picture, so the same files assembled two ways read as two
    /// different cuts. Each file now lands as one `Source edit` on every lane
    /// it has a channel for, starting where all of them are free.
    #[test]
    fn a_file_with_sound_lands_as_linked_picture_and_sound() {
        let fixture = |name: &str| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures")
                .join(name)
        };
        let (shot, tone) = (
            fixture("fixture-640x360-24fps-3s-av.mp4"),
            fixture("fixture-sine-48k-2s.wav"),
        );
        let dir = tempfile::tempdir().expect("scratch directory");
        let out = dir.path().join("linked.aria");
        let args = Args::parse([
            "new",
            out.to_str().expect("utf-8 path"),
            shot.to_str().expect("utf-8 path"),
            tone.to_str().expect("utf-8 path"),
        ])
        .expect("arguments parse");
        if let Err(err) = run(&args) {
            panic!("aria new failed: {}", err.message());
        }
        let project = aria_models::Project::load_from_file(&out).expect("the project loads");
        let lanes: Vec<(String, Vec<(i64, i64)>)> = project
            .timeline()
            .tracks_ordered()
            .map(|track| {
                let spans = track
                    .clips_ordered()
                    .map(|clip| {
                        let start = clip.timeline.start.value;
                        (start, start + clip.timeline.duration.value)
                    })
                    .collect();
                (track.name.clone(), spans)
            })
            .collect();
        // `tracks_ordered` runs bottom to top: the audio lane, then the video.
        assert_eq!(
            lanes,
            vec![
                ("A1".to_string(), vec![(0, 48), (48, 96)]),
                ("V1".to_string(), vec![(0, 48)]),
            ],
            "the shot's picture and sound share 0..48 and the tone follows on A1"
        );
        let links: Vec<_> = project
            .timeline()
            .tracks_ordered()
            .filter_map(|track| track.clips_ordered().next())
            .map(|clip| clip.link)
            .collect();
        assert!(links[0].is_some(), "the shot's sound is linked");
        assert_eq!(links[0], links[1], "the picture and its sound share a link");
    }
}
