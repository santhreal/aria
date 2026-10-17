//! Project → UI presentation state, shaped in shared Rust.
//!
//! Both mobile shells render the same lane-stack editor: ordered rows around a
//! magnetic main track, PiP lanes above it, text/sticker/effect lanes between
//! it and the audio floor, audio lanes at the bottom. That ordering — plus all
//! the seconds/ticks conversion, keyframe surfacing, and label derivation — is
//! presentation *logic*, so it lives here rather than being reimplemented in
//! Swift and Kotlin. The result is a plain serde tree the shells decode into
//! their view models after every mutation.
//!
//! Wire shape: snake_case keys, seconds as `f64` (converted from ticks here,
//! so shells never do rational math), ids as raw `u64`. The `kind` strings on
//! lanes and clips are a stable protocol vocabulary.

use serde::Serialize;

use aria_models::{
    AudioRole, ChromaKey, Clip, ClipSource, ColorAdjustments, Filter, Generator, Mask, Param,
    Project, Rational, StabilizeLevel, TextStyle, Track, TrackId, TrackKind,
};
use aria_render::canvas_size;

/// Session flags riding alongside the project snapshot (owned by the engine,
/// not the model). Split out so `UiState::build` is a pure function of data
/// available in host tests without a GPU-backed engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionMeta {
    pub revision: u64,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// The whole editor presentation for one revision of the project.
#[derive(Debug, Serialize)]
pub struct UiState {
    pub revision: u64,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub name: String,
    /// Timeline frame rate (`{"num": 30, "den": 1}`).
    pub fps: Rational,
    /// End of the whole timeline (all lanes), in seconds.
    pub duration_seconds: f64,
    /// End of the sequential main track, in seconds.
    pub main_duration_seconds: f64,
    pub canvas: UiCanvas,
    /// Lane rows in UI order, top row first.
    pub lanes: Vec<UiLane>,
}

#[derive(Debug, Serialize)]
pub struct UiCanvas {
    /// Aspect preset by ratio name (`"auto"`, `"16:9"`, …) — the
    /// `CanvasAspect` wire names.
    pub aspect: aria_models::CanvasAspect,
    /// Opaque background color `[r, g, b]`.
    pub background: [u8; 3],
    /// Resolved output size in pixels (what preview/export composite).
    pub width: u32,
    pub height: u32,
}

/// One lane row. `kind` ∈ `video | text | sticker | effect | filter |
/// adjustment | audio`.
#[derive(Debug, Serialize)]
pub struct UiLane {
    pub id: u64,
    pub kind: &'static str,
    pub is_main: bool,
    pub enabled: bool,
    pub muted: bool,
    pub locked: bool,
    /// Clips on this lane ordered by start time.
    pub clips: Vec<UiClip>,
}

/// One placed clip. `kind` ∈ `video | image | audio | text | sticker | shape
/// | solid | effect | filter | adjustment`.
#[derive(Debug, Serialize)]
pub struct UiClip {
    pub id: u64,
    pub kind: &'static str,
    /// Display label: media file stem, text content, or the kind name.
    pub label: String,
    pub start_seconds: f64,
    pub length_seconds: f64,

    /// Media-backed fields (`None`/defaults for generated clips).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// In-point of the visible source window, in seconds of source time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trim_in_seconds: Option<f64>,
    /// Full length of the backing media, in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_duration_seconds: Option<f64>,
    pub has_audio: bool,
    pub is_image: bool,

    pub speed: f64,
    pub reversed: bool,
    /// Flat gain sampled at the clip start (envelope-aware displays come
    /// later; the slider writes a constant anyway).
    pub volume: f32,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,

    /// Canvas placement sampled at the clip start, in UI coordinates
    /// (`pos` 0..1 with 0.5 = canvas center). `None` on audio lanes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<UiTransform>,
    /// Clip-relative times (seconds) of transform keyframes, merged across
    /// properties — the diamonds the timeline draws.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keyframe_seconds: Vec<f64>,

    /// Text clips: content + full style (the model's `TextStyle` wire shape).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_style: Option<TextStyle>,
    /// Solid / shape fill sampled at the clip start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rgba: Option<[u8; 4]>,

    /// Effect-chain catalog ids, in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,
    /// Transition at this clip's right junction (main track).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_after: Option<UiTransition>,
    /// Link-group id (e.g. video + extracted audio).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<u64>,

    /// Look properties (persist-only this milestone; model wire shapes).
    /// The mask stack, top first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<Mask>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chroma_key: Option<ChromaKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stabilize: Option<StabilizeLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    #[serde(skip_serializing_if = "ColorAdjustments::is_neutral")]
    pub adjust: ColorAdjustments,
    /// Animation catalog ids per slot (combo excludes in/out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation_in: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation_out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation_combo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_role: Option<AudioRole>,
    /// Catalog id when the speed curve matches a preset exactly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_preset: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UiTransform {
    pub pos_x: f32,
    pub pos_y: f32,
    pub scale: f32,
    pub rotation_degrees: f32,
    pub opacity: f32,
}

