//! GPU pass registry for clip effects and track transitions.
//!
//! One row per effect: its catalog id, the WGSL passes that render it, and the
//! parameter that makes it inert. The row is the only place an effect's *render*
//! is declared — [`crate::effect_render`] builds one pipeline per shader listed
//! here, dispatch looks a row up by id, and [`effect_is_noop`] reads the inert
//! rule from the row. Adding an effect is one row plus its WGSL plus its catalog
//! entry.
//!
//! # Who owns a parameter
//!
//! `aria_models::shipped_effect_catalog()` does, and this crate holds no second copy.
//! Names, order and value kinds come from that catalog — a data file a user can
//! replace — and the uniform slots are derived from it: scalar one float, vec2
//! two, colour four, in catalog order. There is therefore no list of slots here
//! to drift from the one the inspector browses and the engine validates against.
//!
//! # Fail closed on a bad catalog
//!
//! A derived slot layout is only real if the shader reads it, so every effect is
//! checked against its own WGSL ([`crate::passes_uniform`]) before it becomes
//! renderable: an id whose catalog parameters the shader does not read exactly,
//! or that overflows the uniform array the shader declares, is refused and
//! reported. A refused id has no descriptor, so dispatch skips it and the
//! inspector shows it as passthrough — the same as an id with no row at all.
//! Nothing renders with slots the shader disagrees about.
//!
//! An id with no row has no pass at all and is skipped rather than dispatched,
//! which is what [`effect_is_noop`] reports for an unknown effect.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use aria_models::EffectParamKind;

use crate::passes_uniform as uniform;
use crate::registered_passes::{RegisteredPass, registered_effect, registered_transition};

/// One parameter slot of a pass: the name the catalog knows it by, and how many
/// floats it occupies once the instance's values are flattened into the uniform.
///
/// The width is what turns a slot into an offset: `color_overlay`'s `amount`
/// sits at flat index 6 because a color took four floats and a vec2 took two,
/// and nothing should have to know that by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassParam {
    pub name: &'static str,
    pub floats: u8,
    pub offset: u8,
}

impl PassParam {
    /// A single float. Transitions declare their own `progress` slot with this;
    /// an effect's slots come from its catalog kind instead.
    pub const fn scalar(name: &'static str) -> Self {
        Self {
            name,
            floats: 1,
            offset: 0,
        }
    }

    /// Parameter slot with explicit width and flat float offset.
    pub const fn new(name: &'static str, floats: u8, offset: u8) -> Self {
        Self {
            name,
            floats,
            offset,
        }
    }

    /// The slot a catalog parameter occupies. Exhaustive, so a new
    /// [`EffectParamKind`] cannot ship without a width here.
    fn from_kind(name: &'static str, kind: EffectParamKind, offset: u8) -> Self {
        let floats = kind.shader_floats() as u8;
        Self {
            name,
            floats,
            offset,
        }
    }
}

/// Static descriptor for one catalog effect or transition pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassDescriptor {
    pub id: &'static str,
    /// Uniform slot order (values come from [`PassInstance`]).
    pub params: &'static [PassParam],
    /// Precomputed total floats the parameter slots occupy in the flattened uniform.
    pub float_count: usize,
}

impl PassDescriptor {
    /// Total floats the parameter slots occupy in the flattened uniform.
    #[inline]
    pub const fn float_count(&self) -> usize {
        self.float_count
    }

    /// Flat index of `name`'s first float in [`PassInstance::params`], or `None`
    /// when this pass has no such slot.
    #[inline]
    pub fn param_offset(&self, name: &str) -> Option<usize> {
        for param in self.params {
            if param.name == name {
                return Some(param.offset as usize);
            }
        }
        None
    }
}

/// What renders one effect instance: a shipped pass by catalog id, or a pass
/// registered while the process runs.
#[derive(Debug, Clone)]
pub enum PassId {
    Shipped(&'static str),
    Registered(Arc<RegisteredPass>),
}

impl PassId {
    /// The catalog id.
    pub fn as_str(&self) -> &str {
        match self {
            PassId::Shipped(id) => id,
            PassId::Registered(pass) => pass.id(),
        }
    }

    /// Whether this pass holds state across frames.
    pub fn is_stateful(&self) -> bool {
        match self {
            PassId::Shipped(_) => false,
            PassId::Registered(pass) => pass.is_stateful(),
        }
    }

