//! `aria inspect` — print what a project file contains.
//!
//! The point is diagnosis without the editor: which media a project wants,
//! whether those files are still on disk, and where its clips sit. Offline media
//! is called offline here, because a project that opens with three missing files
//! looks identical to a healthy one in a directory listing.

use aria_core::count::counted;
use aria_models::{ClipSource, Generator, MediaKind, MediaSource, Project};

use crate::args::Args;
use crate::fail::Failure;
use crate::project;

pub(crate) const FLAGS: &[&str] = &["clips"];
pub(crate) const LANE_FLAGS: [&str; 8] = [
    "disabled",
    "muted",
    "solo",
    "locked",
    "main",
    "duck source",
    "not targeted",
    "no sync lock",
];

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let path = args.required_path(0, "a project file")?;
    let project = project::load(&path)?;
    let timeline = project.timeline();
    let rate = timeline.frame_rate;
    let duration = timeline.duration();
    let canvas = aria_render::canvas_size(&project);

    println!("project   {}", path.display());
    println!("schema    v{}", project.schema().version);
    println!("canvas    {}x{}", canvas.0, canvas.1);
    println!("rate      {}/{}", rate.num, rate.den);
    let empty_label = if duration.value == 0 { " (empty)" } else { "" };
    println!(
        "duration  {} ({} ticks){empty_label}",
        project::clock(duration.seconds()),
        duration.value
    );
    let markers = timeline.markers();
    if !markers.is_empty() {
        println!("markers   {}", markers.len());
    }

    println!("\nmedia");
    let mut media: Vec<_> = project.media_iter().collect();
    media.sort_by(|a, b| a.path.cmp(&b.path));
    if media.is_empty() {
        println!("  (none)");
    }
    for source in media {
        let offline = if source.path.exists() {
            ""
        } else {
            "  OFFLINE"
        };
        println!(
            "  {} {}{}",
            media_line(source),
            source.path.display(),
            offline
        );
    }

    println!("\nlanes (bottom to top)");
    let tracks: Vec<_> = timeline.tracks_ordered().collect();
    if tracks.is_empty() {
        println!("  (none)");
    }
    for track in tracks {
        let clips = track.clips_ordered();
        let mut notes = Vec::new();
        for (active, label) in [
            (!track.enabled, LANE_FLAGS[0]),
            (track.muted, LANE_FLAGS[1]),
            (track.solo, LANE_FLAGS[2]),
            (track.locked, LANE_FLAGS[3]),
            (track.main, LANE_FLAGS[4]),
            (track.duck_source, LANE_FLAGS[5]),
            (!track.targeted, LANE_FLAGS[6]),
            (!track.sync_lock, LANE_FLAGS[7]),
        ] {
            if active {
                notes.push(label);
            }
        }
        let notes = if notes.is_empty() {
            String::new()
        } else {
            format!("  [{}]", notes.join(", "))
        };
        let count_str = if clips.is_empty() {
            "0 clips (empty)".to_string()
        } else {
            counted(clips.len(), "clip", "clips").to_string()
        };
        println!(
            "  {:?} \"{}\"  {}{}",
            track.kind, track.name, count_str, notes
        );
        if !args.present("clips") {
            continue;
        }
        for clip in clips {
            println!(
                "    {} → {}  {}",
                project::clock(clip.timeline.start.seconds()),
                project::clock(
                    aria_core::RationalTime::new(clip.timeline.end_tick(), rate).seconds()
                ),
                describe(&project, &clip.content)
            );
        }
    }
    Ok(())
}

/// What a pool entry is, as its kind has it: a video's size, rate and length,
/// a sound's length, and a still's size. A still has no rate or length of its
/// own; its duration is the length a placement starts at.
fn media_line(source: &MediaSource) -> String {
    let length = project::clock(source.duration.seconds());
    match source.kind() {
        MediaKind::Video => format!(
            "{}x{} {}/{} {length}",
            source.width, source.height, source.frame_rate.num, source.frame_rate.den
        ),
        MediaKind::Audio => format!("audio {length}"),
        MediaKind::Image => format!("{}x{} still", source.width, source.height),
    }
}

/// One line describing what a clip plays.
fn describe(project: &Project, content: &ClipSource) -> String {
    match content {
        ClipSource::Media { media, source } => {
            let name = project
                .media(*media)
                .map(|entry| {
                    entry
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| entry.path.display().to_string())
                })
                .unwrap_or_else(|| "<missing pool entry>".to_string());
            format!(
                "{name} (source {} + {})",
                source.start.value, source.duration.value
            )
        }
        ClipSource::Generated(generator) => match generator_detail(generator) {
            Some(detail) => format!("{} {detail}", generator.spec().id),
            None => generator.spec().id.to_string(),
        },
        ClipSource::Multicam { group, source } => {
            format!(
                "Multicam {} (source {} + {})",
                group.raw(),
                source.start.value,
                source.duration.value
            )
        }
        ClipSource::Sequence { sequence, source } => {
            let name = project
                .sequence(*sequence)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| format!("Sequence {}", sequence.raw()));
            format!(
                "{name} (source {} + {})",
                source.start.value, source.duration.value
            )
        }
    }
}

/// The part of a generated clip its catalog row cannot name: the words in a
/// title, the color of a solid, the file a Lottie plays.
///
/// Exhaustive on purpose. Every arm extracts one variant's own payload, and the
/// variants that carry none are named rather than absorbed by a wildcard, so a
/// new generator has to be described here instead of printing a debug dump.
fn generator_detail(generator: &Generator) -> Option<String> {
    match generator {
        Generator::Text { content, .. } => Some(format!("\"{content}\"")),
        Generator::SolidColor { rgba } => Some(format!("{rgba:?}")),
        Generator::Shape { shape, .. } => Some(format!("{shape:?}")),
        Generator::Sticker { asset } => Some(format!("\"{asset}\"")),
        Generator::Lottie {
            path,
            width,
            height,
        } => Some(format!("\"{path}\" {width}x{height}")),
        Generator::Effect | Generator::Filter | Generator::Adjustment => None,
    }
}
#[cfg(test)]
mod tests {
    use aria_core::Rational;
    use aria_models::{MediaKind, MediaSource};

    use super::media_line;

    /// WHY: `aria inspect` printed a sound as `0x0 1000/1 0:00:02.000`, the
    /// size and tick rate its probe fills in for no picture, and a still with
    /// the rate and length a placement starts at. Each kind prints what it
    /// has; a new kind fails to compile until `media_line` prints it.
    #[test]
    fn a_pool_entry_prints_what_its_kind_has() {
        let cases = [
            (
                MediaSource::new("a.mp4", 640, 360, Rational::FPS_24, 72, true),
                MediaKind::Video,
                "640x360 24/1 0:00:03.000",
            ),
            (
                MediaSource::new("b.mp4", 640, 360, Rational::FPS_24, 48, false),
                MediaKind::Video,
                "640x360 24/1 0:00:02.000",
            ),
            (
                MediaSource::new("c.wav", 0, 0, Rational::new(1000, 1), 2000, true),
                MediaKind::Audio,
                "audio 0:00:02.000",
            ),
            (
                MediaSource::image("d.png", 512, 256),
                MediaKind::Image,
                "512x256 still",
            ),
        ];
        for (source, kind, line) in cases {
            assert_eq!(source.kind(), kind, "{}", source.path.display());
            assert_eq!(media_line(&source), line, "{}", source.path.display());
        }
    }
}
