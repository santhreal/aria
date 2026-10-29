//! The selected clip as the model holds it, projected once per change.
//!
//! The inspector draws values the projection does not carry: a parameter's
//! sampled value at the playhead, its keyframes and their easings, the ramp
//! curve, a title's generator, and the quad the compositor puts the selected
//! layer on. Every one of those used to answer by taking the session lock while
//! elements were being built — `param_rows` once per inspector group, and
//! `keyframes_of`, `selected_curve`, `selected_text_generator` and
//! `selected_layer_quad` again each — so a frame took a dozen locks and one of
//! them resolved the whole scene. A preview composite holds the same lock for
//! its whole decode, so any of those reads could park the window behind it.
//!
//! So they are part of the projection instead. This is built beside the store
//! from the same engine read, keyed by the same `ProjectionKey`, and is
//! therefore exactly as fresh as the rows drawn next to it: a value here can
//! never disagree with the clip the timeline is drawing, which is a stronger
//! guarantee than the per-element locks gave.
//!
//! One clip is cloned per re-projection. The projection already walks every
//! track, clip and keyframe, so the copy is small beside the work that produced
//! it, and it is paid once per change rather than once per element per frame.
//!
//! The clip's animatable properties are here for the same reason and are worse
//! without it: enumerating them clones the whole property catalog, and the
//! inspector did that once for the row list, again per row for the keys, and
//! again per sample while drawing a curve. They are one list now, built here.

use aria_engine::Engine;
use aria_models::{
    Clip, ClipId, ClipParam, ClipSource, EffectInstance, EffectParamKind, EffectParamSpec,
    Generator, ParentFrame, Project, RationalTime, StoredParams,
};
use aria_render::{LayerSource, SceneLayer};

use crate::app::canvas_overlay::motion_path::{OwnPath, paths_of};
use crate::app::clip_keys::{KeyParam, key_params_of};
use crate::app::clip_parent::ParentChoices;

/// Everything the inspector and the canvas overlay read about the selected clip.
///
/// Comparable so a test can hold what a scrub left and check it against a full
/// re-projection field for field: a value sampled at the playhead that the scrub
/// path forgot to re-take shows up as a difference rather than as a stale
/// number nobody can see.
#[derive(Debug, PartialEq)]
pub struct SelectedDetail {
    /// The clip as the model holds it: params, keyframes, content, extent.
    pub model: Clip,
    /// Whether the clip sits on an audio lane, which hides its visual
    /// properties.
    pub audio_lane: bool,
    /// Every property the clip can animate, with its keys, in catalog order.
    /// A surface reads this rather than enumerating the catalog itself.
    pub params: std::sync::Arc<[KeyParam]>,
    /// The selected layer's quad at the playhead in canvas pixels, `(center,
    /// size)`, or `None` when the clip places no picture at this instant or
    /// draws a title or pen path with no ink.
    pub quad: Option<([f32; 2], [f32; 2])>,
    /// A title's character boundaries at the playhead, by line and then by
    /// character, where the Type tool draws and places its caret. `None` for a
    /// clip that is not a title or is not on screen at this instant.
    pub carets: Option<std::sync::Arc<[TitleCaret]>>,
    /// The clip with its drivers evaluated at the playhead, `None` when it has
    /// no drivers. The inspector draws a driven property's value from this.
    pub driven: Option<Clip>,
    /// Every driver on the clip, as its row shows it.
    pub drivers: Vec<DriverView>,
    /// The clip's parent and the clips the Parent row offers.
    pub parents: ParentChoices,
    /// The frame the clip's parents lend it at the playhead, `None` while no
    /// parent covers it. The canvas overlay moves a child in this frame.
    pub frame: Option<ParentFrame>,
    /// The clip's motion paths in their own units, one per keyframed point. A
    /// path holds a value per frame of its span, so it is sampled once per
    /// change rather than once per paint.
    pub paths: std::sync::Arc<[OwnPath]>,
    /// The clip's own transform at the playhead: its keyframes sampled there,
    /// before drivers and parents. A transform edit starts from this pose, so
    /// a move at any instant keeps the picture where it is drawn rather than
    /// where the clip's first frame holds it.
    pub pose: crate::types::UiClipTransform,
    /// The other selected clips, in selection order; empty with one clip
    /// selected. The inspector offers the properties they share with this one
    /// and draws a dash where they read differently.
    pub peers: std::sync::Arc<[Peer]>,
}

