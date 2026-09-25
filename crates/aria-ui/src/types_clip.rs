//! Clip, track and media view models: the shapes the timeline and library render.
//!
//! Moved out of `types.rs` to stay under the file ceiling. Each type travels
//! with its own `impl` blocks, and `types.rs` re-exports this module with
//! `pub use`, so every existing `crate::types::Name` path still resolves.

use super::*;

macro_rules! ui_track_kinds {
    (
        $(
            $(#[$meta:meta])*
            $ui:ident => [$($model:path),+ $(,)?]
        ),+ $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum UiTrackKind {
            $(
                $(#[$meta])*
                $ui,
            )+
        }

        impl UiTrackKind {
            pub fn from_model(kind: aria_models::TrackKind) -> Self {
                let ui = match kind {
                    $($($model)|+ => Self::$ui,)+
                };
                ui.census();
                ui
            }

            const fn census(self) {
                match self {
                    $(Self::$ui => {},)+
                }
            }
        }
    };
}

ui_track_kinds! {
    Video => [aria_models::TrackKind::Video],
    Audio => [aria_models::TrackKind::Audio],
    Text => [aria_models::TrackKind::Text],
    Generator => [
        aria_models::TrackKind::Effect,
        aria_models::TrackKind::Filter,
        aria_models::TrackKind::Sticker,
    ],
    /// A lane whose bars grade what is below them.
    Adjustment => [aria_models::TrackKind::Adjustment],
    /// A bus. It holds no clips and no drop targets it.
    Submix => [aria_models::TrackKind::Submix],
}

macro_rules! ui_clip_kinds {
    ($($kind:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum UiClipKind {
            $($kind,)+
        }

        impl UiClipKind {
            /// Every kind in declaration order.
            pub const ALL: [UiClipKind; ui_clip_kinds!(@count $($kind),+)] = [
                $(UiClipKind::$kind,)+
            ];

            /// An exhaustive production match. A variant inserted outside this
            /// registry is therefore a compile error, not an omitted UI kind.
            #[allow(dead_code)]
            pub(crate) const fn census(self) {
                match self {
                    $(Self::$kind => {},)+
                }
            }
        }
    };
    (@count $($kind:ident),+) => {
        <[()]>::len(&[$(ui_clip_kinds!(@one $kind)),+])
    };
    (@one $kind:ident) => { () };
}

ui_clip_kinds!(Media, Text, Shape, Generator, Audio);

#[derive(Debug, Clone, PartialEq)]
pub enum UiEasing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier(f32, f32, f32, f32),
    BezierPreset(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiClipTransform {
    pub position_x: f32,
    pub position_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation: f32,
    pub anchor_x: f32,
    pub anchor_y: f32,
    pub opacity: f32,
}

impl Default for UiClipTransform {
    fn default() -> Self {
        Self {
            position_x: 0.0,
            position_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
            anchor_x: 0.5,
            anchor_y: 0.5,
            opacity: 1.0,
        }
    }
}

impl From<UiClipTransform> for aria_models::ClipTransform {
    /// The model transform a projected one describes. One owner, because the
    /// committed edit and the live preview of the same gesture must describe the
    /// same picture: a second conversion is how a drag comes to preview
    /// something other than what it commits.
    fn from(transform: UiClipTransform) -> Self {
        Self {
            position: [transform.position_x, transform.position_y],
            anchor_point: [transform.anchor_x, transform.anchor_y],
            scale: aria_models::Scale2 {
                x: transform.scale_x,
                y: transform.scale_y,
            },
            rotation: transform.rotation,
            opacity: transform.opacity.clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiFilter {
    pub id: SharedString,
    pub amount: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiEffectValue {
    Scalar(f32),
    Color([u8; 4]),
    Vec2([f32; 2]),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiEffectInstance {
    pub id: SharedString,
    pub name: SharedString,
    pub enabled: bool,
    /// One value per catalog slot, with the slot's declared representation.
    pub values: Vec<UiEffectValue>,
}

impl UiEffectInstance {
    pub fn scalar(&self, slot: usize) -> Option<f32> {
        match self.values.get(slot)? {
            UiEffectValue::Scalar(value) => Some(*value),
            UiEffectValue::Color(_) | UiEffectValue::Vec2(_) => None,
        }
    }

    pub fn color(&self, slot: usize) -> Option<[u8; 4]> {
        match self.values.get(slot)? {
            UiEffectValue::Color(value) => Some(*value),
            UiEffectValue::Scalar(_) | UiEffectValue::Vec2(_) => None,
        }
    }

    pub fn vec2(&self, slot: usize) -> Option<[f32; 2]> {
        match self.values.get(slot)? {
            UiEffectValue::Vec2(value) => Some(*value),
            UiEffectValue::Scalar(_) | UiEffectValue::Color(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiGenerator {
    pub kind: SharedString,
    pub content: SharedString,
    pub params: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiClip {
    pub id: u64,
    pub track_id: u64,
    pub media_id: Option<u64>,
    /// The sequence this clip plays, when it is a nest.
    ///
    /// A nest draws as a media clip — it has a picture, a source range and a
    /// name — so the kind cannot carry this. Without it no surface can offer
    /// to open what the operator is pointing at.
    pub nested_sequence: Option<u64>,
    pub name: SharedString,
    pub source_start: f64,
    pub source_end: f64,
    pub timeline_start: f64,
    pub timeline_end: f64,
    pub source_start_ticks: i64,
    pub source_end_ticks: i64,
    pub timeline_start_ticks: i64,
    pub timeline_end_ticks: i64,
    /// Formatted once with the projection rather than once per repaint.
    pub duration_label: SharedString,
    pub thumbnail: Option<Vec<u8>>,
    pub color: u32,
    /// The operator's label colour, when one is set. Overrides the kind tint on
    /// the clip card; `None` draws the lane's own colour.
    pub label: Option<aria_models::MarkerColor>,
    pub selected: bool,
    pub transform: UiClipTransform,
    pub filter: Option<UiFilter>,
    pub effects: Vec<UiEffectInstance>,
    pub generator: Option<UiGenerator>,
    pub kind: UiClipKind,
    pub locked: bool,
    pub linked_to: Option<u64>,
    pub volume: f32,
    pub pan: f32,
    pub fade_in_ticks: i64,
    pub fade_out_ticks: i64,
    pub reversed: bool,
    pub speed_num: i32,
    pub speed_den: i32,
    /// Engine-computed capabilities, so a surface never offers a refused action.
    pub can_split: bool,
    pub can_reverse: bool,
    pub can_extract_audio: bool,
    pub can_ripple_delete: bool,
    pub can_freeze_frame: bool,
    pub can_stabilize: bool,
    /// Look state, so a picker shows what is applied rather than a default.
    pub blend_mode: BlendMode,
    pub denoise: bool,
    pub preserve_pitch: bool,
    pub stabilize: Option<StabilizeLevel>,
    /// Framing state: crop window, mirroring, and motion blur.
    pub crop: CropRect,
    pub flip_h: bool,
    pub flip_v: bool,
    pub motion_blur: MotionBlur,
    /// Mask, chroma-key, and track-matte state, sampled at tick 0 for display.
    pub mask: Option<UiMask>,
    pub chroma: Option<UiChroma>,
    pub matte: Option<UiMatte>,
    /// Layer styles and the loaded LUT, flattened for display.
    pub styles: UiStyles,
    pub lut: Option<UiLut>,
    /// Slot animations and the audio-role tag.
    pub animation_in: Option<UiAnimation>,
    pub animation_out: Option<UiAnimation>,
    pub animation_combo: Option<UiAnimation>,
    pub audio_role: Option<AudioRole>,
    /// The transition at this clip's right junction, and whether that junction
    /// can carry one at all (lane supports transitions and a clip abuts).
    pub transition: Option<UiTransition>,
    pub can_transition: bool,
    /// Template authoring marks: the replaceable slot, and whether a text
    /// clip's content can be retyped when the project is used as a template.
    pub replaceable: Option<UiSlot>,
    pub text_editable: bool,
    /// Every keyframe tick on any animatable property, clip-local and deduped.
    /// A readout for the expanded strip; per-property editing goes through
    /// `AppState::keyframes_of`.
    pub keyframe_ticks: Vec<i64>,
    /// The clip's detected beat grid, already mapped to timeline ticks and
    /// clipped to this clip's source window (so a trim or a split shows only the
    /// beats it kept, and a retime moves them with the content).
    ///
    /// The project owns a beat grid; the analysis cache is only where it was
    /// measured. The ruler and the snap magnet read these.
    pub beat_ticks: Vec<i64>,
    /// The text-effect preset baked onto a title's style, if any. The Text tab
    /// marks this chip active.
    pub text_effect: Option<SharedString>,
}

/// A catalog animation applied to one slot, flattened for display.
#[derive(Debug, Clone, PartialEq)]
pub struct UiAnimation {
    pub id: SharedString,
    pub label: &'static str,
    pub speed: f32,
    pub intensity: f32,
    pub stagger: f32,
}

/// A transition at a clip's right junction, flattened for display.
#[derive(Debug, Clone, PartialEq)]
pub struct UiTransition {
    pub id: SharedString,
    /// Owned: a transition's label comes from the catalog file.
    pub label: SharedString,
    pub duration_ticks: i64,
}

/// A clip marked as a fillable template slot, flattened for display.
#[derive(Debug, Clone, PartialEq)]
pub struct UiSlot {
    pub order: u32,
    pub accepts: SlotMedia,
    pub label: Option<SharedString>,
}

/// A clip's shaped alpha mask, flattened for display.
///
/// Mirrors `aria_models::Mask`; `SetClipMask` carries the whole mask, so an edit
/// to one knob has to rebuild the rest from this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiMask {
    pub kind: MaskKind,
    pub feather: f32,
    pub center: [f32; 2],
    pub size: [f32; 2],
    pub rotation: f32,
    pub roundness: f32,
    pub invert: bool,
}

/// A clip's chroma key, flattened for display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiChroma {
    pub rgb: [u8; 3],
    pub strength: f32,
    pub shadow: f32,
}

/// A clip's track matte, flattened for display.
///
/// The stencil is the clip on the lane directly above; this only says how that
/// lane is read. `SetClipMatte` carries the whole value, so an edit to one knob
/// has to rebuild the rest from this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiMatte {
    pub source: MatteSource,
    pub invert: bool,
    pub softness: f32,
}

/// A clip's four layer-style blocks, flattened for display.
///
/// A `None` colour means the block is absent. `SetClipLayerStyles` carries all
/// four, so an edit to one has to rebuild the others from this.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UiStyles {
    pub shadow: Option<[u8; 4]>,
    pub shadow_offset: [f32; 2],
    pub shadow_blur: f32,
    pub glow: Option<[u8; 4]>,
    pub glow_radius: f32,
    pub glow_intensity: f32,
    pub outline: Option<[u8; 4]>,
    pub outline_width: f32,
    pub background: Option<[u8; 4]>,
    pub background_padding: f32,
    pub background_radius: f32,
}

/// The `.cube` LUT applied to a clip, flattened for display.
#[derive(Debug, Clone, PartialEq)]
pub struct UiLut {
    pub path: SharedString,
    pub intensity: f32,
}

impl UiLut {
    /// The file name, which is what a LUT row shows.
    pub fn name(&self) -> &str {
        std::path::Path::new(&*self.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiTrack {
    pub id: u64,
    pub kind: UiTrackKind,
    pub name: SharedString,
    pub height: f32,
    /// Draw waveforms on this lane's audio clips.
    pub waveform: bool,
    /// Draw filmstrips on this lane's media clips.
    pub filmstrip: bool,
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,
    pub enabled: bool,
    /// This lane takes source-monitor edits and media drops of its kind.
    pub targeted: bool,
    /// This lane rides along with an insert on another lane.
    pub sync_lock: bool,
    /// Voice lane: what a duck measures the dips from.
    pub duck_source: bool,
    pub collapsed: bool,
    pub clips: Vec<UiClip>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiMarker {
    pub id: u64,
    pub tick: i64,
    pub name: SharedString,
    pub color: MarkerColor,
    /// The marker's note, empty when it has none.
    pub comment: SharedString,
}

/// One plugin as the Plugins list shows it: what it says it is, what it actually
/// registered, whether it is mounted, and why it is not.
#[derive(Debug, Clone, PartialEq)]
pub struct UiPluginRow {
    pub id: SharedString,
    pub name: SharedString,
    pub version: SharedString,
    pub requires: Vec<SharedString>,
    /// Short service names, without their module path.
    pub services: Vec<SharedString>,
    /// Services this plugin asked for and did not declare in its manifest, so it
    /// was refused them. A plugin missing a declaration otherwise degrades in
    /// silence, because a surface with no service hides itself.
    pub denied: Vec<SharedString>,
    pub enabled: bool,
    /// What mounting this plugin cost, in milliseconds, or `None` for a plugin
    /// that is not mounted. A start is the sum of these, so the list that names
    /// the plugins is where the cost of having them belongs.
    pub mount_ms: Option<f32>,
    /// The reason the last mount attempt failed, when it did.
    pub error: Option<SharedString>,
}

/// One catalog as the Catalogs list shows it: how many entries are in effect,
/// whether they came from the user's file, and why a file on disk is not in use.
#[derive(Debug, Clone, PartialEq)]
pub struct UiCatalogRow {
    pub name: SharedString,
    pub entries: usize,
    pub overridden: bool,
    pub problem: Option<SharedString>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiSequence {
    pub tracks: Vec<UiTrack>,
    pub duration_ticks: i64,
    pub frame_rate: Rational,
    /// Shared rather than owned: the ruler and the Markers panel each read the
    /// whole list once per frame, and the projection rebuilds it whole.
    pub markers: std::sync::Arc<[UiMarker]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiMediaItem {
    pub id: u64,
    pub name: SharedString,
    pub path: SharedString,
    pub width: u32,
    pub height: u32,
    /// The source's own frame rate, which need not be the sequence's.
    pub frame_rate: Rational,
    pub duration_ticks: i64,
    pub has_audio: bool,
    pub kind: UiClipKind,
    pub missing: bool,
    /// The operator's grouping colour for this pool entry, when set.
    pub label: Option<aria_models::MarkerColor>,
    /// Unix seconds this entry was imported, when the project recorded one.
    pub imported_at: Option<u64>,
    /// Codec names the probe read, when the platform's demuxer named them. A
    /// row states "unknown" rather than inferring one from the extension.
    pub video_codec: Option<SharedString>,
    pub audio_codec: Option<SharedString>,
    /// The pool folder this entry sits in, or `None` at the pool root.
    pub bin: Option<u64>,
    /// Whether the renderer decodes this item through a proxy.
    ///
    /// Never saved and never read from the project: the substitution belongs to
    /// the renderer, and this is filled by the one pass that holds the engine.
    /// A pool row that had to ask per frame would take the engine lock per row.
    pub proxied: bool,
    /// The item's 2D position in Freeform view within its bin, if arranged.
    pub position: Option<(f32, f32)>,
}

/// A pool folder, as the sidebar draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct UiBin {
    pub id: u64,
    pub name: SharedString,
    pub parent: Option<u64>,
    /// Depth from the pool root, so a row indents without walking the tree
    /// again at paint time.
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiRulerTick {
    pub position_ticks: i64,
    pub label: SharedString,
    pub major: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiClipMove {
    pub clip_id: u64,
    pub delta_ticks: i64,
    pub new_track: Option<u64>,
}
