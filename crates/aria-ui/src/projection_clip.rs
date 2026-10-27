//! Clips and media as the timeline and the pool draw them.
//!
//! Split out of `projection.rs` to stay under the file ceiling. Each item is
//! `pub(super)`, so the parent and its other children reach it by name;
//! [`Placement`] is also read by the frame that draws a held drag.

use super::effect::{effect_to_ui, track_tint, transition_to_ui};
use super::lanes::{LinkFacts, MediaFact};
use super::*;
use aria_models::LabelColor;
use gpui::SharedString;

const MEDIA_STATUS_TTL: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
struct MediaStatus {
    checked_at: std::time::Instant,
    missing: bool,
}

#[derive(Default)]
struct MediaStatusCache {
    by_path: std::collections::HashMap<std::path::PathBuf, MediaStatus>,
}

impl MediaStatusCache {
    fn missing(&mut self, path: &std::path::Path, now: std::time::Instant) -> bool {
        self.missing_with(path, now, |path| std::fs::metadata(path))
    }

    fn missing_with(
        &mut self,
        path: &std::path::Path,
        now: std::time::Instant,
        probe: impl FnOnce(&std::path::Path) -> std::io::Result<std::fs::Metadata>,
    ) -> bool {
        if let Some(status) = self.by_path.get(path)
            && now.saturating_duration_since(status.checked_at) < MEDIA_STATUS_TTL
        {
            return status.missing;
        }
        let status = MediaStatus {
            checked_at: now,
            missing: probe(path).is_err(),
        };
        self.by_path.insert(path.to_path_buf(), status);
        status.missing
    }
}

static MEDIA_STATUS: std::sync::LazyLock<parking_lot::Mutex<MediaStatusCache>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(MediaStatusCache::default()));

fn media_missing(path: &std::path::Path) -> bool {
    MEDIA_STATUS.lock().missing(path, std::time::Instant::now())
}
pub(super) fn invalidate_media_statuses() {
    MEDIA_STATUS.lock().by_path.clear();
}

/// `track` as a lane holding `clips`. Everything but the clips is read from the
/// lane itself, so a lane whose clips carry over is still drawn as it is now.
pub(super) fn lane_to_ui(track: &Track, clips: UiClips) -> UiTrack {
    UiTrack {
        id: track_id(track.id),
        kind: UiTrackKind::from_model(track.kind),
        name: track.name.clone().into(),
        height: track.display.height as f32,
        waveform: track.display.waveform,
        filmstrip: track.display.filmstrip,
        muted: track.muted,
        solo: track.solo,
        locked: track.locked,
        enabled: track.enabled,
        targeted: track.targeted,
        sync_lock: track.sync_lock,
        duck_source: track.duck_source,
        collapsed: track.display.collapsed,
        channel: aria_models::SourceChannel::of_lane(track.kind),
        patched: false,
        clips,
    }
}

/// What a media, multicam, or sequence clip reads as: sound or picture.
///
/// The catalog answers it, so a lane kind added later cannot be drawn as
/// picture by omission — and the three carriers that ask cannot disagree.
fn media_clip_kind(kind: TrackKind) -> UiClipKind {
    if kind.is_audio() {
        UiClipKind::Audio
    } else {
        UiClipKind::Media
    }
}

/// The label `docs/spec/look.md` gives a clip with none of its own: Yellow on
/// an adjustment, effect, or filter lane, Tan for a nested sequence, Mango for
/// a generator, Forest for sound, and Teal for picture.
fn default_label(content: &ClipSource, track: TrackKind) -> LabelColor {
    let adjustment = match track {
        TrackKind::Effect | TrackKind::Filter | TrackKind::Adjustment => true,
        TrackKind::Video
        | TrackKind::Audio
        | TrackKind::Text
        | TrackKind::Sticker
        | TrackKind::Submix => false,
    };
    match content {
        _ if adjustment => LabelColor::ADJUSTMENT,
        ClipSource::Sequence { .. } => LabelColor::NESTED,
        ClipSource::Generated(_) => LabelColor::GENERATOR,
        ClipSource::Media { .. } | ClipSource::Multicam { .. } if track.is_audio() => {
            LabelColor::AUDIO
        }
        ClipSource::Media { .. } | ClipSource::Multicam { .. } => LabelColor::VIDEO,
    }
}

