//! Tests for this module.
//!
//! The cases live in sibling `tests_*.rs` files, one per area, declared at the
//! bottom. This file holds only what every area needs: the constants and the
//! fixture builders. A child sees them through `use super::*`.

use super::*;
use aria_models::MediaSource;

const R24: Rational = Rational::FPS_24;

/// 24 fps project: one video track with a 10 s media clip at 0 s, one
/// text track with a title from 2 s to 5 s, and a 60 s media source.
fn fixture() -> (Project, u64, u64, u64, u64, u64) {
    let mut project = Project::new("fixture", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/agent-fixture.mp4",
        1920,
        1080,
        R24,
        60 * 24,
        true,
    ));
    let video = project.add_track(TrackKind::Video, "V1");
    let text = project.add_track(TrackKind::Text, "Titles");
    let clip = project
        .add_clip(
            video,
            media,
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let title = project
        .add_generated(
            text,
            Generator::text("INTRO"),
            TimeRange::at_rate(48, 72, R24),
        )
        .unwrap();
    (
        project,
        media.raw(),
        video.raw(),
        text.raw(),
        clip.raw(),
        title.raw(),
    )
}

fn lower(project: &Project, cmd: WireCommand) -> EditCommand {
    match validate(&cmd, project).expect("command should validate") {
        Lowered::Edit(Command::Edit(edit)) => edit,
        other => panic!("expected edit, got {other:?}"),
    }
}

fn reject(project: &Project, cmd: WireCommand) -> String {
    validate(&cmd, project)
        .expect_err("command should be rejected")
        .message
}

#[path = "tests_position_tangents.rs"]
mod position_tangents;

#[path = "tests_add_clip.rs"]
mod add_clip;

#[path = "tests_clip_crop.rs"]
mod clip_crop;

#[path = "tests_set_clip.rs"]
mod set_clip;

#[path = "tests_look_commands.rs"]
mod look_commands;

#[path = "tests_duplicate_clip.rs"]
mod duplicate_clip;

#[path = "tests_set_canvas.rs"]
mod set_canvas;

#[path = "tests_apply_easing.rs"]
mod apply_easing;

#[path = "tests_lane_census.rs"]
mod lane_census;

#[path = "tests_grade_census.rs"]
mod grade_census;

#[path = "tests_routing_census.rs"]
mod routing_census;

#[path = "tests_lookup_bounds.rs"]
mod lookup_bounds;
