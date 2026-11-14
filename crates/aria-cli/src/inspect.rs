//! `aria inspect` — print what a project file contains.
//!
//! The point is diagnosis without the editor: which media a project wants,
//! whether those files are still on disk, and where its clips sit. Offline media
//! is called offline here, because a project that opens with three missing files
//! looks identical to a healthy one in a directory listing.

use std::borrow::Cow;

use aria_core::count::counted;
use aria_core::{Rational, RationalTime};
use aria_models::{
    ClipSource, Generator, MediaKind, MediaSource, Project, Shape, Track, Transition,
    TransitionAlignment,
};

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

    println!("project   {}", one_line(&path.to_string_lossy()));
    println!("schema    v{}", project.schema().version);
    println!("canvas    {}x{}", canvas.0, canvas.1);
    println!("rate      {} fps", rate.fps_text());
    let empty_label = if duration.value == 0 { " (empty)" } else { "" };
    println!(
        "duration  {} ({}){empty_label}",
        project::clock(duration.seconds()),
        counted(duration.value, "frame", "frames")
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
            one_line(&source.path.to_string_lossy()),
            offline
        );
    }

    println!("\nlanes (bottom to top)");
    let tracks: Vec<_> = timeline.tracks_ordered().collect();
    if tracks.is_empty() {
        println!("  (none)");
    }
    for track in tracks {
        println!("{}", lane_line(track));
        if args.present("clips") {
            for line in clip_lines(&project, track, rate) {
                println!("{line}");
            }
        }
    }
    Ok(())
}

/// A lane's header: its kind, its name, its clip count, and the state flags
/// that are on.
fn lane_line(track: &Track) -> String {
    let clips = track.clips_ordered().len();
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
    let count = if clips == 0 {
        "0 clips (empty)".to_string()
    } else {
        counted(clips, "clip", "clips").to_string()
    };
    format!("  {:?} {:?}  {count}{notes}", track.kind, track.name)
}

/// One line per clip on `track`, in timeline order, each followed by a line
/// for the transition at the cut after it.
fn clip_lines(project: &Project, track: &Track, rate: Rational) -> Vec<String> {
    let mut lines = Vec::new();
    for clip in track.clips_ordered() {
        lines.push(format!(
            "    {} → {}  {}",
            project::clock(clip.timeline.start.seconds()),
            project::clock(RationalTime::new(clip.timeline.end_tick(), rate).seconds()),
            describe(project, &clip.content)
        ));
        for transition in track.transitions().iter().filter(|t| t.left == clip.id) {
            lines.push(format!("      {}", transition_line(transition, rate)));
        }
    }
    lines
}

/// A transition as its catalog id, its length, and where its window sits on
/// the cut.
fn transition_line(transition: &Transition, rate: Rational) -> String {
    let place = match transition.alignment {
        TransitionAlignment::Center => "centered on the cut",
        TransitionAlignment::Start => "starting at the cut",
        TransitionAlignment::End => "ending at the cut",
    };
    format!(
        "transition {} {}, {place}",
        one_line(&transition.transition_id),
        project::clock(RationalTime::new(transition.duration, rate).seconds())
    )
}

/// `text` on one line: a control character, such as a newline in a file name,
/// prints as its escape, so one clip or pool entry prints as one line.
fn one_line(text: &str) -> Cow<'_, str> {
    if !text.contains(char::is_control) {
        return Cow::Borrowed(text);
    }
    let mut line = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if c.is_control() {
            line.extend(c.escape_debug());
        } else {
            line.push(c);
        }
    }
    Cow::Owned(line)
}

/// What a pool entry is, as its kind has it: a video's size, rate and length,
/// a sound's length, and a still's size. A still has no rate or length of its
/// own; its duration is the length a placement starts at.
fn media_line(source: &MediaSource) -> String {
    let length = project::clock(source.duration.seconds());
    match source.kind() {
        MediaKind::Video => format!(
            "{}x{} {} fps {length}",
            source.width,
            source.height,
            source.frame_rate.fps_text()
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
                .map(|entry| match entry.path.file_name() {
                    Some(name) => one_line(&name.to_string_lossy()).into_owned(),
                    None => one_line(&entry.path.to_string_lossy()).into_owned(),
                })
                .unwrap_or_else(|| "<missing pool entry>".to_string());
            format!("{name} ({})", source_span(*source))
        }
        ClipSource::Generated(generator) => match generator_detail(generator) {
            Some(detail) => format!("{} {detail}", generator.spec().id),
            None => generator.spec().id.to_string(),
        },
        ClipSource::Multicam { group, source } => {
            format!("Multicam {} ({})", group.raw(), source_span(*source))
        }
        ClipSource::Sequence { sequence, source } => {
            let name = project
                .sequence(*sequence)
                .map(|s| one_line(&s.name).into_owned())
                .unwrap_or_else(|| format!("Sequence {}", sequence.raw()));
            format!("{name} ({})", source_span(*source))
        }
    }
}