    /// Whether this pass runs a CPU callback rather than a WGSL shader.
    pub fn is_cpu(&self) -> bool {
        match self {
            PassId::Shipped(_) => false,
            PassId::Registered(pass) => pass.is_cpu(),
        }
    }
}

impl From<&'static str> for PassId {
    fn from(id: &'static str) -> Self {
        PassId::Shipped(id)
    }
}

/// Two registered ids are equal when they name one registration: an id that was
/// dropped and registered again is a different pass.
impl PartialEq for PassId {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PassId::Shipped(a), PassId::Shipped(b)) => a == b,
            (PassId::Registered(a), PassId::Registered(b)) => a.serial() == b.serial(),
            _ => false,
        }
    }
}

impl PartialEq<&str> for PassId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// One effect instance sampled while the render scene is resolved.
///
/// The scene retains this owned row and the compositor borrows it directly for
/// every frame that uses the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct PassInstance {
    pub id: PassId,
    pub params: Vec<f32>,
    /// Seconds since the clip started, read by a registered pass as
    /// `aria.time`. Shipped passes do not read it, and scene resolution leaves
    /// it 0 for them so equal frames compare equal.
    pub time: f32,
    /// Full effect instance state for CPU effects reading non-float parameters
    /// (text, font, file, layer references, exact curve/gradient control points).
    pub instance: Option<aria_models::EffectInstance>,
}

impl PassInstance {
    pub fn new(id: impl Into<PassId>, params: Vec<f32>) -> Self {
        Self {
            id: id.into(),
            params,
            time: 0.0,
            instance: None,
        }
    }

    /// This instance at `seconds` since the clip started.
    #[must_use]
    pub fn with_time(mut self, seconds: f32) -> Self {
        self.time = seconds;
        self
    }

    /// Attach the full [`aria_models::EffectInstance`] for CPU effect callbacks.
    #[must_use]
    pub fn with_instance(mut self, instance: aria_models::EffectInstance) -> Self {
        self.instance = Some(instance);
        self
    }
    /// Whether drawing this instance changes nothing; see [`effect_is_noop`].
    /// A registration that was dropped draws nothing.
    pub fn is_noop(&self) -> bool {
        match &self.id {
            PassId::Shipped(id) => effect_is_noop(id, &self.params),
            PassId::Registered(pass) => pass.is_removed(),
        }
    }

    /// Whether this effect holds state across frames.
    pub fn is_stateful(&self) -> bool {
        self.id.is_stateful()
    }

    /// Whether this effect runs a CPU callback rather than a WGSL shader.
    pub fn is_cpu(&self) -> bool {
        self.id.is_cpu()
    }

    /// Offscreen slots the chain needs to draw this instance without writing
    /// the texture it reads: three for the shipped blur, which keeps its input
    /// readable across passes, two otherwise. A registered graph draws its
    /// passes before the last into targets of its own.
    pub(crate) fn slot_demand(&self) -> usize {
        match &self.id {
            PassId::Shipped(id) if *id == GAUSSIAN_BLUR => 3,
            PassId::Shipped(_) | PassId::Registered(_) => 2,
        }
    }
}

/// The value at or below which a pass changes nothing, and the parameter it
/// reads. A pass with no such parameter always draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InertBelow {
    /// Name of a parameter in the effect's catalog entry.
    pub param: &'static str,
    /// Inert when the sampled value is `<=` this.
    pub value: f32,
}

/// One row of the effect registry: which catalog id this renders, and what
/// renders it.
///
/// No parameter list: the slots come from the catalog entry named by [`id`], so
/// a row cannot describe an effect differently from the surface that edits it.
///
/// [`id`]: EffectPass::id
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EffectPass {
    /// The `aria_models::effect_catalog` id this row renders.
    pub id: &'static str,
    /// WGSL sources rendering this effect, in dispatch order. One entry for a
    /// single-pass effect; a separable effect lists each pass it runs.
    pub shaders: &'static [&'static str],
    /// Settings that make the pass a no-op, so the chain can skip it.
    pub inert: Option<InertBelow>,
}

/// Which shader renders a transition id, and the variant that shader switches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionPass {
    /// Cross-dissolve between the two frames.
    Crossfade,
    /// Dip through opaque black, or white.
    Dip { white: bool },
    /// Hard edge sweeping across the frame.
    Wipe { direction: WipeDirection },
    /// The incoming frame pushes the outgoing one out of frame.
    Slide,
}

