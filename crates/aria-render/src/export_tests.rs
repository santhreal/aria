//! What an export is going to write, before and during the encode pass.
//!
//! Defect class closed: wrong frame-span arithmetic, partial or corrupt files
//! left at the destination when an encoder fails or is cancelled mid-export, and
//! audio samples pushed to a video-only encoder (`supports_audio == false`).
//!
//! What this does not catch: hardware platform encoder output bitstreams, which
//! are covered by `tests/export_pipeline.rs`.

use super::*;
use aria_core::Rational;
use aria_models::{Generator, TimeRange, TrackKind};

const FPS: Rational = Rational::FPS_30;
const FRAMES: i64 = 30;

/// A one-second solid on one lane: the shortest project that has a duration.
fn one_second() -> Project {
    let mut project = Project::new("export-span", FPS);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [10, 120, 200, 255],
            },
            TimeRange::at_rate(0, FRAMES, FPS),
        )
        .expect("place the solid generator");
    project
}

fn ranged(project: &Project, from: i64, to: i64) -> ExportSettings {
    let mut settings = ExportSettings::for_project(project);
    settings.range = Some((RationalTime::new(from, FPS), RationalTime::new(to, FPS)));
    settings
}

fn refusal(project: &Project, settings: ExportSettings) -> EmptyExport {
    match export_frame_count(project, settings) {
        Err(RenderError::Empty(reason)) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn the_whole_timeline_is_every_frame_of_it() {
    let project = one_second();
    let settings = ExportSettings::for_project(&project);
    assert_eq!(
        export_frame_count(&project, settings).expect("a placed clip is exportable"),
        FRAMES as u64
    );
}

#[test]
fn an_empty_sequence_has_nothing_to_export() {
    let project = Project::new("export-span-empty", FPS);
    let settings = ExportSettings::for_project(&project);
    assert_eq!(refusal(&project, settings), EmptyExport::Sequence);
}

/// A lane is not content. Adding a track is the first edit in a new project,
/// and it must not make the export button look like it would produce a file.
#[test]
fn a_lane_with_no_clips_is_the_same_refusal() {
    let mut project = Project::new("export-span-lane", FPS);
    project.add_track(TrackKind::Video, "V1");
    let settings = ExportSettings::for_project(&project);
    assert_eq!(refusal(&project, settings), EmptyExport::Sequence);
}

#[test]
fn a_range_whose_ends_meet_covers_no_frame() {
    let project = one_second();
    let settings = ranged(&project, 10, 10);
    assert_eq!(refusal(&project, settings), EmptyExport::Range);
}

/// An in point after the out point is a marked range dragged backwards;
/// `frame_span` orders it to an empty span rather than a negative one, and an
/// empty span is a refusal.
#[test]
fn a_range_that_runs_backwards_covers_no_frame() {
    let project = one_second();
    let settings = ranged(&project, 20, 5);
    assert_eq!(refusal(&project, settings), EmptyExport::Range);
}

/// Past the end both ends clamp to the duration, so the span is empty. The
/// reason is the range, not the sequence: the sequence has content, and a
/// message to place a clip would point at the wrong control.
#[test]
fn a_range_entirely_past_the_end_is_refused_as_a_range() {
    let project = one_second();
    let settings = ranged(&project, FRAMES * 2, FRAMES * 3);
    assert_eq!(refusal(&project, settings), EmptyExport::Range);
}

#[test]
fn a_range_wider_than_the_timeline_is_clamped_not_refused() {
    let project = one_second();
    let settings = ranged(&project, 0, FRAMES * 4);
    assert_eq!(
        export_frame_count(&project, settings).expect("a clamped range still has frames"),
        FRAMES as u64
    );
}

#[test]
fn one_frame_is_a_range_worth_exporting() {
    let project = one_second();
    let settings = ranged(&project, 7, 8);
    assert_eq!(
        export_frame_count(&project, settings).expect("one frame is exportable"),
        1
    );
}

/// The span the caller is refused on is the span the frame loop would have run,
/// so a passing count and the loop's own arithmetic cannot disagree.
#[test]
fn the_count_is_the_span_the_frame_loop_walks() {
    let project = one_second();
    let settings = ranged(&project, 4, 19);
    let (start, end) = settings.frame_span(&project);
    assert_eq!(
        export_frame_count(&project, settings).expect("a mid-timeline range is exportable"),
        (end - start) as u64
    );
}

/// Every reason says something of its own, and says it inside the refusal.
/// Derived from [`EmptyExport::ALL`], so a new reason that reuses another's
/// words, or has none, fails here rather than in the export dialog.
#[test]
fn every_reason_names_its_own_way_out() {
    let mut seen: Vec<String> = Vec::new();
    for reason in EmptyExport::ALL {
        let text = reason.to_string();
        assert!(text.len() > 20, "{reason:?} explains nothing: {text}");
        assert!(
            text.contains('—'),
            "{reason:?} names no corrective action: {text}"
        );
        assert!(
            !seen.contains(&text),
            "{reason:?} reuses another reason's words: {text}"
        );
        let wrapped = RenderError::Empty(reason).to_string();
        assert!(
            wrapped.contains(&text),
            "the refusal drops the reason: {wrapped}"
        );
        seen.push(text);
    }
    assert_eq!(seen.len(), EmptyExport::ALL.len());
}

/// `resolved_for_encoder` validates dimensions, rate, and span without consulting
/// the platform encoder capability table, and forces `audio = false` when the
/// encoder does not support audio.
#[test]
fn resolved_for_encoder_respects_audio_capability_and_validates_bounds() {
    let project = one_second();
    let mut fmt = ExportFormat::default();
    fmt.audio = Some(true);
    fmt.size = Some((640, 361));

    let with_audio =
        ExportSettings::resolved_for_encoder(&project, fmt, true).expect("valid settings");
    assert!(with_audio.audio);
    assert_eq!(with_audio.size, (640, 360), "dimensions are evened");

    let without_audio =
        ExportSettings::resolved_for_encoder(&project, fmt, false).expect("valid settings");
    assert!(
        !without_audio.audio,
        "audio must be disabled when encoder does not support audio"
    );

    let bad_size = ExportFormat {
        size: Some((1, 100)),
        ..ExportFormat::default()
    };
    assert!(matches!(
        ExportSettings::resolved_for_encoder(&project, bad_size, true),
        Err(RenderError::Format(ExportFormatError::Size {
            asked: (1, 100)
        }))
    ));
}

/// A one-second project whose pool holds `media` and nothing on its timeline
/// that reads it, so a refusal can only come from the destination check.
fn with_media(media: &std::path::Path) -> Project {
    let mut project = one_second();
    project.add_media(aria_models::MediaSource::new(
        media, 64, 64, FPS, FRAMES, true,
    ));
    project
}

fn destination_refusal(result: Result<(), RenderError>) -> Option<ExportInput> {
    match result {
        Ok(()) => None,
        Err(RenderError::DestinationIsInput { what, .. }) => Some(what),
        Err(other) => panic!("expected the destination check, got {other:?}"),
    }
}

/// WHY: `aria export p.aria clip.mp4` with `clip.mp4` in the pool, and
/// `aria export p.aria p.aria`, exited 0 after replacing the footage or the
/// project with the export. The check is the one owner of "is this destination
/// an input", every spelling of one file matches, and a file that is not an
/// input, or a destination that does not exist yet, passes. Not caught: a
/// hard link to a source, which publishing leaves intact anyway.
#[test]
fn a_destination_that_is_an_input_is_refused_in_every_spelling() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).expect("sub");
    let media = dir.path().join("clip.mp4");
    let project_file = dir.path().join("p.aria");
    let other = dir.path().join("other.mp4");
    for file in [&media, &project_file, &other] {
        std::fs::write(file, b"input").expect("write input");
    }
    let project = with_media(&media);
    let check = |destination: &std::path::Path| {
        destination_refusal(check_export_destination(
            &project,
            destination,
            Some(&project_file),
        ))
    };

    assert_eq!(check(&media), Some(ExportInput::Media));
    assert_eq!(check(&sub.join("../clip.mp4")), Some(ExportInput::Media));
    assert_eq!(check(&project_file), Some(ExportInput::Project));
    assert_eq!(check(&sub.join("./../p.aria")), Some(ExportInput::Project));
    #[cfg(unix)]
    {
        let link = dir.path().join("link.mp4");
        std::os::unix::fs::symlink(&media, &link).expect("symlink");
        assert_eq!(check(&link), Some(ExportInput::Media));
    }
    assert_eq!(check(&other), None, "a file the export does not read");
    assert_eq!(check(&dir.path().join("new.mp4")), None, "a new file");
    assert_eq!(
        destination_refusal(check_export_destination(&project, &project_file, None)),
        None,
        "without a project file only the media are inputs"
    );

    let message = check_export_destination(&project, &media, None)
        .expect_err("media")
        .to_string();
    assert_eq!(
        message,
        format!(
            "{} is media this project uses; export to another file",
            media.display()
        )
    );
}