/// Another selected clip, as the inspector reads it beside the selected one.
#[derive(Debug, PartialEq)]
pub struct Peer {
    /// The clip as the model holds it.
    pub model: Clip,
    /// Whether the clip sits on an audio lane.
    pub audio_lane: bool,
    /// Every property the clip can animate, with its keys, in catalog order.
    pub params: std::sync::Arc<[KeyParam]>,
    /// The clip with its drivers evaluated at the playhead, `None` when it has
    /// no drivers: what its driven properties read.
    pub driven: Option<Clip>,
}

impl Peer {
    fn of(project: &Project, model: Clip, at: RationalTime) -> Self {
        let audio_lane = on_audio_lane(project, model.id);
        let params = key_params_of(&model, audio_lane);
        let driven = driven_clip(project, &model, at);
        Self {
            model,
            audio_lane,
            params,
            driven,
        }
    }
}

/// One boundary between two characters of a title, on the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TitleCaret {
    /// Characters of the title's text before the boundary.
    pub index: usize,
    /// Zero-based line the boundary is drawn on.
    pub line: usize,
    /// The top of the caret in canvas pixels.
    pub top: [f32; 2],
    /// The bottom of the caret in canvas pixels.
    pub bottom: [f32; 2],
}

/// One driver as the inspector draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct DriverView {
    /// The driven property.
    pub param: ClipParam,
    /// The driver's source text.
    pub source: gpui::SharedString,
    /// Why the driver failed at the playhead.
    pub fault: Option<gpui::SharedString>,
}

impl SelectedDetail {
    /// Project the detail for the first clip of `selection` at `at`, with the
    /// rest of the selection as its peers.
    ///
    /// `None` when nothing is selected or the first clip is not in the project
    /// — a selection can name a clip an undo has taken away, and a stale
    /// selection draws nothing rather than panicking. A peer the project no
    /// longer holds is left out for the same reason.
    pub fn of(engine: &mut Engine, selection: &[ClipId], at: RationalTime) -> Option<Self> {
        let (&clip, others) = selection.split_first()?;
        let project = engine.project();
        let held = project.timeline().clip(clip)?;
        let model = held.clone();
        let audio_lane = on_audio_lane(project, clip);
        let params = key_params_of(&model, audio_lane);
        let peers = others
            .iter()
            .filter_map(|&peer| {
                let model = project.timeline().clip(peer)?.clone();
                Some(Peer::of(project, model, at))
            })
            .collect();
        let paths = paths_of(&model, audio_lane);
        let (driven, drivers) = Self::driven_of(project, held, at, &[]);
        let parents = ParentChoices::of(project, held);
        let frame = Self::frame_of(project, held, at);
        let pose = Self::pose_of(held, at);
        // Last, because measuring takes the engine mutably.
        let (quad, carets) = Self::measured(engine, &model, at);
        Some(Self {
            params,
            paths,
            pose,
            quad,
            carets,
            driven,
            drivers,
            parents,
            frame,
            model,
            audio_lane,
            peers,
        })
    }

    /// The same clip at another instant.
    ///
    /// A poll where only the playhead moved is every frame of playback and
    /// every frame of a scrub. Nothing else the projection reads has changed, so
    /// the clip and its property rows are the values already held — the rows are
    /// the clip's own properties and their keys, which no instant can move — and
    /// only what is genuinely sampled at the playhead is taken again: here, and
    /// on a peer, its drivers. `scrub_stale_tests` holds this against a full
    /// re-projection field for field, so a field that does move with the
    /// playhead cannot be kept here by mistake.
    pub fn resampled(&self, engine: &mut Engine, at: RationalTime) -> Self {
        let (driven, drivers) = Self::driven_of(engine.project(), &self.model, at, &self.drivers);
        let frame = Self::frame_of(engine.project(), &self.model, at);
        let peers = if self.peers.iter().any(|peer| !peer.model.drivers.is_empty()) {
            let project = engine.project();
            self.peers
                .iter()
                .map(|peer| Peer {
                    model: peer.model.clone(),
                    audio_lane: peer.audio_lane,
                    params: std::sync::Arc::clone(&peer.params),
                    driven: driven_clip(project, &peer.model, at),
                })
                .collect()
        } else {
            std::sync::Arc::clone(&self.peers)
        };
        let (quad, carets) = Self::measured(engine, &self.model, at);
        Self {
            model: self.model.clone(),
            audio_lane: self.audio_lane,
            params: std::sync::Arc::clone(&self.params),
            quad,
            carets,
            driven,
            drivers,
            parents: self.parents.clone(),
            frame,
            paths: std::sync::Arc::clone(&self.paths),
            pose: Self::pose_of(&self.model, at),
            peers,
        }
    }