/// The part of a source a clip plays, as the times it plays from and to. The
/// ticks are in the source's own rate, a video's frames and a sound's
/// milliseconds, so two clips' raw ticks compare nothing.
fn source_span(source: aria_core::TimeRange) -> String {
    let end = aria_core::RationalTime::new(
        source.start.value.saturating_add(source.duration.value),
        source.start.rate,
    );
    format!(
        "source {} → {}",
        project::clock(source.start.seconds()),
        project::clock(end.seconds())
    )
}

/// The part of a generated clip its catalog row cannot name: the words in a
/// title, the color of a solid, the figure a shape draws, the file a Lottie
/// plays.
///
/// Exhaustive on purpose. Every arm extracts one variant's own payload, and the
/// variants that carry none are named rather than absorbed by a wildcard, so a
/// new generator has to be described here instead of printing a debug dump.
fn generator_detail(generator: &Generator) -> Option<String> {
    match generator {
        Generator::Text { content, .. } => Some(format!("{content:?}")),
        Generator::SolidColor { rgba } => Some(hex(*rgba)),
        Generator::Shape { shape, .. } => Some(figure(shape)),
        Generator::Sticker { asset } => Some(format!("{asset:?}")),
        Generator::Lottie {
            path,
            width,
            height,
        } => Some(format!("{path:?} {width}x{height}")),
        Generator::Effect | Generator::Filter | Generator::Adjustment => None,
    }
}

