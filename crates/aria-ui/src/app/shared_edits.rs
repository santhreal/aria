//! Edits that land on every selected clip that has the property.
//!
//! With several clips selected, Effect Controls draws the first clip's rows and
//! an edit there gives every selected clip that has the property the value the
//! row writes. Every setter on [`AppState`] edits the first selected clip, so a
//! shared edit runs the same setter once per clip, with the selection narrowed
//! to that clip, inside one undo step. A setter therefore needs no second,
//! many-clip form, and what it refuses on one clip it refuses on the others.
//!
//! Two shapes cover every row:
//!
//! - [`AppState::edit_each`] repeats the edit on every clip: a reset, a typed
//!   choice, a keyframe press, an effect added or moved. Each clip reads its own
//!   state, so a reset puts each clip at its own default.
//! - [`AppState::land_each`] runs the edit on the first clip and gives the
//!   others the value it landed on. A step or a scrub is relative to what the
//!   first clip read; repeating it would move clips that read differently by the
//!   same amount and leave them different, where the row shows one value.
//!
//! An effect header's menu names its clip, so its verbs take the clip rather
//! than the selection: [`AppState::effect_verb`] runs one on that clip and, when
//! it is the drawn clip, on the others holding the same effect there.
//!
//! A block's chip or switch reaches the other selected clips of the drawn
//! clip's kind that read what the drawn clip reads through it
//! ([`AppState::peers_reading`]): a switch flips only where it reads as the
//! drawn one does, so every clip ends where the drawn one does, and a clear
//! reaches the clips that have something to clear. A block draws a dash where
//! those clips read differently ([`AppState::peers_differ`]).
//!
//! The first clip goes last in `edit_each`, so a draft an edit opens and the
//! status line it leaves are the first clip's, which is the one drawn.

use aria_engine::EngineError;
use aria_models::{ClipId, ClipParam, ClipSource, Generator, ParamValue, TextStyle};

use crate::app::AppState;
use crate::app::param_units::Scale;
use crate::app::selected_detail::SelectedDetail;
use crate::types::UiClip;

impl AppState {
    /// Whether an edit reaches the other selected clips. Not while a title is
    /// being typed into in the Viewer: its rows then read and style the
    /// characters selected in that title, which no other clip has.
    pub(crate) fn edits_shared(&self) -> bool {
        !self.text_editing()
    }

    /// The other selected clips that have `param` as the first one has it, in
    /// selection order. On an effect slot that is the same effect at the same
    /// place in the chain.
    pub(crate) fn peers_sharing(&self, param: ClipParam) -> Vec<ClipId> {
        let Some(detail) = self.shared_detail() else {
            return Vec::new();
        };
        detail
            .peers
            .iter()
            .filter(|peer| detail.shares(peer, param))
            .map(|peer| peer.model.id)
            .collect()
    }

    /// The other selected clips an effect-chain edit reaches: with `Some`, the
    /// clips that hold the first clip's effect at that index there; with `None`,
    /// for an effect being added, the clips on the same kind of lane.
    pub(crate) fn peers_sharing_effect(&self, index: Option<usize>) -> Vec<ClipId> {
        let Some(detail) = self.shared_detail() else {
            return Vec::new();
        };
        detail
            .peers
            .iter()
            .filter(|peer| match index {
                Some(index) => detail.same_effect(peer, index),
                None => peer.audio_lane == detail.audio_lane,
            })
            .map(|peer| peer.model.id)
            .collect()
    }

    /// The other selected clips that share `param` and whose driver on it is
    /// in the state the first clip's is: a driver toggle adds where the first
    /// clip has none and removes where it has one, and leaves the rest alone.
    pub(crate) fn peers_driven_alike(&self, param: ClipParam) -> Vec<ClipId> {
        let Some(detail) = self.shared_detail() else {
            return Vec::new();
        };
        let driven = detail.model.driver(param).is_some();
        detail
            .peers
            .iter()
            .filter(|peer| detail.shares(peer, param))
            .filter(|peer| peer.model.driver(param).is_some() == driven)
            .map(|peer| peer.model.id)
            .collect()
    }

    /// Run `edit` on every clip in `peers`, then on the first selected clip, as
    /// one undo step; the first clip's result is returned. Without peers this
    /// is `edit` on its own, with no group around it.
    pub(crate) fn edit_each<T>(
        &mut self,
        peers: Vec<ClipId>,
        mut edit: impl FnMut(&mut Self) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        if peers.is_empty() {
            return edit(self);
        }
        self.one_step(|state| {
            state.each_peer(&peers, &mut edit)?;
            edit(state)
        })
    }

