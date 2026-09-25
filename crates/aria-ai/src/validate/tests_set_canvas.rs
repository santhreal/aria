//! `set_canvas` cases.

use super::*;
use crate::wire;

#[test]
fn shift_rejects_sub_frame_delta_and_link_lists_are_bounded() {
    let (project, _, video, _, clip, _) = fixture();
    let msg = reject(
        &project,
        WireCommand::ShiftClips(wire::ShiftClips {
            track: video,
            from: 0.0,
            delta: 0.001,
        }),
    );
    assert!(msg.contains("rounds to zero frames"), "{msg}");

    let msg = reject(
        &project,
        WireCommand::LinkClips(wire::LinkClips { clips: vec![clip] }),
    );
    assert!(msg.contains("at least two"), "{msg}");

    let msg = reject(
        &project,
        WireCommand::LinkClips(wire::LinkClips {
            clips: vec![clip; MAX_MULTI_CLIP_REFS + 1],
        }),
    );
    assert!(msg.contains("at most 64"), "{msg}");
}

#[test]
fn unlink_one_member_lowers_to_the_complete_group_command() {
    let (mut project, _, _, _, clip, title) = fixture();
    let link = aria_models::LinkId::next();
    for id in [clip, title] {
        project
            .timeline_mut()
            .clip_mut(ClipId::from_raw(id))
            .unwrap()
            .link = Some(link);
    }

    let edit = lower(
        &project,
        WireCommand::UnlinkClips(wire::UnlinkClips { clips: vec![title] }),
    );
    assert_eq!(
        edit,
        EditCommand::UnlinkClips {
            clips: vec![ClipId::from_raw(title)]
        }
    );
    assert_eq!(
        project.clip(ClipId::from_raw(clip)).unwrap().link,
        Some(link)
    );
    assert_eq!(
        project.clip(ClipId::from_raw(title)).unwrap().link,
        Some(link)
    );
}

#[test]
fn unlink_rejects_invalid_lists_before_mutation() {
    let (project, _, _, _, clip, title) = fixture();

    let msg = reject(
        &project,
        WireCommand::UnlinkClips(wire::UnlinkClips { clips: vec![] }),
    );
    assert!(msg.contains("at least one"), "{msg}");

    let msg = reject(
        &project,
        WireCommand::UnlinkClips(wire::UnlinkClips {
            clips: vec![clip, clip],
        }),
    );
    assert!(msg.contains("duplicate clip id"), "{msg}");
    assert!(msg.contains(&clip.to_string()), "{msg}");

    let msg = reject(
        &project,
        WireCommand::UnlinkClips(wire::UnlinkClips {
            clips: vec![clip, 999],
        }),
    );
    assert!(msg.contains("clip 999 does not exist"), "{msg}");

    let msg = reject(
        &project,
        WireCommand::UnlinkClips(wire::UnlinkClips {
            clips: vec![clip, title],
        }),
    );
    assert!(
        msg.contains("all referenced clips are already unlinked"),
        "{msg}"
    );

    let msg = reject(
        &project,
        WireCommand::UnlinkClips(wire::UnlinkClips {
            clips: vec![clip; MAX_MULTI_CLIP_REFS + 1],
        }),
    );
    assert!(msg.contains("at most 64"), "{msg}");

    assert_eq!(project.clip(ClipId::from_raw(clip)).unwrap().link, None);
    assert_eq!(project.clip(ClipId::from_raw(title)).unwrap().link, None);
}