/// A color as `#rrggbb`, with the alpha byte appended when it is not opaque.
fn hex([r, g, b, a]: [u8; 4]) -> String {
    if a == u8::MAX {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// A shape's figure in words, with the count that sets it apart.
fn figure(shape: &Shape) -> String {
    match shape {
        Shape::Rectangle => "rectangle".to_string(),
        Shape::Ellipse => "ellipse".to_string(),
        Shape::Polygon { sides } => format!("polygon, {}", counted(*sides, "side", "sides")),
        Shape::Star { points, .. } => format!("star, {}", counted(*points, "point", "points")),
        Shape::Line => "line".to_string(),
        Shape::Arrow => "arrow".to_string(),
        Shape::Heart => "heart".to_string(),
        Shape::Path(path) => format!(
            "{} path, {}",
            if path.closed { "closed" } else { "open" },
            counted(path.points.len(), "point", "points")
        ),
    }
}
#[cfg(test)]
mod tests {
    use aria_core::{Rational, TimeRange};
    use aria_models::{
        ClipSource, Generator, MediaKind, MediaSource, MulticamGroupId, Param, Project, SequenceId,
        Shape, ShapePath, ShapePathPoint, TrackKind, TransitionAlignment,
    };

    use super::{
        clip_lines, describe, generator_detail, lane_line, media_line, one_line, transition_line,
    };

    /// WHY: `aria inspect --clips` printed a title written as `Hello` and
    /// `World` on two lines as two output lines, the second starting at column
    /// zero with no clip before it, and a lane, file or title holding a `"`
    /// printed a quote that ended its own quoting early. Every string a project
    /// stores and inspect prints, quoted or not, prints on one line with its
    /// control characters escaped. Not caught: a new string field printed
    /// outside these functions.
    #[test]
    fn written_text_prints_on_one_line() {
        let text = |content: &str| Generator::Text {
            content: content.into(),
            style: Default::default(),
        };
        let cases = [
            (text("Hello\nWorld"), r#""Hello\nWorld""#),
            (text(r#"say "hi""#), r#""say \"hi\"""#),
            (text("tab\there\r"), r#""tab\there\r""#),
            (text("café"), r#""café""#),
            (
                Generator::Sticker {
                    asset: "star\nburst".into(),
                },
                r#""star\nburst""#,
            ),
            (
                Generator::Lottie {
                    path: "a\nb.json".into(),
                    width: 64,
                    height: 32,
                },
                r#""a\nb.json" 64x32"#,
            ),
        ];
        for (generator, detail) in cases {
            assert_eq!(generator_detail(&generator).as_deref(), Some(detail));
        }

        for (raw, line) in [
            ("plain.mp4", "plain.mp4"),
            ("a\nb.mp4", r"a\nb.mp4"),
            ("bell\u{7}\u{7f}", r"bell\u{7}\u{7f}"),
            ("é ü", "é ü"),
            ("", ""),
        ] {
            assert_eq!(one_line(raw), line);
        }

        let mut project = Project::new("p", Rational::FPS_24);
        let media = project.add_media(MediaSource::new(
            "/m/two\nlines.mp4",
            640,
            360,
            Rational::FPS_24,
            48,
            true,
        ));
        let content = ClipSource::Media {
            media,
            source: TimeRange::at_rate(0, 24, Rational::FPS_24),
        };
        assert_eq!(
            describe(&project, &content),
            r"two\nlines.mp4 (source 0:00:00.000 → 0:00:01.000)"
        );
        let lane = project.add_track(TrackKind::Text, "B\nroll \"2\"");
        let lane = project.timeline().track(lane).unwrap();
        assert_eq!(
            lane_line(lane),
            r#"  Text "B\nroll \"2\""  0 clips (empty)"#
        );
    }

    /// WHY: `aria inspect --clips` printed no transitions, so a project with a
    /// crossfade at every cut listed the same as one with hard cuts. A
    /// transition prints on the line after the clip on its outgoing side, and
    /// only there, with its id, its length and where its window sits, for
    /// every alignment.
    #[test]
    fn a_transition_prints_after_the_clip_it_leaves() {
        let mut project = Project::new("p", Rational::FPS_24);
        let track = project.add_track(TrackKind::Sticker, "S");
        let solid = |at| {
            (
                Generator::SolidColor {
                    rgba: [255, 0, 0, 255],
                },
                TimeRange::at_rate(at, 48, Rational::FPS_24),
            )
        };
        let mut clips = Vec::new();
        for at in [0, 48, 96] {
            let (generator, timeline) = solid(at);
            clips.push(project.add_generated(track, generator, timeline).unwrap());
        }
        project.add_transition(clips[0], "crossfade").unwrap();
        let lane = project.timeline().track(track).unwrap();
        assert_eq!(
            clip_lines(&project, lane, Rational::FPS_24),
            [
                "    0:00:00.000 → 0:00:02.000  solid #ff0000",
                "      transition crossfade 0:00:01.000, centered on the cut",
                "    0:00:02.000 → 0:00:04.000  solid #ff0000",
                "    0:00:04.000 → 0:00:06.000  solid #ff0000",
            ]
        );

        let places = [
            (TransitionAlignment::Center, "centered on the cut"),
            (TransitionAlignment::Start, "starting at the cut"),
            (TransitionAlignment::End, "ending at the cut"),
        ];
        assert_eq!(places.len(), TransitionAlignment::ALL.len());
        let mut transition = lane.transitions()[0].clone();
        transition.duration = 12;
        for alignment in TransitionAlignment::ALL {
            let place = places.iter().find(|(a, _)| *a == alignment).unwrap().1;
            transition.alignment = alignment;
            assert_eq!(
                transition_line(&transition, Rational::FPS_24),
                format!("transition crossfade 0:00:00.500, {place}")
            );
        }
    }

    /// WHY: `aria inspect --clips` printed a solid as its color's byte array,
    /// `solid [255, 0, 0, 255]`, and a shape as its Rust debug form, a star as
    /// `shape Star { points: 5, inner_ratio: Constant(0.5) }` and a pen path
    /// as every anchor and handle on one line. A solid prints its color as
    /// hex and a shape its figure in words, for every figure. Not caught: a
    /// new figure, which fails to compile in `figure` instead.
    #[test]
    fn a_generated_clip_prints_its_content_in_words() {
        let project = Project::new("p", Rational::FPS_24);
        let shape = |shape| {
            ClipSource::Generated(Generator::Shape {
                shape,
                rgba: Param::Constant([255, 255, 255, 255]),
                width: Param::Constant(100.0),
                height: Param::Constant(100.0),
                corner_radius: Param::Constant(0.0),
                stroke: None,
            })
        };
        let path = |closed, points: usize| {
            Shape::Path(ShapePath {
                points: (0..points)
                    .map(|at| ShapePathPoint::corner([at as f32, 0.0]))
                    .collect(),
                closed,
            })
        };
        let solid = |rgba| ClipSource::Generated(Generator::SolidColor { rgba });
        let cases = [
            (solid([255, 0, 0, 255]), "solid #ff0000"),
            (solid([0, 128, 15, 64]), "solid #00800f40"),
            (shape(Shape::Rectangle), "shape rectangle"),
            (shape(Shape::Ellipse), "shape ellipse"),
            (shape(Shape::Polygon { sides: 6 }), "shape polygon, 6 sides"),
            (
                shape(Shape::Star {
                    points: 5,
                    inner_ratio: Param::Constant(0.5),
                }),
                "shape star, 5 points",
            ),
            (shape(Shape::Line), "shape line"),
            (shape(Shape::Arrow), "shape arrow"),
            (shape(Shape::Heart), "shape heart"),
            (shape(path(true, 4)), "shape closed path, 4 points"),
            (shape(path(false, 2)), "shape open path, 2 points"),
        ];
        for (content, line) in cases {
            assert_eq!(describe(&project, &content), line);
        }
    }

    /// WHY: `aria inspect` printed a sound as `0x0 1000/1 0:00:02.000`, the
    /// size and tick rate its probe fills in for no picture, a still with the
    /// rate and length a placement starts at, and a video's rate as `24/1`.
    /// Each kind prints what it has, a rate in frames a second; a new kind
    /// fails to compile until `media_line` prints it.
    #[test]
    fn a_pool_entry_prints_what_its_kind_has() {
        let cases = [
            (
                MediaSource::new("a.mp4", 640, 360, Rational::FPS_24, 72, true),
                MediaKind::Video,
                "640x360 24 fps 0:00:03.000",
            ),
            (
                MediaSource::new("b.mp4", 640, 360, Rational::FPS_24, 48, false),
                MediaKind::Video,
                "640x360 24 fps 0:00:02.000",
            ),
            (
                MediaSource::new("n.mp4", 1920, 1080, Rational::FPS_29_97, 30, true),
                MediaKind::Video,
                "1920x1080 29.97 fps 0:00:01.001",
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

    /// WHY: `aria inspect --clips` printed a clip's source window as raw ticks,
    /// `(source 0 + 90)` for three seconds of a 30 fps video and `(source 0 +
    /// 2000)` for two seconds of sound, each in its own source's rate with no
    /// unit. Every kind of source window, media at a frame rate and at a sound's
    /// tick rate, a multicam group, and a sequence, prints as the source times
    /// it plays from and to, and a window whose end tick is past `i64::MAX`
    /// prints without overflow. Not caught: a source whose `start` and
    /// `duration` rates differ, which loading refuses.
    #[test]
    fn a_clip_source_window_prints_as_the_times_it_plays() {
        let mut project = Project::new("p", Rational::FPS_24);
        let video = project.add_media(MediaSource::new(
            "v.mp4",
            640,
            360,
            Rational::FPS_30,
            90,
            true,
        ));
        let ms = Rational::new(1000, 1);
        let sound = project.add_media(MediaSource::new("s.wav", 0, 0, ms, 2000, true));
        let cases = [
            (
                ClipSource::Media {
                    media: video,
                    source: TimeRange::at_rate(15, 60, Rational::FPS_30),
                },
                "v.mp4 (source 0:00:00.500 → 0:00:02.500)",
            ),
            (
                ClipSource::Media {
                    media: sound,
                    source: TimeRange::at_rate(250, 2000, ms),
                },
                "s.wav (source 0:00:00.250 → 0:00:02.250)",
            ),
            (
                ClipSource::Multicam {
                    group: MulticamGroupId::from_raw(7),
                    source: TimeRange::at_rate(48, 24, Rational::FPS_24),
                },
                "Multicam 7 (source 0:00:02.000 → 0:00:03.000)",
            ),
            (
                ClipSource::Sequence {
                    sequence: SequenceId::from_raw(9),
                    source: TimeRange::at_rate(0, 72, Rational::FPS_24),
                },
                "Sequence 9 (source 0:00:00.000 → 0:00:03.000)",
            ),
            (
                ClipSource::Media {
                    media: video,
                    source: TimeRange::at_rate(i64::MAX - 10, 30, Rational::new(i32::MAX, 1)),
                },
                "v.mp4 (source 1193046:28:18.000 → 1193046:28:18.000)",
            ),
        ];
        for (content, line) in cases {
            assert_eq!(describe(&project, &content), line);
        }
    }
}