/// Edge the incoming frame of a wipe enters by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WipeDirection {
    Left,
    Right,
    Up,
    Down,
}

impl TransitionPass {
    /// Value the shader reads from the second uniform slot to pick its variant.
    pub fn mode(self) -> f32 {
        match self {
            Self::Crossfade | Self::Slide => 0.0,
            Self::Dip { white } => f32::from(u8::from(white)),
            Self::Wipe { direction } => match direction {
                WipeDirection::Left => 0.0,
                WipeDirection::Right => 1.0,
                WipeDirection::Up => 2.0,
                WipeDirection::Down => 3.0,
            },
        }
    }

    /// The WGSL that renders this pass. Exhaustive: a new variant has to name
    /// its shader before it compiles.
    pub(crate) fn shader(self) -> &'static str {
        match self {
            Self::Crossfade => include_str!("../shaders/transition_crossfade.wgsl"),
            Self::Dip { .. } => include_str!("../shaders/transition_dip.wgsl"),
            Self::Wipe { .. } => include_str!("../shaders/transition_wipe.wgsl"),
            Self::Slide => include_str!("../shaders/transition_slide.wgsl"),
        }
    }
}

/// One row of the transition registry: a catalog id and the pass rendering it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransitionRow {
    pub id: &'static str,
    pub pass: TransitionPass,
}

/// The separable blur. Layer styles blur with the same two passes, so they read
/// this row instead of owning a second pair of pipelines.
pub(crate) const GAUSSIAN_BLUR: &str = "gaussian_blur";

const EFFECTS: &[EffectPass] = &[
    EffectPass {
        id: GAUSSIAN_BLUR,
        shaders: &[
            include_str!("../shaders/effect_blur_h.wgsl"),
            include_str!("../shaders/effect_blur_v.wgsl"),
        ],
        inert: Some(InertBelow {
            param: "radius",
            value: 0.0,
        }),
    },
    EffectPass {
        id: "vignette",
        shaders: &[include_str!("../shaders/effect_vignette.wgsl")],
        inert: Some(InertBelow {
            param: "amount",
            value: 0.0,
        }),
    },
    EffectPass {
        id: "sharpen",
        shaders: &[include_str!("../shaders/effect_sharpen.wgsl")],
        inert: Some(InertBelow {
            param: "amount",
            value: 0.0,
        }),
    },
    EffectPass {
        id: "pixelate",
        shaders: &[include_str!("../shaders/effect_pixelate.wgsl")],
        // A one-pixel cell is the image itself.
        inert: Some(InertBelow {
            param: "size",
            value: 1.0,
        }),
    },
    EffectPass {
        id: "glitch",
        shaders: &[include_str!("../shaders/effect_glitch.wgsl")],
        inert: Some(InertBelow {
            param: "amount",
            value: 0.0,
        }),
    },
    EffectPass {
        id: "chromatic_aberration",
        shaders: &[include_str!("../shaders/effect_chromatic_aberration.wgsl")],
        inert: Some(InertBelow {
            param: "amount",
            value: 0.0,
        }),
    },
    EffectPass {
        id: "grain",
        shaders: &[include_str!("../shaders/effect_grain.wgsl")],
        inert: Some(InertBelow {
            param: "amount",
            value: 0.0,
        }),
    },
    EffectPass {
        id: "glow",
        shaders: &[include_str!("../shaders/effect_glow.wgsl")],
        // A glow with no intensity adds no light, whatever the threshold is.
        inert: Some(InertBelow {
            param: "intensity",
            value: 0.0,
        }),
    },
    EffectPass {
        id: "zoom_blur",
        shaders: &[include_str!("../shaders/effect_zoom_blur.wgsl")],
        inert: Some(InertBelow {
            param: "amount",
            value: 0.0,
        }),
    },
    EffectPass {
        id: "mirror",
        shaders: &[include_str!("../shaders/effect_mirror.wgsl")],
        // Every mode of a mirror moves pixels, so there is no inert setting.
        inert: None,
    },
    EffectPass {
        id: "color_overlay",
        shaders: &[include_str!("../shaders/effect_color_overlay.wgsl")],
        inert: Some(InertBelow {
            param: "amount",
            value: 0.0,
        }),
    },
    EffectPass {
        id: "duotone",
        shaders: &[include_str!("../shaders/effect_duotone.wgsl")],
        inert: Some(InertBelow {
            param: "intensity",
            value: 0.0,
        }),
    },
    // Generators: each draws a picture of its own over the transparent solid
    // that carries it, so no setting leaves the layer as it was.
    EffectPass {
        id: "color_matte",
        shaders: &[include_str!("../shaders/effect_color_matte.wgsl")],
        inert: None,
    },
    EffectPass {
        id: "linear_gradient",
        shaders: &[include_str!("../shaders/effect_linear_gradient.wgsl")],
        inert: None,
    },
    EffectPass {
        id: "radial_gradient",
        shaders: &[include_str!("../shaders/effect_radial_gradient.wgsl")],
        inert: None,
    },
    EffectPass {
        id: "checkerboard",
        shaders: &[include_str!("../shaders/effect_checkerboard.wgsl")],
        inert: None,
    },
    EffectPass {
        id: "grid",
        shaders: &[include_str!("../shaders/effect_grid.wgsl")],
        inert: None,
    },
    EffectPass {
        id: "fractal_noise",
        shaders: &[include_str!("../shaders/effect_fractal_noise.wgsl")],
        inert: None,
    },
    EffectPass {
        id: "color_bars",
        shaders: &[include_str!("../shaders/effect_color_bars.wgsl")],
        inert: None,
    },
];

