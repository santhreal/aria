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
use aria_core::{NAMED_RATES, Rational, RationalTime, resample};
use aria_engine::{ApplyOutcome, Engine, EngineConfig, Probed};
use aria_models::{TrackId, TrackKind};

use crate::args::Args;
use crate::fail::Failure;
use crate::project::clock;

pub(crate) const FLAGS: &[&str] = &["fps"];

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let out = args.required_path(0, "an output project file")?;
    // Saving replaces the output, so `aria new clip.mp4 other.mp4`, the output
    // left out, replaced the footage with a project.
    if std::fs::symlink_metadata(&out).is_ok() {
        return Err(Failure::Usage(format!(
            "{} already exists; aria new writes a new project file",
            out.display()
        )));
    }
    let media: Vec<PathBuf> = args.positional[1..].iter().map(PathBuf::from).collect();
    // Every path is checked before the first import, so a misspelled or
    // folder path late in the list fails before the files ahead of it are
    // probed. `analyze` states the same reasons through the same check.
    if let Some(failure) = media.iter().find_map(|path| Failure::not_media(path)) {
        return Err(failure);
    }
    let rate = rate_from(args)?;
    let paths = media
        .iter()
        .map(|path| path.canonicalize().map_err(|err| Failure::io(path, &err)))
        .collect::<Result<Vec<_>, _>>()?;
    let imports: Vec<Command> = paths
        .iter()
        .map(|path| Command::Project(ProjectCommand::Import { path: path.clone() }))
        .collect();
    // Every file is probed before the first is placed, several at a time, so
    // a long list waits for its slowest probe rather than for the sum.
    let probes = Probed::ahead_all(&imports, |_| false);

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
    // Printed once the project is saved: a placement the save then loses is
    // not one.
    let mut placed = Vec::with_capacity(paths.len());
    for ((path, import), probed) in paths.iter().zip(imports).zip(probes) {
        let ApplyOutcome::Imported { media } = engine
            .apply_probed(import, probed)
            .map_err(|err| format!("{}: {err}", path.display()))?
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
            .map_err(|err| format!("{}: {err}", path.display()))?;
        if channels.0 {
            video_cursor = at + length;
        }
        if channels.1 {
            audio_cursor = at + length;
        }
        placed.push(format!(
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
        ));
    }

    engine
        .apply(Command::Project(ProjectCommand::Save { path: out.clone() }))
        .map_err(|err| format!("{}: {err}", out.display()))?;
    for line in &placed {
        println!("{line}");
    }
    let timeline = engine.project().timeline();
    let duration = timeline.duration();
    // A file listed twice is one pool entry placed twice, and a file with
    // picture and sound places two clips, so the clips and the files are
    // counted apart.
    println!(
        "wrote {}: {} at {} fps, {} from {}",
        out.display(),
        clock(duration.seconds()),
        rate.fps_text(),
        counted(timeline.clip_count(), "clip", "clips"),
        counted(engine.project().media_count(), "file", "files")
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
    Rational::named(raw).ok_or_else(|| {
        Failure::Usage(format!(
            "--fps wants one of {}, got \"{raw}\"",
            NAMED_RATES
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
    /// rate is exact, and a rate nobody offers is refused with the list. A
    /// spelling with zeros after the decimal point, such as `29.970`, was
    /// refused as well, though it is the same number.
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

        // A rate spelled with zeros after its decimal point is that rate.
        for (key, rate) in NAMED_RATES {
            let point = if key.contains('.') { "" } else { "." };
            for raw in [format!("{key}{point}0"), format!("{key}{point}00")] {
                let args = Args::parse(["new", "out.aria", "--fps", raw.as_str()]).unwrap();
                assert_eq!(rate_from(&args).ok(), Some(rate), "{raw}");
            }
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
            fixture("fixture-640x360-24fps-2s-av.mp4"),
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

    /// WHY: `aria new`'s usage said each file lands "after the previous one",
    /// but a sound file after a still with no sound starts at `0` under the
    /// still: a file starts at the latest clip end on the lanes it lands on,
    /// and lanes it has no channel for do not hold it back. Each order of a
    /// picture-and-sound shot, a still and a tone is assembled and its lane
    /// spans compared. Not caught: a second video lane, which `aria new` never
    /// creates.
    #[test]
    fn each_file_starts_at_the_latest_clip_end_on_its_own_lanes() {
        let fixture = |name: &str| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures")
                .join(name)
                .to_str()
                .expect("utf-8 path")
                .to_owned()
        };
        let (shot, still, tone) = (
            fixture("fixture-640x360-24fps-2s-av.mp4"),
            fixture("texture.png"),
            fixture("fixture-sine-48k-2s.wav"),
        );
        let (shot, still, tone) = (shot.as_str(), still.as_str(), tone.as_str());
        type Spans = Vec<(i64, i64)>;
        let cases: [(Vec<&str>, Spans, Spans); 3] = [
            (vec![still, tone], vec![(0, 48)], vec![(0, 120)]),
            (vec![tone, still], vec![(0, 48)], vec![(0, 120)]),
            (
                vec![still, shot, tone],
                vec![(120, 168), (168, 216)],
                vec![(0, 120), (120, 168)],
            ),
        ];
        for (files, audio, video) in cases {
            let dir = tempfile::tempdir().expect("scratch directory");
            let out = dir.path().join("order.aria");
            let mut argv = vec!["new", out.to_str().expect("utf-8 path")];
            argv.extend(&files);
            if let Err(err) = run(&Args::parse(argv).expect("arguments parse")) {
                panic!("aria new {files:?} failed: {}", err.message());
            }
            let project = aria_models::Project::load_from_file(&out).expect("the project loads");
            let spans = |name: &str| -> Spans {
                project
                    .timeline()
                    .tracks_ordered()
                    .filter(|track| track.name == name)
                    .flat_map(|track| track.clips_ordered())
                    .map(|clip| (clip.timeline.start.value, clip.timeline.end_tick()))
                    .collect()
            };
            assert_eq!((spans("A1"), spans("V1")), (audio, video), "{files:?}");
        }
    }

    /// WHY: `aria new a.mp4 b.mp4`, with the output left out, saved a project
    /// over `a.mp4`, and `aria new p.aria` replaced an existing project; both
    /// exited 0. An output that exists, as a file, a folder or a link, is a
    /// usage error and keeps its bytes. Not caught: a file created between the
    /// check and the save.
    #[test]
    fn an_output_that_exists_is_a_usage_error_and_keeps_its_bytes() {
        let dir = tempfile::tempdir().expect("scratch directory");
        let file = dir.path().join("a.mp4");
        std::fs::write(&file, b"the footage").expect("write file");
        let folder = dir.path().join("folder.aria");
        std::fs::create_dir(&folder).expect("create folder");
        let mut outputs = vec![file.clone(), folder];
        #[cfg(unix)]
        {
            let dangling = dir.path().join("dangling.aria");
            std::os::unix::fs::symlink(dir.path().join("missing"), &dangling).expect("symlink");
            outputs.push(dangling);
        }
        for out in outputs {
            let args = Args::parse(["new", out.to_str().expect("utf-8 path")]).expect("parse");
            assert_eq!(
                run(&args),
                Err(Failure::Usage(format!(
                    "{} already exists; aria new writes a new project file",
                    out.display()
                ))),
                "{}",
                out.display()
            );
        }
        assert_eq!(std::fs::read(&file).expect("read file"), b"the footage");
        assert!(
            !dir.path().join("missing").exists(),
            "the link was followed"
        );
    }

    /// WHY: a file among several that failed to import printed the decoder's
    /// reason alone, "decode failed: GStreamer: Could not determine type of
    /// stream", with no word of which file it was. The failure names the file,
    /// wherever in the list it is, and no project is written.
    #[test]
    fn a_file_that_does_not_import_is_named_in_the_failure() {
        let shot = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4");
        let dir = tempfile::tempdir().expect("scratch directory");
        let junk = dir.path().join("junk.mp4");
        std::fs::write(&junk, b"not a video").expect("write the junk file");
        let out = dir.path().join("cut.aria");
        let args = Args::parse([
            "new",
            out.to_str().expect("utf-8 path"),
            shot.to_str().expect("utf-8 path"),
            junk.to_str().expect("utf-8 path"),
        ])
        .expect("parse");
        let err = run(&args).expect_err("a file that is not media imported");
        let named = format!("{}: ", junk.canonicalize().expect("junk exists").display());
        assert!(
            err.message().starts_with(&named),
            "the failure does not name {}: {}",
            junk.display(),
            err.message()
        );
        assert_eq!(err.code(), 1);
        assert!(!out.exists(), "a project was written");
    }

    /// WHY: a media file that does not exist failed with "nothere.mp4: No such
    /// file or directory (os error 2)", the OS error and its code, and a folder
    /// failed as "decode failed: GStreamer: ... is a directory", both only after
    /// every file listed before them was imported. Each path is checked first:
    /// the failure states the path and why it is not media, whichever place in
    /// the list it has, and no project is written. Not caught: a file removed
    /// or replaced between the check and its import.
    #[test]
    fn a_media_path_that_is_not_a_file_is_named_before_any_import() {
        let shot = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4");
        let dir = tempfile::tempdir().expect("scratch directory");
        let folder = dir.path().join("clips");
        std::fs::create_dir(&folder).expect("create folder");
        let out = dir.path().join("cut.aria");
        let (out_arg, shot_arg) = (
            out.to_str().expect("utf-8 path"),
            shot.to_str().expect("utf-8 path"),
        );
        let cases = [
            (dir.path().join("nothere.mp4"), "no such file"),
            (folder, "is a directory; name a media file"),
        ];
        for (bad, why) in &cases {
            let bad_arg = bad.to_str().expect("utf-8 path");
            for media in [[shot_arg, bad_arg], [bad_arg, shot_arg]] {
                let args = Args::parse(["new", out_arg, media[0], media[1]]).expect("parse");
                assert_eq!(
                    run(&args),
                    Err(Failure::Failed(format!("{}: {why}", bad.display()))),
                    "{media:?}"
                );
                assert!(!out.exists(), "a project was written for {media:?}");
            }
        }
    }

    /// A decoder whose probes each wait, up to [`TOGETHER`], until `files`
    /// probes have started.
    struct Gathering {
        files: usize,
        /// Probes started, and how many of them saw every file probing at once.
        state: std::sync::Mutex<(usize, usize)>,
        started: std::sync::Condvar,
    }

    /// Long enough that only probes run one after another miss it.
    const TOGETHER: std::time::Duration = std::time::Duration::from_secs(5);

    impl aria_decoder::ExternalDecoder for Gathering {
        fn probe(
            &self,
            _path: &std::path::Path,
        ) -> Result<aria_decoder::MediaProbe, aria_core::DecodeError> {
            let mut state = self.state.lock().expect("the probe count");
            state.0 += 1;
            self.started.notify_all();
            let deadline = std::time::Instant::now() + TOGETHER;
            while state.0 < self.files {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                state = self
                    .started
                    .wait_timeout(state, left)
                    .expect("the probe count")
                    .0;
            }
            if state.0 >= self.files {
                state.1 += 1;
            }
            Ok(aria_decoder::MediaProbe {
                width: 640,
                height: 360,
                frame_rate: Rational::FPS_24,
                frame_count: 48,
                has_audio: false,
                is_image: false,
                video_codec: None,
                audio_codec: None,
            })
        }

        fn open_video(
            &self,
            _path: &std::path::Path,
        ) -> Result<Box<dyn aria_core::VideoDecoder>, aria_core::DecodeError> {
            Err(aria_core::DecodeError::unsupported(
                "this decoder only probes",
            ))
        }
    }

    /// WHY: `aria new` probed each file as it placed it, so a list of files on
    /// a network drive waited for the sum of their probes. The decoder here
    /// holds each probe until every file is being probed, so the probes are
    /// observed running together, not timed, and each file is probed once:
    /// the placement adopts the probe taken ahead. Not caught: a file listed
    /// twice, which is probed once per listing.
    #[test]
    fn aria_new_probes_its_files_together_and_each_once() {
        const FILES: usize = 3;
        let decoder = std::sync::Arc::new(Gathering {
            files: FILES,
            state: std::sync::Mutex::new((0, 0)),
            started: std::sync::Condvar::new(),
        });
        let _registered = aria_decoder::register_external_decoder(
            "newgather",
            &["newgather".to_owned()],
            100,
            std::sync::Arc::clone(&decoder) as std::sync::Arc<dyn aria_decoder::ExternalDecoder>,
        );
        let dir = tempfile::tempdir().expect("scratch directory");
        let takes: Vec<String> = (0..FILES)
            .map(|index| {
                let path = dir.path().join(format!("take-{index}.newgather"));
                std::fs::write(&path, b"take").expect("write the take");
                path.to_str().expect("utf-8 path").to_owned()
            })
            .collect();
        let out = dir.path().join("together.aria");
        let mut words = vec![
            "new".to_owned(),
            out.to_str().expect("utf-8 path").to_owned(),
        ];
        words.extend(takes);
        let args = Args::parse(words).expect("arguments parse");

        run(&args).unwrap_or_else(|err| panic!("aria new failed: {}", err.message()));

        let (probes, together) = *decoder.state.lock().expect("the probe count");
        assert_eq!(probes, FILES, "one probe per file");
        assert_eq!(
            together, FILES,
            "every probe runs while the others are running"
        );
    }
}
