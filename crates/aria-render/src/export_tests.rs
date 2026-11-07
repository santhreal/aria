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

/// WHY: `aria export p.aria out.mp4 --bitrate 0` exited 0 with a file at
/// nvh264enc's own default bitrate, which reads a zero bitrate as "use the
/// preset". A target of zero is refused before an encoder opens, for every
/// caller of the resolution; one kbit/s and an unset bitrate pass.
///
/// Not caught: a bitrate above zero that an encoder clamps to its range; the
/// encoder's range check reports that one.
#[test]
fn a_zero_bitrate_is_refused_and_any_other_passes() {
    let project = one_second();
    for (bitrate_kbps, refused) in [(Some(0), true), (Some(1), false), (None, false)] {
        let format = ExportFormat {
            bitrate_kbps,
            ..ExportFormat::default()
        };
        let resolved = ExportSettings::resolved_for_encoder(&project, format, true);
        assert_eq!(
            matches!(
                resolved,
                Err(RenderError::Format(ExportFormatError::ZeroBitrate))
            ),
            refused,
            "{bitrate_kbps:?}: {resolved:?}"
        );
        if !refused {
            assert_eq!(resolved.expect("resolves").bitrate_kbps, bitrate_kbps);
        }
    }
}

/// One frame of a solid in a project whose sequence runs at `rate`.
fn one_frame_at(rate: Rational) -> Project {
    let mut project = Project::new("export-rate", rate);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [10, 120, 200, 255],
            },
            TimeRange::at_rate(0, 1, rate),
        )
        .expect("place the solid generator");
    project
}