/// Progress is the only parameter a transition takes; the variant it renders
/// comes from its row, not from the instance.
const TRANSITION_PARAMS: &[PassParam] = &[PassParam::scalar("progress")];

const TRANSITIONS: &[TransitionRow] = &[
    TransitionRow {
        id: "crossfade",
        pass: TransitionPass::Crossfade,
    },
    TransitionRow {
        id: "dip_to_black",
        pass: TransitionPass::Dip { white: false },
    },
    TransitionRow {
        id: "dip_to_white",
        pass: TransitionPass::Dip { white: true },
    },
    TransitionRow {
        id: "wipe_left",
        pass: TransitionPass::Wipe {
            direction: WipeDirection::Left,
        },
    },
    TransitionRow {
        id: "wipe_right",
        pass: TransitionPass::Wipe {
            direction: WipeDirection::Right,
        },
    },
    TransitionRow {
        id: "wipe_up",
        pass: TransitionPass::Wipe {
            direction: WipeDirection::Up,
        },
    },
    TransitionRow {
        id: "wipe_down",
        pass: TransitionPass::Wipe {
            direction: WipeDirection::Down,
        },
    },
    TransitionRow {
        id: "slide",
        pass: TransitionPass::Slide,
    },
];

/// The parameter slots of every renderable effect, derived from the model
/// catalog and checked against the shader that would read them.
///
/// Catalog order, not registry order: the catalog is the list the inspector
/// browses, and an effect it does not carry cannot be edited, so there is
/// nothing to render. An id the catalog carries and no row renders is absent
/// too, which is the passthrough case.
///
/// Two statics rather than one because a descriptor borrows its slots for
/// `'static`: the slots have to be a place that outlives the projection, and a
/// static is that place without leaking.
static PARAM_SLOTS: LazyLock<Vec<(&'static str, Vec<PassParam>, usize)>> =
    LazyLock::new(derive_slots);

/// Total floats `params` occupy once flattened into the uniform.
fn float_count(params: &[PassParam]) -> usize {
    params.iter().map(|param| usize::from(param.floats)).sum()
}

/// Do `params` describe exactly the floats `shaders` read?
///
/// The whole refusal rests on this: one parameter more, one fewer, or one of a
/// different width is a different set of `(slot, component)` pairs than the
/// shader touches, and a set the shader does not touch must not be fed to it.
fn slots_agree(params: &[PassParam], shaders: &[&'static str]) -> bool {
    uniform::slots_for(float_count(params)) == uniform::reads_of(shaders)
}

/// Derive one row's slots, or refuse the effect and say why.
///
/// The refusal is the point: these slots feed a shader's uniform, and the
/// catalog they come from is a file a user can replace. An extra parameter, a
/// missing one, a reordering that changes widths, or a set too wide for the
/// array the shader declares would all feed a pass the wrong floats, so the
/// effect stops being renderable instead.
fn derive_slots() -> Vec<(&'static str, Vec<PassParam>, usize)> {
    aria_models::shipped_effect_catalog()
        .iter()
        .filter_map(|spec| {
            let pass = EFFECTS.iter().find(|row| row.id == spec.id)?;
            let mut offset = 0u8;
            let params: Vec<PassParam> = spec
                .params
                .iter()
                .map(|param| {
                    let p = PassParam::from_kind(param.name.as_str(), param.kind, offset);
                    offset = offset.saturating_add(p.floats);
                    p
                })
                .collect();
            let floats = offset as usize;
            if !slots_agree(&params, pass.shaders) {
                tracing::error!(
                    effect = %spec.id,
                    declared = ?uniform::slots_for(floats),
                    read = ?uniform::reads_of(pass.shaders),
                    "effect catalog declares parameters its shader does not read; effect refused"
                );
                return None;
            }
            let capacity = pass
                .shaders
                .iter()
                .filter_map(|source| uniform::declared_param_slots(source))
                .min();
            match capacity {
                Some(slots) if floats <= slots * 4 => Some((pass.id, params, floats)),
                Some(slots) => {
                    tracing::error!(
                        effect = %spec.id,
                        floats,
                        holds = slots * 4,
                        "effect parameters overflow the uniform its shader declares; effect refused"
                    );
                    None
                }
                None => {
                    tracing::error!(
                        effect = %spec.id,
                        "effect shader declares no params uniform; effect refused"
                    );
                    None
                }
            }
        })
        .collect()
}

/// The public catalog projection, built once. Callers outside the crate want
/// identity and parameter slots, not shader sources.
static EFFECT_DESCRIPTORS: LazyLock<Vec<PassDescriptor>> = LazyLock::new(|| {
    let slots: &'static Vec<(&'static str, Vec<PassParam>, usize)> = &PARAM_SLOTS;
    slots
        .iter()
        .map(|(id, params, float_count)| PassDescriptor {
            id,
            params: params.as_slice(),
            float_count: *float_count,
        })
        .collect()
});

static EFFECT_DESCRIPTOR_BY_ID: LazyLock<HashMap<&'static str, &'static PassDescriptor>> =
    LazyLock::new(|| {
        shipped_effect_descriptors()
            .iter()
            .map(|descriptor| (descriptor.id, descriptor))
            .collect()
    });

/// Inert-parameter offsets derived once with the descriptor catalog.
///
/// The draw path asks this table instead of scanning parameter names on every
/// layer and frame. A catalog override receives a newly derived table when the
/// process initializes.
static INERT_PARAM_OFFSET_BY_ID: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| {
    shipped_effect_descriptors()
        .iter()
        .filter_map(|descriptor| {
            let inert = EFFECTS
                .iter()
                .find(|pass| pass.id == descriptor.id)?
                .inert?;
            let offset = descriptor
                .params
                .iter()
                .find(|param| param.name == inert.param)?
                .offset;
            Some((descriptor.id, usize::from(offset)))
        })
        .collect()
});