#[derive(Debug, Serialize)]
pub struct UiTransition {
    pub id: String,
    pub duration_seconds: f64,
}

/// The sequential "main" lane (CapCut's magnetic track). Delegates to the
/// model's designated main track. `None` until a video track exists.
pub fn main_track(project: &Project) -> Option<TrackId> {
    project.timeline().main_track()
}

/// Lane rows in UI order (top row first):
///
/// 1. overlay video lanes, top = highest in the compositing stack;
/// 2. the main track;
/// 3. text / sticker / effect / filter / adjustment lanes, top = highest;
/// 4. audio lanes, top = *lowest* (the audio floor grows downward, so a newly
///    added audio lane lands at the bottom, matching the shells).
///
/// UI row position is a presentation convention, not the compositing order —
/// text lanes sit *below* the main row yet composite *above* it.
pub fn lane_rows(project: &Project) -> Vec<TrackId> {
    let main = main_track(project);
    let tracks: Vec<&Track> = project.timeline().tracks_ordered().collect();

    let mut rows: Vec<TrackId> = Vec::with_capacity(tracks.len());
    rows.extend(
        tracks
            .iter()
            .rev()
            .filter(|t| is_media_picture(t.kind) && Some(t.id) != main)
            .map(|t| t.id),
    );
    rows.extend(main);
    rows.extend(
        tracks
            .iter()
            .rev()
            .filter(|t| in_overlay_block(t.kind))
            .map(|t| t.id),
    );
    rows.extend(tracks.iter().filter(|t| t.kind.is_audio()).map(|t| t.id));
    rows
}

