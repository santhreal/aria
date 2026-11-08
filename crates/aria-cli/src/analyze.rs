//! `aria analyze` — run Aria's detectors over a media file and print what they found.
//!
//! Same seam the editor uses (`aria-detect`), same detector defaults
//! (`aria-analysis`), so a run here and a run in the editor agree on the same
//! file. Nothing is written: the moments index belongs to a project, and this
//! command is for looking.

use std::path::Path;

use aria_analysis::{
    BeatConfig, LoudnessConfig, ShotBoundaryKind, ShotDetectionConfig, SilenceConfig,
};
use aria_core::count::counted;

use crate::args::Args;
use crate::fail::Failure;
use crate::project::clock;

pub(crate) const FLAGS: &[&str] = &["beats", "silence", "loudness", "shots", "limit"];

/// Rows one detector prints before the rest are summarized.
pub(crate) const DEFAULT_LIMIT: usize = 40;

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let path = args.required_path(0, "a media file")?;
    if let Some(failure) = Failure::not_media(&path) {
        return Err(failure);
    }
    let limit = args.number("limit")?.map_or(DEFAULT_LIMIT, |n| n as usize);
    let shots = args.present("shots");
    // With no detector flag named, all three audio detectors run: they share
    // one decode, so asking for every one costs what asking for one costs.
    // Naming `--shots` (or any audio flag) runs only the named detectors.
    let named =
        args.present("beats") || args.present("silence") || args.present("loudness") || shots;
    let beats = args.present("beats") || !named;
    let silence = args.present("silence") || !named;
    let loudness = args.present("loudness") || !named;

    let probed = aria_decoder::probe(&path)
        .map_err(|err| Failure::Failed(format!("{}: {err}", path.display())))?;
    let audio = beats || silence || loudness;
    if let Some(missing) = missing_track(&probed, shots, audio) {
        return Err(Failure::Failed(format!("{}: {missing}", path.display())));
    }
    if shots {
        run_shots(&path, limit)?;
    }
    if audio {
        run_audio(&path, limit, beats, silence, loudness)?;
    }
    Ok(())
}

/// The track a requested detector reads that `probed` lacks, stated before a
/// decode starts. Each detector's decode failed on its own with "source has
/// no audio track" or "source has no video track" behind two prefixes, and a
/// run with no detector named on a silent video did not mention `--shots`.
fn missing_track(
    probed: &aria_decoder::MediaProbe,
    shots: bool,
    audio: bool,
) -> Option<&'static str> {
    let video = probed.width > 0 && !probed.is_image;
    if shots && !video {
        return Some("the file has no video track for --shots to sample");
    }
    if audio && !probed.has_audio {
        return Some(if video {
            "the file has no audio track; --shots detects the shots in its video"
        } else {
            "the file has no audio track"
        });
    }
    None
}

fn run_shots(path: &Path, limit: usize) -> Result<(), Failure> {
    eprintln!(
        "sampling {} at {} fps",
        path.display(),
        aria_detect::SHOT_SAMPLE_FPS
    );
    let boundaries = aria_detect::detect_shots_sampled(
        path,
        aria_detect::SHOT_SAMPLE_FPS,
        aria_detect::MAX_SHOT_SECONDS,
        ShotDetectionConfig::default(),
        || false,
    )?;
    println!("shots     {}", boundaries.len());
    for boundary in boundaries.iter().take(limit) {
        println!(
            "{}",
            shot_row(
                boundary.timestamp_seconds(),
                boundary.score(),
                boundary.kind()
            )
        );
    }
    elided(boundaries.len(), limit);
    Ok(())
}

/// One shot boundary row: when, how far the change stands out, and what
/// changed, in words.
fn shot_row(seconds: f64, score: f32, kind: ShotBoundaryKind) -> String {
    let kind = match kind {
        ShotBoundaryKind::VisualChange => "cut",
        ShotBoundaryKind::DimensionChange => "size change",
    };
    format!("  {}  score {score:.2}  {kind}", clock(seconds))
}

fn run_audio(
    path: &Path,
    limit: usize,
    beats: bool,
    silence: bool,
    loudness: bool,
) -> Result<(), Failure> {
    eprintln!("decoding {} for audio analysis", path.display());
    let pcm = aria_detect::decode_mono(path, || false)?;
    let view = aria_detect::validated_pcm(&pcm)?;
    println!(
        "audio     {} at {} Hz mono",
        clock(view.duration_seconds()),
        aria_detect::ANALYSIS_SAMPLE_RATE
    );

    if beats {
        let events = view.detect_beats(BeatConfig::default());
        println!("beats     {}", events.len());
        for event in events.iter().take(limit) {
            println!(
                "  {}  {:+.1} dB",
                clock(event.time_seconds),
                event.strength_db
            );
        }
        elided(events.len(), limit);
    }
    if silence {
        let spans = view.detect_silence(SilenceConfig::default());
        println!("silence   {}", spans.len());
        for span in spans.iter().take(limit) {
            println!(
                "  {} → {}  ({})",
                clock(span.span.start_seconds()),
                clock(span.span.end_seconds()),
                clock(span.span.duration_seconds())
            );
        }
        elided(spans.len(), limit);
    }
    if loudness {
        let contour = view.loudness_contour(LoudnessConfig::default());
        println!("loudness  {}", counted(contour.len(), "window", "windows"));
        if contour.is_empty() {
            println!("  (the file is shorter than one loudness window)");
            return Ok(());
        }
        let mut quietest = f32::INFINITY;
        let mut loudest = f32::NEG_INFINITY;
        for point in &contour {
            quietest = quietest.min(point.dbfs);
            loudest = loudest.max(point.dbfs);
        }
        println!("  range {quietest:.1} to {loudest:.1} dBFS");
        for point in contour.iter().take(limit) {
            println!(
                "  {} → {}  {:.1} dBFS",
                clock(point.span.start_seconds()),
                clock(point.span.end_seconds()),
                point.dbfs
            );
        }
        elided(contour.len(), limit);
    }
    Ok(())
}