    /// Whether `peer` has `param` as this clip has it: the same property, and
    /// on an effect slot the same effect at the same place in its chain.
    pub fn shares(&self, peer: &Peer, param: ClipParam) -> bool {
        match param {
            ClipParam::Effect { effect, .. } => self.same_effect(peer, effect as usize),
            _ => peer.params.iter().any(|row| row.spec.param == param),
        }
    }

    /// Whether `peer` holds the effect this clip holds at `index`, at `index`.
    pub fn same_effect(&self, peer: &Peer, index: usize) -> bool {
        match (self.model.effects.get(index), peer.model.effects.get(index)) {
            (Some(mine), Some(theirs)) => mine.effect_id == theirs.effect_id,
            _ => false,
        }
    }

    /// What each peer that shares `param` reads there at absolute tick
    /// `playhead`, held inside that peer as a row holds its own clip.
    pub fn peer_values(
        &self,
        param: ClipParam,
        playhead: i64,
    ) -> impl Iterator<Item = aria_models::ParamValue> + '_ {
        self.peers
            .iter()
            .filter(move |peer| self.shares(peer, param))
            .filter_map(move |peer| {
                let row = peer.params.iter().find(|row| row.spec.param == param)?;
                let shown = peer.driven.as_ref().unwrap_or(&peer.model);
                row.spec.sample(shown, peer.model.param_tick(playhead))
            })
    }

    /// The slots of the effect at `index` that a peer holding the same effect
    /// sets differently, among the kinds no curve carries: a switch, a choice,
    /// a curve, a gradient, text and a layer. A keyframeable slot's row reports
    /// this itself.
    pub fn mixed_effect_slots(&self, index: usize) -> Vec<usize> {
        let Some(mine) = self.model.effects.get(index) else {
            return Vec::new();
        };
        if !self.peers.iter().any(|peer| self.same_effect(peer, index)) {
            return Vec::new();
        }
        let Ok(spec) = mine.spec() else {
            return Vec::new();
        };
        (0..spec.params.len())
            .filter(|&slot| {
                self.peers
                    .iter()
                    .filter(|peer| self.same_effect(peer, index))
                    .any(|peer| {
                        static_slot_differs(&spec.params[slot], mine, &peer.model.effects[index])
                    })
            })
            .collect()
    }

    /// Whether a peer that shares `param` reads differently from `mine` at
    /// absolute tick `playhead`: on `component` of a vector, or anywhere on the
    /// whole value. `mine` is what this clip's row shows there.
    pub fn mixed(
        &self,
        param: ClipParam,
        component: Option<u8>,
        playhead: i64,
        mine: aria_models::ParamValue,
    ) -> bool {
        use aria_models::ParamValue;
        self.peer_values(param, playhead)
            .any(|theirs| match (mine, theirs, component) {
                (ParamValue::Vec2(a), ParamValue::Vec2(b), Some(axis)) => {
                    a.get(axis as usize) != b.get(axis as usize)
                }
                (ParamValue::Vec3(a), ParamValue::Vec3(b), Some(axis)) => {
                    a.get(axis as usize) != b.get(axis as usize)
                }
                _ => mine != theirs,
            })
    }

    /// Whether a peer that shares `param` holds something other than its own
    /// reset value at absolute tick `playhead`. A reset reaches every such
    /// clip, so the row's reset button has something to do even where the
    /// drawn clip already reads its default.
    pub fn peer_off_default(&self, param: ClipParam, playhead: i64) -> bool {
        self.peers
            .iter()
            .filter(|peer| self.shares(peer, param))
            .any(|peer| {
                let Some(row) = peer.params.iter().find(|row| row.spec.param == param) else {
                    return false;
                };
                let Some(default) = row.spec.default_value(&peer.model) else {
                    return false;
                };
                row.spec
                    .sample(&peer.model, peer.model.param_tick(playhead))
                    != Some(default)
            })
    }

    /// `clip`'s own transform sampled at `at`, in the clip's parameter time.
    fn pose_of(clip: &Clip, at: RationalTime) -> crate::types::UiClipTransform {
        clip.transform.sample(clip.param_tick(at.value)).into()
    }

    /// The frame `clip`'s parents lend it at `at`, as the renderer places it.
    /// Costs nothing for a clip without a parent.
    fn frame_of(project: &Project, clip: &Clip, at: RationalTime) -> Option<ParentFrame> {
        clip.parent?;
        let (width, height) = aria_render::canvas_size(project);
        aria_render::parent_frame(
            project,
            clip,
            at.value as f64,
            project.timeline().frame_rate,
            [width as f32, height as f32],
            None,
        )
    }

    /// The clip with its drivers evaluated at `at`, and each driver with the
    /// failure it had there. Source text already shared in `before` is reused,
    /// so a playhead move copies no source. Costs nothing for a clip without
    /// drivers.
    fn driven_of(
        project: &Project,
        clip: &Clip,
        at: RationalTime,
        before: &[DriverView],
    ) -> (Option<Clip>, Vec<DriverView>) {
        let Some(driven) = driven_clip(project, clip, at) else {
            return (None, Vec::new());
        };
        let drivers = clip
            .drivers
            .iter()
            .map(|driver| {
                let source = before
                    .iter()
                    .find(|view| view.param == driver.param && *view.source == *driver.code.source)
                    .map_or_else(
                        || gpui::SharedString::from(driver.code.source.clone()),
                        |view| view.source.clone(),
                    );
                DriverView {
                    param: driver.param,
                    source,
                    fault: aria_render::driver_fault(clip.id, driver.param)
                        .map(|fault| fault.message.into()),
                }
            })
            .collect();
        (Some(driven), drivers)
    }

    /// The selected layer's quad at `at` and, for a title, its caret
    /// boundaries: the resolved layer, measured by the renderer the way a
    /// composite places it.
    ///
    /// The resolver and the renderer, not a recomputation, so the handles sit
    /// on the quad the compositor draws: the aspect fit, the per-axis scale,
    /// the anchor and the position offset are already in the layer, and a
    /// title or pen path is sized from the raster memo the preview draws from.
    /// The carets come from the layout the title's glyphs are placed from.
    fn measured(engine: &mut Engine, clip: &Clip, at: RationalTime) -> Measured {
        let Ok(scene) = aria_render::resolve(engine.project(), at) else {
            return (None, None);
        };
        let canvas = [scene.width as f32, scene.height as f32];
        let Some(layer) = scene
            .layers
            .iter()
            .find(|layer| layer.clip == Some(clip.id))
        else {
            return (None, blank_title_carets(clip, at, canvas));
        };
        let quad = engine.layer_quad(layer, canvas);
        (quad, title_carets(engine, layer, canvas))
    }
}