/// WHY: only `aria export --fps` held a rate to [`MAX_FPS`]. A project at
/// 2000 fps, written by a plugin, a script or an imported cut, exported
/// unchecked through every other path, and above the ceiling `nvh264enc`
/// writes the last frame a `1001/30000` s duration. The rate the frames are
/// sampled at, asked for or the project's own, is refused at resolve and at
/// the frame count every export pass reads, with the ceiling in the message.
///
/// Not caught: a rate at or below the ceiling that one encoder still writes
/// wrongly; the encoder suites own that.
#[test]
fn a_rate_no_encoder_writes_is_refused_on_every_export_path() {
    let max = crate::MAX_FPS;
    let rates = [
        (Rational::new(max, 1), true),
        (Rational::new(2 * max, 2), true),
        (Rational::FPS_29_97, true),
        (Rational::new(max + 1, 1), false),
        (Rational::new(2 * max + 1, 2), false),
        (Rational::new(i32::MAX, 1), false),
        (Rational::new(0, 1), false),
        (Rational::new(-24, 1), false),
        (Rational::new(24, 0), false),
        (Rational::new(24, -1), false),
    ];
    fn refused<T>(result: &Result<T, RenderError>, rate: Rational) -> bool {
        matches!(
            result,
            Err(RenderError::Format(ExportFormatError::Rate { asked })) if *asked == rate
        )
    }
    let project = one_second();
    for (rate, writes) in rates {
        assert_eq!(exportable_rate(rate), writes, "{rate:?}");
        let format = ExportFormat {
            frame_rate: Some(rate),
            ..ExportFormat::default()
        };
        let asked = ExportSettings::resolved_for_encoder(&project, format, true);
        assert_eq!(refused(&asked, rate), !writes, "asked {rate:?}: {asked:?}");
        if writes {
            assert_eq!(asked.expect("resolves").frame_rate, rate);
        }
        if !rate.is_valid() {
            continue;
        }
        let own = one_frame_at(rate);
        let resolved = ExportSettings::resolved_for_encoder(&own, ExportFormat::default(), true);
        assert_eq!(
            refused(&resolved, rate),
            !writes,
            "own {rate:?}: {resolved:?}"
        );
        let counted = export_frame_count(&own, ExportSettings::for_project(&own));
        assert_eq!(
            refused(&counted, rate),
            !writes,
            "count {rate:?}: {counted:?}"
        );
    }
    assert_eq!(
        RenderError::from(ExportFormatError::Rate {
            asked: Rational::new(2000, 1)
        })
        .to_string(),
        "2000/1 fps is not a frame rate an export writes; use a rate above 0 and at most 1000 fps"
    );
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

/// The source file and a project that reads it: the one-second solid, then
/// the file on V1 and on A1 over frames 30..40. The file is not written, so it
/// is missing until a test writes it.
struct Offline {
    _dir: tempfile::TempDir,
    media: std::path::PathBuf,
    project: Project,
    picture: aria_models::TrackId,
    sound: aria_models::TrackId,
}

fn offline() -> Offline {
    let dir = tempfile::tempdir().expect("tempdir");
    let media = dir.path().join("gone.mp4");
    let mut project = one_second();
    let id = project.add_media(aria_models::MediaSource::new(
        &media, 64, 64, FPS, FRAMES, true,
    ));
    let picture = project.add_track(TrackKind::Video, "V1");
    let sound = project.add_track(TrackKind::Audio, "A1");
    for lane in [picture, sound] {
        project
            .add_clip(
                lane,
                id,
                TimeRange::at_rate(0, 10, FPS),
                RationalTime::new(FRAMES, FPS),
            )
            .expect("place the file");
    }
    Offline {
        _dir: dir,
        media,
        project,
        picture,
        sound,
    }
}

/// The file the check names, or `None` when it passes.
fn gone(result: Result<(), RenderError>) -> Option<std::path::PathBuf> {
    match result {
        Ok(()) => None,
        Err(RenderError::MediaOffline { path }) => Some(path),
        Err(other) => panic!("expected the media check, got {other:?}"),
    }
}

/// WHY: an export over a clip whose file was moved or deleted queued a job
/// that failed at its first frame with "decode error: failed to open source:
/// couldn't resolve ...". The check names the file before anything opens, for
/// every lane and span the export reads, and passes every one it does not:
/// a span that ends where the clip starts, a disabled picture lane, a muted
/// or switched-off sound lane, and picture lanes in a sound export.
#[test]
fn a_missing_source_is_refused_where_the_export_reads_it_and_only_there() {
    let Offline {
        _dir,
        media,
        mut project,
        picture,
        sound,
    } = offline();
    let mut whole = ExportSettings::for_project(&project);
    whole.audio = true;
    let span = |from, to| {
        let mut settings = ranged(&project, from, to);
        settings.audio = true;
        settings
    };

    assert_eq!(
        gone(check_export_media(&project, whole, true)),
        Some(media.clone())
    );
    assert_eq!(
        check_export_media(&project, whole, true)
            .expect_err("missing")
            .to_string(),
        format!(
            "{} is missing; move the file back or relink it in the media pool",
            media.display()
        )
    );
    assert_eq!(gone(check_export_media(&project, span(0, 30), true)), None);
    assert_eq!(
        gone(check_export_media(&project, span(29, 31), true)),
        Some(media.clone())
    );
    assert_eq!(
        gone(check_export_media(&project, span(39, 40), true)),
        Some(media.clone()),
        "the clip's last frame"
    );

    project
        .timeline_mut()
        .track_mut(picture)
        .expect("V1")
        .enabled = false;
    assert_eq!(
        gone(check_export_media(&project, whole, true)),
        Some(media.clone()),
        "A1 still reads the file"
    );
    assert_eq!(
        gone(check_export_media(&project, whole.without_audio(), true)),
        None,
        "a disabled picture lane in a video-only export"
    );
    project
        .timeline_mut()
        .track_mut(picture)
        .expect("V1")
        .enabled = true;
    assert_eq!(
        gone(check_export_media(&project, whole.without_audio(), true)),
        Some(media.clone()),
        "an enabled picture lane"
    );
    assert_eq!(
        gone(check_export_media(&project, whole, false)),
        Some(media.clone()),
        "a sound export reads A1"
    );
    project.timeline_mut().track_mut(sound).expect("A1").muted = true;
    assert_eq!(
        gone(check_export_media(&project, whole, false)),
        None,
        "a sound export reads no picture lane and no muted one"
    );

    std::fs::write(&media, b"footage").expect("write the file back");
    assert_eq!(gone(check_export_media(&project, whole, true)), None);
}

/// WHY: a nest reads the files of the sequence it holds, and that sequence
/// can hold another nest. A check that read only the open timeline passed an
/// export whose only file was two nests down. Not caught: a nest's own span
/// is not narrowed to the part of the sequence the nest plays.
#[test]
fn a_missing_source_inside_a_nest_is_refused_at_any_depth() {
    let dir = tempfile::tempdir().expect("tempdir");
    let media = dir.path().join("deep.mp4");
    let mut project = one_second();
    let id = project.add_media(aria_models::MediaSource::new(
        &media, 64, 64, FPS, FRAMES, true,
    ));
    let ten = TimeRange::at_rate(0, 10, FPS);
    let start = RationalTime::new(0, FPS);

    let inner = project.create_sequence("inner", FPS);
    project.set_active_sequence(Some(inner)).expect("inner");
    let lane = project.add_track(TrackKind::Video, "V1");
    project.add_clip(lane, id, ten, start).expect("the file");

    let outer = project.create_sequence("outer", FPS);
    project.set_active_sequence(Some(outer)).expect("outer");
    let lane = project.add_track(TrackKind::Video, "V1");
    project
        .add_sequence_clip(lane, inner, ten, start)
        .expect("inner in outer");

    project.set_active_sequence(None).expect("root");
    let lane = project.add_track(TrackKind::Video, "V2");
    project
        .add_sequence_clip(lane, outer, ten, start)
        .expect("outer on the root");

    let settings = ExportSettings::for_project(&project);
    assert_eq!(
        gone(check_export_media(&project, settings, true)),
        Some(media.clone())
    );
    std::fs::write(&media, b"footage").expect("write the file");
    assert_eq!(gone(check_export_media(&project, settings, true)), None);
}

/// WHY: a multicam clip reads the angle its cut list makes live, and the check
/// read only plain media clips, so an export over a cut to a camera whose file
/// was gone encoded every frame up to the cut and then failed. The check names
/// a missing angle live anywhere in the clip's source window and passes one
/// that is live only outside it: before the window, or from a cut on the
/// window's end. Not caught: a window wider than the part of the clip the
/// exported span plays.
#[test]
fn a_missing_multicam_angle_is_refused_where_the_clip_makes_it_live() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (wide, close) = (dir.path().join("wide.mp4"), dir.path().join("close.mp4"));
    std::fs::write(&wide, b"footage").expect("write the wide angle");
    let mut project = one_second();
    let ids = [&wide, &close].map(|path| {
        project.add_media(aria_models::MediaSource::new(
            path, 64, 64, FPS, FRAMES, false,
        ))
    });
    let angles = ids.map(|media| aria_models::MulticamAngle::new(media, 0));
    let group = project
        .create_multicam_group("cams", angles.to_vec())
        .expect("group");
    project
        .multicam_group_mut(group)
        .expect("the group")
        .switch_angle(10, 1)
        .expect("cut to close");
    let lane = project.add_track(TrackKind::Video, "V1");
    let at = RationalTime::new(0, FPS);
    let place = |project: &mut Project, from: i64, len: i64| {
        let clip = project
            .add_multicam_clip(lane, group, TimeRange::at_rate(from, len, FPS), at)
            .expect("place the multicam clip");
        let settings = ExportSettings::for_project(project);
        let result = gone(check_export_media(project, settings, true));
        project.remove_clip(clip).expect("lift the clip");
        result
    };

    assert_eq!(place(&mut project, 0, 20), Some(close.clone()));
    assert_eq!(place(&mut project, 12, 5), Some(close.clone()));
    assert_eq!(
        place(&mut project, 0, 10),
        None,
        "a cut on the window's end"
    );
    assert_eq!(place(&mut project, 0, 9), None);
    std::fs::write(&close, b"footage").expect("write the close angle");
    assert_eq!(place(&mut project, 0, 20), None);
}