impl UiState {
    /// Shape `project` (plus the engine's session flags) into the UI tree.
    pub fn build(project: &Project, meta: SessionMeta) -> Self {
        let timeline = project.timeline();
        let rate = timeline.frame_rate;
        let main = main_track(project);
        let (width, height) = canvas_size(project);

        let lanes = lane_rows(project)
            .into_iter()
            .filter_map(|id| timeline.track(id))
            .map(|track| UiLane {
                id: track.id.raw(),
                kind: track.kind.id(),
                is_main: Some(track.id) == main,
                enabled: track.enabled,
                muted: track.muted,
                locked: track.locked,
                clips: track
                    .clips_ordered()
                    .into_iter()
                    .map(|clip| build_clip(project, track, clip, rate))
                    .collect(),
            })
            .collect();

        let main_duration_seconds = main
            .and_then(|id| timeline.track(id))
            .map(|t| ticks_to_seconds(t.content_end(), rate))
            .unwrap_or(0.0);

        UiState {
            revision: meta.revision,
            dirty: meta.dirty,
            can_undo: meta.can_undo,
            can_redo: meta.can_redo,
            name: project.name.clone(),
            fps: rate,
            duration_seconds: timeline.duration().seconds(),
            main_duration_seconds,
            canvas: UiCanvas {
                aspect: timeline.canvas().aspect,
                background: timeline.canvas().background,
                width,
                height,
            },
            lanes,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("UiState serialization is infallible")
    }
}

fn build_clip(project: &Project, track: &Track, clip: &Clip, rate: Rational) -> UiClip {
    let (kind, label, text, text_style, rgba) = describe_content(project, track, clip);

    let (media, path, trim_in, source_duration, has_audio, is_image) = match &clip.content {
        ClipSource::Media { media, source } => {
            let src = project.media(*media);
            (
                Some(media.raw()),
                src.map(|m| m.path().to_string_lossy().into_owned()),
                Some(source.start.seconds()),
                src.map(|m| m.duration.seconds()),
                src.is_some_and(|m| m.has_audio),
                src.is_some_and(|m| m.is_image),
            )
        }
        ClipSource::Generated(_) | ClipSource::Multicam { .. } | ClipSource::Sequence { .. } => {
            (None, None, None, None, false, false)
        }
    };

    let transform = track.kind.is_visual().then(|| {
        let t = clip.transform.sample(0);
        UiTransform {
            // Model position is an offset from canvas center normalized to
            // canvas dims; the shells use 0..1 with 0.5 at the center.
            pos_x: 0.5 + t.position[0],
            pos_y: 0.5 + t.position[1],
            scale: t.scale.x,
            rotation_degrees: t.rotation,
            opacity: t.opacity,
        }
    });

    let transition_after = track.transition_at(clip.id).map(|t| UiTransition {
        id: t.transition_id.clone(),
        duration_seconds: ticks_to_seconds(t.duration, rate),
    });

    UiClip {
        id: clip.id.raw(),
        kind,
        label,
        start_seconds: clip.timeline.start.seconds(),
        length_seconds: clip.timeline.duration.seconds(),
        media,
        path,
        trim_in_seconds: trim_in,
        source_duration_seconds: source_duration,
        has_audio,
        is_image,
        speed: clip.speed.as_f64(),
        reversed: clip.reversed,
        volume: clip.volume.sample(0),
        fade_in_seconds: ticks_to_seconds(clip.fade_in, rate),
        fade_out_seconds: ticks_to_seconds(clip.fade_out, rate),
        transform,
        keyframe_seconds: transform_keyframe_seconds(clip, rate),
        text,
        text_style,
        rgba,
        effects: clip.effects.iter().map(|e| e.effect_id.clone()).collect(),
        transition_after,
        link: clip.link.map(|l| l.raw()),
        masks: clip.masks.clone(),
        chroma_key: clip.chroma_key.clone(),
        stabilize: clip.stabilize,
        filter: clip.filter.clone(),
        adjust: clip.adjust.clone(),
        animation_in: clip.animation_in.as_ref().map(|a| a.id.clone()),
        animation_out: clip.animation_out.as_ref().map(|a| a.id.clone()),
        animation_combo: clip.animation_combo.as_ref().map(|a| a.id.clone()),
        audio_role: clip.audio_role,
        speed_preset: aria_models::speed_preset_id(&clip.speed_curve).map(str::to_owned),
    }
}

/// (kind, label, text, text_style, rgba) for one clip's content.
fn describe_content(
    project: &Project,
    track: &Track,
    clip: &Clip,
) -> (
    &'static str,
    String,
    Option<String>,
    Option<TextStyle>,
    Option<[u8; 4]>,
) {
    match &clip.content {
        ClipSource::Media { media, .. } => {
            let label = project
                .media(*media)
                .and_then(|m| {
                    m.path()
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "media".into());
            let kind = if track.kind.is_audio() {
                "audio"
            } else if project.media(*media).is_some_and(|m| m.is_image) {
                "image"
            } else {
                "video"
            };
            (kind, label, None, None, None)
        }
        ClipSource::Generated(generator) => match generator {
            Generator::Text { content, style } => (
                "text",
                content.clone(),
                Some(content.clone()),
                Some(style.clone()),
                None,
            ),
            Generator::SolidColor { rgba } => ("solid", "Color".into(), None, None, Some(*rgba)),
            Generator::Shape { rgba, .. } => {
                ("shape", "Shape".into(), None, None, Some(rgba.sample(0)))
            }
            Generator::Sticker { asset } => {
                let label = aria_models::sticker_spec(asset)
                    .map_or_else(|| "Sticker".into(), |s| s.label.to_owned());
                ("sticker", label, None, None, None)
            }
            Generator::Lottie { path, .. } => ("sticker", lottie_label(path), None, None, None),
            Generator::Effect => ("effect", "Effect".into(), None, None, None),
            Generator::Filter => ("filter", "Filter".into(), None, None, None),
            Generator::Adjustment => ("adjustment", "Adjustment".into(), None, None, None),
        },
        ClipSource::Multicam { group, .. } => (
            "multicam",
            format!("Multicam {}", group.raw()),
            None,
            None,
            None,
        ),
        ClipSource::Sequence { sequence, .. } => {
            let label = project
                .sequence(*sequence)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| format!("Sequence {}", sequence.raw()));
            ("sequence", label, None, None, None)
        }
    }
}

/// Merged, sorted clip-relative transform keyframe times in seconds.
/// Display label for a file-backed Lottie: the file stem, or a generic
/// fallback for degenerate paths.
fn lottie_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map_or_else(|| "Animation".to_owned(), str::to_owned)
}

fn transform_keyframe_seconds(clip: &Clip, rate: Rational) -> Vec<f64> {
    let mut ticks: Vec<i64> = Vec::new();
    fn collect<T>(param: &Param<T>, into: &mut Vec<i64>) {
        into.extend(param.keyframes().iter().map(|kf| kf.tick));
    }
    collect(&clip.transform.position, &mut ticks);
    collect(&clip.transform.anchor_point, &mut ticks);
    collect(&clip.transform.scale, &mut ticks);
    collect(&clip.transform.rotation, &mut ticks);
    collect(&clip.transform.opacity, &mut ticks);
    ticks.sort_unstable();
    ticks.dedup();
    ticks
        .into_iter()
        .map(|t| ticks_to_seconds(t, rate))
        .collect()
}

/// Whether a lane kind holds imported picture media — the overlay video group
/// that sits above the main row.
pub(crate) fn is_media_picture(kind: TrackKind) -> bool {
    let spec = kind.spec();
    spec.is_visual && spec.accepts_media
}

/// Whether a lane kind belongs to the overlay block: a picture lane that holds
/// generated content rather than imported media.
///
/// Read off the kind's own columns, so a new generated lane kind joins the block
/// by existing instead of by being added to a list here.
pub(crate) fn in_overlay_block(kind: TrackKind) -> bool {
    let spec = kind.spec();
    spec.is_visual && !spec.accepts_media
}

fn ticks_to_seconds(ticks: i64, rate: Rational) -> f64 {
    ticks as f64 * rate.seconds_per_unit()
}

#[cfg(test)]
#[path = "ui_state_tests.rs"]
mod tests;