/// `clip` on `track` as its bar draws it. `links` holds what a linked clip
/// reads of its partners. `fact` is the pool entry of the media the clip
/// places, `None` for a clip placing none or placing media no longer in the
/// pool.
pub(super) fn clip_to_ui(
    project: &Project,
    clip: &Clip,
    track: &Track,
    selection: &[ClipId],
    abuts_right: bool,
    links: &LinkFacts,
    fact: Option<&MediaFact>,
) -> UiClip {
    let transform = clip.transform.sample(0);
    let (media, name, kind, generator) = match &clip.content {
        ClipSource::Media { media, .. } => (
            Some(media_id(*media)),
            fact.map_or_else(
                || SharedString::from(format!("Media {}", media.raw())),
                |fact| fact.name.clone(),
            ),
            media_clip_kind(track.kind),
            None,
        ),
        ClipSource::Generated(generator) => {
            let (kind, name, ui_gen) = generator_view(generator);
            (None, name, kind, Some(ui_gen))
        }
        ClipSource::Multicam { group, .. } => (
            None,
            project.multicam_group(*group).map_or_else(
                || SharedString::from(format!("Multicam {}", group.raw())),
                |group| group.name.clone().into(),
            ),
            media_clip_kind(track.kind),
            None,
        ),
        ClipSource::Sequence { sequence, .. } => (
            None,
            project
                .sequence(*sequence)
                .map(|s| SharedString::from(s.name.clone()))
                .unwrap_or_else(|| format!("Sequence {}", sequence.raw()).into()),
            media_clip_kind(track.kind),
            None,
        ),
    };

    // Capabilities come from the engine's own computation so a menu can never
    // offer an action the command layer would refuse.
    let caps = ClipCapabilities::for_clip_with(project, clip, track.kind, &links.audio);
    let drift = links.drifts.get(&clip.id).copied();
    // The text-effect chip the Text tab marks active. Read from the style rather
    // than remembered in the UI, so a preset applied by any path shows here.
    let text_effect = match &clip.content {
        ClipSource::Generated(Generator::Text { style, .. }) => {
            style.effect_preset.clone().map(SharedString::from)
        }
        ClipSource::Generated(
            Generator::SolidColor { .. }
            | Generator::Shape { .. }
            | Generator::Sticker { .. }
            | Generator::Lottie { .. }
            | Generator::Effect
            | Generator::Filter
            | Generator::Adjustment,
        )
        | ClipSource::Media { .. }
        | ClipSource::Multicam { .. }
        | ClipSource::Sequence { .. } => None,
    };
    let Placement {
        source_start,
        source_end,
        source_start_ticks,
        source_end_ticks,
        timeline_start,
        timeline_end,
        timeline_start_ticks,
        timeline_end_ticks,
        duration_label,
        reversed,
        speed_num,
        speed_den,
        speed_label,
        beat_ticks,
        keyframe_ticks,
        markers,
        line,
    } = Placement::of(clip, project.timeline().frame_rate, track.kind.is_audio());

    UiClip {
        id: clip_id(clip.id),
        track_id: track_id(track.id),
        media_id: media,
        nested_sequence: match &clip.content {
            ClipSource::Sequence { sequence, .. } => Some(sequence.raw()),
            ClipSource::Media { .. } | ClipSource::Generated(_) | ClipSource::Multicam { .. } => {
                None
            }
        },
        // A given name outranks the one the source would give: a
        // renamed clip keeps its name through a relink or a media swap.
        name: clip.name.clone().map(Into::into).unwrap_or(name),
        source_start,
        source_end,
        timeline_start,
        timeline_end,
        source_start_ticks,
        source_end_ticks,
        timeline_start_ticks,
        timeline_end_ticks,
        duration_label,
        thumbnail: None,
        color: track_tint(track.kind),
        label: clip.label,
        default_label: default_label(&clip.content, track.kind),
        selected: selection.contains(&clip.id),
        transform: transform.into(),
        filter: clip.filter.as_ref().map(filter_to_ui),
        effects: clip.effects.iter().map(effect_to_ui).collect(),
        audio_inserts: crate::app::audio_inserts::rows(&clip.audio_effects),
        can_hold_inserts: project.clip_insert_refusal(clip, track).is_ok(),
        generator,
        kind,
        locked: track.locked,
        linked_to: clip.link.map(|id| id.raw()),
        grouped_to: clip.group.map(|id| id.raw()),
        parent: clip.parent.map(|id| id.raw()),
        enabled: clip.enabled,
        time_remap: clip.speed_curve.is_animated(),
        volume: clip.volume.sample(0),
        pan: clip.pan.sample(0),
        fade_in_ticks: clip.fade_in,
        fade_out_ticks: clip.fade_out,
        reversed,
        speed_num,
        speed_den,
        can_split: caps.allows(ClipAction::Split),
        can_reverse: caps.allows(ClipAction::Reverse),
        can_extract_audio: caps.allows(ClipAction::ExtractAudio),
        can_ripple_delete: caps.allows(ClipAction::RippleDelete),
        can_freeze_frame: caps.allows(ClipAction::FreezeFrame),
        can_stabilize: caps.allows(ClipAction::Stabilize),
        blend_mode: clip.blend_mode,
        denoise: clip.denoise,
        preserve_pitch: clip.preserve_pitch,
        stabilize: clip.stabilize,
        crop: clip.crop.sample(0),
        flip_h: clip.flip_h,
        flip_v: clip.flip_v,
        motion_blur: clip.motion_blur,
        masks: clip
            .masks
            .iter()
            .map(|mask| UiMask {
                kind: mask.kind,
                mode: mask.mode,
                feather: mask.feather.sample(0),
                center: mask.center.sample(0),
                size: mask.size.sample(0),
                rotation: mask.rotation.sample(0),
                roundness: mask.roundness.sample(0),
                opacity: mask.opacity.sample(0),
                expansion: mask.expansion.sample(0),
                invert: mask.invert,
            })
            .collect(),
        chroma: clip.chroma_key.as_ref().map(|chroma| UiChroma {
            rgb: chroma.rgb,
            strength: chroma.strength.sample(0),
            shadow: chroma.shadow.sample(0),
        }),
        matte: clip.matte.as_ref().map(|matte| UiMatte {
            source: matte.source,
            invert: matte.invert,
            softness: matte.softness.sample(0),
        }),
        styles: UiStyles {
            shadow: clip.styles.shadow.as_ref().map(|s| s.rgba.sample(0)),
            shadow_offset: clip
                .styles
                .shadow
                .as_ref()
                .map(|s| s.offset.sample(0))
                .unwrap_or([0.0, 0.0]),
            shadow_blur: clip
                .styles
                .shadow
                .as_ref()
                .map(|s| s.blur.sample(0))
                .unwrap_or(0.0),
            glow: clip.styles.glow.as_ref().map(|s| s.rgba.sample(0)),
            glow_radius: clip
                .styles
                .glow
                .as_ref()
                .map(|s| s.radius.sample(0))
                .unwrap_or(0.0),
            glow_intensity: clip
                .styles
                .glow
                .as_ref()
                .map(|s| s.intensity.sample(0))
                .unwrap_or(0.0),
            outline: clip.styles.outline.as_ref().map(|s| s.rgba.sample(0)),
            outline_width: clip
                .styles
                .outline
                .as_ref()
                .map(|s| s.width.sample(0))
                .unwrap_or(0.0),
            background: clip.styles.background.as_ref().map(|s| s.rgba.sample(0)),
            background_padding: clip
                .styles
                .background
                .as_ref()
                .map(|s| s.padding.sample(0))
                .unwrap_or(0.0),
            background_radius: clip
                .styles
                .background
                .as_ref()
                .map(|s| s.radius.sample(0))
                .unwrap_or(0.0),
        },
        lut: clip.lut.as_ref().map(|lut| UiLut {
            path: lut.path.clone().into(),
            intensity: lut.intensity.sample(0),
        }),
        animation_in: clip.animation_in.as_ref().map(animation_to_ui),
        animation_out: clip.animation_out.as_ref().map(animation_to_ui),
        animation_combo: clip.animation_combo.as_ref().map(animation_to_ui),
        audio_role: clip.audio_role,
        transition: track.transition_at(clip.id).map(transition_to_ui),
        transition_media: track.kind.transition_media().filter(|_| abuts_right),
        replaceable: clip.replaceable.as_ref().map(|slot| UiSlot {
            order: slot.order,
            accepts: slot.accepts,
            label: slot.label.clone().map(Into::into),
        }),
        text_editable: clip.text_editable,
        keyframe_ticks,
        beat_ticks,
        text_effect,
        clip_type: clip
            .clip_type
            .as_deref()
            .map(|id| clip_type_to_ui(clip, id)),
        missing_plugin: missing_plugin(clip, track),
        markers,
        speed_label,
        drift,
        line,
        offline: fact
            .filter(|fact| fact.missing)
            .map(|fact| super::bar::offline(&fact.path)),
        fx: super::bar::fx_badge(clip),
    }
}

