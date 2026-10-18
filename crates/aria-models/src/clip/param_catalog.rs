//! Every animatable clip property, as data.
//!
//! A property used to be an enum variant plus a hand-written arm in each place
//! that cared: the model's write route, the range check, the wire schema, and
//! the editor's keyframe strip. Nothing tied those together, so a property could
//! exist in the model and be unreachable everywhere else, and no test went red.
//! That is how a keyframe strip ends up offering six of the properties a clip
//! actually animates.
//!
//! A row here carries the property's identity, its value kind, its declared
//! range, and a route to the [`Param`] it names. The route doubles as the
//! availability rule: an optional block that is not enabled has no route, and a
//! property with no route is not offered. So a surface enumerates the catalog
//! and asks each row whether it applies, instead of keeping its own list.
//!
//! Effect instance parameters are not here. They are described by
//! [`crate::effect_catalog`] per effect id and are already data.
//!
//! [`ClipParam::Speed`] is not here either: a speed ramp is keyframed on its own
//! normalized tick scale through `set_clip_speed_curve`, not through
//! `set_param_keyframe`, so it is not an animatable property in this sense.

use std::sync::LazyLock;

use crate::clip::{
    Clip, ClipParam, ClipSource, CropRect, Generator, LookParam, MAX_SHAPE_DIM, MAX_STROKE_WIDTH,
    MaskParam, ParamValue, Scale2, Shape, ShapeParam, StyleParam, TextParam,
};
use crate::param::{Easing, Keyframe, Param};

/// The value kind a property's keyframes carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Scalar,
    Vec2,
    Vec3,
    Color,
    Rect,
}

/// Which lanes a property means anything on.
///
/// A visual property on an audio-lane clip and an audio envelope on a generated
/// clip are both refused by the engine, so offering either is a dead row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamScope {
    /// Visual clips: anything on a lane that composites pixels.
    Visual,
    /// Media-backed clips, which are the only ones that carry audio.
    Audio,
}

