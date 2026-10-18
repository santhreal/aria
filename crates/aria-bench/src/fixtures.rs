//! Local fixture generation for media and project benchmark workloads.
//!
//! Every fixture named in `docs/spec/performance.md` is synthesized locally in
//! `bench/fixtures/`; nothing is fetched over the network.

use std::path::{Path, PathBuf};
use std::time::Instant;

use aria_models::{
    CanvasAspect, CanvasSettings, ClipParam, ClipTransform, ColorAdjustments, Easing,
    EffectInstance, Generator, Marker, MarkerColor, MediaSource, PROJECT_SCHEMA_VERSION, Param,
    ParamValue, Project, Rational, RationalTime, Scale2, TextStyle, TimeRange, TrackKind,
};
use serde::{Deserialize, Serialize};

/// Canonical fixture names defined by `docs/spec/performance.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixtureId {
    H264_1080p60,
    Hevc2160p60,
    ProRes2160p30,
    LongGopScrub,
    Project10k,
    Stack4k,
}

impl FixtureId {
    pub const ALL: [Self; 6] = [
        Self::H264_1080p60,
        Self::Hevc2160p60,
        Self::ProRes2160p30,
        Self::LongGopScrub,
        Self::Project10k,
        Self::Stack4k,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::H264_1080p60 => "h264-1080p60",
            Self::Hevc2160p60 => "hevc-2160p60",
            Self::ProRes2160p30 => "prores-2160p30",
            Self::LongGopScrub => "long-gop-scrub",
            Self::Project10k => "project-10k",
            Self::Stack4k => "stack-4k",
        }
    }

    pub const fn as_str(self) -> &'static str {
        self.name()
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim() {
            "h264-1080p60" => Some(Self::H264_1080p60),
            "hevc-2160p60" => Some(Self::Hevc2160p60),
            "prores-2160p30" => Some(Self::ProRes2160p30),
            "long-gop-scrub" | "long-gop-10m" => Some(Self::LongGopScrub),
            "project-10k" => Some(Self::Project10k),
            "stack-4k" => Some(Self::Stack4k),
            _ => None,
        }
    }

    pub const fn file_name(self) -> &'static str {
        match self {
            Self::H264_1080p60 => "h264-1080p60.mp4",
            Self::Hevc2160p60 => "hevc-2160p60.mp4",
            Self::ProRes2160p30 => "prores-2160p30.mov",
            Self::LongGopScrub => "long-gop-scrub.mp4",
            Self::Project10k => "project-10k.aria",
            Self::Stack4k => "stack-4k.aria",
        }
    }

    pub fn path(self, fixtures_dir: &Path) -> PathBuf {
        fixtures_dir.join(self.file_name())
    }
}

/// Summary of one generated or verified fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureSummary {
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub cached: bool,
    pub generation_ms: f64,
}

/// Ensure all 6 benchmark fixtures exist in `fixtures_dir`, generating any
/// missing or incomplete fixture locally.
pub fn ensure_all_fixtures(fixtures_dir: &Path) -> Result<Vec<FixtureSummary>, String> {
    std::fs::create_dir_all(fixtures_dir).map_err(|err| {
        format!(
            "failed to create fixtures directory {}: {err}",
            fixtures_dir.display()
        )
    })?;

    let mut out = Vec::with_capacity(FixtureId::ALL.len());
    for id in FixtureId::ALL {
        out.push(ensure_fixture(fixtures_dir, id)?);
    }
    Ok(out)
}