/// Every file publisher runs the check before it opens an encoder or renders,
/// and the input is left byte for byte. The sound export needs no GPU; the
/// video and frame exports run where a headless renderer opens.
#[test]
fn every_file_publisher_leaves_an_input_destination_as_it_was() {
    let dir = tempfile::tempdir().expect("tempdir");
    let media = dir.path().join("clip.mp4");
    std::fs::write(&media, b"the footage").expect("write media");
    let project = with_media(&media);
    let settings = ExportSettings::for_project(&project);
    let unchanged = |what: &str| {
        assert_eq!(
            std::fs::read(&media).expect("read media"),
            b"the footage",
            "{what} changed the input"
        );
    };

    let sound = crate::export_audio_to_file_observed(&project, &media, settings, &mut |_, _| {
        panic!("the sound export started before the destination check")
    });
    assert!(
        matches!(
            sound,
            Err(RenderError::DestinationIsInput {
                what: ExportInput::Media,
                ..
            })
        ),
        "sound: {sound:?}"
    );
    unchanged("the sound export");

    let Ok(mut renderer) = crate::Renderer::new_headless() else {
        return;
    };
    let video = export_to_file_with_encoder_observed(
        &mut renderer,
        &project,
        &media,
        settings,
        &mut |_, _| panic!("the video export opened an encoder before the destination check"),
        &mut |_, _| true,
    );
    assert!(
        matches!(
            video,
            Err(RenderError::DestinationIsInput {
                what: ExportInput::Media,
                ..
            })
        ),
        "video: {video:?}"
    );
    unchanged("the video export");

    let frame = crate::export_frame_png(&mut renderer, &project, RationalTime::new(0, FPS), &media);
    assert!(
        matches!(
            frame,
            Err(RenderError::DestinationIsInput {
                what: ExportInput::Media,
                ..
            })
        ),
        "frame: {frame:?}"
    );
    unchanged("the frame export");
}