/// Where a property lives on a clip, borrowed for reading.
///
/// `Absent` is the whole availability rule: a mask feather has no route until a
/// mask exists, and a stroke colour has none until the shape has a stroke.
pub(crate) enum ParamRoute<'a> {
    Scalar(&'a Param<f32>),
    Vec2(&'a Param<[f32; 2]>),
    Vec3(&'a Param<[f32; 3]>),
    Scale(&'a Param<Scale2>),
    Color(&'a Param<[u8; 4]>),
    Rect(&'a Param<CropRect>),
    /// A value the clip does not store: an effect parameter nobody has written
    /// yet reads its catalog default, which is what the renderer is using. It
    /// samples to that value and carries no keyframes.
    Const(ParamValue),
    Absent,
}

/// How a row finds its value.
///
/// A directly addressed property reads its route off the [`ClipParam`]
/// registry, which owns the field. An effect parameter is addressed by position
/// instead, because it exists once per effect on the clip rather than once per
/// clip. Everything else — the members of a delegated axis — names its own
/// field here, because only the axis knows where its blocks live.
#[derive(Clone, Copy)]
enum Route {
    Fixed(fn(&Clip) -> ParamRoute<'_>),
    Effect {
        effect: u32,
        param: u32,
    },
    /// A mask property is addressed by the mask's index in the stack, for the
    /// same reason.
    Mask {
        mask: u32,
        param: MaskParam,
    },
    /// Read the route off the row `ClipParam` itself declares. Only the
    /// directly addressed properties use this: a delegated wrapper's route
    /// resolves back through this catalog, so pointing one here would spin.
    Axis,
}

/// The bounds a property's value is validated against.
///
/// Both sides are separate because the model's bounds are: a blur radius is
/// non-negative with no ceiling, an opacity is `[0, 1]`, a rotation is only
/// finite. A single `Option<(min, max)>` forced a one-sided rule to declare
/// nothing, so a stepper walked a value into a command the model then refused.
/// A side left `None` is a claim that the model accepts anything there, and
/// `a_declared_range_is_the_range_the_model_enforces` proves it by writing a far
/// value on that side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParamRange {
    pub min: Option<f32>,
    pub max: Option<f32>,
}

mod range;

/// One animatable property.
#[derive(Clone)]
pub struct ParamSpec {
    /// Stable id for surfaces that address a property by name (keys, palette,
    /// inspector rows, docs).
    pub id: std::borrow::Cow<'static, str>,
    pub label: std::borrow::Cow<'static, str>,
    /// The block a row belongs to, so a panel draws its rows together without
    /// keeping its own list of which properties it owns. Shipped groups:
    /// `transform`, `framing`, `audio`, `filter`, `lut`, `grade`, `chroma`,
    /// `matte`, `shadow`, `glow`, `outline`, `background`, `text`, `shape`. A
    /// mask property's group is `mask-<index>` and an effect parameter's is
    /// `effect-<index>`, so two masks, or two instances of one effect, are two
    /// groups.
    pub group: std::borrow::Cow<'static, str>,
    pub param: ClipParam,
    pub kind: ParamKind,
    pub scope: ParamScope,
    /// The bounds commands validate against, one side at a time.
    pub range: ParamRange,
    route: Route,
}

/// Two rows are equal when they describe the same property the same way.
///
/// The route is left out: it is a function of the row's identity — a property
/// with the same id, param, kind and range reaches the same field — and
/// comparing function addresses answers nothing. A projected row is compared to
/// a freshly built one to prove a value was re-taken, and that comparison must
/// be about the property, not about where the linker put a route.
impl PartialEq for ParamSpec {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.label == other.label
            && self.group == other.group
            && self.param == other.param
            && self.kind == other.kind
            && self.scope == other.scope
            && self.range == other.range
    }
}

impl std::fmt::Debug for ParamSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParamSpec")
            .field("id", &self.id)
            .field("param", &self.param)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl ParamSpec {
    /// Where this row's value lives on `clip`, borrowed for reading.
    ///
    /// [`ParamRoute::Absent`] is the availability rule: an optional block that
    /// is not enabled has no route, and a property with no route is not
    /// offered.
    pub(crate) fn resolve<'a>(&self, clip: &'a Clip) -> ParamRoute<'a> {
        match self.route {
            Route::Fixed(route) => route(clip),
            Route::Effect { effect, param } => effect_rows::effect_route(clip, effect, param),
            Route::Mask { mask, param } => mask_rows::mask_route(clip, mask, param),
            Route::Axis => self.param.route(clip),
        }
    }

    /// Whether this property is reachable on `clip` right now: the block that
    /// owns it exists, and the clip is the kind the property applies to.
    pub fn available(&self, clip: &Clip) -> bool {
        let kind_ok = match self.scope {
            // A generated clip is visual; a media clip is visual unless it sits
            // on an audio lane, which the lane, not the clip, decides. Callers
            // that know the lane narrow further.
            ParamScope::Visual => true,
            ParamScope::Audio => matches!(clip.content, ClipSource::Media { .. }),
        };
        kind_ok && !matches!(self.resolve(clip), ParamRoute::Absent)
    }

    /// The property's value at a clip-local tick, or `None` when the property is
    /// not reachable on this clip.
    ///
    /// This is what pins a keyframe to what is already on screen rather than to
    /// a default. Gated on [`ParamSpec::available`] so one rule decides both
    /// whether a row is offered and whether it answers: a row that samples a
    /// value the operator was never offered is how a dead control gets shipped.
    pub fn sample(&self, clip: &Clip, local: i64) -> Option<ParamValue> {
        if !self.available(clip) {
            return None;
        }
        Some(match self.resolve(clip) {
            ParamRoute::Scalar(p) => ParamValue::Scalar(p.sample(local)),
            ParamRoute::Vec2(p) => ParamValue::Vec2(p.sample(local)),
            ParamRoute::Vec3(p) => ParamValue::Vec3(p.sample(local)),
            ParamRoute::Scale(p) => {
                let scale = p.sample(local);
                ParamValue::Vec2([scale.x, scale.y])
            }
            ParamRoute::Color(p) => ParamValue::Color(p.sample(local)),
            ParamRoute::Rect(p) => {
                let r = p.sample(local);
                ParamValue::Rect([r.x, r.y, r.w, r.h])
            }
            ParamRoute::Const(value) => value,
            ParamRoute::Absent => return None,
        })
    }

    /// The value a reset returns this property to on `clip`, or `None` when the
    /// property is not reachable on this clip or nothing resets it.
    ///
    /// Gated on [`ParamSpec::available`] for the reason [`ParamSpec::sample`]
    /// is: a row that is not offered does not answer.
    pub fn default_value(&self, clip: &Clip) -> Option<ParamValue> {
        if !self.available(clip) {
            return None;
        }
        self.param.default_value(clip)
    }

    /// Every keyframe on this property as `(clip-local tick, easing)`.
    ///
    /// The value is deliberately dropped: a strip draws where the keys are and
    /// which curve leaves each one, and a uniform shape means one drawing path
    /// for all four value kinds.
    pub fn keyframes(&self, clip: &Clip) -> Vec<(i64, Easing)> {
        let mut marks = Vec::new();
        self.visit_keyframes(clip, |tick, easing| marks.push((tick, easing)));
        marks
    }

    /// Call `visit` with every keyframe on this property as `(clip-local tick,
    /// easing)`, in tick order, without collecting them.
    ///
    /// Resolves the route once. [`ParamSpec::keyframes`] is this collected.
    pub fn visit_keyframes(&self, clip: &Clip, mut visit: impl FnMut(i64, Easing)) {
        if matches!(self.scope, ParamScope::Audio)
            && !matches!(clip.content, ClipSource::Media { .. })
        {
            return;
        }
        fn marks<T: Clone>(p: &Param<T>, visit: &mut impl FnMut(i64, Easing)) {
            p.keyframes().iter().for_each(|k| visit(k.tick, k.easing));
        }
        match self.resolve(clip) {
            ParamRoute::Scalar(p) => marks(p, &mut visit),
            ParamRoute::Vec2(p) => marks(p, &mut visit),
            ParamRoute::Vec3(p) => marks(p, &mut visit),
            ParamRoute::Scale(p) => marks(p, &mut visit),
            ParamRoute::Color(p) => marks(p, &mut visit),
            ParamRoute::Rect(p) => marks(p, &mut visit),
            // A value the clip does not store cannot be keyframed until someone
            // writes one, which turns the route into a real `Param`.
            ParamRoute::Const(_) | ParamRoute::Absent => {}
        }
    }

    /// The easing on the keyframe at `local`, if there is one.
    pub fn easing_at(&self, clip: &Clip, local: i64) -> Option<Easing> {
        self.keyframes(clip)
            .into_iter()
            .find(|(tick, _)| *tick == local)
            .map(|(_, easing)| easing)
    }

    /// Effective `(incoming, outgoing)` temporal ease of the keyframe at
    /// `local`, derived from the adjacent segments when the keyframe stores no
    /// explicit handle. `None` when no keyframe sits at `local`.
    pub fn temporal_ease_at(
        &self,
        clip: &Clip,
        local: i64,
    ) -> Option<(crate::TemporalEase, crate::TemporalEase)> {
        if !self.available(clip) {
            return None;
        }
        match self.resolve(clip) {
            ParamRoute::Scalar(p) => p.keyframe_temporal_ease(local),
            ParamRoute::Vec2(p) => p.keyframe_temporal_ease(local),
            ParamRoute::Vec3(p) => p.keyframe_temporal_ease(local),
            ParamRoute::Scale(p) => p.keyframe_temporal_ease(local),
            ParamRoute::Color(p) => p.keyframe_temporal_ease(local),
            ParamRoute::Rect(p) => p.keyframe_temporal_ease(local),
            ParamRoute::Const(_) | ParamRoute::Absent => None,
        }
    }

    /// This property's curve on `clip` as a [`Param<ParamValue>`], or `None`
    /// when the property is not available on `clip`.
    pub fn curve(&self, clip: &Clip) -> Option<Param<ParamValue>> {
        if !self.available(clip) {
            return None;
        }
        fn map_param<T: Copy>(
            param: &Param<T>,
            mut f: impl FnMut(T) -> ParamValue,
        ) -> Param<ParamValue> {
            match param {
                Param::Constant(v) => Param::Constant(f(*v)),
                Param::Keyframed { keyframes } => Param::Keyframed {
                    keyframes: keyframes
                        .iter()
                        .map(|kf| Keyframe {
                            tick: kf.tick,
                            value: f(kf.value),
                            easing: kf.easing,
                            tangents: kf.tangents,
                        })
                        .collect(),
                },
            }
        }
        Some(match self.resolve(clip) {
            ParamRoute::Scalar(p) => map_param(p, ParamValue::Scalar),
            ParamRoute::Vec2(p) => map_param(p, ParamValue::Vec2),
            ParamRoute::Vec3(p) => map_param(p, ParamValue::Vec3),
            ParamRoute::Scale(p) => map_param(p, |s| ParamValue::Vec2([s.x, s.y])),
            ParamRoute::Color(p) => map_param(p, ParamValue::Color),
            ParamRoute::Rect(p) => map_param(p, |r| ParamValue::Rect([r.x, r.y, r.w, r.h])),
            ParamRoute::Const(value) => Param::Constant(value),
            ParamRoute::Absent => return None,
        })
    }
}

/// The style block a style property lives on, so a row states its owner once.
fn shadow(clip: &Clip) -> Option<&crate::look::LayerShadow> {
    clip.styles.shadow.as_ref()
}

fn text_style(clip: &Clip) -> Option<&crate::clip::TextStyle> {
    match &clip.content {
        ClipSource::Generated(Generator::Text { style, .. }) => Some(style),
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
    }
}

/// The shape generator's fields, when this clip is one.
type ShapeFields<'a> = (
    &'a Shape,
    &'a Param<[u8; 4]>,
    &'a Param<f32>,
    &'a Param<f32>,
    &'a Param<f32>,
    Option<&'a crate::clip::ShapeStroke>,
);

fn shape_fields(clip: &Clip) -> Option<ShapeFields<'_>> {
    match &clip.content {
        ClipSource::Generated(Generator::Shape {
            shape,
            rgba,
            width,
            height,
            corner_radius,
            stroke,
        }) => Some((shape, rgba, width, height, corner_radius, stroke.as_ref())),
        ClipSource::Generated(
            Generator::Text { .. }
            | Generator::SolidColor { .. }
            | Generator::Sticker { .. }
            | Generator::Lottie { .. }
            | Generator::Effect
            | Generator::Filter
            | Generator::Adjustment,
        )
        | ClipSource::Media { .. }
        | ClipSource::Multicam { .. }
        | ClipSource::Sequence { .. } => None,
    }
}