/// Where a card's clip sits and how it plays its take: each field of the card
/// a trim, a move or a retime changes. The projection draws each card with
/// it, and a held drag draws each clip its release changes with it, so a held
/// card is drawn as the release projects it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placement {
    pub source_start: f64,
    pub source_end: f64,
    pub source_start_ticks: i64,
    pub source_end_ticks: i64,
    pub timeline_start: f64,
    pub timeline_end: f64,
    pub timeline_start_ticks: i64,
    pub timeline_end_ticks: i64,
    pub duration_label: SharedString,
    pub reversed: bool,
    pub speed_num: i32,
    pub speed_den: i32,
    pub speed_label: Option<SharedString>,
    pub beat_ticks: Vec<i64>,
    pub keyframe_ticks: Vec<i64>,
    pub markers: Vec<UiMarker>,
    pub line: Option<crate::types::UiClipLine>,
}

impl Placement {
    /// `clip`'s placement in a sequence at `rate`, on a sound lane when
    /// `audio`.
    pub(crate) fn of(clip: &Clip, rate: aria_models::Rational, audio: bool) -> Self {
        let seconds = |range: aria_models::TimeRange| {
            let start = range.start.seconds();
            (start, range.end().map_or(start, |end| end.seconds()))
        };
        let source = clip.source_range();
        let (source_start, source_end) = source.map_or((0.0, 0.0), seconds);
        let (timeline_start, timeline_end) = seconds(clip.timeline);
        let (start, end) = (clip.timeline.start.value, clip.timeline.end_tick());
        Self {
            source_start,
            source_end,
            source_start_ticks: source.map_or(0, |source| source.start.value),
            source_end_ticks: source.map_or(0, |source| source.end_tick()),
            timeline_start,
            timeline_end,
            timeline_start_ticks: start,
            timeline_end_ticks: end,
            duration_label: crate::timecode::format_ticks((end - start).max(0), rate.num, rate.den)
                .into(),
            reversed: clip.reversed,
            speed_num: clip.speed.num,
            speed_den: clip.speed.den,
            speed_label: super::bar::speed_label(clip),
            beat_ticks: clip.beat_timeline_ticks(),
            keyframe_ticks: keyframe_ticks(clip),
            markers: clip_markers_to_ui(clip),
            line: super::bar::clip_line(clip, audio),
        }
    }

