//! The largest video size an `Auto` canvas reads: which placed source it is,
//! and which edits drop the cached size.

use super::*;
use crate::clip::TextStyle;
use crate::look::TrackMatte;

/// The size every case below starts from: the one 1920×1080 source placed.
const LARGEST: Option<(u32, u32)> = Some((1920, 1080));

/// The cached size, `None` when the next read walks the timeline again.
fn cached(project: &Project) -> Option<(u32, u32)> {
    project.largest_used_video.get().copied().flatten()
}

/// Apply `edit`, which must succeed, and require the cached size kept.
#[track_caller]
fn keeps<T>(
    project: &mut Project,
    what: &str,
    edit: impl FnOnce(&mut Project) -> Result<T, ModelError>,
) {
    if let Err(err) = edit(project) {
        panic!("{what} was refused: {err}");
    }
    assert_eq!(
        cached(project),
        LARGEST,
        "{what} dropped the largest video size"
    );
}

#[test]
fn largest_used_video_dimensions_cache_and_invalidate_at_mutation_boundaries() {
    let (mut project, media, track) = project_with_media(100);
    let larger = project.add_media(MediaSource::new(
        "/tmp/larger.mp4",
        3840,
        2160,
        R24,
        100,
        false,
    ));
    project.add_clip(track, media, tr(0, 24), rt(0)).unwrap();

    assert_eq!(project.largest_used_video_dimensions(), Some((1920, 1080)));
    assert_eq!(
        project.largest_used_video.get().copied().flatten(),
        Some((1920, 1080))
    );
    assert_eq!(project.largest_used_video_dimensions(), Some((1920, 1080)));

    project.add_clip(track, larger, tr(0, 24), rt(24)).unwrap();
    assert!(project.largest_used_video.get().is_none());
    assert_eq!(project.largest_used_video_dimensions(), Some((3840, 2160)));

    let source = project.media_mut(media).expect("used media");
    source.width = 7680;
    source.height = 4320;
    assert!(project.largest_used_video.get().is_none());
    assert_eq!(project.largest_used_video_dimensions(), Some((7680, 4320)));
}

/// Of placed sources with one area, an `Auto` canvas takes the one whose clip
/// starts first, whatever order the clips were placed in.
///
/// WHY: the walk read a lane's clips in hash order, so a tie in area was
/// settled by the hash of the clip ids. Each rotation places the eight shapes
/// in one order at rotated starts, so a walk in any fixed order of the clip
/// ids, hash or placement, misses all but one rotation. What it does not
/// catch: a tie across lanes, which `aria-ui`'s
/// `the_largest_video_off_the_basis_is_the_one_the_walk_finds` covers.
#[test]
fn a_tie_in_area_goes_to_the_clip_that_starts_first() {
    const TIED: [(u32, u32); 8] = [
        (1920, 1080),
        (1080, 1920),
        (2160, 960),
        (960, 2160),
        (1440, 1440),
        (1600, 1296),
        (1296, 1600),
        (2880, 720),
    ];
    for rotation in 0..TIED.len() {
        let mut project = Project::new("tie", R24);
        let track = project.add_track(TrackKind::Video, "V1");
        let mut first = None;
        for (&(width, height), slot) in TIED.iter().zip((0..8).cycle().skip(rotation)) {
            let media =
                project.add_media(MediaSource::new("tied.mp4", width, height, R24, 100, false));
            project
                .add_clip(track, media, tr(0, 12), rt(24 * slot))
                .unwrap();
            if slot == 0 {
                first = Some((width, height));
            }
        }
        assert_eq!(
            project.largest_video_dimensions_on(project.timeline()),
            first,
            "rotation {rotation}"
        );
    }
}

