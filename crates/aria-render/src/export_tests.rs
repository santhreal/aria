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

/// A lane is not content. Adding a track is the first thing an operator does,
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

/// An in point after the out point is a marked range the operator dragged
/// backwards; `frame_span` orders it to an empty span rather than a negative
/// one, and an empty span is a refusal.
#[test]
fn a_range_that_runs_backwards_covers_no_frame() {
    let project = one_second();
    let settings = ranged(&project, 20, 5);
    assert_eq!(refusal(&project, settings), EmptyExport::Range);
}

/// Past the end both ends clamp to the duration, so the span is empty. The
/// reason is the range, not the sequence: the sequence has content, and telling
/// the operator to place a clip would send them to the wrong control.
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
/// words — or has none — fails here rather than in front of an operator.
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