/// `clip` with its drivers evaluated at `at`, `None` for a clip without
/// drivers, whose properties read as the model holds them.
fn driven_clip(project: &Project, clip: &Clip, at: RationalTime) -> Option<Clip> {
    if clip.drivers.is_empty() {
        return None;
    }
    Some(
        aria_render::DriverFrame::new(project)
            .apply(clip, at.value as f64)
            .into_owned(),
    )
}

/// Whether `theirs` sets the slot `pspec` describes differently from `mine`,
/// for the kinds no curve carries. A keyframeable kind reads `false`: its row
/// compares sampled values instead.
fn static_slot_differs(
    pspec: &EffectParamSpec,
    mine: &EffectInstance,
    theirs: &EffectInstance,
) -> bool {
    match pspec.kind {
        EffectParamKind::Bool => mine.boolean(pspec) != theirs.boolean(pspec),
        EffectParamKind::Choice => mine.choice(pspec) != theirs.choice(pspec),
        EffectParamKind::Curve => mine.curve(pspec) != theirs.curve(pspec),
        EffectParamKind::Gradient => mine.gradient(pspec) != theirs.gradient(pspec),
        EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => {
            mine.text(pspec) != theirs.text(pspec)
        }
        EffectParamKind::Layer => mine.layer(pspec) != theirs.layer(pspec),
        EffectParamKind::Scalar
        | EffectParamKind::Int
        | EffectParamKind::Color
        | EffectParamKind::Vec2
        | EffectParamKind::Point3
        | EffectParamKind::Size
        | EffectParamKind::Angle => false,
    }
}