/// Build one row. Keeps a row to its facts, its group, and its route.
fn spec(
    id: &'static str,
    label: &'static str,
    group: &'static str,
    param: ClipParam,
    kind: ParamKind,
    scope: ParamScope,
    range: ParamRange,
    route: fn(&Clip) -> ParamRoute<'_>,
) -> ParamSpec {
    ParamSpec {
        id: std::borrow::Cow::Borrowed(id),
        label: std::borrow::Cow::Borrowed(label),
        group: std::borrow::Cow::Borrowed(group),
        param,
        kind,
        scope,
        range,
        route: Route::Fixed(route),
    }
}

fn look(param: LookParam) -> ClipParam {
    ClipParam::Look { param }
}

fn style(param: StyleParam) -> ClipParam {
    ClipParam::Style { param }
}

fn text(param: TextParam) -> ClipParam {
    ClipParam::Text { param }
}

fn shape(param: ShapeParam) -> ClipParam {
    ClipParam::Shape { param }
}

const UNIT: ParamRange = ParamRange::between(0.0, 1.0);
const SIGNED_UNIT: ParamRange = ParamRange::between(-1.0, 1.0);
const QUAD_UNIT: ParamRange = ParamRange::between(0.0, 4.0);

/// The rows the directly addressed properties contribute.
///
/// Read from the [`ClipParam`] registry, which owns each one's id, label,
/// group, kind, scope, range and route. A new direct property appears here by
/// declaring its row there — there is no second list to keep in step.
fn direct_rows() -> Vec<ParamSpec> {
    crate::clip::param_axis::direct_rows()
        .map(|(param, id, browse)| ParamSpec {
            id: std::borrow::Cow::Borrowed(id),
            label: std::borrow::Cow::Borrowed(browse.label),
            group: std::borrow::Cow::Borrowed(browse.group),
            param,
            kind: browse.kind,
            scope: param.scope(),
            range: browse.range,
            route: Route::Axis,
        })
        .collect()
}