    /// Run `edit` on the first selected clip, then give every clip in `peers`
    /// what each of `landed` reads on the first clip afterwards, as one undo
    /// step. A property with a component gives only that axis of a vector, so
    /// a peer keeps its own value on the other axis. Without peers this is
    /// `edit` on its own.
    pub(crate) fn land_each(
        &mut self,
        peers: Vec<ClipId>,
        landed: &[(ClipParam, Option<u8>)],
        edit: impl FnOnce(&mut Self) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        if peers.is_empty() {
            return edit(self);
        }
        self.one_step(|state| {
            edit(state)?;
            let values: Vec<Landed> = landed
                .iter()
                .filter_map(|&(param, component)| match component {
                    Some(axis) => Some(Landed::Axis(
                        param,
                        axis,
                        state.param_scalar(param, Some(axis))?,
                    )),
                    None => Some(Landed::Whole(param, state.param_value(param)?)),
                })
                .collect();
            state.each_peer(&peers, &mut |state| {
                values.iter().try_for_each(|value| match *value {
                    Landed::Axis(param, axis, to) => state.set_param_scalar(param, Some(axis), to),
                    Landed::Whole(param, to) => state.set_param_value(param, to),
                })
            })
        })
    }

    /// [`Self::land_each`] for a value no property row carries: run `edit` on
    /// the first selected clip, then give every clip in `peers` what `read`
    /// takes off the first clip afterwards, through `give`, as one undo step.
    /// Without peers this is `edit` on its own.
    pub(crate) fn land_with<V>(
        &mut self,
        peers: Vec<ClipId>,
        edit: impl FnOnce(&mut Self) -> Result<(), EngineError>,
        read: impl FnOnce(&Self) -> Option<V>,
        mut give: impl FnMut(&mut Self, &V) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        if peers.is_empty() {
            return edit(self);
        }
        self.one_step(|state| {
            edit(state)?;
            let Some(landed) = read(state) else {
                return Ok(());
            };
            state.each_peer(&peers, &mut |state| give(state, &landed))
        })
    }

    /// Run an effect header verb on the effect at `index` in `clip`'s chain,
    /// and, when `clip` is the drawn clip, on every other selected clip that
    /// holds the same effect there, as one undo step named `label` (or after
    /// its first command). The drawn clip goes last. A menu opened on a clip
    /// that is not drawn acts on that clip alone.
    pub(crate) fn effect_verb(
        &mut self,
        clip: ClipId,
        index: usize,
        label: Option<&'static str>,
        mut verb: impl FnMut(&mut Self, ClipId) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let peers = if self.selected_clip() == Some(clip) {
            self.peers_sharing_effect(Some(index))
        } else {
            Vec::new()
        };
        if peers.is_empty() {
            return verb(self, clip);
        }
        let body = |state: &mut Self| {
            for &peer in &peers {
                verb(state, peer)?;
            }
            verb(state, clip)
        };
        match label {
            Some(label) => self.one_step_as(label, body),
            None => self.one_step(body),
        }
    }

    /// A catalog row's step or scrub, in stored units: move `param` by `delta`
    /// on the first selected clip and give the other clips that have it the
    /// same value.
    pub fn nudge_shared_param(
        &mut self,
        param: ClipParam,
        component: Option<u8>,
        delta: f32,
    ) -> Result<(), EngineError> {
        self.nudge_shared_param_in(param, component, Scale::Linear(1.0), delta)
    }

    /// [`Self::nudge_shared_param`] with `delta` in the units `scale` shows
    /// the property in: a percent, a decibel.
    pub fn nudge_shared_param_in(
        &mut self,
        param: ClipParam,
        component: Option<u8>,
        scale: Scale,
        delta: f32,
    ) -> Result<(), EngineError> {
        let peers = self.peers_sharing(param);
        self.land_each(peers, &[(param, component)], |state| {
            state.nudge_param_in(param, component, scale, delta)
        })
    }

    /// The slots no curve carries on the effect at `index` that another
    /// selected clip holding the same effect there sets differently: the
    /// switches, choices, curves, gradients, text and layers Effect Controls
    /// prints a dash on.
    pub fn effect_slots_mixed(&self, index: usize) -> Vec<usize> {
        self.shared_detail()
            .map_or_else(Vec::new, |detail| detail.mixed_effect_slots(index))
    }