/// A write to how a clip draws or sounds keeps the largest video size an
/// `Auto` canvas reads.
///
/// WHY: every per-clip write reached the clip through `timeline_mut`, which
/// drops the cached size, so each frame of a parameter scrub walked every clip
/// on the timeline to size the canvas again. The look, effect, mask and audio
/// setters dress a media clip, each checked as it runs; then every row
/// `clip_params_for` offers on that clip and on a title takes a constant and a
/// keyframe, so a property, effect slot or mask added later is written without
/// being named here. What it does not catch: a setter not called below, and an
/// edit outside this crate that reaches a clip through `timeline_mut`. Either
/// costs a walk on the next frame, not a wrong size.
#[test]
fn a_write_to_how_a_clip_draws_keeps_the_largest_video_size() {
    let (mut project, media, track) = project_with_media(100);
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();
    let text = project.add_track(TrackKind::Text, "T1");
    let title = project
        .add_generated(
            text,
            Generator::Text {
                content: "Aria".into(),
                style: TextStyle::default(),
            },
            tr(0, 48),
        )
        .unwrap();
    assert_eq!(project.largest_used_video_dimensions(), LARGEST);

    let p = &mut project;
    keeps(p, "a transform", |p| {
        p.set_transform(clip, ClipTransform::default(), None)
    });
    keeps(p, "a blend mode", |p| {
        p.set_blend_mode(clip, BlendMode::Multiply)
    });
    keeps(p, "motion blur", |p| {
        p.set_motion_blur(
            clip,
            MotionBlur {
                enabled: true,
                ..MotionBlur::default()
            },
        )
    });
    keeps(p, "layer styles", |p| {
        p.set_layer_styles(
            clip,
            LayerStyles {
                shadow: Some(LayerShadow::default()),
                ..Default::default()
            },
        )
    });
    keeps(p, "a chroma key", |p| {
        p.set_clip_chroma_key(
            clip,
            Some(ChromaKey {
                rgb: [3, 240, 17],
                strength: 0.72.into(),
                shadow: 0.18.into(),
            }),
        )
    });
    keeps(p, "a track matte", |p| {
        p.set_clip_matte(clip, Some(TrackMatte::luma()))
    });
    keeps(p, "stabilization", |p| {
        p.set_clip_stabilize(clip, Some(StabilizeLevel::Recommended))
    });
    keeps(p, "a filter", |p| {
        p.set_clip_filter(clip, Some(Filter::new("noir")))
    });
    keeps(p, "a LUT", |p| {
        p.set_clip_lut(clip, Some(Lut::new("look.cube")))
    });
    keeps(p, "adjustments", |p| {
        p.set_clip_adjustments(
            clip,
            ColorAdjustments {
                contrast: (-0.5).into(),
                ..Default::default()
            },
        )
    });
    keeps(p, "an entrance", |p| {
        p.set_clip_animation(clip, AnimationSlot::In, Some(AnimationRef::new("fade_in")))
    });
    keeps(p, "a mask", |p| {
        p.add_clip_mask(clip, Mask::new(MaskKind::Circle))
    });
    keeps(p, "a second mask", |p| {
        p.add_clip_mask(clip, Mask::new(MaskKind::Rectangle))
    });
    keeps(p, "a mask replaced", |p| {
        p.set_clip_mask(clip, 1, Mask::new(MaskKind::Linear))
    });
    keeps(p, "a mask moved", |p| p.move_clip_mask(clip, 0, 1));
    keeps(p, "an effect", |p| p.add_effect(clip, "gaussian_blur"));
    keeps(p, "a second effect", |p| {
        p.add_effect(clip, "gaussian_blur")
    });
    keeps(p, "an effect moved", |p| p.move_effect(clip, 0, 1));
    keeps(p, "an effect bypassed", |p| {
        p.set_effect_bypass(clip, 1, true)
    });
    keeps(p, "pitch lock", |p| p.set_clip_pitch(clip, false));
    keeps(p, "noise reduction", |p| p.set_clip_denoise(clip, true));
    keeps(p, "a name", |p| {
        p.set_clip_name(clip, Some("Take 2".into()))
    });
    keeps(p, "a clip type", |p| {
        p.set_clip_type(clip, Some("test.type".into()))
    });
    keeps(p, "a line property", |p| {
        p.set_clip_line_param(clip, Some(ClipParam::Scale))
    });
    keeps(p, "the audio mix", |p| {
        p.set_clip_audio(clip, Some(0.5), rt(6), rt(6))
    });
    keeps(p, "a crop", |p| {
        p.set_clip_crop(clip, CropRect::FULL, true, false, None)
    });
    keeps(p, "beats", |p| p.set_clip_beats(clip, vec![12, 24]));

    let mut swept = Vec::new();
    for id in [clip, title] {
        let rows = crate::clip_params_for(p.require_clip(id).unwrap());
        for row in rows {
            let value = row
                .sample(p.require_clip(id).unwrap(), 0)
                .unwrap_or_else(|| panic!("{} is offered and samples nothing", row.id));
            keeps(p, &format!("{} as a constant", row.id), |p| {
                p.set_param_constant(id, row.param, value)
            });
            keeps(p, &format!("{} as a keyframe", row.id), |p| {
                p.set_param_keyframe(id, row.param, rt(12), value, Easing::Linear, None)
            });
            swept.push(row.param);
        }
    }
    for (what, offered) in [
        (
            "an effect parameter",
            swept
                .iter()
                .any(|param| matches!(param, ClipParam::Effect { .. })),
        ),
        (
            "a mask property",
            swept
                .iter()
                .any(|param| matches!(param, ClipParam::Mask { .. })),
        ),
        (
            "a title property",
            swept
                .iter()
                .any(|param| matches!(param, ClipParam::Text { .. })),
        ),
    ] {
        assert!(offered, "no row offered {what}, so none was written");
    }

    keeps(p, "a mask removed", |p| p.remove_clip_mask(clip, 0));
    keeps(p, "an effect removed", |p| p.remove_effect(clip, 0));
    keeps(p, "the effect chain replaced", |p| {
        p.set_clip_effects(clip, Vec::new())
    });
}

/// An edit that changes the media a clip places drops the cached size, and
/// the next read finds the new source; one that leaves it keeps the size.
///
/// WHY: [`Project::edit_clip`] keeps the size a whole parameter scrub reads
/// on the promise that a write to how a clip draws places no other source.
/// This holds the check that backs the promise: an edit that does swap the
/// source must still drop the size, or the canvas stays at the old source's
/// size until some unrelated edit drops it.
#[test]
fn an_edit_that_swaps_the_placed_media_drops_the_largest_video_size() {
    let (mut project, media, track) = project_with_media(100);
    let larger = project.add_media(MediaSource::new(
        "/tmp/larger.mp4",
        3840,
        2160,
        R24,
        100,
        false,
    ));
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();
    assert_eq!(project.largest_used_video_dimensions(), LARGEST);

    let placed = |project: &mut Project, to: MediaId| {
        project.edit_clip(clip, |clip| {
            if let ClipSource::Media { media, .. } = &mut clip.content {
                *media = to;
            }
            Ok(())
        })
    };
    keeps(&mut project, "the same media placed again", |p| {
        placed(p, media)
    });
    placed(&mut project, larger).unwrap();
    assert_eq!(cached(&project), None, "a swapped source kept the old size");
    assert_eq!(project.largest_used_video_dimensions(), Some((3840, 2160)));
}