/// Ensure a single fixture exists in `fixtures_dir`.
pub fn ensure_fixture(fixtures_dir: &Path, id: FixtureId) -> Result<FixtureSummary, String> {
    std::fs::create_dir_all(fixtures_dir).map_err(|err| {
        format!(
            "failed to create fixtures directory {}: {err}",
            fixtures_dir.display()
        )
    })?;
    let path = id.path(fixtures_dir);
    if is_valid_fixture(fixtures_dir, id, &path) {
        if id == FixtureId::LongGopScrub {
            ensure_long_gop_alias(fixtures_dir, &path)?;
        }
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        return Ok(FixtureSummary {
            name: id.name().to_string(),
            path,
            bytes,
            cached: true,
            generation_ms: 0.0,
        });
    }

    let started = Instant::now();
    match id {
        FixtureId::H264_1080p60 => generate_h264_1080p60(fixtures_dir, &path)?,
        FixtureId::Hevc2160p60 => generate_hevc_2160p60(fixtures_dir, &path)?,
        FixtureId::ProRes2160p30 => generate_prores_2160p30(fixtures_dir, &path)?,
        FixtureId::LongGopScrub => {
            // A stale alias is a hard link to the old file; drop it so the
            // alias names the new one.
            let _ = std::fs::remove_file(fixtures_dir.join("long-gop-10m.mp4"));
            generate_long_gop_scrub(fixtures_dir, &path)?;
            ensure_long_gop_alias(fixtures_dir, &path)?;
        }
        FixtureId::Project10k => {
            ensure_fixture(fixtures_dir, FixtureId::H264_1080p60)?;
            let project = build_project_10k(fixtures_dir)?;
            project
                .save_to_file(&path)
                .map_err(|err| format!("failed to save {}: {err}", path.display()))?;
        }
        FixtureId::Stack4k => {
            ensure_fixture(fixtures_dir, FixtureId::Hevc2160p60)?;
            let project = build_stack_4k(fixtures_dir)?;
            project
                .save_to_file(&path)
                .map_err(|err| format!("failed to save {}: {err}", path.display()))?;
        }
    }
    if is_media(id) {
        std::fs::write(version_path(&path), MEDIA_FIXTURE_VERSION.to_string())
            .map_err(|err| format!("failed to record the version of {}: {err}", path.display()))?;
    }
    let generation_ms = started.elapsed().as_secs_f64() * 1000.0;
    let bytes = std::fs::metadata(&path)
        .map_err(|err| {
            format!(
                "generated fixture {} missing metadata: {err}",
                path.display()
            )
        })?
        .len();

    Ok(FixtureSummary {
        name: id.name().to_string(),
        path,
        bytes,
        cached: false,
        generation_ms,
    })
}

/// Version of the media fixture recipe, stored beside each media fixture in
/// `<file>.version`. A media fixture with another version, or none, is
/// generated again.
///
/// 2: the video loops a video-only segment and the audio is one tone of the
/// whole length, so every video track is constant frame rate with no holes.
/// Version 1 files lost one frame each loop.
const MEDIA_FIXTURE_VERSION: u32 = 2;

fn is_media(id: FixtureId) -> bool {
    matches!(
        id,
        FixtureId::H264_1080p60
            | FixtureId::Hevc2160p60
            | FixtureId::ProRes2160p30
            | FixtureId::LongGopScrub
    )
}

fn version_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".version");
    PathBuf::from(name)
}

fn is_valid_fixture(_fixtures_dir: &Path, id: FixtureId, path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() < 4096 {
        return false;
    }
    match id {
        FixtureId::H264_1080p60
        | FixtureId::Hevc2160p60
        | FixtureId::ProRes2160p30
        | FixtureId::LongGopScrub => {
            meta.len() > 100_000
                && std::fs::read_to_string(version_path(path))
                    .is_ok_and(|v| v.trim() == MEDIA_FIXTURE_VERSION.to_string())
        }
        FixtureId::Project10k => {
            let Some(project) = current_project(path) else {
                return false;
            };
            let tracks = project.timeline().tracks_ordered().count();
            let clips: usize = project
                .timeline()
                .tracks_ordered()
                .map(|t| t.clips_ordered().len())
                .sum();
            let markers = project.timeline().markers().len();
            let staggered = project
                .timeline()
                .tracks_ordered()
                .filter(|t| t.kind == TrackKind::Video)
                .nth(1)
                .and_then(|t| t.clips_ordered().next())
                .is_some_and(|c| c.timeline.start.value > 0);
            tracks == 50 && clips == 10_000 && markers == 500 && staggered
        }
        FixtureId::Stack4k => {
            let Some(project) = current_project(path) else {
                return false;
            };
            let tracks = project.timeline().tracks_ordered().count();
            tracks >= 4 && project.timeline().canvas().resolution == Some([3840, 2160])
        }
    }
}

/// The project fixture at `path` when this build's schema wrote it. A
/// fixture an older build wrote is generated again, so the scenarios that
/// open and save it measure the project a current save produces, not a
/// migrated one.
fn current_project(path: &Path) -> Option<Project> {
    Project::load_from_file(path)
        .ok()
        .filter(|project| project.schema.version == PROJECT_SCHEMA_VERSION)
}