    /// The other selected clips of the first one's kind whose row reads what
    /// the first clip's row reads through `reads`, in lane order. A block's
    /// chip or switch reaches these.
    pub(crate) fn peers_reading<T: PartialEq>(&self, reads: impl Fn(&UiClip) -> T) -> Vec<ClipId> {
        let Some(drawn) = self.shared_row() else {
            return Vec::new();
        };
        let mine = reads(drawn);
        self.peer_rows(drawn)
            .filter(|row| reads(row) == mine)
            .map(|row| ClipId::from_raw(row.id))
            .collect()
    }

    /// Whether another selected clip of the first one's kind reads `read`
    /// differently from the first: the dash a block draws over a value no
    /// property row carries.
    pub fn peers_differ<T: PartialEq>(&self, read: impl Fn(&UiClip) -> T) -> bool {
        let Some(drawn) = self.shared_row() else {
            return false;
        };
        let mine = read(drawn);
        self.peer_rows(drawn).any(|row| read(row) != mine)
    }

    /// The other selected titles whose type reads what the first title's
    /// reads through `reads`, in selection order: the [`Self::peers_reading`]
    /// of the Character and Paragraph switches, which no row carries.
    pub(crate) fn titles_reading<T: PartialEq>(
        &self,
        reads: impl Fn(&TextStyle) -> T,
    ) -> Vec<ClipId> {
        let Some((detail, mine)) = self.shared_title() else {
            return Vec::new();
        };
        let mine = reads(mine);
        detail
            .peers
            .iter()
            .filter(|peer| text_style(&peer.model).is_some_and(|style| reads(style) == mine))
            .map(|peer| peer.model.id)
            .collect()
    }

    /// Whether another selected title's type reads `read` differently from
    /// the first title's.
    pub fn titles_differ<T: PartialEq>(&self, read: impl Fn(&TextStyle) -> T) -> bool {
        let Some((detail, mine)) = self.shared_title() else {
            return false;
        };
        let mine = read(mine);
        detail
            .peers
            .iter()
            .filter_map(|peer| text_style(&peer.model))
            .any(|style| read(style) != mine)
    }

    /// The first selected clip's row, while its edits reach the others.
    fn shared_row(&self) -> Option<&UiClip> {
        self.shared_detail()?;
        self.selected_ui_clip_ref()
    }

    /// The rows of the other selected clips of `drawn`'s kind.
    fn peer_rows<'a>(&'a self, drawn: &'a UiClip) -> impl Iterator<Item = &'a UiClip> + 'a {
        self.store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter(move |row| row.selected && row.id != drawn.id && row.kind == drawn.kind)
    }

    /// The first selected clip's detail and its type, when it is a title and
    /// its edits reach the others.
    fn shared_title(&self) -> Option<(&SelectedDetail, &TextStyle)> {
        let detail = self.shared_detail()?;
        Some((detail, text_style(&detail.model)?))
    }

    /// The selected clip's detail while its edits reach the other selected
    /// clips.
    fn shared_detail(&self) -> Option<&SelectedDetail> {
        self.selected.as_ref().filter(|_| self.edits_shared())
    }

    /// Run `edit` with each clip of `peers` as the only selected clip, then put
    /// the selection back. Stops at the first failure; the caller's undo group
    /// takes back what the earlier clips landed. The mask the Viewer edits is
    /// the first clip's and stays so, though a mask edit on another clip moves
    /// it there.
    fn each_peer<T>(
        &mut self,
        peers: &[ClipId],
        edit: &mut impl FnMut(&mut Self) -> Result<T, EngineError>,
    ) -> Result<(), EngineError> {
        let selection = std::mem::take(&mut self.selection);
        let active_mask = self.tool_session.active_mask;
        let mut result = Ok(());
        for &peer in peers {
            self.selection = vec![peer];
            self.refresh_projected_selection();
            if let Err(err) = edit(self) {
                result = Err(err);
                break;
            }
        }
        self.selection = selection;
        self.tool_session.active_mask = active_mask;
        self.refresh_projected_selection();
        result
    }
}

/// A title's type, or `None` when `clip` is not a title.
fn text_style(clip: &aria_models::Clip) -> Option<&TextStyle> {
    match &clip.content {
        ClipSource::Generated(Generator::Text { style, .. }) => Some(style),
        _ => None,
    }
}

/// What a property reads on the first clip after an edit, to give the others.
enum Landed {
    /// One axis of a vector property.
    Axis(ClipParam, u8, f32),
    /// The whole value.
    Whole(ClipParam, ParamValue),
}
