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

impl From<aria_models::ClipTransform> for UiClipTransform {
    /// A sampled model transform as the editor holds it, the inverse of the
    /// conversion below.
    fn from(transform: aria_models::ClipTransform) -> Self {
        Self {
            position_x: transform.position[0],
            position_y: transform.position[1],
            scale_x: transform.scale.x,
            scale_y: transform.scale.y,
            rotation: transform.rotation,
            anchor_x: transform.anchor_point[0],
            anchor_y: transform.anchor_point[1],
            opacity: transform.opacity,
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

#[derive(Debug, Clone, PartialEq)]
pub enum UiEffectValue {
    Scalar(f32),
    Int(i32),
    Bool(bool),
    Choice(SharedString),
    Color([u8; 4]),
    Vec2([f32; 2]),
    Point3([f32; 3]),
    Curve(Vec<aria_models::EffectCurvePoint>),
    Gradient(Vec<aria_models::EffectGradientStop>),
    Text(SharedString),
    Layer(Option<u64>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiEffectInstance {
    pub id: SharedString,
    pub name: SharedString,
    /// Whether the effect is switched off: drawn in the chain, rendered by
    /// nothing.
    pub bypass: bool,
    /// One value per catalog slot, with the slot's declared representation.
    pub values: Vec<UiEffectValue>,
    /// Visibility per catalog slot (`true` when `visible_when` is satisfied or absent).
    pub visible: Vec<bool>,
}

impl UiEffectInstance {
    pub fn scalar(&self, slot: usize) -> Option<f32> {
        match self.values.get(slot)? {
            UiEffectValue::Scalar(value) => Some(*value),
            UiEffectValue::Int(value) => Some(*value as f32),
            _ => None,
        }
    }

    pub fn int(&self, slot: usize) -> Option<i32> {
        match self.values.get(slot)? {
            UiEffectValue::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn boolean(&self, slot: usize) -> Option<bool> {
        match self.values.get(slot)? {
            UiEffectValue::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn bool_val(&self, slot: usize) -> Option<bool> {
        self.boolean(slot)
    }

    pub fn choice(&self, slot: usize) -> Option<&str> {
        match self.values.get(slot)? {
            UiEffectValue::Choice(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    pub fn color(&self, slot: usize) -> Option<[u8; 4]> {
        match self.values.get(slot)? {
            UiEffectValue::Color(value) => Some(*value),
            _ => None,
        }
    }

    pub fn vec2(&self, slot: usize) -> Option<[f32; 2]> {
        match self.values.get(slot)? {
            UiEffectValue::Vec2(value) => Some(*value),
            _ => None,
        }
    }

    pub fn point3(&self, slot: usize) -> Option<[f32; 3]> {
        match self.values.get(slot)? {
            UiEffectValue::Point3(value) => Some(*value),
            _ => None,
        }
    }

    pub fn curve(&self, slot: usize) -> Option<&[aria_models::EffectCurvePoint]> {
        match self.values.get(slot)? {
            UiEffectValue::Curve(points) => Some(points.as_slice()),
            _ => None,
        }
    }

    pub fn gradient(&self, slot: usize) -> Option<&[aria_models::EffectGradientStop]> {
        match self.values.get(slot)? {
            UiEffectValue::Gradient(stops) => Some(stops.as_slice()),
            _ => None,
        }
    }

    pub fn text(&self, slot: usize) -> Option<&str> {
        match self.values.get(slot)? {
            UiEffectValue::Text(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    pub fn layer(&self, slot: usize) -> Option<Option<u64>> {
        match self.values.get(slot)? {
            UiEffectValue::Layer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn is_slot_visible(&self, slot: usize) -> bool {
        self.visible.get(slot).copied().unwrap_or(true)
    }

    pub fn is_visible(&self, slot: usize) -> bool {
        self.is_slot_visible(slot)
    }
}
/// One parameter of an audio insert, as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct UiInsertParam {
    pub label: SharedString,
    /// The value at the chain's first tick.
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// Whether the parameter is keyframed.
    pub automated: bool,
}

/// One audio insert in a chain, as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct UiInsert {
    pub effect_id: SharedString,
    /// The catalog's label, or the id marked as not installed.
    pub label: SharedString,
    pub installed: bool,
    pub bypass: bool,
    /// One per catalog slot, in slot order. Empty when not installed.
    pub params: Vec<UiInsertParam>,
    /// Whether the effect reads a key signal from another lane.
    pub reads_key: bool,
    /// The lane keying it, if any.
    pub key: Option<u64>,
}

/// A lane that can key an insert on one chain.
#[derive(Debug, Clone, PartialEq)]
pub struct UiKeyChoice {
    pub track: u64,
    pub name: SharedString,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiGenerator {
    /// The generator's own name, whatever the clip is renamed to: a title's
    /// text, `Solid`, `Shape`, a sticker's asset. The content strip prints it.
    pub label: SharedString,
    pub kind: SharedString,
    pub content: SharedString,
    pub params: Vec<f32>,
    /// The colour the generator fills with at the clip's first frame (RGBA,
    /// 0-255): a solid's colour, a shape's fill, a title's text fill. The
    /// content strip draws it as a swatch.
    pub swatch: Option<[u8; 4]>,
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
    /// The operator's label colour, when one is set. `None` draws the clip
    /// type's colour, else [`Self::default_label`].
    pub label: Option<aria_models::LabelColor>,
    /// The label `docs/spec/look.md` gives a clip of this content on this lane.
    pub default_label: aria_models::LabelColor,
    pub selected: bool,
    pub transform: UiClipTransform,
    pub filter: Option<UiFilter>,
    pub effects: Vec<UiEffectInstance>,
    /// The clip's audio insert chain, in signal order.
    pub audio_inserts: Vec<UiInsert>,
    /// Whether the clip carries sound, so an insert chain on it is accepted.
    pub can_hold_inserts: bool,
    pub generator: Option<UiGenerator>,
    pub kind: UiClipKind,
    pub locked: bool,
    pub linked_to: Option<u64>,
    pub grouped_to: Option<u64>,
    /// The clip this one is parented to.
    pub parent: Option<u64>,
    pub enabled: bool,
    pub time_remap: bool,
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
    /// The mask stack, top first, then chroma-key and track-matte state, all
    /// sampled at tick 0 for display.
    pub masks: Vec<UiMask>,
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
    /// The transition at this clip's right junction, and which kind that
    /// junction takes: `None` when the lane takes none or no clip abuts.
    pub transition: Option<UiTransition>,
    pub transition_media: Option<aria_models::TransitionMedia>,
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
    /// The plugin clip type the clip was placed as, when it was.
    pub clip_type: Option<UiClipType>,
    /// `Missing plugin: <id>` while a plugin behind the clip is missing,
    /// failed, or disabled: the first id no catalog lists among the clip's
    /// type, effects, audio effects, and the transition at its right edge.
    /// The timeline draws it as the clip's badge.
    pub missing_plugin: Option<SharedString>,
    /// Markers attached to this clip, projected into timeline ticks.
    pub markers: Vec<UiMarker>,
    /// The speed the name strip prints after the name, when it is not 100 %:
    /// `120%`, or `-100%` for a clip played backwards.
    pub speed_label: Option<SharedString>,
    /// Frames this clip sits from a linked clip placing the same file, while
    /// the link has drifted out of sync. Drawn on the bar as `+3` or `-3`.
    pub drift: Option<i64>,
    /// The property the bar draws as its line, with its keyframes.
    pub line: Option<UiClipLine>,
    /// The file name of the media the clip places, while that file is
    /// missing. The bar draws a red fill with "Media Offline" and this name.
    pub offline: Option<SharedString>,
    /// What the fx badge on the bar reports.
    pub fx: UiFxBadge,
}

impl UiClip {
    /// The label the clip is drawn in: the operator's, else the clip type's
    /// colour, else the default for its kind.
    pub fn drawn_label(&self) -> aria_models::LabelColor {
        self.label
            .or_else(|| {
                self.clip_type
                    .as_ref()
                    .and_then(|kind| kind.colour)
                    .map(aria_models::LabelColor::from)
            })
            .unwrap_or(self.default_label)
    }

    /// The animation in `slot`, if one is applied there.
    pub fn animation(&self, slot: aria_models::AnimationSlot) -> Option<&UiAnimation> {
        match slot {
            aria_models::AnimationSlot::In => self.animation_in.as_ref(),
            aria_models::AnimationSlot::Out => self.animation_out.as_ref(),
            aria_models::AnimationSlot::Combo => self.animation_combo.as_ref(),
        }
    }
}

/// A clip's plugin clip type, flattened for display.
#[derive(Debug, Clone, PartialEq)]
pub struct UiClipType {
    /// The catalog id the clip holds.
    pub id: SharedString,
    /// The type's name, or the id while no plugin lists the type.
    pub label: SharedString,
    /// The timeline colour, `None` while no plugin lists the type.
    pub colour: Option<aria_models::MarkerColor>,
    /// The icon's asset path.
    pub icon: Option<&'static str>,
    /// Why the type cannot draw now: no plugin lists it, or its generator is
    /// missing or has no parameter a handle names.
    pub missing: Option<SharedString>,
    /// The Viewer handles, in the order the type declares them.
    pub handles: Vec<UiTypeHandle>,
}

/// One Viewer handle of a clip type: a vec2 parameter of the generator in the
/// clip's chain. The value is a point on the clip's layer, as fractions of its
/// width and height: `[0, 0]` is the top left corner and `[1, 1]` the bottom
/// right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiTypeHandle {
    /// The generator's index in the clip's effect chain.
    pub effect: usize,
    /// The parameter's slot in the generator's catalog row.
    pub param: usize,
    /// The parameter's range, which a drag is held inside.
    pub min: f32,
    pub max: f32,
}

impl UiTypeHandle {
    /// The value at `fraction` of the layer, held inside the parameter's
    /// range.
    pub fn value_at(self, fraction: [f32; 2]) -> [f32; 2] {
        let (low, high) = (self.min.min(self.max), self.max.max(self.min));
        [fraction[0].clamp(low, high), fraction[1].clamp(low, high)]
    }
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
    pub alignment: aria_models::TransitionAlignment,
    /// One value per catalog slot, with the slot's declared representation.
    pub values: std::sync::Arc<[UiEffectValue]>,
    /// Visibility per catalog slot (`true` when `visible_when` is satisfied or absent).
    pub visible: std::sync::Arc<[bool]>,
}

impl UiTransition {
    /// Timeline tick window `[start, end)` when the cut sits at `cut`.
    pub fn window(&self, cut: i64) -> (i64, i64) {
        aria_models::transition_window(self.alignment, self.duration_ticks.max(1), cut)
    }

    pub fn scalar(&self, slot: usize) -> Option<f32> {
        match self.values.get(slot)? {
            UiEffectValue::Scalar(value) => Some(*value),
            UiEffectValue::Int(value) => Some(*value as f32),
            _ => None,
        }
    }

    pub fn int(&self, slot: usize) -> Option<i32> {
        match self.values.get(slot)? {
            UiEffectValue::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn boolean(&self, slot: usize) -> Option<bool> {
        match self.values.get(slot)? {
            UiEffectValue::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn bool_val(&self, slot: usize) -> Option<bool> {
        self.boolean(slot)
    }

    pub fn choice(&self, slot: usize) -> Option<&str> {
        match self.values.get(slot)? {
            UiEffectValue::Choice(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    pub fn color(&self, slot: usize) -> Option<[u8; 4]> {
        match self.values.get(slot)? {
            UiEffectValue::Color(value) => Some(*value),
            _ => None,
        }
    }

    pub fn vec2(&self, slot: usize) -> Option<[f32; 2]> {
        match self.values.get(slot)? {
            UiEffectValue::Vec2(value) => Some(*value),
            _ => None,
        }
    }

    pub fn point3(&self, slot: usize) -> Option<[f32; 3]> {
        match self.values.get(slot)? {
            UiEffectValue::Point3(value) => Some(*value),
            _ => None,
        }
    }

    pub fn curve(&self, slot: usize) -> Option<&[aria_models::EffectCurvePoint]> {
        match self.values.get(slot)? {
            UiEffectValue::Curve(points) => Some(points.as_slice()),
            _ => None,
        }
    }

    pub fn gradient(&self, slot: usize) -> Option<&[aria_models::EffectGradientStop]> {
        match self.values.get(slot)? {
            UiEffectValue::Gradient(stops) => Some(stops.as_slice()),
            _ => None,
        }
    }

    pub fn text(&self, slot: usize) -> Option<&str> {
        match self.values.get(slot)? {
            UiEffectValue::Text(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    pub fn layer(&self, slot: usize) -> Option<Option<u64>> {
        match self.values.get(slot)? {
            UiEffectValue::Layer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn is_slot_visible(&self, slot: usize) -> bool {
        self.visible.get(slot).copied().unwrap_or(true)
    }
}

/// A clip marked as a fillable template slot, flattened for display.
#[derive(Debug, Clone, PartialEq)]
pub struct UiSlot {
    pub order: u32,
    pub accepts: SlotMedia,
    pub label: Option<SharedString>,
}

/// One mask of a clip's mask stack, flattened for display.
///
/// Mirrors `aria_models::Mask` at tick 0. Edits start from the model mask, not
/// from this, so a keyframed property survives an edit to another one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiMask {
    pub kind: MaskKind,
    pub mode: MaskMode,
    pub feather: f32,
    pub center: [f32; 2],
    pub size: [f32; 2],
    pub rotation: f32,
    pub roundness: f32,
    pub opacity: f32,
    pub expansion: f32,
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
    /// The source channel this lane takes in a Source tab's edit, if any.
    pub channel: Option<aria_models::SourceChannel>,
    /// A Source tab's edit lands this lane's channel here. Set by
    /// [`UiSequence::mark_patched_lanes`].
    pub patched: bool,
    pub clips: Vec<UiClip>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiMarker {
    pub id: u64,
    pub tick: i64,
    pub duration_ticks: i64,
    pub name: SharedString,
    pub color: MarkerColor,
    /// The marker's note, empty when it has none.
    pub comment: SharedString,
    pub kind: MarkerKind,
    /// Owning clip id (`Some(clip_id)` for a clip marker, `None` for a sequence marker).
    pub clip_id: Option<u64>,
    /// Clip-source start tick when `clip_id.is_some()`.
    pub clip_tick: Option<i64>,
    /// Clip-source duration ticks when `clip_id.is_some()`.
    pub clip_duration_ticks: Option<i64>,
    /// Clip-source tick rate when `clip_id.is_some()`.
    pub clip_rate: Option<Rational>,
}

/// One plugin as the Plugins list shows it: what it says it is, what it actually
/// registered, whether it is mounted, and why it is not.
#[derive(Debug, Clone, PartialEq)]
pub struct UiPluginRow {
    pub id: SharedString,
    pub name: SharedString,
    pub version: SharedString,
    /// "built-in", or an installed package's tier: "native", "WASM" or "data".
    pub tier: SharedString,
    /// Whether the row is an installed package, which can be reloaded,
    /// uninstalled and opened in its folder.
    pub installed: bool,
    /// Whether the row is the package `aria plugin dev` loads from its source
    /// folder: it reloads when its files change and has nothing to uninstall.
    pub development: bool,
    /// Whether the row offers Enable and Disable. The plugins the shell cannot
    /// run without, and a package that did not read, offer neither.
    pub can_toggle: bool,
    pub requires: Vec<SharedString>,
    /// Short service names, without their module path.
    pub services: Vec<SharedString>,
    /// What the plugin registered, as "kind id": "effect com.example.glow".
    pub contributions: Vec<SharedString>,
    /// What an installed package is granted, one line per permission.
    pub permissions: Vec<SharedString>,
    /// Services this plugin asked for and did not declare in its manifest, so it
    /// was refused them. A plugin missing a declaration otherwise degrades in
    /// silence, because a surface with no service hides itself.
    pub denied: Vec<SharedString>,
    /// Mounted, or waiting for its first activation event.
    pub enabled: bool,
    /// The activation events a package waits for before it loads, as
    /// `plugin.toml` spells them; empty once it loads, and for one that loads
    /// at start.
    pub waiting_for: Vec<SharedString>,
    /// What mounting this plugin cost, in milliseconds, or `None` for a plugin
    /// that is not mounted. A start is the sum of these, so the list that names
    /// the plugins is where the cost of having them belongs.
    pub mount_ms: Option<f32>,
    /// Smoothed mean per-frame render cost across this plugin's effects,
    /// transitions, and generators, in milliseconds, or `None` when the plugin
    /// is not mounted or has not rendered a frame yet.
    pub frame_ms: Option<f32>,
    /// Maximum single-frame render cost observed since mount, in milliseconds,
    /// or `None` when the plugin has not rendered a frame yet.
    pub frame_max_ms: Option<f32>,
    /// Smoothed mean per-block audio cost across this plugin's audio
    /// processors, in milliseconds, or `None` when the plugin is not mounted
    /// or has not processed an audio block yet.
    pub block_ms: Option<f32>,
    /// Maximum single-block audio cost observed since mount, in milliseconds,
    /// or `None` when the plugin has not processed an audio block yet.
    pub block_max_ms: Option<f32>,
    /// The reason the last mount attempt failed, or the plugin failed after
    /// it, when it did.
    pub error: Option<SharedString>,
    /// The latest fault the host recovered from while the plugin stays
    /// mounted, such as an audio effect bypassed for missing its deadlines.
    pub fault: Option<SharedString>,
    /// How many lines the plugin logged this session. The lines are read only
    /// for a row whose log is open.
    pub log_lines: usize,
    /// Whether the row shows its log: the Log toggle's choice, or, before one,
    /// whether the row has an error or a fault.
    pub log_open: bool,
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
    /// The sequence's patch for each source channel, as saved.
    pub source_patching: aria_models::SourcePatching,
    /// How the ruler, the timeline timecode and the Viewer's clock count time.
    pub time_display: aria_models::TimeDisplay,
}

impl UiSequence {
    /// Ticks in one wall-clock second: the sequence rate as a float, so a
    /// 23.976 sequence advances 23.976 ticks a second. Every tick-to-pixel
    /// conversion on the timeline divides by this; the rate's numerator is
    /// not it (24000 for 23.976).
    pub fn ticks_per_second(&self) -> f32 {
        (self.frame_rate.as_f64() as f32).max(1.0)
    }

    /// The lane a Source tab's `channel` lands on: its patched lane while that
    /// is still on the sequence, otherwise the topmost targeted lane of the
    /// channel's kind, otherwise the topmost lane of that kind. `None` when the
    /// channel is patched off or the sequence has no lane of its kind.
    pub fn patched_lane(&self, channel: aria_models::SourceChannel) -> Option<u64> {
        let takes = |lane: &&UiTrack| lane.channel == Some(channel);
        let auto = || {
            self.tracks
                .iter()
                .filter(takes)
                .find(|lane| lane.targeted)
                .or_else(|| self.tracks.iter().find(takes))
                .map(|lane| lane.id)
        };
        match self.source_patching.get(channel) {
            aria_models::Patch::Off => None,
            aria_models::Patch::Track(id) => self
                .tracks
                .iter()
                .filter(takes)
                .find(|lane| lane.id == id.raw())
                .map(|lane| lane.id)
                .or_else(auto),
            aria_models::Patch::Auto => auto(),
        }
    }

    /// Light `patched` on the lane each source channel lands on, and on no
    /// other.
    pub fn mark_patched_lanes(&mut self) {
        let lit: Vec<u64> = aria_models::SourceChannel::ALL
            .into_iter()
            .filter_map(|channel| self.patched_lane(channel))
            .collect();
        for lane in &mut self.tracks {
            lane.patched = lit.contains(&lane.id);
        }
    }
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
    /// Length in the source's own ticks, at [`Self::frame_rate`].
    pub duration_ticks: i64,
    /// The length a placement of the whole source takes in the open sequence,
    /// as its timecode. Formatted once with the projection. An audio-only
    /// source counts milliseconds, so its own ticks are never read as frames.
    pub duration_label: SharedString,
    pub has_audio: bool,
    pub kind: UiClipKind,
    pub missing: bool,
    /// The operator's grouping colour for this pool entry, when set.
    pub label: Option<aria_models::LabelColor>,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiClipMove {
    pub clip_id: u64,
    pub delta_ticks: i64,
    pub new_track: Option<u64>,
}