/// Every file publisher runs the media check before it opens a staging file,
/// an encoder or a mix, so a missing source leaves no file behind and the
/// caller reads the file's name rather than a decoder's message.
#[test]
fn every_file_publisher_refuses_a_missing_source_before_it_opens_anything() {
    let Offline {
        _dir,
        media,
        project,
        ..
    } = offline();
    let out = media.with_file_name("out.mp4");
    let mut settings = ExportSettings::for_project(&project);
    settings.audio = true;

    let sound = crate::export_audio_to_file_observed(&project, &out, settings, &mut |_, _| {
        panic!("the sound export started before the media check")
    });
    assert!(
        matches!(&sound, Err(RenderError::MediaOffline { path }) if *path == media),
        "sound: {sound:?}"
    );
    assert!(!out.exists(), "the sound export wrote a file");

    let Ok(mut renderer) = crate::Renderer::new_headless() else {
        return;
    };
    let video = export_to_file_with_encoder_observed(
        &mut renderer,
        &project,
        &out,
        settings,
        &mut |_, _| panic!("the video export opened an encoder before the media check"),
        &mut |_, _| true,
    );
    assert!(
        matches!(&video, Err(RenderError::MediaOffline { path }) if *path == media),
        "video: {video:?}"
    );
    assert!(!out.exists(), "the video export wrote a file");
}