    /// The placement `card` is drawn at.
    #[cfg(test)]
    pub(crate) fn of_card(card: &UiClip) -> Self {
        Self {
            source_start: card.source_start,
            source_end: card.source_end,
            source_start_ticks: card.source_start_ticks,
            source_end_ticks: card.source_end_ticks,
            timeline_start: card.timeline_start,
            timeline_end: card.timeline_end,
            timeline_start_ticks: card.timeline_start_ticks,
            timeline_end_ticks: card.timeline_end_ticks,
            duration_label: card.duration_label.clone(),
            reversed: card.reversed,
            speed_num: card.speed_num,
            speed_den: card.speed_den,
            speed_label: card.speed_label.clone(),
            beat_ticks: card.beat_ticks.clone(),
            keyframe_ticks: card.keyframe_ticks.clone(),
            markers: card.markers.clone(),
            line: card.line.clone(),
        }
    }

    /// Draw `card` at this placement.
    pub(crate) fn write(self, card: &mut UiClip) {
        let Self {
            source_start,
            source_end,
            source_start_ticks,
            source_end_ticks,
            timeline_start,
            timeline_end,
            timeline_start_ticks,
            timeline_end_ticks,
            duration_label,
            reversed,
            speed_num,
            speed_den,
            speed_label,
            beat_ticks,
            keyframe_ticks,
            markers,
            line,
        } = self;
        card.source_start = source_start;
        card.source_end = source_end;
        card.source_start_ticks = source_start_ticks;
        card.source_end_ticks = source_end_ticks;
        card.timeline_start = timeline_start;
        card.timeline_end = timeline_end;
        card.timeline_start_ticks = timeline_start_ticks;
        card.timeline_end_ticks = timeline_end_ticks;
        card.duration_label = duration_label;
        card.reversed = reversed;
        card.speed_num = speed_num;
        card.speed_den = speed_den;
        card.speed_label = speed_label;
        card.beat_ticks = beat_ticks;
        card.keyframe_ticks = keyframe_ticks;
        card.markers = markers;
        card.line = line;
    }
}

