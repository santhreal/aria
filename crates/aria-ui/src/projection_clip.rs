//! Clip: 10 helpers from [`super`].
//!
//! Split out of `projection.rs` to stay under the file ceiling. A pure
//! move. Each item is `pub(super)` so the parent and its other children can
//! reach it by name; nothing else changed.

use super::effect::{effect_to_ui, track_tint};
use super::*;
use gpui::SharedString;

const MEDIA_STATUS_TTL: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
struct MediaStatus {
    checked_at: std::time::Instant,
    missing: bool,
    modified: Option<std::time::SystemTime>,
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
        let metadata = probe(path).ok();
        let status = MediaStatus {
            checked_at: now,
            missing: metadata.is_none(),
            modified: metadata.and_then(|metadata| metadata.modified().ok()),
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

pub(super) fn track_to_ui(project: &Project, track: &Track, selection: &[ClipId]) -> UiTrack {
    let ordered = track.clips_ordered();
    let clips = ordered
        .iter()
        .enumerate()
        .map(|(index, clip)| {
            let abuts_right = ordered
                .get(index + 1)
                .is_some_and(|next| clip.timeline.end_tick() == next.timeline.start.value);
            clip_to_ui(project, clip, track, selection, abuts_right)
        })
        .collect();
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

pub(super) fn clip_to_ui(
    project: &Project,
    clip: &Clip,
    track: &Track,
    selection: &[ClipId],
    abuts_right: bool,
) -> UiClip {
    let transform = clip.transform.sample(0);
    let (
        media,
        source_start,
        source_end,
        source_start_ticks,
        source_end_ticks,
        name,
        kind,
        generator,
    ) = match &clip.content {
        ClipSource::Media { media, source } => (
            Some(media_id(*media)),
            source.start.seconds(),
            source
                .end()
                .map(|t| t.seconds())
                .unwrap_or(source.start.seconds()),
            source.start.value,
            source.end_tick(),
            media_clip_name(media.raw()).into(),
            media_clip_kind(track.kind),
            None,
        ),
        ClipSource::Generated(generator) => {
            let (kind, name, ui_gen) = generator_view(generator);
            (None, 0.0, 0.0, 0, 0, name, kind, Some(ui_gen))
        }
        ClipSource::Multicam { group, source } => (
            None,
            source.start.seconds(),
            source
                .end()
                .map(|t| t.seconds())
                .unwrap_or(source.start.seconds()),
            source.start.value,
            source.end_tick(),
            SharedString::from(format!("Multicam {}", group.raw())),
            media_clip_kind(track.kind),
            None,
        ),
        ClipSource::Sequence { sequence, source } => (
            None,
            source.start.seconds(),
            source
                .end()
                .map(|t| t.seconds())
                .unwrap_or(source.start.seconds()),
            source.start.value,
            source.end_tick(),
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
    let caps = ClipCapabilities::for_clip(project, clip, track.kind);
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
    let frame_rate = project.timeline().frame_rate;
    let duration_label = crate::timecode::format_ticks(
        (clip.timeline.end_tick() - clip.timeline.start.value).max(0),
        frame_rate.num,
        frame_rate.den,
    )
    .into();

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
        // The operator's own name outranks the one the source would give: a
        // renamed clip keeps its name through a relink or a media swap.
        name: clip.name.clone().map(Into::into).unwrap_or(name),
        source_start,
        source_end,
        timeline_start: clip.timeline.start.seconds(),
        timeline_end: clip
            .timeline
            .end()
            .map(|t| t.seconds())
            .unwrap_or(clip.timeline.start.seconds()),
        source_start_ticks,
        source_end_ticks,
        timeline_start_ticks: clip.timeline.start.value,
        timeline_end_ticks: clip.timeline.end_tick(),
        duration_label,
        thumbnail: None,
        color: track_tint(track.kind),
        label: clip.label,
        selected: selection.contains(&clip.id),
        transform: UiClipTransform {
            position_x: transform.position[0],
            position_y: transform.position[1],
            scale_x: transform.scale.x,
            scale_y: transform.scale.y,
            rotation: transform.rotation,
            anchor_x: transform.anchor_point[0],
            anchor_y: transform.anchor_point[1],
            opacity: transform.opacity,
        },
        filter: clip.filter.as_ref().map(filter_to_ui),
        effects: clip.effects.iter().map(effect_to_ui).collect(),
        generator,
        kind,
        locked: track.locked,
        linked_to: clip.link.map(|id| id.raw()),
        volume: clip.volume.sample(0),
        pan: clip.pan.sample(0),
        fade_in_ticks: clip.fade_in,
        fade_out_ticks: clip.fade_out,
        reversed: clip.reversed,
        speed_num: clip.speed.num,
        speed_den: clip.speed.den,
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
        mask: clip.mask.as_ref().map(|mask| UiMask {
            kind: mask.kind,
            feather: mask.feather.sample(0),
            center: mask.center.sample(0),
            size: mask.size.sample(0),
            rotation: mask.rotation.sample(0),
            roundness: mask.roundness.sample(0),
            invert: mask.invert,
        }),
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
        transition: track.transition_at(clip.id).map(|transition| UiTransition {
            id: transition.transition_id.clone().into(),
            label: aria_models::transition_spec(&transition.transition_id)
                .map(|spec| SharedString::from(spec.label.clone()))
                .unwrap_or_else(|| "Transition".into()),
            duration_ticks: transition.duration,
        }),
        can_transition: track.kind.supports_transitions() && abuts_right,
        replaceable: clip.replaceable.as_ref().map(|slot| UiSlot {
            order: slot.order,
            accepts: slot.accepts,
            label: slot.label.clone().map(Into::into),
        }),
        text_editable: clip.text_editable,
        keyframe_ticks: keyframe_ticks(clip),
        beat_ticks: clip.beat_timeline_ticks(),
        text_effect,
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
    aria_models::visit_clip_params(clip, |spec| {
        ticks.extend(
            spec.keyframes(clip)
                .into_iter()
                .map(|(tick, _)| clip.param_timeline_tick(tick) - clip.timeline.start.value),
        );
    });
    ticks.sort_unstable();
    ticks.dedup();
    ticks
}

pub(super) fn media_to_ui(media: &MediaSource, project: &Project) -> UiMediaItem {
    let name = media
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("media")
        .to_string()
        .into();
    let position = project
        .pool_item_position(media.bin, media.id)
        .map(|pos| (pos.x, pos.y));
    UiMediaItem {
        id: media_id(media.id),
        name,
        path: media.path.display().to_string().into(),
        width: media.width,
        height: media.height,
        frame_rate: media.frame_rate,
        duration_ticks: media.duration.value,
        has_audio: media.has_audio,
        kind: match media.kind() {
            MediaKind::Audio => UiClipKind::Audio,
            MediaKind::Image | MediaKind::Video => UiClipKind::Media,
        },
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

pub(super) fn media_clip_name(id: u64) -> String {
    format!("Media {id}")
}

pub(super) fn generator_view(generator: &Generator) -> (UiClipKind, SharedString, UiGenerator) {
    match generator {
        Generator::Text { content, .. } => (
            UiClipKind::Text,
            content.clone().into(),
            UiGenerator {
                kind: "text".into(),
                content: content.clone().into(),
                params: Vec::new(),
            },
        ),
        Generator::SolidColor { rgba } => (
            UiClipKind::Generator,
            "Solid".into(),
            UiGenerator {
                kind: "solid".into(),
                content: SharedString::default(),
                params: rgba.iter().map(|c| f32::from(*c) / 255.0).collect(),
            },
        ),
        Generator::Shape { .. } => (
            UiClipKind::Shape,
            "Shape".into(),
            UiGenerator {
                kind: "shape".into(),
                content: SharedString::default(),
                params: Vec::new(),
            },
        ),
        Generator::Sticker { asset } => (
            UiClipKind::Generator,
            asset.clone().into(),
            UiGenerator {
                kind: "sticker".into(),
                content: asset.clone().into(),
                params: Vec::new(),
            },
        ),
        Generator::Lottie {
            path,
            width,
            height,
        } => (
            UiClipKind::Generator,
            path.clone().into(),
            UiGenerator {
                kind: "lottie".into(),
                content: path.clone().into(),
                params: vec![*width as f32, *height as f32],
            },
        ),
        Generator::Effect => (
            UiClipKind::Generator,
            "Effect".into(),
            UiGenerator {
                kind: "effect".into(),
                content: SharedString::default(),
                params: Vec::new(),
            },
        ),
        Generator::Filter => (
            UiClipKind::Generator,
            "Filter".into(),
            UiGenerator {
                kind: "filter".into(),
                content: SharedString::default(),
                params: Vec::new(),
            },
        ),
        Generator::Adjustment => (
            UiClipKind::Generator,
            "Adjustment".into(),
            UiGenerator {
                kind: "adjustment".into(),
                content: SharedString::default(),
                params: Vec::new(),
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
        assert!(cache.by_path[path].modified.is_none());

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
        assert!(cache.by_path[path].modified.is_some());
        assert_eq!(probes.load(Ordering::Relaxed), 2);
    }
}
