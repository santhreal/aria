//! Media fixtures are constant frame rate, and a fixture made by an older
//! recipe, or missing a file it shows, is made again rather than reused.
//!
//! Closes the class where looping an audio-and-video segment left a hole in
//! the video track at every loop (AAC padding makes the segment longer than
//! its video), which the av-sync and playback scenarios then measured as a
//! product defect. Does not check picture content or audio sample values.

use std::path::{Path, PathBuf};

use super::{
    FixtureId, MEDIA_FIXTURE_VERSION, PHOTO_SIZE, build_stack_4k, ensure_fixture, is_valid_fixture,
    loop_segment, photo_paths, version_path,
};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aria-bench-fixtures-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A looped fixture holds every frame of every loop: 3 loops of a 1 s, 60 fps
/// segment are 180 frames, with a tone the length of the video.
#[test]
fn a_looped_fixture_has_every_frame() {
    let dir = scratch("loop");
    let out = dir.join("looped.mp4");
    loop_segment(
        &dir.join(".seg.mp4"),
        "testsrc2=size=320x180:rate=60:duration=1",
        &[
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-g",
            "60",
            "-pix_fmt",
            "yuv420p",
        ],
        "2",
        &["-t", "3"],
        "3",
        &["-c:a", "aac", "-b:a", "128k"],
        &out,
        "test loop",
    )
    .expect("generate the looped fixture");

    let probed = aria_decoder::probe(&out).expect("probe the looped fixture");
    assert!(probed.has_audio, "the fixture has no audio track");
    // The container duration hides a hole; the decoded frames do not.
    let mut decoder = aria_decoder::open_video_decoder(&out, aria_decoder::OutputMode::Cpu)
        .expect("open the looped fixture");
    let mut times = Vec::new();
    while let Some(frame) = decoder.next_frame().expect("decode the looped fixture") {
        times.push(frame.pts.seconds());
    }
    assert_eq!(times.len(), 180, "a loop lost frames");
    for (index, pair) in times.windows(2).enumerate() {
        let step = pair[1] - pair[0];
        assert!(
            (step - 1.0 / 60.0).abs() < 0.5 / 60.0,
            "a {step:.4} s gap after frame {index}"
        );
    }
    assert!(
        !dir.join(".seg.mp4").exists(),
        "the segment was left behind"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn media_file(dir: &Path) -> PathBuf {
    let path = FixtureId::H264_1080p60.path(dir);
    std::fs::write(&path, vec![0u8; 200_000]).expect("write a media-sized file");
    path
}

/// A media fixture is reused only when its version file holds the current
/// recipe version: none, an older one, or garbage regenerates it.
#[test]
fn a_media_fixture_from_another_recipe_is_not_reused() {
    let dir = scratch("version");
    let path = media_file(&dir);
    let id = FixtureId::H264_1080p60;

    assert!(
        !is_valid_fixture(&dir, id, &path),
        "a fixture with no version file was reused"
    );
    for stale in ["1", "", "two"] {
        std::fs::write(version_path(&path), stale).expect("write version");
        assert!(
            !is_valid_fixture(&dir, id, &path),
            "a fixture at version {stale:?} was reused"
        );
    }
    std::fs::write(version_path(&path), MEDIA_FIXTURE_VERSION.to_string()).expect("write version");
    assert!(
        is_valid_fixture(&dir, id, &path),
        "a current fixture was not reused"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A project fixture is reused only when this build's schema wrote it: one an
/// older build wrote opens as a migrated project, whose save copies the whole
/// timeline to restamp it, so `project-io` measured a migration, not a save.
#[test]
fn a_project_fixture_from_an_older_schema_is_not_reused() {
    let dir = scratch("schema");
    let id = FixtureId::Stack4k;
    let path = id.path(&dir);
    build_stack_4k(&dir)
        .expect("build the stack-4k project")
        .save_to_file(&path)
        .expect("save the stack-4k project");
    assert!(
        is_valid_fixture(&dir, id, &path),
        "a fixture this build wrote was not reused"
    );

    let mut doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read the fixture")).expect("json");
    doc["schema"]["version"] = (aria_models::PROJECT_SCHEMA_VERSION - 1).into();
    std::fs::write(&path, serde_json::to_vec_pretty(&doc).expect("json")).expect("write");
    assert!(
        aria_models::Project::load_from_file(&path).is_ok(),
        "the restamped fixture does not open, so the check below proves nothing"
    );
    assert!(
        !is_valid_fixture(&dir, id, &path),
        "a fixture an older schema wrote was reused"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The stills fixture is reused only with every photo it shows on disk, and
/// each photo decodes at the size its project states, so `playback` measures
/// 12 MP pictures rather than a missing file drawn as nothing.
#[test]
fn a_stills_fixture_missing_a_photo_is_made_again() {
    let dir = scratch("stills");
    let id = FixtureId::Stills4k;
    let made = ensure_fixture(&dir, id).expect("make the stills-4k fixture");
    assert!(!made.cached, "an empty directory held a fixture");
    let photos = photo_paths(&dir);
    let project = aria_models::Project::load_from_file(&id.path(&dir)).expect("open stills-4k");
    let shown: Vec<_> = project
        .media_iter()
        .map(|media| (media.path().to_path_buf(), media.width, media.height))
        .collect();
    for photo in &photos {
        let image = aria_decoder::decode_image(photo).expect("decode a stills-4k photo");
        assert_eq!(
            [image.width, image.height],
            PHOTO_SIZE,
            "{}",
            photo.display()
        );
        assert!(
            shown.contains(&(photo.clone(), PHOTO_SIZE[0], PHOTO_SIZE[1])),
            "{} is not in the project at its size: {shown:?}",
            photo.display()
        );
    }
    assert!(
        ensure_fixture(&dir, id).expect("reuse stills-4k").cached,
        "a complete fixture was made again"
    );

    std::fs::remove_file(&photos[1]).expect("remove a photo");
    assert!(
        !is_valid_fixture(&dir, id, &id.path(&dir)),
        "a fixture missing a photo was reused"
    );
    assert!(!ensure_fixture(&dir, id).expect("remake stills-4k").cached);
    assert!(photos[1].exists(), "the missing photo was not made again");
    let _ = std::fs::remove_dir_all(&dir);
}