/// The badge a clip draws while a plugin behind it is missing, failed, or
/// disabled: the first id no catalog lists among its type, effects, and audio
/// effects, then the transition at its right edge. The clip keeps every id.
fn missing_plugin(clip: &aria_models::Clip, track: &aria_models::Track) -> Option<SharedString> {
    let id = clip.missing_plugin_id().or_else(|| {
        track
            .transition_at(clip.id)
            .map(|transition| transition.transition_id.as_str())
            .filter(|id| {
                aria_models::transition_spec(id).is_none()
                    && aria_models::AudioTransition::from_id(id).is_none()
            })
    })?;
    Some(format!("Missing plugin: {id}").into())
}

fn clip_markers_to_ui(clip: &aria_models::Clip) -> Vec<UiMarker> {
    clip.markers
        .iter()
        .filter_map(|marker| {
            let start = marker.tick.value;
            let (tick, duration_ticks) = if marker.duration.value > 0 {
                let end = start.saturating_add(marker.duration.value);
                let (tl_start, tl_end) = clip.timeline_span_for_clip_time(start, end)?;
                (tl_start, (tl_end - tl_start).max(0))
            } else {
                let tl_tick = clip.timeline_tick_for_clip_time(start)?;
                (tl_tick, 0)
            };
            Some(UiMarker {
                id: marker.id.raw(),
                tick,
                duration_ticks,
                name: marker.name.clone().into(),
                color: marker.color,
                comment: marker.comment.clone().into(),
                kind: marker.kind,
                clip_id: Some(clip.id.raw()),
                clip_tick: Some(marker.tick.value),
                clip_duration_ticks: Some(marker.duration.value),
                clip_rate: Some(marker.tick.rate),
            })
        })
        .collect()
}

/// Flatten the clip type `id` a clip holds. A type no plugin lists keeps its
/// id as its name and reports itself missing; the clip keeps its chain.
pub(super) fn clip_type_to_ui(clip: &aria_models::Clip, id: &str) -> UiClipType {
    let Some(spec) = aria_models::clip_type_spec(id) else {
        return UiClipType {
            id: SharedString::from(id),
            label: SharedString::from(id),
            colour: None,
            icon: None,
            missing: Some(format!("no plugin lists the clip type `{id}`").into()),
            handles: Vec::new(),
        };
    };
    let effect = clip
        .effects
        .iter()
        .position(|fx| fx.effect_id == spec.generator);
    let generator = aria_models::effect_spec(&spec.generator);
    let handles = match (effect, &generator) {
        (Some(effect), Some(generator)) => spec
            .handles
            .iter()
            .filter_map(|name| {
                let param = generator.params.iter().position(|param| {
                    param.name == *name
                        && matches!(
                            param.kind,
                            aria_models::EffectParamKind::Vec2
                                | aria_models::EffectParamKind::Point3
                        )
                })?;
                let slot = &generator.params[param];
                Some(UiTypeHandle {
                    effect,
                    param,
                    min: slot.min,
                    max: slot.max,
                })
            })
            .collect(),
        _ => Vec::new(),
    };
    let missing = match effect {
        Some(_) => spec.unavailable(),
        None => Some(format!("the chain holds no `{}`", spec.generator)),
    };
    UiClipType {
        id: SharedString::from(id),
        label: SharedString::from(spec.label.as_str()),
        colour: Some(spec.colour),
        icon: spec.icon.as_ref().map(crate::icons::clip_type_icon_path),
        missing: missing.map(SharedString::from),
        handles,
    }
}

/// Flatten a stored animation reference, resolving its catalog label so a panel
/// never re-derives one.
pub(super) fn animation_to_ui(animation: &aria_models::AnimationRef) -> UiAnimation {
    UiAnimation {
        id: animation.id.clone().into(),
        label: aria_models::animation_spec(&animation.id)
            .map(|spec| spec.label.as_str())
            .unwrap_or("Animation"),
        speed: animation.speed,
        intensity: animation.intensity,
        stagger: animation.stagger,
    }
}