/// The public catalog projection of [`TRANSITIONS`], built once.
/// Precomputed const table of transition pass descriptors.
const TRANSITION_DESCRIPTORS_TABLE: &[PassDescriptor] = &[
    PassDescriptor {
        id: "crossfade",
        params: TRANSITION_PARAMS,
        float_count: 1,
    },
    PassDescriptor {
        id: "dip_to_black",
        params: TRANSITION_PARAMS,
        float_count: 1,
    },
    PassDescriptor {
        id: "dip_to_white",
        params: TRANSITION_PARAMS,
        float_count: 1,
    },
    PassDescriptor {
        id: "wipe_left",
        params: TRANSITION_PARAMS,
        float_count: 1,
    },
    PassDescriptor {
        id: "wipe_right",
        params: TRANSITION_PARAMS,
        float_count: 1,
    },
    PassDescriptor {
        id: "wipe_up",
        params: TRANSITION_PARAMS,
        float_count: 1,
    },
    PassDescriptor {
        id: "wipe_down",
        params: TRANSITION_PARAMS,
        float_count: 1,
    },
    PassDescriptor {
        id: "slide",
        params: TRANSITION_PARAMS,
        float_count: 1,
    },
];

static TRANSITION_DESCRIPTORS: &[PassDescriptor] = TRANSITION_DESCRIPTORS_TABLE;