/// Say how many rows were held back, so a truncated list never reads as a short one.
fn elided(total: usize, limit: usize) {
    if total > limit {
        println!("  … {} more (raise --limit)", total - limit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: `aria analyze --shots` printed each boundary's kind as its Rust
    /// variant name, `VisualChange` or `DimensionChange`, where the manual
    /// calls a boundary a cut. Every kind prints in words. Not caught: a new
    /// kind, which fails to compile in `shot_row` instead.
    #[test]
    fn a_shot_boundary_prints_its_kind_in_words() {
        assert_eq!(
            shot_row(1.5, 0.8234, ShotBoundaryKind::VisualChange),
            "  0:00:01.500  score 0.82  cut"
        );
        assert_eq!(
            shot_row(62.0, 1.0, ShotBoundaryKind::DimensionChange),
            "  0:01:02.000  score 1.00  size change"
        );
    }

    /// Passing `--shots` alone on a video-only file must run only shot detection
    /// and succeed without attempting an audio decode on a stream with no audio.
    /// The fixture is H.264 with no audio stream.
    #[test]
    fn shots_flag_alone_succeeds_on_video_without_audio_stream() {
        let video = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-160x90-30fps-cuts.mp4");
        let raw = [
            "analyze".to_owned(),
            video.to_string_lossy().into_owned(),
            "--shots".to_owned(),
        ];
        let parsed = Args::parse(&raw).expect("valid args");
        run(&parsed).expect("--shots on video-only file must succeed without audio decode");
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    /// WHY: `aria analyze silent.mp4` failed with "audio decode failed:
    /// unsupported: source has no audio track" and no mention of `--shots`,
    /// and `--shots` on a sound file failed inside the shot sampler. Each
    /// detector's track is checked against the probe before a decode starts,
    /// for sound alone, picture alone, picture and sound, and a still.
    #[test]
    fn a_detector_whose_track_the_file_lacks_is_named_before_a_decode() {
        const NO_VIDEO: &str = "the file has no video track for --shots to sample";
        const NO_AUDIO: &str = "the file has no audio track";
        const NO_AUDIO_SHOTS: &str =
            "the file has no audio track; --shots detects the shots in its video";
        for (name, shots_only, audio_only, both) in [
            (
                "fixture-sine-48k-2s.wav",
                Some(NO_VIDEO),
                None,
                Some(NO_VIDEO),
            ),
            (
                "fixture-160x90-30fps-cuts.mp4",
                None,
                Some(NO_AUDIO_SHOTS),
                Some(NO_AUDIO_SHOTS),
            ),
            ("fixture-640x360-24fps-2s-av.mp4", None, None, None),
            (
                "texture.png",
                Some(NO_VIDEO),
                Some(NO_AUDIO),
                Some(NO_VIDEO),
            ),
        ] {
            let probed = aria_decoder::probe(&fixture(name)).expect("probe the fixture");
            for (shots, audio, want) in [
                (true, false, shots_only),
                (false, true, audio_only),
                (true, true, both),
            ] {
                assert_eq!(
                    missing_track(&probed, shots, audio),
                    want,
                    "{name} shots {shots} audio {audio}"
                );
            }
        }
        let silent = fixture("fixture-160x90-30fps-cuts.mp4");
        let parsed = Args::parse(["analyze".to_owned(), silent.to_string_lossy().into_owned()])
            .expect("valid args");
        let failure = run(&parsed).expect_err("a silent video ran the audio detectors");
        assert_eq!(
            failure.message(),
            format!("{}: {NO_AUDIO_SHOTS}", silent.display())
        );
    }

    /// WHY: `aria analyze <folder>` failed with "decode failed: GStreamer:
    /// ... is a directory. Pick another file." after the decoder opened the
    /// path, while `aria new` states a folder or a missing file before any
    /// decode. Both commands state the reason through [`Failure::not_media`],
    /// whichever detectors are named. Not caught: a path replaced between the
    /// check and the probe.
    #[test]
    fn a_media_path_that_is_not_a_file_fails_as_aria_new_states_it() {
        let dir = tempfile::tempdir().expect("scratch directory");
        let folder = dir.path().join("clips");
        std::fs::create_dir(&folder).expect("create folder");
        let cases = [
            (dir.path().join("nothere.wav"), "no such file"),
            (folder, "is a directory; name a media file"),
        ];
        for (bad, why) in cases {
            let want = Err(Failure::Failed(format!("{}: {why}", bad.display())));
            let flag_sets: [&[&str]; 3] = [&[], &["--shots"], &["--beats", "--shots"]];
            for flags in flag_sets {
                let mut raw = vec!["analyze".to_owned(), bad.to_string_lossy().into_owned()];
                raw.extend(flags.iter().map(|flag| (*flag).to_owned()));
                let parsed = Args::parse(&raw).expect("valid args");
                assert_eq!(run(&parsed), want, "{raw:?}");
            }
        }
    }
}
