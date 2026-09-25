//! Export: what `ProjectCommand::Export` writes, and what it refuses.
//!
//! The class this closes: every other engine test stops at project state. Export
//! is the one command whose result is a file — it renders every frame of the
//! sequence through the compositor and muxes them — so a timeline that edits
//! correctly and exports nothing, exports the wrong length, or writes a file no
//! decoder can open passes every other test in this crate.
//!
//! Refusals run everywhere: an empty sequence is rejected before a frame is
//! rendered, so those cases need neither GPU nor encoder. The cases that write a
//! file are skipped where [`aria_encoder::capability`] reports no backend, which
//! is a platform fact rather than a failure; where there is one, they run and a
//! failure is real.
//!
//! What is asserted about a written file: the frame count the command reports,
//! that a probe can read it back, and the duration and dimensions that probe
//! finds. Not the pixels — a readback of the composited canvas is the renderer's
//! test, and asking a lossy H.264 file about a colour proves the encoder's
//! chroma handling rather than the timeline's.

mod common;

use std::path::Path;

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_models::{Generator, TrackKind};
use common::{
    add_generated, add_media_clip, add_track, export_to, import_asset, rt, temp_engine, tr,
};

/// Whether this platform can write a file at all. A refusal test does not need
/// this; a test that reads back what was written does.
fn can_encode() -> bool {
    aria_encoder::capability().available
}

/// The exported file, as the importer sees it: how many frames the container
/// reports, at what rate, and at what size.
///
/// A frame count rather than a millisecond duration: the count is the same
/// number the export command reports, so the two can be compared without a
/// tolerance window standing in for the claim.
fn probed(path: &Path) -> aria_decoder::MediaProbe {
    aria_decoder::probe(path).unwrap_or_else(|e| panic!("probe export {}: {e}", path.display()))
}

/// The canvas the sequence renders at, which is the size the file must carry.
fn canvas(engine: &aria_engine::Engine) -> (u32, u32) {
    aria_render::canvas_size(engine.project())
}

/// A generated 24-frame second on its own track, so a case needs no media file.
fn one_second(engine: &mut aria_engine::Engine) {
    let track = add_track(engine, TrackKind::Sticker, "T1");
    add_generated(
        engine,
        track,
        Generator::SolidColor {
            rgba: [10, 120, 200, 255],
        },
        tr(0, 24),
    );
}

/// `ProjectCommand::Export` with no choices made: the project's own output.
fn export(path: &std::path::Path) -> Command {
    Command::Project(ProjectCommand::Export {
        path: path.to_path_buf(),
        format: aria_commands::ExportFormat::PROJECT,
    })
}

#[test]
fn an_empty_sequence_is_refused_before_anything_is_written() {
    let (dir, mut engine) = temp_engine();
    let out = dir.path().join("empty.mp4");
    let err = engine
        .apply(export(&out))
        .expect_err("an empty sequence has nothing to export");
    assert!(
        format!("{err}").contains("no content"),
        "the refusal must say what is missing, got: {err}"
    );
    assert!(
        !out.exists(),
        "a refused export left a file behind: {}",
        out.display()
    );
}

#[test]
fn a_track_with_no_clips_is_refused_the_same_way() {
    let (dir, mut engine) = temp_engine();
    let _track = add_track(&mut engine, TrackKind::Video, "V1");
    let out = dir.path().join("track_only.mp4");
    let err = engine
        .apply(export(&out))
        .expect_err("a lane with no clips has nothing to export");
    assert!(format!("{err}").contains("no content"), "got: {err}");
}