/// Renderable transition ids, in registry order.
static TRANSITION_IDS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| TRANSITIONS.iter().map(|row| row.id).collect());

/// Every shipped effect the compositor can dispatch: the catalog entries that
/// have a row here and whose parameters their shader reads exactly. Registered
/// effects are not listed; [`renders_effect`] answers for both.
pub fn shipped_effect_descriptors() -> &'static [PassDescriptor] {
    &EFFECT_DESCRIPTORS
}

/// The derived descriptor of the shipped effect `id`, or `None` when no shipped
/// pass renders it.
pub fn effect_descriptor(id: &str) -> Option<&'static PassDescriptor> {
    EFFECT_DESCRIPTOR_BY_ID.get(id).copied()
}

/// Every shipped transition the compositor can dispatch, as pass descriptors.
pub fn shipped_transition_descriptors() -> &'static [PassDescriptor] {
    TRANSITION_DESCRIPTORS
}

/// Every shipped transition id the compositor can dispatch.
pub fn shipped_transition_ids() -> &'static [&'static str] {
    &TRANSITION_IDS
}

/// What renders the catalog effect `effect_id`: a shipped pass, a registered
/// one, or `None` when nothing does.
pub fn pass_id(effect_id: &str) -> Option<PassId> {
    if let Some(descriptor) = effect_descriptor(effect_id) {
        return Some(PassId::Shipped(descriptor.id));
    }
    registered_effect(effect_id).map(PassId::Registered)
}

/// Whether a shipped or registered pass renders the effect `id`.
pub fn renders_effect(id: &str) -> bool {
    effect_descriptor(id).is_some() || registered_effect(id).is_some()
}

/// Whether a shipped or registered pass renders the transition `id`. Any other
/// id crossfades.
pub fn renders_transition(id: &str) -> bool {
    TRANSITION_PASS_BY_ID.contains_key(id) || registered_transition(id).is_some()
}

/// The registry rows, for the pipeline builder and dispatch.
pub(crate) fn effect_passes() -> &'static [EffectPass] {
    EFFECTS
}

/// Every registry row, by effect id.
static EFFECT_PASS_BY_ID: LazyLock<HashMap<&'static str, &'static EffectPass>> =
    LazyLock::new(|| EFFECTS.iter().map(|pass| (pass.id, pass)).collect());

static TRANSITION_PASS_BY_ID: LazyLock<HashMap<&'static str, TransitionPass>> =
    LazyLock::new(|| TRANSITIONS.iter().map(|row| (row.id, row.pass)).collect());

/// Whether a shipped pass renders the transition `id`.
pub(crate) fn is_shipped_transition(id: &str) -> bool {
    TRANSITION_PASS_BY_ID.contains_key(id)
}

/// Which shipped shader renders a transition id. Any other id crossfades,
/// which is the only variant that needs nothing from the catalog; a registered
/// transition is drawn by its own pass before this is asked.
pub fn transition_pass(id: &str) -> TransitionPass {
    TRANSITION_PASS_BY_ID
        .get(id)
        .copied()
        .unwrap_or(TransitionPass::Crossfade)
}

/// Return `true` when an effect pass can be skipped because it would change
/// nothing: an inert parameter value, or an id with no pass behind it.
///
/// An instance whose parameter count disagrees with its descriptor is never
/// inert. Skipping it here would drop it before
/// `validate_and_pack_effect_params` could reject it, turning a malformed
/// instance into a silently missing effect instead of an error.
pub fn effect_is_noop(id: &str, params: &[f32]) -> bool {
    let Some(desc) = effect_descriptor(id) else {
        // A registered effect has no inert setting, so it always draws.
        return registered_effect(id).is_none();
    };
    let Some(pass) = EFFECT_PASS_BY_ID.get(id).copied() else {
        return true;
    };
    if params.len() != desc.float_count() {
        return false;
    }
    let Some(inert) = pass.inert else {
        return false;
    };
    let Some(&offset) = INERT_PARAM_OFFSET_BY_ID.get(id) else {
        return false;
    };
    params.get(offset).is_none_or(|&v| v <= inert.value)
}

#[cfg(test)]
#[path = "passes_drift.rs"]
mod drift;
