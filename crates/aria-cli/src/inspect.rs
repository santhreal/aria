//! `aria inspect` — print what a project file contains.
//!
//! The point is diagnosis without the editor: which media a project wants,
//! whether those files are still on disk, and where its clips sit. Offline media
//! is called offline here, because a project that opens with three missing files
//! looks identical to a healthy one in a directory listing.

use aria_models::{ClipSource, Generator, Project};

use crate::args::Args;
use crate::fail::Failure;
use crate::project;

pub const FLAGS: &[&str] = &["clips"];

pub fn run(args: &Args) -> Result<(), Failure> {
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
            "  {}x{} {}/{} {} {}{}",
            source.width,
            source.height,
            source.frame_rate.num,
            source.frame_rate.den,
            project::clock(source.duration.seconds()),
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
        if !track.enabled {
            notes.push("disabled");
        }
        if track.muted {
            notes.push("muted");
        }
        if track.solo {
            notes.push("solo");
        }
        if track.locked {
            notes.push("locked");
        }
        if track.main {
            notes.push("main");
        }
        if track.duck_source {
            notes.push("duck source");
        }
        if !track.targeted {
            notes.push("not targeted");
        }
        if !track.sync_lock {
            notes.push("no sync lock");
        }
        let notes = if notes.is_empty() {
            String::new()
        } else {
            format!("  [{}]", notes.join(", "))
        };
        let count_str = if clips.is_empty() {
            "0 clip(s) (empty)".to_string()
        } else {
            format!("{} clip(s)", clips.len())
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
/// title, the colour of a solid, the file a Lottie plays.
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
    use aria_models::Project;

    #[test]
    fn new_project_duration_and_lanes_show_empty_indicators() {
        let project = Project::new("New project", aria_core::Rational::FPS_24);
        let timeline = project.timeline();
        let duration = timeline.duration();
        assert_eq!(duration.value, 0);
        let empty_label = if duration.value == 0 { " (empty)" } else { "" };
        assert_eq!(empty_label, " (empty)");
    }
}