#[test]
fn marker_commands_lower_to_engine() {
    let (project, _, _, _, _, _) = fixture();
    let edit = lower(
        &project,
        WireCommand::AddMarker(wire::AddMarker {
            at: 2.0,
            name: Some("intro".into()),
            color: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::AddMarker {
            at: RationalTime::new(48, R24),
            name: "intro".into(),
            color: None,
        }
    );

    // Negative positions are rejected before reaching the engine.
    let msg = reject(
        &project,
        WireCommand::AddMarker(wire::AddMarker {
            at: -1.0,
            name: None,
            color: None,
        }),
    );
    assert!(msg.contains("must not be negative"), "{msg}");

    let mut project = project;
    let id = project
        .timeline_mut()
        .add_marker(
            aria_models::Marker::new(RationalTime::new(48, R24), "mid", MarkerColor::Blue)
                .with_comment("check the cut"),
        )
        .unwrap();
    let edit = lower(
        &project,
        WireCommand::SetMarker(wire::SetMarker {
            marker: id.raw(),
            at: Some(3.0),
            name: Some("outro".into()),
            color: Some(WireMarkerColor::Green),
            comment: Some("regraded".into()),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetMarker {
            marker: id,
            at: RationalTime::new(72, R24),
            name: "outro".into(),
            color: MarkerColor::Green,
            comment: "regraded".into(),
        }
    );

    // Omitted fields keep the marker's current state, the comment included.
    let edit = lower(
        &project,
        WireCommand::SetMarker(wire::SetMarker {
            marker: id.raw(),
            at: None,
            name: None,
            color: None,
            comment: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetMarker {
            marker: id,
            at: RationalTime::new(48, R24),
            name: "mid".into(),
            color: MarkerColor::Blue,
            comment: "check the cut".into(),
        }
    );

    let msg = reject(
        &project,
        WireCommand::RemoveMarker(wire::RemoveMarker { marker: 404 }),
    );
    assert!(msg.contains("marker 404 does not exist"), "{msg}");
    assert!(msg.contains(&id.raw().to_string()), "{msg}");
}

#[test]
fn set_canvas_lowers_and_keeps_omitted_fields() {
    let (mut project, _, _, _, _, _) = fixture();
    let edit = lower(
        &project,
        WireCommand::SetCanvas(wire::SetCanvas {
            aspect: Some(wire::WireCanvasAspect::Tall9x16),
            background: Some([20, 20, 28]),
            resolution: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetCanvas {
            aspect: CanvasAspect::Tall9x16,
            background: [20, 20, 28],
            resolution: None,
        }
    );

    // Omitted fields keep the project's current canvas settings.
    project
        .timeline_mut()
        .set_canvas(aria_models::CanvasSettings {
            aspect: CanvasAspect::Square1x1,
            background: [255, 0, 0],
            resolution: None,
        });
    let edit = lower(
        &project,
        WireCommand::SetCanvas(wire::SetCanvas {
            aspect: None,
            background: Some([0, 0, 0]),
            resolution: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetCanvas {
            aspect: CanvasAspect::Square1x1,
            background: [0, 0, 0],
            resolution: None,
        }
    );
    let edit = lower(
        &project,
        WireCommand::SetCanvas(wire::SetCanvas {
            aspect: Some(wire::WireCanvasAspect::Auto),
            background: None,
            resolution: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetCanvas {
            aspect: CanvasAspect::Auto,
            background: [255, 0, 0],
            resolution: None,
        }
    );
}

/// The wire says three different things about the size: omit it (keep), send
/// `[0, 0]` (derive it again), or send pixels (use them). A size the encoder
/// cannot take is refused here, with the bounds and the escape hatch named, so
/// the agent can correct itself without a round trip through the engine.
#[test]
fn set_canvas_resolution_keeps_derives_or_refuses() {
    let (mut project, _, _, _, _, _) = fixture();
    project
        .timeline_mut()
        .set_canvas(aria_models::CanvasSettings {
            aspect: CanvasAspect::Wide16x9,
            background: [0, 0, 0],
            resolution: Some([3840, 2160]),
        });

    // Omitted keeps the stored size.
    let edit = lower(
        &project,
        WireCommand::SetCanvas(wire::SetCanvas {
            aspect: None,
            background: None,
            resolution: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetCanvas {
            aspect: CanvasAspect::Wide16x9,
            background: [0, 0, 0],
            resolution: Some([3840, 2160]),
        }
    );

    // `[0, 0]` hands the size back to the aspect or the footage.
    let edit = lower(
        &project,
        WireCommand::SetCanvas(wire::SetCanvas {
            aspect: None,
            background: None,
            resolution: Some([0, 0]),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetCanvas {
            aspect: CanvasAspect::Wide16x9,
            background: [0, 0, 0],
            resolution: None,
        }
    );

    // Explicit pixels are taken as given.
    let edit = lower(
        &project,
        WireCommand::SetCanvas(wire::SetCanvas {
            aspect: None,
            background: None,
            resolution: Some([1280, 720]),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetCanvas {
            aspect: CanvasAspect::Wide16x9,
            background: [0, 0, 0],
            resolution: Some([1280, 720]),
        }
    );

    for bad in [[1921, 1080], [8, 8], [12000, 1080], [1920, 0]] {
        let msg = reject(
            &project,
            WireCommand::SetCanvas(wire::SetCanvas {
                aspect: None,
                background: None,
                resolution: Some(bad),
            }),
        );
        assert!(msg.contains("not encodable"), "{bad:?}: {msg}");
        assert!(msg.contains("[0, 0]"), "{bad:?}: {msg}");
    }
}

#[test]
fn non_finite_and_negative_times_are_rejected() {
    let (project, _, _, _, clip, _) = fixture();
    let msg = reject(
        &project,
        WireCommand::TrimClip(wire::TrimClip {
            clip,
            start: f64::NAN,
            duration: 1.0,
        }),
    );
    assert!(msg.contains("finite"), "{msg}");

    let msg = reject(
        &project,
        WireCommand::TrimClip(wire::TrimClip {
            clip,
            start: -1.0,
            duration: 1.0,
        }),
    );
    assert!(msg.contains("must not be negative"), "{msg}");
}