/// The catalog range each slider shape draws on.
macro_rules! grade_range {
    (signed) => {
        SIGNED_UNIT
    };
    (positive) => {
        UNIT
    };
    (multiplicative) => {
        QUAD_UNIT
    };
}

/// One catalog row per grade slider, from the adjustment row list: the id, the
/// label and the range come from the row, and the route is the field it names.
macro_rules! grade_rows {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        /// Every grade slider's catalog row, in row order.
        fn grade_specs() -> Vec<ParamSpec> {
            use ParamKind::Scalar;
            use ParamScope::Visual;
            vec![$(spec(
                $id,
                $label,
                "grade",
                look(LookParam::$variant),
                Scalar,
                Visual,
                grade_range!($shape),
                |c| ParamRoute::Scalar(&c.adjust.$field),
            ),)*]
        }
    };
}

crate::adjust_slider_rows!(grade_rows);

static CATALOG: LazyLock<Vec<ParamSpec>> = LazyLock::new(|| {
    use ParamKind::{Color, Scalar, Vec2};
    use ParamScope::Visual;
    let mut rows = direct_rows();
    rows.extend([
        // --- look: filter and LUT -------------------------------------------
        spec(
            "filter-intensity",
            "Filter amount",
            "filter",
            look(LookParam::FilterIntensity),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.filter {
                Some(f) => ParamRoute::Scalar(&f.intensity),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "lut-intensity",
            "LUT amount",
            "lut",
            look(LookParam::LutIntensity),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.lut {
                Some(l) => ParamRoute::Scalar(&l.intensity),
                None => ParamRoute::Absent,
            },
        ),
    ]);
    // One row per adjustment, expanded from the grade slider row list, so an
    // adjustment reaches the inspector, the keyframe strip and the parameter
    // axis by existing rather than by being repeated here.
    rows.extend(grade_specs());
    rows.extend([
        // --- look: chroma key -----------------------------------------------
        spec(
            "chroma-strength",
            "Key strength",
            "chroma",
            look(LookParam::ChromaStrength),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.chroma_key {
                Some(k) => ParamRoute::Scalar(&k.strength),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "chroma-shadow",
            "Key shadow",
            "chroma",
            look(LookParam::ChromaShadow),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.chroma_key {
                Some(k) => ParamRoute::Scalar(&k.shadow),
                None => ParamRoute::Absent,
            },
        ),
        // --- look: track matte ----------------------------------------------
        spec(
            "matte-softness",
            "Matte softness",
            "matte",
            look(LookParam::MatteSoftness),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.matte {
                Some(m) => ParamRoute::Scalar(&m.softness),
                None => ParamRoute::Absent,
            },
        ),
        // --- layer styles ---------------------------------------------------
        spec(
            "style-shadow-color",
            "Shadow colour",
            "shadow",
            style(StyleParam::ShadowColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match shadow(c) {
                Some(s) => ParamRoute::Color(&s.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-shadow-offset",
            "Shadow offset",
            "shadow",
            style(StyleParam::ShadowOffset),
            Vec2,
            Visual,
            ParamRange::ANY,
            |c| match shadow(c) {
                Some(s) => ParamRoute::Vec2(&s.offset),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-shadow-blur",
            "Shadow blur",
            "shadow",
            style(StyleParam::ShadowBlur),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match shadow(c) {
                Some(s) => ParamRoute::Scalar(&s.blur),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-glow-color",
            "Glow colour",
            "glow",
            style(StyleParam::GlowColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match &c.styles.glow {
                Some(g) => ParamRoute::Color(&g.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-glow-radius",
            "Glow radius",
            "glow",
            style(StyleParam::GlowRadius),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match &c.styles.glow {
                Some(g) => ParamRoute::Scalar(&g.radius),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-glow-intensity",
            "Glow intensity",
            "glow",
            style(StyleParam::GlowIntensity),
            Scalar,
            Visual,
            ParamRange::between(0.0, 4.0),
            |c| match &c.styles.glow {
                Some(g) => ParamRoute::Scalar(&g.intensity),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-outline-color",
            "Outline colour",
            "outline",
            style(StyleParam::OutlineColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match &c.styles.outline {
                Some(o) => ParamRoute::Color(&o.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-outline-width",
            "Outline width",
            "outline",
            style(StyleParam::OutlineWidth),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match &c.styles.outline {
                Some(o) => ParamRoute::Scalar(&o.width),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-background-color",
            "Background colour",
            "background",
            style(StyleParam::BackgroundColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match &c.styles.background {
                Some(b) => ParamRoute::Color(&b.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-background-padding",
            "Background padding",
            "background",
            style(StyleParam::BackgroundPadding),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match &c.styles.background {
                Some(b) => ParamRoute::Scalar(&b.padding),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-background-radius",
            "Background radius",
            "background",
            style(StyleParam::BackgroundRadius),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match &c.styles.background {
                Some(b) => ParamRoute::Scalar(&b.radius),
                None => ParamRoute::Absent,
            },
        ),
        // --- text style -----------------------------------------------------
        spec(
            "text-size",
            "Text size",
            "text",
            text(TextParam::Size),
            Scalar,
            Visual,
            ParamRange::between(f32::EPSILON, crate::clip::text::MAX_TEXT_SIZE),
            |c| match text_style(c) {
                Some(s) => ParamRoute::Scalar(&s.size),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-fill",
            "Text colour",
            "text",
            text(TextParam::Fill),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match text_style(c) {
                Some(s) => ParamRoute::Color(&s.fill),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-letter-spacing",
            "Letter spacing",
            "text",
            text(TextParam::LetterSpacing),
            Scalar,
            Visual,
            ParamRange::between(
                -crate::clip::text::MAX_TEXT_LETTER_SPACING,
                crate::clip::text::MAX_TEXT_LETTER_SPACING,
            ),
            |c| match text_style(c) {
                Some(s) => ParamRoute::Scalar(&s.letter_spacing),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-line-spacing",
            "Line spacing",
            "text",
            text(TextParam::LineSpacing),
            Scalar,
            Visual,
            ParamRange::between(f32::EPSILON, crate::clip::text::MAX_TEXT_LINE_SPACING),
            |c| match text_style(c) {
                Some(s) => ParamRoute::Scalar(&s.line_spacing),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-stroke-width",
            "Text stroke width",
            "text",
            text(TextParam::StrokeWidth),
            Scalar,
            Visual,
            ParamRange::between(0.0, crate::clip::text::MAX_TEXT_STROKE_WIDTH),
            |c| match text_style(c).and_then(|s| s.stroke.as_ref()) {
                Some(s) => ParamRoute::Scalar(&s.width),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-stroke-color",
            "Text stroke colour",
            "text",
            text(TextParam::StrokeColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match text_style(c).and_then(|s| s.stroke.as_ref()) {
                Some(s) => ParamRoute::Color(&s.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-shadow-blur",
            "Text shadow blur",
            "text",
            text(TextParam::ShadowBlur),
            Scalar,
            Visual,
            ParamRange::between(0.0, 1.0),
            |c| match text_style(c).and_then(|s| s.shadow.as_ref()) {
                Some(s) => ParamRoute::Scalar(&s.blur),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-shadow-distance",
            "Text shadow distance",
            "text",
            text(TextParam::ShadowDistance),
            Scalar,
            Visual,
            ParamRange::between(0.0, crate::clip::text::MAX_TEXT_SHADOW_DISTANCE),
            |c| match text_style(c).and_then(|s| s.shadow.as_ref()) {
                Some(s) => ParamRoute::Scalar(&s.distance),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-shadow-color",
            "Text shadow colour",
            "text",
            text(TextParam::ShadowColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match text_style(c).and_then(|s| s.shadow.as_ref()) {
                Some(s) => ParamRoute::Color(&s.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-background-color",
            "Text card colour",
            "text",
            text(TextParam::BackgroundColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match text_style(c).and_then(|s| s.background.as_ref()) {
                Some(b) => ParamRoute::Color(&b.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-background-radius",
            "Text card radius",
            "text",
            text(TextParam::BackgroundRadius),
            Scalar,
            Visual,
            UNIT,
            |c| match text_style(c).and_then(|s| s.background.as_ref()) {
                Some(b) => ParamRoute::Scalar(&b.radius),
                None => ParamRoute::Absent,
            },
        ),
        // --- shape generator ------------------------------------------------
        spec(
            "shape-width",
            "Shape width",
            "shape",
            shape(ShapeParam::Width),
            Scalar,
            Visual,
            ParamRange::between(f32::EPSILON, MAX_SHAPE_DIM),
            |c| match shape_fields(c) {
                Some((_, _, width, ..)) => ParamRoute::Scalar(width),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-height",
            "Shape height",
            "shape",
            shape(ShapeParam::Height),
            Scalar,
            Visual,
            ParamRange::between(f32::EPSILON, MAX_SHAPE_DIM),
            |c| match shape_fields(c) {
                Some((_, _, _, height, ..)) => ParamRoute::Scalar(height),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-corner-radius",
            "Corner radius",
            "shape",
            shape(ShapeParam::CornerRadius),
            Scalar,
            Visual,
            ParamRange::between(0.0, MAX_SHAPE_DIM),
            |c| match shape_fields(c) {
                Some((_, _, _, _, radius, _)) => ParamRoute::Scalar(radius),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-inner-ratio",
            "Star inner ratio",
            "shape",
            shape(ShapeParam::InnerRatio),
            Scalar,
            Visual,
            UNIT,
            |c| match shape_fields(c) {
                Some((Shape::Star { inner_ratio, .. }, ..)) => ParamRoute::Scalar(inner_ratio),
                _ => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-fill",
            "Shape fill",
            "shape",
            shape(ShapeParam::Fill),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match shape_fields(c) {
                Some((_, rgba, ..)) => ParamRoute::Color(rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-stroke-width",
            "Shape stroke width",
            "shape",
            shape(ShapeParam::StrokeWidth),
            Scalar,
            Visual,
            ParamRange::between(0.0, MAX_STROKE_WIDTH),
            |c| match shape_fields(c).and_then(|f| f.5) {
                Some(s) => ParamRoute::Scalar(&s.width),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-stroke-color",
            "Shape stroke colour",
            "shape",
            shape(ShapeParam::StrokeColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match shape_fields(c).and_then(|f| f.5) {
                Some(s) => ParamRoute::Color(&s.rgba),
                None => ParamRoute::Absent,
            },
        ),
    ]);
    rows
});

/// Every animatable clip property, in the order a surface should browse them.
pub fn clip_param_catalog() -> &'static [ParamSpec] {
    &CATALOG
}

/// The static row describing `param`, or `None` for a property that is not one
/// of the rows every clip declares.
///
/// An effect parameter or a mask property has no static row: it exists once per
/// effect or mask on one clip, so it comes from [`clip_params_for`] instead.
pub fn param_spec(param: ClipParam) -> Option<&'static ParamSpec> {
    CATALOG.iter().find(|spec| spec.param == param)
}

/// The row an id names without a clip at hand: a row every clip declares, or a
/// mask row (`mask-<index>-<property>`) for the mask at that index on whichever
/// clip it is read on. `None` for an effect parameter, which is addressed
/// through its effect, and for any other string.
///
/// A preset and a script hold property ids apart from any clip, so this is
/// what they resolve through.
pub fn property_row(id: &str) -> Option<std::borrow::Cow<'static, ParamSpec>> {
    if let Some(spec) = CATALOG.iter().find(|spec| spec.id == id) {
        return Some(std::borrow::Cow::Borrowed(spec));
    }
    let ClipParam::Mask { mask, param } = mask_row_param(id)? else {
        return None;
    };
    Some(std::borrow::Cow::Owned(mask_rows::row(mask, param)))
}

mod effect_rows;
mod mask_rows;
pub use effect_rows::{clip_params_for, visit_clip_params};
/// The `ClipParam::Effect` row reads its route and its reset value here: an
/// effect slot is addressed by position, so both need the clip's chain.
pub(crate) use effect_rows::{effect_default, effect_range, effect_route};
pub use mask_rows::mask_param_group;
/// The `ClipParam::Mask` row reads its route here: a mask is addressed by its
/// index in the stack.
pub(crate) use mask_rows::mask_route;
/// A mask row id parsed back to its property; [`property_row`] is the public
/// face of it.
pub(crate) use mask_rows::mask_row_param;

#[cfg(test)]
mod tests;