/// A codec the installed backend does not write is refused by name, rather than
/// swapped for one that happens to work: a script that asked for HEVC and got
/// H.264 has no way to notice.
#[test]
fn a_codec_this_machine_cannot_write_is_refused_by_name() {
    let capability = aria_encoder::capability();
    let Some(missing) = aria_commands::VideoCodec::ALL
        .into_iter()
        .find(|codec| !capability.codecs.contains(codec))
    else {
        return; // this machine writes every codec Aria offers
    };
    let (dir, mut engine) = temp_engine();
    one_second(&mut engine);
    let out = dir.path().join("wrong-codec.mp4");
    let err = engine
        .apply(Command::Project(ProjectCommand::Export {
            path: out.clone(),
            format: aria_commands::ExportFormat {
                video_codec: Some(missing),
                ..aria_commands::ExportFormat::PROJECT
            },
        }))
        .expect_err("a codec the backend cannot write is refused");
    let text = format!("{err}");
    assert!(text.contains(missing.label()), "got: {text}");
    assert!(!out.exists(), "a refused export wrote a file");
}

/// The format reaches the encoder: a half-size export writes half-size frames.
#[test]
fn a_requested_size_is_what_the_file_carries() {
    if !can_encode() {
        return;
    }
    let (dir, mut engine) = temp_engine();
    one_second(&mut engine);
    let (w, h) = canvas(&engine);
    let out = dir.path().join("half.mp4");
    engine
        .apply(Command::Project(ProjectCommand::Export {
            path: out.clone(),
            format: aria_commands::ExportFormat {
                size: Some((w / 2, h / 2)),
                ..aria_commands::ExportFormat::PROJECT
            },
        }))
        .expect("a half-size export");
    let probe = probed(&out);
    assert_eq!(
        (probe.width, probe.height),
        ((w / 2) & !1, (h / 2) & !1),
        "the requested size is what was written"
    );
}

/// A range on the command renders only that span, the same as the sheet's.
#[test]
fn a_requested_range_shortens_the_file() {
    if !can_encode() {
        return;
    }
    let (dir, mut engine) = temp_engine();
    one_second(&mut engine);
    let out = dir.path().join("ranged.mp4");
    engine
        .apply(Command::Project(ProjectCommand::Export {
            path: out.clone(),
            format: aria_commands::ExportFormat {
                range: Some((rt(6), rt(18))),
                ..aria_commands::ExportFormat::PROJECT
            },
        }))
        .expect("a ranged export");
    assert_eq!(probed(&out).frame_count, 12, "the span is what was written");
}

#[test]
fn a_destination_whose_parent_does_not_exist_is_refused() {
    let (dir, mut engine) = temp_engine();
    one_second(&mut engine);
    let out = dir.path().join("no-such-directory").join("out.mp4");
    let err = engine
        .apply(export(&out))
        .expect_err("a missing parent directory is not created");
    let text = format!("{err}");
    assert!(
        !text.is_empty() && !out.exists(),
        "expected a refusal naming the path, got: {text}"
    );
}

#[test]
fn a_generated_second_exports_twenty_four_frames_a_decoder_can_read() {
    if !can_encode() {
        return;
    }
    let (dir, mut engine) = temp_engine();
    one_second(&mut engine);
    let out = dir.path().join("solid.mp4");
    assert_eq!(export_to(&mut engine, &out), 24, "one second at 24 fps");

    let probe = probed(&out);
    assert_eq!(
        probe.frame_count, 24,
        "the file must carry the frames the command reported"
    );
    assert_eq!(
        (probe.width, probe.height),
        canvas(&engine),
        "the file must carry the sequence's canvas, not the encoder's default"
    );
}

#[test]
fn a_longer_sequence_exports_proportionally_more_frames() {
    if !can_encode() {
        return;
    }
    let (dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Sticker, "T1");
    add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [200, 30, 30, 255],
        },
        tr(0, 72),
    );
    let out = dir.path().join("three-seconds.mp4");
    assert_eq!(export_to(&mut engine, &out), 72);
    assert_eq!(
        probed(&out).frame_count,
        72,
        "three seconds at 24 fps is 72 frames in the container too"
    );
}