fn ensure_long_gop_alias(fixtures_dir: &Path, scrub_path: &Path) -> Result<(), String> {
    let alias = fixtures_dir.join("long-gop-10m.mp4");
    if alias.exists() {
        return Ok(());
    }
    if std::fs::hard_link(scrub_path, &alias).is_err() {
        std::fs::copy(scrub_path, &alias).map_err(|err| {
            format!(
                "failed to create compatibility fixture {}: {err}",
                alias.display()
            )
        })?;
    }
    Ok(())
}

fn run_ffmpeg(args: &[&str], label: &str) -> Result<(), String> {
    crate::process::run_ffmpeg(args, label)
}

/// Encode a video-only segment from `testsrc2` with `video_args`, then loop it
/// `loops` extra times into `out` with a 440 Hz stereo tone of `tone_secs`
/// encoded once by `audio_args`.
///
/// The segment carries no audio: a segment with AAC is longer than its video
/// by the encoder padding, and every loop of it left that much of a hole in
/// the video track (3,517 frames in a 60 s file at 60 fps).
fn loop_segment(
    seg_path: &Path,
    source: &str,
    video_args: &[&str],
    loops: &str,
    length_args: &[&str],
    tone_secs: &str,
    audio_args: &[&str],
    out_path: &Path,
    label: &str,
) -> Result<(), String> {
    let seg_str = seg_path.to_string_lossy();
    let out_str = out_path.to_string_lossy();
    let mut segment = vec![
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        source,
    ];
    segment.extend_from_slice(video_args);
    segment.push(&seg_str);
    run_ffmpeg(&segment, &format!("{label} segment"))?;

    let tone = format!("sine=frequency=440:sample_rate=48000:duration={tone_secs}");
    let mut mux = vec![
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-stream_loop",
        loops,
        "-i",
        &seg_str,
        "-f",
        "lavfi",
        "-i",
        &tone,
        "-af",
        "pan=stereo|c0=c0|c1=c0",
        "-map",
        "0:v",
        "-map",
        "1:a",
    ];
    mux.extend_from_slice(length_args);
    mux.extend_from_slice(&["-c:v", "copy"]);
    mux.extend_from_slice(audio_args);
    mux.push(&out_str);
    let muxed = run_ffmpeg(&mux, &format!("{label} mux"));
    let _ = std::fs::remove_file(seg_path);
    muxed
}

fn generate_h264_1080p60(fixtures_dir: &Path, out_path: &Path) -> Result<(), String> {
    loop_segment(
        &fixtures_dir.join(".seg-h264-1080p60.mp4"),
        "testsrc2=size=1920x1080:rate=60:duration=1",
        &[
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-profile:v",
            "high",
            "-pix_fmt",
            "yuv420p",
            "-b:v",
            "20M",
            "-minrate",
            "20M",
            "-maxrate",
            "20M",
            "-bufsize",
            "20M",
            "-g",
            "60",
            "-keyint_min",
            "60",
            "-sc_threshold",
            "0",
        ],
        "59",
        &["-t", "60"],
        "60",
        &["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"],
        out_path,
        "h264-1080p60",
    )
}

fn generate_hevc_2160p60(fixtures_dir: &Path, out_path: &Path) -> Result<(), String> {
    loop_segment(
        &fixtures_dir.join(".seg-hevc-2160p60.mp4"),
        "testsrc2=size=3840x2160:rate=60:duration=1",
        &[
            "-c:v",
            "libx265",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p10le",
            "-x265-params",
            "keyint=60:min-keyint=60:scenecut=0:vbv-maxrate=80000:vbv-bufsize=80000:log-level=error",
            "-b:v",
            "80M",
            "-tag:v",
            "hvc1",
        ],
        "59",
        &["-t", "60", "-tag:v", "hvc1"],
        "60",
        &["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"],
        out_path,
        "hevc-2160p60",
    )
}