/// WHY: `aria export p.aria out.mp4 --size 99999x99999` opened the encoder
/// before anything rendered, and GStreamer, with no NV12 layout for the frame,
/// failed it with "encode error: unsupported: GStreamer has no NV12 layout for
/// a 99998x99998 frame". The export checks the size against the device before
/// the staging file or the encoder opens, for a side past the largest texture
/// on either axis. Not caught: a size the device renders and no encoder takes.
#[test]
fn a_size_past_the_device_is_refused_before_the_encoder_opens() {
    let Ok(mut renderer) = crate::Renderer::new_headless() else {
        return;
    };
    let gpu = renderer.gpu().expect("a headless renderer has its device");
    let past = gpu.device.limits().max_texture_dimension_2d + 2;
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("out.mp4");
    let project = one_second();
    for size in [(past, 16), (16, past), (99_998, 99_998)] {
        let mut settings = ranged(&project, 0, 2);
        settings.size = size;
        let result = export_to_file_with_encoder_observed(
            &mut renderer,
            &project,
            &out,
            settings,
            &mut |_, _| panic!("the export opened an encoder for {size:?}"),
            &mut |_, _| true,
        );
        assert!(
            matches!(
                &result,
                Err(RenderError::Compositor(
                    aria_compositor::CompositorError::CanvasTooLarge { width, height, .. }
                )) if (*width, *height) == size
            ),
            "{size:?}: {result:?}"
        );
        let left = std::fs::read_dir(dir.path())
            .expect("list the tempdir")
            .count();
        assert_eq!(left, 0, "{size:?} left a file beside the destination");
    }
}

/// WHY: an encoder can change while it encodes. The GStreamer encoder reopens
/// on the next candidate when a hardware encoder opens and then rejects the
/// first frame, and the export read the encoder's choice before the first
/// frame, so `aria export` reported the hardware encoder for a file the
/// software one wrote. Does not cover the reopen itself, which the
/// `aria-encoder` `session_limit` test covers.
#[test]
fn the_export_reports_the_encoder_that_wrote_the_file() {
    use aria_core::{EncodeError, EncoderChoice, VideoEncoder, VideoFrame};

    /// Reports a hardware encoder until it takes a frame, then the software
    /// one it moved to.
    struct Reopens {
        pushed: bool,
    }

    impl VideoEncoder for Reopens {
        fn push(&mut self, _: &VideoFrame) -> Result<(), EncodeError> {
            self.pushed = true;
            Ok(())
        }

        fn finish(&mut self) -> Result<(), EncodeError> {
            Ok(())
        }

        fn choice(&self) -> EncoderChoice {
            if !self.pushed {
                return EncoderChoice::only("hardware", Some(true));
            }
            EncoderChoice {
                encoder: "software".into(),
                hardware: Some(false),
                passed_over: vec!["hardware opened and could not encode".into()],
            }
        }
    }

    let Ok(mut renderer) = crate::Renderer::new_headless() else {
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("out.mp4");
    let project = one_second();
    let exported = export_to_file_with_encoder_observed(
        &mut renderer,
        &project,
        &out,
        ranged(&project, 0, 2),
        &mut |path, _| {
            std::fs::write(path, b"encoded").expect("write the staging file");
            Ok(Box::new(Reopens { pushed: false }))
        },
        &mut |_, _| true,
    )
    .expect("the export finishes");
    assert_eq!(exported.frames, 2);
    assert_eq!(
        exported.encoder.to_string(),
        "encoder software (software)\nskipped hardware opened and could not encode"
    );
}
