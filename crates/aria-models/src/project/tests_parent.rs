//! Clip parenting in the project: which clip may parent which, when a parent
//! applies, and how a chain of parents composes.
//!
//! Closes the class where a parent link makes a placement loop (a clip under
//! itself, directly or through a chain), reaches across sequences, holds a
//! parent that never overlaps the child, or grows a chain the placement walk
//! cannot bound. Also pins that a refused write leaves the clip as it was.
//!
//! Not covered: rewriting the child's transform so it keeps its place, which
//! `clip::parent` tests, and the rendered placement, which `aria-render` tests.

use super::*;
use crate::clip::{AnimatedTransform, MAX_PARENT_DEPTH, ParentFrame, Scale2};

/// One video lane per `(start, duration)` span, each holding one clip there.
fn stacked(spans: &[(i64, i64)]) -> (Project, Vec<ClipId>) {
    let (mut project, media, _) = project_with_media(1_000);
    let clips = spans
        .iter()
        .map(|&(start, duration)| {
            let track = project.add_track(TrackKind::Video, "V");
            project
                .add_clip(track, media, tr(0, duration), rt(start))
                .unwrap()
        })
        .collect();
    (project, clips)
}

fn clips_of(project: &Project, ids: &[ClipId]) -> Vec<Clip> {
    ids.iter()
        .map(|id| project.clip(*id).unwrap().clone())
        .collect()
}

fn parent(
    project: &mut Project,
    child: ClipId,
    to: Option<ClipId>,
) -> Result<Option<ClipId>, ModelError> {
    let transform = project.clip(child).unwrap().transform.clone();
    project.set_clip_parent(child, to, transform)
}

#[test]
fn a_parent_must_overlap_the_child_by_at_least_one_tick() {
    let (project, clips) = stacked(&[(0, 48), (47, 48), (48, 48), (100, 10)]);
    let (child, touching, abutting, apart) = (clips[0], clips[1], clips[2], clips[3]);
    // Each pair in both roles: a parent after the child and one before it.
    for (a, b) in [(child, touching), (touching, child)] {
        assert_eq!(project.check_parent(a, b), Ok(()));
    }
    for other in [abutting, apart] {
        for (a, b) in [(child, other), (other, child)] {
            assert_eq!(
                project.check_parent(a, b),
                Err(ModelError::ParentApart {
                    child: a,
                    parent: b
                })
            );
        }
    }
}

#[test]
fn a_parent_must_be_a_clip_on_the_child_s_sequence() {
    let (mut project, clips) = stacked(&[(0, 48)]);
    let media = project.media_iter().next().unwrap().id;
    let sequence = project.create_sequence("Inner", R24);
    project.set_active_sequence(Some(sequence)).unwrap();
    let track = project.add_track(TrackKind::Video, "V1");
    let inner = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();
    project.set_active_sequence(None).unwrap();
    assert_eq!(
        project.check_parent(clips[0], inner),
        Err(ModelError::ParentElsewhere {
            child: clips[0],
            parent: inner
        })
    );
    let missing = ClipId::from_raw(9_999);
    assert_eq!(
        project.check_parent(clips[0], missing),
        Err(ModelError::UnknownClip(missing))
    );
    assert_eq!(
        project.check_parent(missing, clips[0]),
        Err(ModelError::UnknownClip(missing))
    );
}

#[test]
fn a_clip_cannot_sit_under_itself() {
    let (mut project, clips) = stacked(&[(0, 48), (0, 48), (0, 48)]);
    let (a, b, c) = (clips[0], clips[1], clips[2]);
    assert_eq!(project.check_parent(a, a), Err(ModelError::ParentCycle(a)));
    parent(&mut project, a, Some(b)).unwrap();
    parent(&mut project, b, Some(c)).unwrap();
    let before = clips_of(&project, &clips);
    for (child, under) in [(b, a), (c, a), (c, b)] {
        assert_eq!(
            parent(&mut project, child, Some(under)),
            Err(ModelError::ParentCycle(child)),
            "{child} under {under}"
        );
    }
    assert_eq!(clips_of(&project, &clips), before, "a refused parent wrote");
}

/// A chain may be [`MAX_PARENT_DEPTH`] parents deep and no deeper, and the
/// check that says so ends.
#[test]
fn a_chain_deeper_than_the_limit_is_refused() {
    let (mut project, clips) = stacked(&[(0, 48); MAX_PARENT_DEPTH + 2]);
    for pair in clips[..MAX_PARENT_DEPTH].windows(2) {
        parent(&mut project, pair[0], Some(pair[1])).unwrap();
    }
    // clips[0] sits under MAX_PARENT_DEPTH - 1 parents.
    let (deepest, extra) = (clips[MAX_PARENT_DEPTH], clips[MAX_PARENT_DEPTH + 1]);
    assert_eq!(parent(&mut project, deepest, Some(clips[0])), Ok(None));
    assert_eq!(
        parent(&mut project, extra, Some(deepest)),
        Err(ModelError::ParentTooDeep(extra))
    );
    assert_eq!(project.clip(extra).unwrap().parent, None);
}