fn generate_prores_2160p30(fixtures_dir: &Path, out_path: &Path) -> Result<(), String> {
    loop_segment(
        &fixtures_dir.join(".seg-prores-2160p30.mov"),
        "testsrc2=size=3840x2160:rate=30000/1001",
        &[
            "-frames:v",
            "30",
            "-c:v",
            "prores_ks",
            "-profile:v",
            "3",
            "-pix_fmt",
            "yuv422p10le",
        ],
        "29",
        &["-frames:v", "900", "-shortest"],
        "30.03",
        &["-c:a", "pcm_s16le"],
        out_path,
        "prores-2160p30",
    )
}

fn generate_long_gop_scrub(fixtures_dir: &Path, out_path: &Path) -> Result<(), String> {
    loop_segment(
        &fixtures_dir.join(".seg-long-gop.mp4"),
        "testsrc2=size=1920x1080:rate=30:duration=10",
        &[
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-profile:v",
            "high",
            "-pix_fmt",
            "yuv420p",
            "-b:v",
            "8M",
            "-g",
            "300",
            "-keyint_min",
            "300",
            "-sc_threshold",
            "0",
        ],
        "59",
        &["-t", "600"],
        "600",
        &["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"],
        out_path,
        "long-gop-scrub",
    )
}

/// Build the `project-10k` fixture in memory:
/// 1 sequence, 10,000 clips on 50 tracks (25 video + 25 audio, 200 clips each),
/// 2,000 keyframes, and 500 markers.
pub fn build_project_10k(fixtures_dir: &Path) -> Result<Project, String> {
    let rate = Rational::FPS_24;
    let mut project = Project::new("project-10k", rate);
    let media_path = FixtureId::H264_1080p60.path(fixtures_dir);
    let clip_len = 6i64;
    let clips_per_track = 200usize;
    let source_ticks = 60 * 24;

    let video_media = project.add_media(MediaSource::new(
        &media_path,
        1920,
        1080,
        rate,
        source_ticks,
        true,
    ));

    let mut keyframes_remaining = 2_000usize;

    for lane in 0..25usize {
        let v_track = project.add_track(TrackKind::Video, format!("V{}", lane + 1));
        for idx in 0..clips_per_track {
            let start_tick = ((idx * 25 + lane) as i64) * clip_len;
            let source_offset = ((idx as i64) * clip_len) % (source_ticks - clip_len);
            let source = TimeRange::at_rate(source_offset, clip_len, rate);
            let clip_id = project
                .add_clip(
                    v_track,
                    video_media,
                    source,
                    RationalTime::new(start_tick, rate),
                )
                .map_err(|err| format!("failed to add video clip ({lane}, {idx}): {err}"))?;
            if lane < 3 && keyframes_remaining >= 2 {
                project
                    .set_param_keyframe(
                        clip_id,
                        ClipParam::Opacity,
                        RationalTime::new(start_tick, rate),
                        ParamValue::Scalar(0.85),
                        Easing::Linear,
                        None,
                    )
                    .map_err(|err| format!("video keyframe 0 failed: {err}"))?;
                project
                    .set_param_keyframe(
                        clip_id,
                        ClipParam::Opacity,
                        RationalTime::new(start_tick + clip_len / 2, rate),
                        ParamValue::Scalar(1.0),
                        Easing::Linear,
                        None,
                    )
                    .map_err(|err| format!("video keyframe 1 failed: {err}"))?;
                keyframes_remaining -= 2;
            }
        }
    }

    for lane in 0..25usize {
        let a_track = project.add_track(TrackKind::Audio, format!("A{}", lane + 1));
        for idx in 0..clips_per_track {
            let start_tick = ((idx * 25 + lane) as i64) * clip_len;
            let source_offset = ((idx as i64) * clip_len) % (source_ticks - clip_len);
            let source = TimeRange::at_rate(source_offset, clip_len, rate);
            let clip_id = project
                .add_clip(
                    a_track,
                    video_media,
                    source,
                    RationalTime::new(start_tick, rate),
                )
                .map_err(|err| format!("failed to add audio clip ({lane}, {idx}): {err}"))?;
            if keyframes_remaining >= 2 {
                project
                    .set_param_keyframe(
                        clip_id,
                        ClipParam::Volume,
                        RationalTime::new(start_tick, rate),
                        ParamValue::Scalar(0.85),
                        Easing::Linear,
                        None,
                    )
                    .map_err(|err| format!("audio keyframe 0 failed: {err}"))?;
                project
                    .set_param_keyframe(
                        clip_id,
                        ClipParam::Volume,
                        RationalTime::new(start_tick + clip_len / 2, rate),
                        ParamValue::Scalar(1.0),
                        Easing::Linear,
                        None,
                    )
                    .map_err(|err| format!("audio keyframe 1 failed: {err}"))?;
                keyframes_remaining -= 2;
            }
        }
    }

    for m in 0..500usize {
        let tick = RationalTime::new((m as i64) * 8, rate);
        let color = match m % 4 {
            0 => MarkerColor::Blue,
            1 => MarkerColor::Green,
            2 => MarkerColor::Yellow,
            _ => MarkerColor::Red,
        };
        project
            .timeline_mut()
            .add_marker(Marker::new(tick, format!("M{m:03}"), color))
            .map_err(|err| format!("failed to add marker {m}: {err}"))?;
    }

    Ok(project)
}

