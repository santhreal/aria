use super::*;
use aria_models::TransitionMedia;

// --- entity lookups (errors list what exists) ------------------------------

const MAX_LISTED_IDS: usize = 10;

pub(super) fn list_ids(mut ids: Vec<u64>) -> String {
    use std::fmt::Write as _;

    if ids.is_empty() {
        return "none".to_string();
    }
    ids.sort_unstable();
    let extra = ids.len().saturating_sub(MAX_LISTED_IDS);
    let mut out = String::new();
    for (index, id) in ids.into_iter().take(MAX_LISTED_IDS).enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{id}");
    }
    if extra > 0 {
        let _ = write!(out, " (and {extra} more)");
    }
    out
}

pub(super) fn clip_ref(project: &Project, raw: u64) -> Result<&Clip, Rejection> {
    project.clip(ClipId::from_raw(raw)).ok_or_else(|| {
        let existing = project
            .timeline()
            .tracks_ordered()
            .flat_map(|t| t.clips())
            .map(|c| c.id.raw())
            .collect();
        Rejection::new(format!(
            "clip {raw} does not exist; clips on the timeline: {}",
            list_ids(existing)
        ))
    })
}

/// Comma-separated catalog effect ids, for rejection messages.
pub(super) fn effect_ids() -> String {
    aria_models::effect_catalog()
        .iter()
        .map(|s| s.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn transition_ids() -> String {
    aria_models::transition_catalog()
        .iter()
        .map(|s| s.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn audio_transition_ids() -> String {
    aria_models::AudioTransition::ALL
        .map(aria_models::AudioTransition::id)
        .join(", ")
}

/// Whether a clip butts directly against a following clip on its track (the
/// next clip starts exactly where this one ends) — the precondition for a
/// transition at its right junction.
pub(super) fn has_right_neighbor(project: &Project, clip: &Clip) -> bool {
    let timeline = project.timeline();
    let Some(track) = timeline.track_of(clip.id).and_then(|id| timeline.track(id)) else {
        return false;
    };
    let end = clip.timeline.end_tick();
    track
        .clips()
        .any(|c| c.id != clip.id && c.timeline.start.value == end)
}

/// Reject when the clip carries no transition at its right junction.
pub(super) fn require_transition(
    project: &Project,
    clip: &Clip,
    raw_clip: u64,
) -> Result<(), Rejection> {
    let timeline = project.timeline();
    let has = timeline
        .track_of(clip.id)
        .and_then(|id| timeline.track(id))
        .is_some_and(|t| t.transition_at(clip.id).is_some());
    if !has {
        return Err(Rejection::new(format!(
            "clip {raw_clip} has no transition at its right cut"
        )));
    }
    Ok(())
}

/// Resolve a wire effect-chain index against a clip, rejecting out-of-range
/// indices with the clip's current chain length.
pub(super) fn effect_index(clip: &Clip, index: u32, raw_clip: u64) -> Result<usize, Rejection> {
    let index = usize::try_from(index).map_err(|_| {
        Rejection::new(format!(
            "clip {raw_clip} has no effect at index {index} (chain length {})",
            clip.effects.len()
        ))
    })?;
    if index >= clip.effects.len() {
        return Err(Rejection::new(format!(
            "clip {raw_clip} has no effect at index {index} (chain length {})",
            clip.effects.len()
        )));
    }
    Ok(index)
}

/// Resolve a wire mask-stack index against a clip, rejecting out-of-range
/// indices with the clip's current stack length.
pub(super) fn mask_index(clip: &Clip, index: u32, raw_clip: u64) -> Result<usize, Rejection> {
    match usize::try_from(index) {
        Ok(at) if at < clip.masks.len() => Ok(at),
        _ => Err(Rejection::new(format!(
            "clip {raw_clip} has no mask at index {index} (stack length {})",
            clip.masks.len()
        ))),
    }
}

/// Resolve `(effect index, param name)` to the chain index, the catalog slot,
/// and the effect's spec (the slot's spec is `spec.params[slot]`).
pub(super) fn effect_param_slot(
    clip: &Clip,
    index: u32,
    param: &str,
    raw_clip: u64,
) -> Result<(usize, usize, std::sync::Arc<aria_models::EffectSpec>), Rejection> {
    let effect = effect_index(clip, index, raw_clip)?;
    let instance = &clip.effects[effect];
    let spec = aria_models::effect_spec(&instance.effect_id).ok_or_else(|| {
        Rejection::new(format!(
            "effect '{}' on clip {raw_clip} is not in the catalog",
            instance.effect_id
        ))
    })?;
    let slot = spec
        .params
        .iter()
        .position(|p| p.name == param)
        .ok_or_else(|| {
            let names: Vec<&str> = spec.params.iter().map(|p| p.name.as_str()).collect();
            Rejection::new(format!(
                "effect '{}' has no parameter '{}'; parameters: {}",
                instance.effect_id,
                param,
                names.join(", ")
            ))
        })?;
    Ok((effect, slot, spec))
}

/// Reject clips on an audio lane, which have no frame to operate on.
pub(super) fn reject_audio_lane(
    project: &Project,
    clip: &Clip,
    why: &str,
    raw_clip: u64,
) -> Result<(), Rejection> {
    let timeline = project.timeline();
    let on_audio = timeline
        .track_of(clip.id)
        .and_then(|id| timeline.track(id))
        .is_some_and(|t| t.kind.is_audio());
    if on_audio {
        return Err(Rejection::new(format!(
            "clip {raw_clip} is on an audio lane; {why}"
        )));
    }
    Ok(())
}

/// Reject a transition the clip's lane does not take at a cut: a lane that
/// composites frames takes a catalog blend, a sound lane an audio crossfade,
/// and effect/filter/adjustment segments neither, because they resolve to
/// canvas-wide passes the renderer can't nest inside a transition (mirrors the
/// model's `TrackKind::transition_media`).
pub(super) fn reject_transition_on_lane(
    project: &Project,
    clip: &Clip,
    raw_clip: u64,
    transition: &str,
) -> Result<(), Rejection> {
    let timeline = project.timeline();
    let Some(kind) = timeline
        .track_of(clip.id)
        .and_then(|id| timeline.track(id))
        .map(|t| t.kind)
    else {
        // A clip on no lane has no cut; the neighbour check refuses it.
        return Ok(());
    };
    let Some(media) = kind.transition_media() else {
        let lanes = |media| {
            aria_models::track_kind_specs()
                .iter()
                .filter(|spec| spec.kind.transition_media() == Some(media))
                .map(|spec| spec.id)
                .collect::<Vec<_>>()
                .join(", ")
        };
        return Err(Rejection::new(format!(
            "clip {raw_clip} is on a {kind:?} lane; picture transitions need a lane that \
             composites frames ({}) and audio transitions a sound lane ({})",
            lanes(TransitionMedia::Picture),
            lanes(TransitionMedia::Audio),
        )));
    };
    let picture = aria_models::transition_spec(transition).is_some();
    let audio = aria_models::AudioTransition::from_id(transition).is_some();
    let (takes, other) = match media {
        TransitionMedia::Picture => (picture, audio.then_some("an audio")),
        TransitionMedia::Audio => (audio, picture.then_some("a picture")),
    };
    if takes {
        return Ok(());
    }
    let (ids, lane) = match media {
        TransitionMedia::Picture => (transition_ids(), format!("a {kind:?} lane")),
        TransitionMedia::Audio => (audio_transition_ids(), "an audio lane".to_owned()),
    };
    Err(Rejection::new(match other {
        Some(other) => format!(
            "'{transition}' is {other} transition and clip {raw_clip} is on {lane}; \
             available transitions: {ids}"
        ),
        None => format!("unknown transition '{transition}'; available transitions: {ids}"),
    }))
}

pub(super) fn track_ref(project: &Project, raw: u64) -> Result<&aria_models::Track, Rejection> {
    project
        .timeline()
        .track(TrackId::from_raw(raw))
        .ok_or_else(|| {
            let existing = project
                .timeline()
                .tracks_ordered()
                .map(|t| t.id.raw())
                .collect();
            Rejection::new(format!(
                "track {raw} does not exist; tracks on the timeline: {}",
                list_ids(existing)
            ))
        })
}

pub(super) fn media_ref(
    project: &Project,
    raw: u64,
) -> Result<&aria_models::MediaSource, Rejection> {
    project.media(MediaId::from_raw(raw)).ok_or_else(|| {
        let existing = project.media_iter().map(|m| m.id.raw()).collect();
        Rejection::new(format!(
            "media {raw} does not exist; media in the pool: {}",
            list_ids(existing)
        ))
    })
}

pub(super) fn marker_ref(project: &Project, raw: u64) -> Result<&Marker, Rejection> {
    project
        .timeline()
        .marker(MarkerId::from_raw(raw))
        .ok_or_else(|| {
            let existing = project
                .timeline()
                .markers()
                .iter()
                .map(|m| m.id.raw())
                .collect();
            Rejection::new(format!(
                "marker {raw} does not exist; markers on the timeline: {}",
                list_ids(existing)
            ))
        })
}

pub(super) fn marker_color(color: WireMarkerColor) -> MarkerColor {
    match color {
        WireMarkerColor::Teal => MarkerColor::Teal,
        WireMarkerColor::Blue => MarkerColor::Blue,
        WireMarkerColor::Purple => MarkerColor::Purple,
        WireMarkerColor::Pink => MarkerColor::Pink,
        WireMarkerColor::Red => MarkerColor::Red,
        WireMarkerColor::Orange => MarkerColor::Orange,
        WireMarkerColor::Yellow => MarkerColor::Yellow,
        WireMarkerColor::Green => MarkerColor::Green,
    }
}

pub(super) fn canvas_aspect(aspect: WireCanvasAspect) -> CanvasAspect {
    match aspect {
        WireCanvasAspect::Auto => CanvasAspect::Auto,
        WireCanvasAspect::Wide16x9 => CanvasAspect::Wide16x9,
        WireCanvasAspect::Tall9x16 => CanvasAspect::Tall9x16,
        WireCanvasAspect::Square1x1 => CanvasAspect::Square1x1,
        WireCanvasAspect::Portrait4x5 => CanvasAspect::Portrait4x5,
        WireCanvasAspect::Cinema21x9 => CanvasAspect::Cinema21x9,
    }
}

// --- kind rules -------------------------------------------------------------

pub(super) fn track_kind(kind: WireTrackKind) -> TrackKind {
    match kind {
        WireTrackKind::Video => TrackKind::Video,
        WireTrackKind::Audio => TrackKind::Audio,
        WireTrackKind::Text => TrackKind::Text,
        WireTrackKind::Sticker => TrackKind::Sticker,
        WireTrackKind::Submix => TrackKind::Submix,
    }
}

pub(super) fn require_media_lane(kind: TrackKind, raw: u64) -> Result<(), Rejection> {
    if matches!(kind, TrackKind::Video | TrackKind::Audio) {
        Ok(())
    } else {
        Err(Rejection::new(format!(
            "track {raw} is a {} lane; media clips need a video or audio track",
            kind.id()
        )))
    }
}

pub(super) fn require_generator_lane(
    track: &aria_models::Track,
    generator: &Generator,
) -> Result<(), Rejection> {
    let content = aria_models::ClipSource::Generated(generator.clone());
    if track.kind.accepts_content(&content) {
        return Ok(());
    }
    // The lane a generator belongs on is a column of its catalog row, not a
    // decision this file makes: a generator added to `aria-models` names its
    // lane there, and this message follows without an arm here.
    let spec = generator.spec();
    Err(Rejection::new(format!(
        "track {} is a {} lane; the {} generator needs a {} track",
        track.id.raw(),
        track.kind.spec().id,
        spec.id,
        spec.lane.spec().id,
    )))
}