#[test]
fn setting_a_parent_writes_the_transform_and_returns_the_last_parent() {
    let (mut project, clips) = stacked(&[(0, 48), (0, 48), (0, 48)]);
    let (child, first, second) = (clips[0], clips[1], clips[2]);
    let mut moved = AnimatedTransform::identity();
    moved.position = Param::Constant([0.2, -0.1]);
    assert_eq!(
        project.set_clip_parent(child, Some(first), moved.clone()),
        Ok(None)
    );
    assert_eq!(project.clip(child).unwrap().transform, moved);
    assert_eq!(parent(&mut project, child, Some(second)), Ok(Some(first)));
    assert_eq!(parent(&mut project, child, None), Ok(Some(second)));
    assert_eq!(project.clip(child).unwrap().parent, None);

    let before = clips_of(&project, &clips);
    let mut flat = AnimatedTransform::identity();
    flat.scale = Param::Constant(Scale2 { x: 0.0, y: 1.0 });
    assert!(matches!(
        project.set_clip_parent(child, Some(first), flat),
        Err(ModelError::InvalidTransform(_))
    ));
    assert_eq!(
        clips_of(&project, &clips),
        before,
        "a refused transform wrote"
    );
}

#[test]
fn a_parent_applies_only_while_it_covers_the_tick() {
    let (mut project, clips) = stacked(&[(0, 96), (24, 48)]);
    let (child, over) = (clips[0], clips[1]);
    parent(&mut project, child, Some(over)).unwrap();
    let clip = project.clip(child).unwrap();
    for (tick, applies) in [
        (0.0, false),
        (23.9, false),
        (24.0, true),
        (71.5, true),
        (72.0, false),
    ] {
        assert_eq!(
            project.active_parent(clip, tick).map(|p| p.id),
            applies.then_some(over),
            "at {tick}"
        );
    }
    let mut gone = project.clone();
    gone.remove_clip(over).unwrap();
    let clip = gone.clip(child).unwrap();
    assert_eq!(gone.active_parent(clip, 30.0).map(|p| p.id), None);
}

fn posed(position: [f32; 2], rotation: f32) -> ClipTransform {
    ClipTransform {
        position,
        rotation,
        ..ClipTransform::IDENTITY
    }
}

/// A grandparent turned a quarter clockwise carries its child, which carries
/// the grandchild: the grandchild's frame is the child's pose placed in the
/// grandparent's frame, read once per parent.
#[test]
fn a_chain_composes_outermost_first() {
    let (mut project, clips) = stacked(&[(0, 96), (0, 96), (0, 48)]);
    let (child, middle, top) = (clips[0], clips[1], clips[2]);
    parent(&mut project, child, Some(middle)).unwrap();
    parent(&mut project, middle, Some(top)).unwrap();
    let canvas = [1920.0, 1080.0];
    let poses = |clip: &Clip| {
        if clip.id == top {
            posed([0.25, 0.0], 90.0)
        } else {
            posed([0.1, 0.0], 10.0)
        }
    };
    let mut read = Vec::new();
    let frame = project
        .parent_frame(project.clip(child).unwrap(), 12.0, canvas, |clip, _| {
            read.push(clip.id);
            poses(clip)
        })
        .unwrap();
    assert_eq!(read, vec![top, middle]);
    let expected = ParentFrame::of(
        &ParentFrame::of(&posed([0.25, 0.0], 90.0), canvas).compose(&posed([0.1, 0.0], 10.0)),
        canvas,
    );
    assert_eq!(frame, expected);
    assert!((frame.pivot[0] - 1440.0).abs() < 1e-3, "{frame:?}");
    assert!((frame.pivot[1] - 732.0).abs() < 1e-3, "{frame:?}");
    assert!((frame.rotation - 100.0).abs() < 1e-4);

    // Past the top clip's end, the middle clip places the child alone.
    let alone = project
        .parent_frame(project.clip(child).unwrap(), 60.0, canvas, |clip, _| {
            poses(clip)
        })
        .unwrap();
    assert_eq!(alone, ParentFrame::of(&posed([0.1, 0.0], 10.0), canvas));
    let top_clip = project.clip(top).unwrap();
    assert_eq!(
        project.parent_frame(top_clip, 12.0, canvas, |clip, _| poses(clip)),
        None
    );
}

/// The `parent` slot of every clip in a saved document, in document order.
fn saved_parents(doc: &serde_json::Value) -> Vec<Option<&serde_json::Value>> {
    match doc {
        serde_json::Value::Object(map) => {
            let own = map.contains_key("transform").then(|| map.get("parent"));
            own.into_iter()
                .chain(map.values().flat_map(saved_parents))
                .collect()
        }
        serde_json::Value::Array(items) => items.iter().flat_map(saved_parents).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn a_parent_is_saved_and_an_unparented_clip_saves_no_parent() {
    let (mut project, clips) = stacked(&[(0, 48), (0, 48)]);
    parent(&mut project, clips[0], Some(clips[1])).unwrap();
    let saved = project.save_to_string().unwrap();
    let doc: serde_json::Value = serde_json::from_str(&saved).unwrap();
    let mut parents = saved_parents(&doc);
    parents.sort_by_key(Option::is_some);
    assert_eq!(
        parents,
        vec![None, Some(&serde_json::to_value(clips[1]).unwrap())],
        "{saved}"
    );
    let loaded = Project::load_from_string(&saved).unwrap();
    assert_eq!(loaded.clip(clips[0]).unwrap().parent, Some(clips[1]));
    assert_eq!(loaded.clip(clips[1]).unwrap().parent, None);
}