/// Whether `clip` sits on an audio lane. The lane, not the clip, decides
/// whether the visual properties are offered, and `TrackKind::is_audio` is the
/// same answer the store's clip kind is drawn from.
fn on_audio_lane(project: &Project, clip: ClipId) -> bool {
    project
        .timeline()
        .track_of(clip)
        .and_then(|track| project.timeline().track(track))
        .is_some_and(|track| track.kind.is_audio())
}

/// A quad `(center, size)` and a title's carets, as [`SelectedDetail`] holds them.
type Measured = (
    Option<([f32; 2], [f32; 2])>,
    Option<std::sync::Arc<[TitleCaret]>>,
);

/// The carets of a resolved title layer, with the renderer's byte offsets
/// turned into character counts of the text the layer draws.
fn title_carets(
    engine: &mut Engine,
    layer: &SceneLayer,
    canvas: [f32; 2],
) -> Option<std::sync::Arc<[TitleCaret]>> {
    let LayerSource::Text { content, .. } = &layer.source else {
        return None;
    };
    let stops = engine.layer_carets(layer, canvas)?;
    let starts: Vec<usize> = content.char_indices().map(|(byte, _)| byte).collect();
    Some(
        stops
            .iter()
            .map(|stop| TitleCaret {
                index: starts.partition_point(|&byte| byte < stop.byte),
                line: stop.line,
                top: stop.top,
                bottom: stop.bottom,
            })
            .collect(),
    )
}

/// The carets of a title on screen whose text is blank, which resolves to no
/// layer: one per character, stacked on the title's position and one text size
/// tall, so the Type tool still has a caret to draw and type at.
fn blank_title_carets(
    clip: &Clip,
    at: RationalTime,
    [width, height]: [f32; 2],
) -> Option<std::sync::Arc<[TitleCaret]>> {
    let ClipSource::Generated(Generator::Text { content, style }) = &clip.content else {
        return None;
    };
    let span = clip.timeline.start.value..clip.timeline.end_tick();
    if !span.contains(&at.value) || !style.case.apply(content).trim().is_empty() {
        return None;
    }
    let local = clip.param_tick(at.value);
    let pose = clip.transform.sample(local);
    let center = [
        width * (0.5 + pose.position[0]),
        height * (0.5 + pose.position[1]),
    ];
    // Text sizes are authored against a 1080-pixel-tall frame.
    let half = style.size.sample(local) * height / 1080.0 * pose.scale.y.abs() * 0.5;
    let caret = |index| TitleCaret {
        index,
        line: 0,
        top: [center[0], center[1] - half],
        bottom: [center[0], center[1] + half],
    };
    Some((0..=content.chars().count()).map(caret).collect())
}

impl crate::app::AppState {
    /// The selected clip as the model holds it, from the projection.
    ///
    /// `None` when nothing is selected and when the selection names a clip the
    /// project no longer has — an undo can take one away — so a stale selection
    /// draws nothing rather than panicking, which is what asking the session
    /// for it did.
    pub(crate) fn selected_model(&self) -> Option<&Clip> {
        self.selected.as_ref().map(|detail| &detail.model)
    }

    /// The selected layer's quad at the playhead in canvas pixels, `(center,
    /// size)`.
    ///
    /// Read from the resolver and the renderer rather than recomputed, so the
    /// handles sit on the same quad the compositor draws — including the aspect
    /// fit, the per-axis scale, the anchor, the position offset, and a title's
    /// or pen path's raster. `None` when nothing is selected, the clip places
    /// no picture at this instant, or it draws a title or pen path with no
    /// ink.
    pub fn selected_layer_quad(&self) -> Option<([f32; 2], [f32; 2])> {
        self.selected.as_ref()?.quad
    }

    /// The selected clip's own transform at the playhead, before drivers and
    /// parents. The canvas handles, a Viewer arrow and a transform drag start
    /// from this; the projected row holds the clip's first frame, which is a
    /// different pose wherever the transform is keyframed. `None` when nothing
    /// is selected.
    pub fn selected_pose(&self) -> Option<crate::types::UiClipTransform> {
        self.selected.as_ref().map(|detail| detail.pose)
    }
}