/// Build the `stack-4k` fixture in memory:
/// Three 4K video layers, each with Transform, Gaussian Blur, and Lumetri basic
/// color, plus a text clip track above.
pub fn build_stack_4k(fixtures_dir: &Path) -> Result<Project, String> {
    let rate = Rational::FPS_60;
    let mut project = Project::new("stack-4k", rate);
    let canvas = CanvasSettings::custom(CanvasAspect::Auto, [0, 0, 0], Some([3840, 2160]))
        .map_err(|err| format!("invalid 4K canvas settings: {err}"))?;
    project.timeline_mut().set_canvas(canvas);

    let hevc_path = FixtureId::Hevc2160p60.path(fixtures_dir);
    let duration_ticks = 30 * 60;
    let media = project.add_media(MediaSource::new(
        &hevc_path,
        3840,
        2160,
        rate,
        60 * 60,
        true,
    ));

    for layer in 0..3usize {
        let track = project.add_track(TrackKind::Video, format!("V{}", layer + 1));
        let source = TimeRange::at_rate((layer as i64) * 60, duration_ticks, rate);
        let clip_id = project
            .add_clip(track, media, source, RationalTime::new(0, rate))
            .map_err(|err| format!("failed to add 4K stack layer {layer}: {err}"))?;

        let scale = 0.96 - (layer as f32) * 0.04;
        let dx = (layer as f32 - 1.0) * 0.01;
        let dy = (layer as f32 - 1.0) * 0.01;
        let rot = (layer as f32) * 0.75;
        project
            .set_transform(
                clip_id,
                ClipTransform {
                    position: [dx, dy],
                    anchor_point: [0.5, 0.5],
                    scale: Scale2::uniform(scale),
                    rotation: rot,
                    opacity: 0.92,
                },
                None,
            )
            .map_err(|err| format!("set_transform failed on layer {layer}: {err}"))?;
        project
            .set_clip_adjustments(
                clip_id,
                ColorAdjustments {
                    exposure: Param::Constant(0.2 + (layer as f32) * 0.05),
                    contrast: Param::Constant(0.15),
                    highlights: Param::Constant(-0.1),
                    shadows: Param::Constant(0.12),
                    temperature: Param::Constant(0.08),
                    tint: Param::Constant(-0.04),
                    saturation: Param::Constant(0.12),
                    ..ColorAdjustments::default()
                },
            )
            .map_err(|err| format!("set_clip_adjustments failed on layer {layer}: {err}"))?;
        if let Some(clip) = project.timeline_mut().clip_mut(clip_id) {
            clip.effects.push(EffectInstance::new("gaussian_blur"));
        }
        project
            .set_effect_param(clip_id, 0, 0, 4.0 + (layer as f32) * 2.0)
            .map_err(|err| format!("set_effect_param failed on layer {layer}: {err}"))?;
    }

    let text_track = project.add_track(TrackKind::Text, "T1");
    project
        .add_generated(
            text_track,
            Generator::Text {
                content: "Aria 4K Stack Benchmark".to_owned(),
                style: TextStyle::default(),
            },
            TimeRange::at_rate(0, duration_ticks, rate),
        )
        .map_err(|err| format!("failed to add text clip on stack-4k: {err}"))?;

    Ok(project)
}

#[cfg(test)]
#[path = "fixtures_tests.rs"]
mod tests;