/// A gap between two clips is exported, not skipped.
///
/// The frame count is the claim: a renderer that dropped the empty stretch would
/// write a shorter file, and the two clips would abut in the export while sitting
/// apart in the timeline.
#[test]
fn a_gap_between_clips_is_exported_as_frames() {
    if !can_encode() {
        return;
    }
    let (dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Sticker, "T1");
    add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [255, 255, 255, 255],
        },
        tr(0, 12),
    );
    add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [255, 255, 255, 255],
        },
        tr(36, 12),
    );
    let out = dir.path().join("gap.mp4");
    assert_eq!(
        export_to(&mut engine, &out),
        48,
        "two 12-frame clips 24 frames apart span 48 frames"
    );
}

#[test]
fn the_export_length_follows_the_track_that_ends_last() {
    if !can_encode() {
        return;
    }
    let (dir, mut engine) = temp_engine();
    let short = add_track(&mut engine, TrackKind::Sticker, "T1");
    let long = add_track(&mut engine, TrackKind::Sticker, "T2");
    add_generated(
        &mut engine,
        short,
        Generator::SolidColor {
            rgba: [0, 0, 255, 255],
        },
        tr(0, 12),
    );
    add_generated(
        &mut engine,
        long,
        Generator::SolidColor {
            rgba: [0, 255, 0, 255],
        },
        tr(0, 36),
    );
    let out = dir.path().join("longest.mp4");
    assert_eq!(export_to(&mut engine, &out), 36);
}

#[test]
fn an_imported_clip_exports_at_the_sequence_canvas() {
    if !can_encode() {
        return;
    }
    let Some(path) = common::small_video_asset() else {
        return;
    };
    let (dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &path);
    let track = add_track(&mut engine, TrackKind::Video, "V1");
    add_media_clip(&mut engine, track, media, tr(0, 24), rt(0));

    let out = dir.path().join("imported.mp4");
    assert_eq!(export_to(&mut engine, &out), 24);
    let probe = probed(&out);
    assert_eq!((probe.width, probe.height), canvas(&engine));
}

/// A trimmed clip exports what is left of it, not what it was.
#[test]
fn trimming_shortens_the_export() {
    if !can_encode() {
        return;
    }
    let (dir, mut engine) = temp_engine();
    let track = add_track(&mut engine, TrackKind::Sticker, "T1");
    let clip = add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [90, 90, 90, 255],
        },
        tr(0, 48),
    );
    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip,
            timeline: tr(0, 24),
        }))
        .expect("trim the tail");

    let out = dir.path().join("trimmed.mp4");
    assert_eq!(export_to(&mut engine, &out), 24, "the trim is what exports");
}

/// Export is not an edit: it must not become an undo step.
///
/// Cheap to get wrong, because every other `ProjectCommand` that touches the
/// project does push one, and a stray entry would make Ctrl+Z after an export
/// undo the last real edit instead.
#[test]
fn exporting_pushes_no_undo_entry() {
    if !can_encode() {
        return;
    }
    let (dir, mut engine) = temp_engine();
    one_second(&mut engine);
    let before = engine.can_undo();
    let out = dir.path().join("undo.mp4");
    export_to(&mut engine, &out);
    assert_eq!(
        engine.can_undo(),
        before,
        "an export changed what undo would do"
    );
}

#[test]
fn exporting_twice_to_one_path_overwrites_it() {
    if !can_encode() {
        return;
    }
    let (dir, mut engine) = temp_engine();
    one_second(&mut engine);
    let out = dir.path().join("twice.mp4");
    assert_eq!(export_to(&mut engine, &out), 24);
    let first = std::fs::metadata(&out).expect("the first export").len();
    assert_eq!(export_to(&mut engine, &out), 24);
    let second = std::fs::metadata(&out).expect("the second export").len();
    assert!(
        first > 0 && second > 0,
        "an overwrite left an empty file: {first} then {second} bytes"
    );
    probed(&out);
}