/// The union of every animatable property's keyframe ticks, deduped and sorted.
///
/// The expanded strip draws one band per clip rather than one per property, so a
/// merged list is what it needs; a per-property list would only be re-merged.
///
/// Derived from the model's per-clip property list rather than named here. This
/// function used to chain six properties by hand — position, scale, rotation,
/// opacity, volume, pan — so a keyframe on a mask feather, a grade wheel, a text
/// style, a shape corner or an effect parameter existed in the model and left no
/// mark on the strip.
pub(super) fn keyframe_ticks(clip: &Clip) -> Vec<i64> {
    let mut ticks = Vec::new();
    let start = clip.timeline.start.value;
    aria_models::visit_clip_keyframes(clip, |tick, _| {
        ticks.push(clip.param_timeline_tick(tick) - start);
    });
    ticks.sort_unstable();
    ticks.dedup();
    ticks
}

/// What a media file is called in the pool, and on the card of a clip that
/// places it with no name of its own: the file name without its extension.
fn media_name(media: &MediaSource) -> SharedString {
    media
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("media")
        .to_string()
        .into()
}

pub(super) fn media_to_ui(media: &MediaSource, project: &Project) -> UiMediaItem {
    let name = media_name(media);
    let position = project
        .pool_item_position(media.bin, media.id)
        .map(|pos| (pos.x, pos.y));
    // The length placement gives the whole source, so the pool and the clip it
    // drops read the same timecode whatever rate the file was probed at.
    let rate = project.timeline().frame_rate;
    let placed = aria_models::resample(media.duration, rate).value;
    UiMediaItem {
        id: media_id(media.id),
        name,
        path: media.path.display().to_string().into(),
        width: media.width,
        height: media.height,
        frame_rate: media.frame_rate,
        duration_ticks: media.duration.value,
        duration_label: crate::timecode::format_ticks(placed, rate.num, rate.den).into(),
        has_audio: media.has_audio,
        kind: match media.kind() {
            MediaKind::Audio => UiClipKind::Audio,
            MediaKind::Image | MediaKind::Video => UiClipKind::Media,
        },
        still: media.kind() == MediaKind::Image,
        missing: media_missing(&media.path),
        label: media.label,
        imported_at: media.imported_at,
        video_codec: media.video_codec.clone().map(Into::into),
        audio_codec: media.audio_codec.clone().map(Into::into),
        bin: media.bin.map(|bin| bin.raw()),
        // The project does not record a substitution; whoever holds the engine
        // fills this in after the projection is built.
        proxied: false,
        position,
    }
}

/// The pool folders in draw order: each parent immediately before its children,
/// siblings by name, with the depth each row indents by.
///
/// The walk starts from the roots and only follows parents that exist, so a file
/// whose parent went missing still lists its bins — at the root, where they can
/// be seen and moved — instead of dropping a subtree.
pub(super) fn bins_to_ui(project: &Project) -> Vec<crate::types::UiBin> {
    let known: std::collections::HashSet<u64> =
        project.bin_iter().map(|bin| bin.id.raw()).collect();
    let mut children: std::collections::HashMap<Option<u64>, Vec<(&str, u64)>> =
        std::collections::HashMap::new();
    for bin in project.bin_iter() {
        let parent = bin
            .parent
            .map(|id| id.raw())
            .filter(|id| known.contains(id));
        children
            .entry(parent)
            .or_default()
            .push((bin.name.as_str(), bin.id.raw()));
    }
    for row in children.values_mut() {
        row.sort_unstable();
    }
    let mut out = Vec::with_capacity(known.len());
    let mut stack: Vec<(Option<u64>, usize)> = vec![(None, 0)];
    // Depth-first, deepest work last: a parent is pushed before its children so
    // the emitted order is the order the panel draws.
    while let Some((parent, depth)) = stack.pop() {
        if let Some(id) = parent {
            let name = project
                .bin(aria_models::BinId::from_raw(id))
                .map(|bin| SharedString::from(bin.name.clone()))
                .unwrap_or_default();
            out.push(crate::types::UiBin {
                id,
                name,
                parent: project
                    .bin(aria_models::BinId::from_raw(id))
                    .and_then(|bin| bin.parent)
                    .map(|bin| bin.raw())
                    .filter(|bin| known.contains(bin)),
                depth: depth - 1,
            });
        }
        if let Some(row) = children.get(&parent) {
            for (_, id) in row.iter().rev() {
                stack.push((Some(*id), depth + 1));
            }
        }
    }
    out
}

