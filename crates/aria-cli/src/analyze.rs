//! `aria analyze` — run Aria's detectors over a media file and print what they found.
//!
//! Same seam the editor uses (`aria-detect`), same detector defaults
//! (`aria-analysis`), so a run here and a run in the editor agree on the same
//! file. Nothing is written: the moments index belongs to a project, and this
//! command is for looking.

use std::path::Path;

use aria_analysis::{BeatConfig, LoudnessConfig, ShotDetectionConfig, SilenceConfig};

use crate::args::Args;
use crate::fail::Failure;
use crate::project::clock;

pub(crate) const FLAGS: &[&str] = &["beats", "silence", "loudness", "shots", "limit"];

/// Rows one detector prints before the rest are summarized.
pub(crate) const DEFAULT_LIMIT: usize = 40;

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let path = args.required_path(0, "a media file")?;
    if !path.exists() {
        return Err(Failure::Failed(format!("{}: no such file", path.display())));
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

    if shots {
        run_shots(&path, limit)?;
    }
    if beats || silence || loudness {
        run_audio(&path, limit, beats, silence, loudness)?;
    }
    Ok(())
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
            "  {}  score {:.2}  {:?}",
            clock(boundary.timestamp_seconds()),
            boundary.score(),
            boundary.kind()
        );
    }
    elided(boundaries.len(), limit);
    Ok(())
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
        println!("loudness  {} windows", contour.len());
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
}