pub(super) fn generator_view(generator: &Generator) -> (UiClipKind, SharedString, UiGenerator) {
    match generator {
        Generator::Text { content, style } => {
            let content = SharedString::from(content.clone());
            (
                UiClipKind::Text,
                content.clone(),
                UiGenerator {
                    label: content.clone(),
                    kind: "text".into(),
                    content,
                    params: Vec::new(),
                    swatch: Some(style.fill.sample(0)),
                },
            )
        }
        Generator::SolidColor { rgba } => (
            UiClipKind::Generator,
            "Solid".into(),
            UiGenerator {
                label: "Solid".into(),
                kind: "solid".into(),
                content: SharedString::default(),
                params: rgba.iter().map(|c| f32::from(*c) / 255.0).collect(),
                swatch: Some(*rgba),
            },
        ),
        Generator::Shape { rgba, .. } => (
            UiClipKind::Shape,
            "Shape".into(),
            UiGenerator {
                label: "Shape".into(),
                kind: "shape".into(),
                content: SharedString::default(),
                params: Vec::new(),
                swatch: Some(rgba.sample(0)),
            },
        ),
        Generator::Sticker { asset } => {
            let asset = SharedString::from(asset.clone());
            (
                UiClipKind::Generator,
                asset.clone(),
                UiGenerator {
                    label: asset.clone(),
                    kind: "sticker".into(),
                    content: asset,
                    params: Vec::new(),
                    swatch: None,
                },
            )
        }
        Generator::Lottie {
            path,
            width,
            height,
        } => {
            let path = SharedString::from(path.clone());
            (
                UiClipKind::Generator,
                path.clone(),
                UiGenerator {
                    label: path.clone(),
                    kind: "lottie".into(),
                    content: path,
                    params: vec![*width as f32, *height as f32],
                    swatch: None,
                },
            )
        }
        Generator::Effect => (
            UiClipKind::Generator,
            "Effect".into(),
            UiGenerator {
                label: "Effect".into(),
                kind: "effect".into(),
                content: SharedString::default(),
                params: Vec::new(),
                swatch: None,
            },
        ),
        Generator::Filter => (
            UiClipKind::Generator,
            "Filter".into(),
            UiGenerator {
                label: "Filter".into(),
                kind: "filter".into(),
                content: SharedString::default(),
                params: Vec::new(),
                swatch: None,
            },
        ),
        Generator::Adjustment => (
            UiClipKind::Generator,
            "Adjustment".into(),
            UiGenerator {
                label: "Adjustment".into(),
                kind: "adjustment".into(),
                content: SharedString::default(),
                params: Vec::new(),
                swatch: None,
            },
        ),
    }
}

pub(super) fn filter_to_ui(filter: &Filter) -> UiFilter {
    UiFilter {
        id: filter.id.clone().into(),
        amount: filter.intensity.sample(0),
    }
}

#[cfg(test)]
mod media_status_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn missing_media_status_is_reused_until_revalidation() {
        let mut cache = MediaStatusCache::default();
        let path = std::path::Path::new("cached-media.mp4");
        let start = std::time::Instant::now();
        let probes = AtomicUsize::new(0);

        let missing = cache.missing_with(path, start, |_| {
            probes.fetch_add(1, Ordering::Relaxed);
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"))
        });
        assert!(missing);
        assert!(cache.by_path[path].missing);

        let missing = cache.missing_with(path, start + MEDIA_STATUS_TTL / 2, |_| {
            panic!("a fresh cached status performed another filesystem probe")
        });
        assert!(missing);
        assert_eq!(probes.load(Ordering::Relaxed), 1);

        let missing = cache.missing_with(path, start + MEDIA_STATUS_TTL, |_| {
            probes.fetch_add(1, Ordering::Relaxed);
            std::fs::metadata(env!("CARGO_MANIFEST_PATH"))
        });
        assert!(!missing);
        assert!(!cache.by_path[path].missing);
        assert_eq!(probes.load(Ordering::Relaxed), 2);
    }
}
