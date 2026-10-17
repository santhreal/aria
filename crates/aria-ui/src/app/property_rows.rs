//! Timeline property rows, keyframe selection, reveal actions, and ease commands.
//!
//! Each clip on a timeline track can open into an outline of property groups
//! (`Transform`, `Effects (n)`, `Masks (n)`, `Audio`) or a filtered set of
//! properties revealed by the `Alt` property chords (`Alt+P/S/R/T/A/E/M/L/U`).
//! Every edit dispatches through [`AppState::apply_command`] or
//! `AppState::one_step` so multi-keyframe moves, pastes, deletions, and ease
//! changes land as a single undo entry.

#[path = "property_rows_graph.rs"]
pub mod graph;
#[path = "property_rows_search.rs"]
mod search;

#[cfg(test)]
#[path = "property_rows_tests.rs"]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    Clip, ClipId, ClipParam, EASY_EASE_INFLUENCE, Easing, ParamSpec, ParamValue, SpatialTangents,
    TemporalEase,
};
use gpui::SharedString;

use super::AppState;
use super::clip_keys::{KeyParam, key_params_of};
use super::effects_browser::contains_folded;
use super::mask_path_keys::{mask_path_label, path_masks};

pub use graph::{
    GraphDragTarget, GraphHandleSide, GraphInfluenceHandle, GraphKeyNode, GraphMode,
    GraphSpatialHandle, TimelineGraphCurve, TimelineGraphModel,
};

/// Height (px) of one timeline outline group header or property row.
pub const PROPERTY_ROW_H: f32 = crate::theme::Theme::ROW_HEIGHT;

/// Height (px) of the timeline graph editor band when open.
pub const GRAPH_EDITOR_H: f32 = 168.0;

/// Top-level outline group shown when a clip is expanded with the twirl triangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PropertyGroup {
    Transform,
    Effects,
    Masks,
    Audio,
}

impl PropertyGroup {
    pub const ALL: [Self; 4] = [Self::Transform, Self::Effects, Self::Masks, Self::Audio];

    /// Classify a catalog parameter into its timeline outline group, if it
    /// belongs to one of the four timeline groups.
    pub fn of(spec: &ParamSpec) -> Option<Self> {
        match spec.param {
            ClipParam::Effect { .. } => return Some(Self::Effects),
            ClipParam::Mask { .. } => return Some(Self::Masks),
            _ => {}
        }
        // Catalog groups are strings: `transform`, `audio`, `effect-<index>`
        // for an effect instance's parameters, and `mask-<index>` for one
        // mask's.
        match spec.group.as_ref() {
            "transform" => Some(Self::Transform),
            "audio" => Some(Self::Audio),
            group if group.starts_with("effect-") => Some(Self::Effects),
            group if group.starts_with("mask-") => Some(Self::Masks),
            _ => None,
        }
    }
}

/// Property reveal action bound to `Alt+P/S/R/T/A/E/M/L/U` and ``Alt+` ``.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealAction {
    Position,
    Scale,
    Rotation,
    Opacity,
    Anchor,
    Effects,
    Masks,
    Audio,
    /// First press reveals animated properties; a second consecutive press
    /// reveals all properties modified from their default value.
    AnimatedOrModified,
    /// Every property modified from its default value, without the animated
    /// press first (Animation > Reveal Modified Properties).
    Modified,
    /// Close every open property row (`Alt+\``).
    CloseAll,
}

/// Distinct visual icon painted for a keyframe on a timeline property row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyframeIconShape {
    /// Linear interpolation (diamond).
    Linear,
    /// Bezier / Easy Ease on both sides (hourglass).
    Bezier,
    /// Ease In on the incoming side, linear on the outgoing side.
    EaseIn,
    /// Linear on the incoming side, Ease Out on the outgoing side.
    EaseOut,
    /// Hold keyframe (square).
    Hold,
    /// Roving interior keyframe (circle).
    Roving,
}

impl KeyframeIconShape {
    /// Classify a keyframe from its index, total keyframe count, and easing.
    pub fn classify(index: usize, total: usize, easing: Easing) -> Self {
        if easing.is_hold() {
            return Self::Hold;
        }
        let interior = index > 0 && index + 1 < total;
        if interior && easing.is_roving() {
            return Self::Roving;
        }
        match easing {
            Easing::Linear => Self::Linear,
            Easing::Hold => Self::Hold,
            Easing::EaseIn => Self::EaseOut,
            Easing::EaseOut => Self::EaseIn,
            Easing::EaseInOut | Easing::Bezier { .. } => Self::Bezier,
            Easing::Temporal {
                in_ease, out_ease, ..
            } => {
                let in_shaped = in_ease.is_some_and(is_shaped_ease);
                let out_shaped = out_ease.is_some_and(is_shaped_ease);
                match (in_shaped, out_shaped) {
                    (true, true) => Self::Bezier,
                    (true, false) => Self::EaseIn,
                    (false, true) => Self::EaseOut,
                    (false, false) => Self::Bezier,
                }
            }
        }
    }
}

fn is_shaped_ease(ease: TemporalEase) -> bool {
    ease.speed.abs() > 1e-4 || (ease.influence - EASY_EASE_INFLUENCE).abs() <= 0.02
}

/// One selected keyframe on a timeline property row or in the graph editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SelectedKeyframe {
    pub clip: ClipId,
    pub param: ClipParam,
    pub tick: i64,
}

/// One keyframe copied onto the keyframe clipboard.
#[derive(Debug, Clone, PartialEq)]
pub struct CopiedKeyframe {
    pub param: ClipParam,
    pub offset_ticks: i64,
    pub value: ParamValue,
    pub easing: Easing,
    pub tangents: Option<SpatialTangents>,
}

/// One keyframe marker projected for a timeline property row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineKeyMark {
    pub tick: i64,
    pub shape: KeyframeIconShape,
    pub easing: Easing,
    pub selected: bool,
    pub roving: bool,
    pub hold: bool,
    pub interior: bool,
}

/// How a property row's readout is dragged and typed, in the units it prints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowReadout {
    /// Printed units per stored unit: 100 for a fraction printed as a percent.
    pub scale: f32,
    /// Digits after the point.
    pub decimals: u8,
    /// One unit of drag, in printed units.
    pub unit: f32,
}

/// One property row drawn under an expanded clip on a timeline track.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelinePropertyRow {
    pub clip: ClipId,
    pub clip_start: i64,
    pub clip_end: i64,
    pub param: ClipParam,
    pub id: SharedString,
    pub label: SharedString,
    pub value_label: SharedString,
    pub scalar_at_playhead: f32,
    /// How the value readout drags and types. `None` for a colour or a
    /// rectangle, which a single number cannot move.
    pub readout: Option<RowReadout>,
    pub keyframed: bool,
    pub keyed_at_playhead: bool,
    pub has_prev_key: bool,
    pub has_next_key: bool,
    pub driver_active: bool,
    /// The value at the playhead is not the one a reset returns to, so the
    /// row's reset button has something to do.
    pub off_default: bool,
    pub selected_for_graph: bool,
    pub keys: Vec<TimelineKeyMark>,
}

/// The Path row of one path mask, drawn under an expanded clip. A keyframe of
/// it is a whole path, so it is no catalog property: the row reads and writes
/// the mask's path curve (`mask_path_keys.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineMaskPathRow {
    pub clip: ClipId,
    /// The mask's index in the clip's stack.
    pub mask: u32,
    pub clip_start: i64,
    pub clip_end: i64,
    pub label: SharedString,
    /// The vertex count of the path at the playhead.
    pub value_label: SharedString,
    pub keyframed: bool,
    pub keyed_at_playhead: bool,
    pub has_prev_key: bool,
    pub has_next_key: bool,
    pub keys: Vec<TimelineKeyMark>,
}

/// One visible outline row under a timeline track.
#[derive(Debug, Clone, PartialEq)]
pub enum TimelineOutlineRow {
    GroupHeader {
        clip: ClipId,
        group: PropertyGroup,
        label: SharedString,
        expanded: bool,
    },
    Property(TimelinePropertyRow),
    MaskPath(TimelineMaskPathRow),
}

/// Context menu opened by right-clicking a keyframe in a property row or graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyframeContextMenu {
    pub clip: ClipId,
    pub param: ClipParam,
    pub tick: i64,
    pub interior: bool,
    pub hold: bool,
    pub roving: bool,
    pub x: f32,
    pub y: f32,
}

/// Context menu opened by right-clicking a keyframe on a mask's Path row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaskPathKeyMenu {
    pub clip: ClipId,
    pub mask: u32,
    /// The keyframe's timeline tick.
    pub tick: i64,
    pub hold: bool,
    pub x: f32,
    pub y: f32,
}

/// In-flight time drag of one keyframe on a mask's Path row. Ticks are on the
/// timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskPathKeyDrag {
    pub clip: ClipId,
    pub mask: u32,
    pub origin: i64,
    pub delta: i64,
}

/// In-flight horizontal time drag of one or more timeline keyframes.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframeTimeDrag {
    pub anchor_clip: ClipId,
    pub anchor_param: ClipParam,
    pub anchor_origin_tick: i64,
    pub delta_ticks: i64,
    pub origins: Vec<(
        ClipId,
        ClipParam,
        i64,
        ParamValue,
        Easing,
        Option<SpatialTangents>,
    )>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClipRevealMode {
    Outline {
        open_groups: BTreeSet<PropertyGroup>,
    },
    Explicit {
        params: Vec<ClipParam>,
        /// The path masks whose Path row is revealed, by index.
        paths: Vec<u32>,
        last_was_animated_u: bool,
    },
}

impl Default for ClipRevealMode {
    fn default() -> Self {
        let mut open_groups = BTreeSet::new();
        open_groups.insert(PropertyGroup::Transform);
        Self::Outline { open_groups }
    }
}

#[derive(Default)]
pub(crate) struct PropertyRowsState {
    clip_modes: HashMap<u64, ClipRevealMode>,
    pub(crate) selected_keyframes: BTreeSet<SelectedKeyframe>,
    pub(crate) copied_keyframes: Vec<CopiedKeyframe>,
    pub(crate) graph_open: bool,
    pub(crate) graph_mode: GraphMode,
    pub(crate) graph_params: Vec<(ClipId, ClipParam)>,
    pub(crate) time_drag: Option<KeyframeTimeDrag>,
    pub(crate) graph_drag: Option<GraphDragTarget>,
    pub(crate) keyframe_menu: Option<KeyframeContextMenu>,
    pub(crate) mask_path_menu: Option<MaskPathKeyMenu>,
    pub(crate) mask_path_drag: Option<MaskPathKeyDrag>,
    pub(crate) collapsed_effect_groups: BTreeSet<(usize, String)>,
    pub(crate) open_effect_groups: BTreeSet<(usize, String)>,
    pub(crate) layer_refusal: Option<super::clip_effect_layers::LayerRefusal>,
    /// A press held on an Effect Controls time area.
    pub(crate) strip_press: Option<super::key_strip::StripPress>,
    /// The property search in the timeline header, and whether it holds the
    /// keyboard.
    query: String,
    search_focused: bool,
}

impl AppState {
    pub(crate) fn with_prop_rows<R>(&self, f: impl FnOnce(&PropertyRowsState) -> R) -> R {
        f(&self.prop_rows)
    }

    pub(crate) fn with_prop_rows_mut<R>(
        &mut self,
        f: impl FnOnce(&mut PropertyRowsState) -> R,
    ) -> R {
        f(&mut self.prop_rows)
    }

    /// Set incoming/outgoing temporal ease and/or hold state on the keyframe
    /// under the playhead.
    pub fn set_keyframe_temporal_ease_at_playhead(
        &mut self,
        param: ClipParam,
        in_ease: Option<TemporalEase>,
        out_ease: Option<TemporalEase>,
        hold: bool,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.has_keyframe_at_playhead(param) {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetKeyframeEase {
            clip,
            param,
            at: self.playhead,
            in_ease,
            out_ease,
            hold,
        }))?;
        Ok(())
    }

    /// Set roving state on the interior keyframe under the playhead.
    pub fn set_keyframe_roving_at_playhead(
        &mut self,
        param: ClipParam,
        roving: bool,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.has_keyframe_at_playhead(param) {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetKeyframeRoving {
            clip,
            param,
            at: self.playhead,
            roving,
        }))?;
        Ok(())
    }

    /// Toggle a clip's outline property rows open or closed (`▸` / `▾`).
    pub fn toggle_clip_outline(&mut self, clip_id: u64) {
        let open = Arc::make_mut(&mut self.expanded_clips);
        if let Some(idx) = open.iter().position(|id| *id == clip_id) {
            open.remove(idx);
            self.with_prop_rows_mut(|s| {
                s.clip_modes.remove(&clip_id);
            });
        } else {
            open.push(clip_id);
            self.with_prop_rows_mut(|s| {
                s.clip_modes.insert(clip_id, ClipRevealMode::default());
            });
        }
        self.refresh_expanded_models();
    }

    /// Toggle one outline group (`Transform`, `Effects`, `Masks`, `Audio`) on an
    /// expanded clip.
    pub fn toggle_property_group(&mut self, clip: ClipId, group: PropertyGroup) {
        let raw = clip.raw();
        let open = Arc::make_mut(&mut self.expanded_clips);
        if !open.contains(&raw) {
            open.push(raw);
        }
        self.with_prop_rows_mut(|s| {
            let mode = s
                .clip_modes
                .entry(raw)
                .or_insert_with(ClipRevealMode::default);
            match mode {
                ClipRevealMode::Outline { open_groups } => {
                    if !open_groups.remove(&group) {
                        open_groups.insert(group);
                    }
                }
                ClipRevealMode::Explicit { .. } => {
                    let mut open_groups = BTreeSet::new();
                    open_groups.insert(group);
                    *mode = ClipRevealMode::Outline { open_groups };
                }
            }
        });
        self.refresh_expanded_models();
    }

    /// Reveal or toggle timeline property rows via `Alt+P/S/R/T/A/E/M/L/U` or
    /// close every open property row via ``Alt+` ``.
    pub fn reveal_properties(&mut self, action: RevealAction, shift: bool) {
        if action == RevealAction::CloseAll {
            Arc::make_mut(&mut self.expanded_clips).clear();
            self.with_prop_rows_mut(|s| {
                s.clip_modes.clear();
            });
            self.refresh_expanded_models();
            return;
        }
        let targets: Vec<ClipId> = if !self.selection.is_empty() {
            self.selection.clone()
        } else {
            let at = self.playhead.value;
            self.store
                .sequence
                .tracks
                .iter()
                .filter(|t| t.targeted && !t.locked)
                .flat_map(|t| t.clips.iter())
                .filter(|c| !c.locked && c.timeline_start_ticks <= at && at < c.timeline_end_ticks)
                .map(|c| ClipId::from_raw(c.id))
                .collect()
        };
        if targets.is_empty() {
            self.status_message = Some("Select a clip first".into());
            return;
        }

        for clip_id in targets {
            let raw = clip_id.raw();
            let prev_animated_u = self.with_prop_rows(|s| {
                matches!(
                    s.clip_modes.get(&raw),
                    Some(ClipRevealMode::Explicit {
                        last_was_animated_u: true,
                        ..
                    })
                ) && self.expanded_clips.contains(&raw)
            });
            // A clip not yet open has no projected model, and this is an
            // edit of the view rather than a frame, so it reads the session.
            let held = {
                let engine = self.engine.lock();
                lane_model(engine.project(), clip_id)
            };
            let (resolved, is_animated_u) = match &held {
                Some((clip, audio_lane)) => {
                    resolve_reveal_params(clip, *audio_lane, action, prev_animated_u)
                }
                None => (Revealed::default(), false),
            };

            let currently_open = self.expanded_clips.contains(&raw);

            if shift {
                let mut next = if currently_open {
                    Revealed {
                        params: self.visible_params_for_clip(clip_id),
                        paths: self.visible_mask_paths(clip_id),
                    }
                } else {
                    Revealed::default()
                };
                if !resolved.is_empty() && next.contains_all(&resolved) {
                    next.remove(&resolved);
                } else {
                    next.add(resolved);
                }
                if next.is_empty() {
                    Arc::make_mut(&mut self.expanded_clips).retain(|id| *id != raw);
                    self.with_prop_rows_mut(|s| {
                        s.clip_modes.remove(&raw);
                    });
                } else {
                    let open = Arc::make_mut(&mut self.expanded_clips);
                    if !open.contains(&raw) {
                        open.push(raw);
                    }
                    self.with_prop_rows_mut(|s| {
                        s.clip_modes.insert(
                            raw,
                            ClipRevealMode::Explicit {
                                params: next.params,
                                paths: next.paths,
                                last_was_animated_u: false,
                            },
                        );
                    });
                }
            } else {
                let same_already_open = currently_open
                    && action != RevealAction::AnimatedOrModified
                    && self.with_prop_rows(|s| {
                        matches!(
                            s.clip_modes.get(&raw),
                            Some(ClipRevealMode::Explicit { params, paths, .. })
                                if *params == resolved.params && *paths == resolved.paths
                        )
                    });
                if same_already_open {
                    Arc::make_mut(&mut self.expanded_clips).retain(|id| *id != raw);
                    self.with_prop_rows_mut(|s| {
                        s.clip_modes.remove(&raw);
                    });
                } else {
                    let open = Arc::make_mut(&mut self.expanded_clips);
                    if !open.contains(&raw) {
                        open.push(raw);
                    }
                    self.with_prop_rows_mut(|s| {
                        s.clip_modes.insert(
                            raw,
                            ClipRevealMode::Explicit {
                                params: resolved.params,
                                paths: resolved.paths,
                                last_was_animated_u: is_animated_u,
                            },
                        );
                    });
                }
            }
        }
        self.refresh_expanded_models();
    }

    /// Re-read the model of every clip whose property rows are open.
    ///
    /// Called when the open set changes and when a new projection is adopted,
    /// which are the only moments the models it holds can have moved.
    pub(crate) fn refresh_expanded_models(&mut self) {
        let models = {
            let engine = self.engine.lock();
            self.expanded_clips
                .iter()
                .filter_map(|&raw| {
                    lane_model(engine.project(), ClipId::from_raw(raw)).map(|held| (raw, held))
                })
                .collect()
        };
        self.expanded_models = models;
    }

    /// The model of `clip_id` and whether it sits on an audio lane, from the
    /// projection: a clip with open property rows, or the selected clip.
    /// `None` for any other clip, which no property row draws.
    pub(crate) fn clip_and_lane_kind(&self, clip_id: ClipId) -> Option<(Clip, bool)> {
        if let Some((clip, audio_lane)) = self.expanded_models.get(&clip_id.raw()) {
            return Some((clip.clone(), *audio_lane));
        }
        let detail = self
            .selected
            .as_ref()
            .filter(|detail| detail.model.id == clip_id)?;
        Some((detail.model.clone(), detail.audio_lane))
    }

    /// The parameters currently visible in property rows for `clip_id`.
    pub fn visible_params_for_clip(&self, clip_id: ClipId) -> Vec<ClipParam> {
        let raw = clip_id.raw();
        if !self.expanded_clips.contains(&raw) {
            return Vec::new();
        }
        let Some((clip, audio_lane)) = self.clip_and_lane_kind(clip_id) else {
            return Vec::new();
        };
        let rows = key_params_of(&clip, audio_lane);
        let needle = self.property_needle();
        match self.reveal_mode(raw, !needle.is_empty()) {
            ClipRevealMode::Outline { open_groups } => rows
                .iter()
                .filter(|r| {
                    PropertyGroup::of(&r.spec).is_some_and(|g| open_groups.contains(&g))
                        && contains_folded(&r.label, &needle)
                })
                .map(KeyParam::param)
                .collect(),
            ClipRevealMode::Explicit { params, .. } => rows
                .iter()
                .map(KeyParam::param)
                .filter(|p| params.contains(p))
                .collect(),
        }
    }

    /// The path masks of `clip_id` whose Path row is visible, by index.
    fn visible_mask_paths(&self, clip_id: ClipId) -> Vec<u32> {
        let raw = clip_id.raw();
        if !self.expanded_clips.contains(&raw) {
            return Vec::new();
        }
        let Some((clip, audio_lane)) = self.clip_and_lane_kind(clip_id) else {
            return Vec::new();
        };
        let needle = self.property_needle();
        match self.reveal_mode(raw, !needle.is_empty()) {
            ClipRevealMode::Outline { open_groups } => {
                if audio_lane || !open_groups.contains(&PropertyGroup::Masks) {
                    return Vec::new();
                }
                path_masks(&clip)
                    .filter(|&mask| {
                        needle.is_empty() || contains_folded(&mask_path_label(mask), &needle)
                    })
                    .collect()
            }
            ClipRevealMode::Explicit { paths, .. } => paths,
        }
    }

    /// Build the visible outline rows (group headers and property rows) for a
    /// timeline track. Empty when the track is collapsed or none of its clips
    /// are expanded.
    pub fn track_outline_rows(&self, track_id: u64) -> Vec<TimelineOutlineRow> {
        let Some(track) = self.store.sequence.tracks.iter().find(|t| t.id == track_id) else {
            return Vec::new();
        };
        if track.collapsed {
            return Vec::new();
        }
        let expanded_on_track: Vec<u64> = track
            .clips
            .iter()
            .map(|c| c.id)
            .filter(|id| self.expanded_clips.contains(id))
            .collect();
        if expanded_on_track.is_empty() {
            return Vec::new();
        }

        let needle = self.property_needle();
        let mut out = Vec::new();
        for raw_clip in expanded_on_track {
            let clip_id = ClipId::from_raw(raw_clip);
            let Some((clip, audio_lane)) = self.clip_and_lane_kind(clip_id) else {
                continue;
            };
            let rows = key_params_of(&clip, audio_lane);
            match self.reveal_mode(raw_clip, !needle.is_empty()) {
                ClipRevealMode::Outline { open_groups } => {
                    for group in PropertyGroup::ALL {
                        if audio_lane
                            && matches!(group, PropertyGroup::Transform | PropertyGroup::Masks)
                        {
                            continue;
                        }
                        let group_rows: Vec<&KeyParam> = rows
                            .iter()
                            .filter(|r| PropertyGroup::of(&r.spec) == Some(group))
                            .collect();
                        let expanded = open_groups.contains(&group);
                        let label = group_header_label(&clip, group);
                        let header = out.len();
                        out.push(TimelineOutlineRow::GroupHeader {
                            clip: clip_id,
                            group,
                            label,
                            expanded,
                        });
                        if expanded {
                            // A path mask's Path row leads its mask's rows.
                            let mut led = None;
                            for row in group_rows {
                                if let ClipParam::Mask { mask, .. } = row.param()
                                    && led != Some(mask)
                                {
                                    led = Some(mask);
                                    out.extend(
                                        self.mask_path_row(&clip, mask)
                                            .filter(|path| contains_folded(&path.label, &needle))
                                            .map(TimelineOutlineRow::MaskPath),
                                    );
                                }
                                if contains_folded(&row.label, &needle) {
                                    out.push(TimelineOutlineRow::Property(
                                        self.build_property_row(&clip, row),
                                    ));
                                }
                            }
                        }
                        // A search leaves out a group with nothing that matches.
                        if !needle.is_empty() && out.len() == header + 1 {
                            out.pop();
                        }
                    }
                }
                ClipRevealMode::Explicit { params, paths, .. } => {
                    // Each revealed Path row goes before the first revealed row
                    // of its mask or of a later one, and the rest after all.
                    let mut paths = paths.into_iter().peekable();
                    for param in params {
                        if let ClipParam::Mask { mask, .. } = param {
                            while let Some(path) = paths.next_if(|path| *path <= mask) {
                                out.extend(
                                    self.mask_path_row(&clip, path)
                                        .map(TimelineOutlineRow::MaskPath),
                                );
                            }
                        }
                        if let Some(row) = rows.iter().find(|r| r.param() == param) {
                            out.push(TimelineOutlineRow::Property(
                                self.build_property_row(&clip, row),
                            ));
                        }
                    }
                    for path in paths {
                        out.extend(
                            self.mask_path_row(&clip, path)
                                .map(TimelineOutlineRow::MaskPath),
                        );
                    }
                }
            }
        }
        out
    }

    fn build_property_row(&self, clip: &Clip, row: &KeyParam) -> TimelinePropertyRow {
        let clip_id = clip.id;
        let param = row.param();
        let at_playhead = self.playhead.value;
        let local_tick = clip.param_tick(at_playhead);
        let sampled = row.spec.sample(clip, local_tick);
        let off_default = sampled
            .zip(row.spec.default_value(clip))
            .is_some_and(|(value, default)| value != default);
        let (value_label, scalar_at_playhead, readout) =
            format_param_value(clip, &row.spec, sampled);
        let raw_keys = row.spec.keyframes(clip);
        let total = raw_keys.len();
        let (selected_set, graph_selected, drag_delta) = self.with_prop_rows(|s| {
            let graph_sel = s.graph_params.contains(&(clip_id, param))
                || (s.graph_params.is_empty() && self.curve_param() == Some(param));
            let delta = s.time_drag.as_ref().map(|d| {
                (
                    d.delta_ticks,
                    d.origins
                        .iter()
                        .map(|(c, p, t, _, _, _)| (*c, *p, *t))
                        .collect::<Vec<_>>(),
                )
            });
            (s.selected_keyframes.clone(), graph_sel, delta)
        });

        let mut keys = Vec::with_capacity(total);
        for (idx, (local_t, easing)) in raw_keys.iter().enumerate() {
            let mut abs_tick = clip.param_timeline_tick(*local_t);
            let selected = selected_set.contains(&SelectedKeyframe {
                clip: clip_id,
                param,
                tick: abs_tick,
            });
            if let Some((delta, ref dragged)) = drag_delta
                && dragged.contains(&(clip_id, param, abs_tick))
            {
                abs_tick =
                    (abs_tick + delta).clamp(clip.timeline.start.value, clip.timeline.end_tick());
            }
            let interior = idx > 0 && idx + 1 < total;
            keys.push(TimelineKeyMark {
                tick: abs_tick,
                shape: KeyframeIconShape::classify(idx, total, *easing),
                easing: *easing,
                selected,
                roving: interior && easing.is_roving(),
                hold: easing.is_hold(),
                interior,
            });
        }

        let keyed_at_playhead = keys.iter().any(|k| k.tick == at_playhead);
        let has_prev_key = keys.iter().any(|k| k.tick < at_playhead);
        let has_next_key = keys.iter().any(|k| k.tick > at_playhead);

        TimelinePropertyRow {
            clip: clip_id,
            clip_start: clip.timeline.start.value,
            clip_end: clip.timeline.end_tick(),
            param,
            id: row.id.clone(),
            label: row.label.clone(),
            value_label,
            scalar_at_playhead,
            readout,
            keyframed: !keys.is_empty(),
            keyed_at_playhead,
            has_prev_key,
            has_next_key,
            driver_active: clip.driver(param).is_some(),
            off_default,
            selected_for_graph: graph_selected,
            keys,
        }
    }
}

fn group_header_label(clip: &Clip, group: PropertyGroup) -> SharedString {
    match group {
        PropertyGroup::Transform => SharedString::new_static("Transform"),
        PropertyGroup::Effects => format!("Effects ({})", clip.effects.len()).into(),
        PropertyGroup::Masks => format!("Masks ({})", clip.masks.len()).into(),
        PropertyGroup::Audio => SharedString::new_static("Audio"),
    }
}

/// What a row prints, the scalar its readout moves (the first component of a
/// pair), and how that readout drags and types.
pub(super) fn format_param_value(
    clip: &Clip,
    spec: &ParamSpec,
    value: Option<ParamValue>,
) -> (SharedString, f32, Option<RowReadout>) {
    let readout = |scale: f32, decimals: u8| RowReadout {
        scale,
        decimals,
        unit: super::clip_params::scrub_unit(&spec.range) * scale,
    };
    match value {
        Some(ParamValue::Scalar(v)) => {
            let (text, shape) = match spec.param {
                ClipParam::Opacity => (format!("{:.0}%", v * 100.0), readout(100.0, 0)),
                ClipParam::Rotation => (format!("{v:.1}°"), readout(1.0, 1)),
                ClipParam::Effect { effect, param } => {
                    let pspec = clip
                        .effects
                        .get(effect as usize)
                        .and_then(|inst| aria_models::effect_spec(&inst.effect_id))
                        .and_then(|espec| espec.param_at(param as usize).cloned());
                    match pspec {
                        Some(p) if p.kind == aria_models::EffectParamKind::Int => (
                            format!("{}", v.round() as i32),
                            RowReadout {
                                scale: 1.0,
                                decimals: 0,
                                unit: 1.0,
                            },
                        ),
                        Some(p)
                            if p.kind == aria_models::EffectParamKind::Angle
                                || p.display
                                    == Some(aria_models::EffectParamDisplay::AngleDial) =>
                        {
                            (format!("{v:.1}°"), readout(1.0, 1))
                        }
                        Some(p)
                            if p.display == Some(aria_models::EffectParamDisplay::Percentage) =>
                        {
                            (format!("{:.0}%", v * 100.0), readout(100.0, 0))
                        }
                        Some(p) if p.unit.as_deref().is_some_and(|u| !u.is_empty()) => {
                            let unit = p.unit.as_deref().unwrap_or_default();
                            (format!("{v:.2} {unit}"), readout(1.0, 2))
                        }
                        _ => (format!("{v:.2}"), readout(1.0, 2)),
                    }
                }
                _ => (format!("{v:.2}"), readout(1.0, 2)),
            };
            (text.into(), v, Some(shape))
        }
        Some(ParamValue::Vec2([x, y])) => {
            if spec.param == ClipParam::Scale {
                (
                    format!("{:.0}%, {:.0}%", x * 100.0, y * 100.0).into(),
                    x,
                    Some(readout(100.0, 0)),
                )
            } else {
                (format!("{x:.1}, {y:.1}").into(), x, Some(readout(1.0, 1)))
            }
        }
        Some(ParamValue::Vec3([x, y, z])) => (
            format!("{x:.1}, {y:.1}, {z:.1}").into(),
            x,
            Some(readout(1.0, 1)),
        ),
        Some(ParamValue::Color([r, g, b, _])) => {
            (format!("#{r:02X}{g:02X}{b:02X}").into(), f32::from(r), None)
        }
        Some(ParamValue::Rect([x, y, w, h])) => {
            (format!("{x:.2}, {y:.2} {w:.2}×{h:.2}").into(), x, None)
        }
        None => (SharedString::new_static("—"), 0.0, None),
    }
}

/// `clip` with every property that has a defined rest value put back at it:
/// the transform, the crop, gain and pan, the colour adjustments, and each
/// effect's parameters (an unwritten effect parameter reads its catalog
/// default). Blocks without a rest value (mask, key, matte, filter, LUT, layer
/// styles, generator geometry) keep their values, so their properties count as
/// modified only when animated or driven.
fn rest_state(clip: &Clip) -> Clip {
    let mut rest = clip.clone();
    rest.transform = Default::default();
    rest.crop = aria_models::Param::Constant(aria_models::CropRect::FULL);
    rest.volume = aria_models::Param::Constant(1.0);
    rest.pan = aria_models::Param::Constant(0.0);
    rest.adjust = Default::default();
    for effect in &mut rest.effects {
        effect.params.clear();
        effect.color_params.clear();
        effect.linear_color_params.clear();
        effect.vec2_params.clear();
        effect.vec3_params.clear();
        effect.bool_params.clear();
        effect.choice_params.clear();
        effect.curve_params.clear();
        effect.gradient_params.clear();
        effect.text_params.clear();
        effect.layer_params.clear();
    }
    rest
}

/// Whether `row` is animated, driven, or holds a value other than its value on
/// `rest` (see [`rest_state`]).
fn is_param_modified(clip: &Clip, rest: &Clip, row: &KeyParam) -> bool {
    if !row.marks.is_empty() || clip.driver(row.param()).is_some() {
        return true;
    }
    let local = clip.param_tick(clip.timeline.start.value);
    match (row.spec.sample(clip, local), row.spec.sample(rest, local)) {
        (Some(value), Some(rest)) => !same_value(value, rest),
        (Some(_), None) => true,
        _ => false,
    }
}

/// Whether two sampled values are equal within the drift a keyframe round trip
/// leaves in an `f32`.
fn same_value(a: ParamValue, b: ParamValue) -> bool {
    const EPSILON: f32 = 1e-4;
    let close = |x: &[f32], y: &[f32]| x.iter().zip(y).all(|(p, q)| (p - q).abs() <= EPSILON);
    match (a, b) {
        (ParamValue::Scalar(x), ParamValue::Scalar(y)) => (x - y).abs() <= EPSILON,
        (ParamValue::Vec2(x), ParamValue::Vec2(y)) => close(&x, &y),
        (ParamValue::Vec3(x), ParamValue::Vec3(y)) => close(&x, &y),
        (ParamValue::Rect(x), ParamValue::Rect(y)) => close(&x, &y),
        (ParamValue::Color(x), ParamValue::Color(y)) => x == y,
        _ => false,
    }
}

/// What a reveal chord opens on one clip: catalog properties, and the path
/// masks whose Path row shows.
#[derive(Debug, Clone, Default, PartialEq)]
struct Revealed {
    params: Vec<ClipParam>,
    /// Mask indices, ascending.
    paths: Vec<u32>,
}

impl Revealed {
    fn is_empty(&self) -> bool {
        self.params.is_empty() && self.paths.is_empty()
    }

    fn contains_all(&self, other: &Self) -> bool {
        other.params.iter().all(|param| self.params.contains(param))
            && other.paths.iter().all(|path| self.paths.contains(path))
    }

    fn remove(&mut self, other: &Self) {
        self.params.retain(|param| !other.params.contains(param));
        self.paths.retain(|path| !other.paths.contains(path));
    }

    fn add(&mut self, other: Self) {
        for param in other.params {
            if !self.params.contains(&param) {
                self.params.push(param);
            }
        }
        for path in other.paths {
            if !self.paths.contains(&path) {
                self.paths.push(path);
            }
        }
        self.paths.sort_unstable();
    }
}

/// The rows a reveal chord opens on `clip`, and whether this press was the
/// first `U` of a pair.
///
/// A Path row opens with the masks, with the animated properties when its path
/// is keyframed, and with the modified ones always: a path is drawn, so it
/// never sits at a default.
fn resolve_reveal_params(
    clip: &Clip,
    audio_lane: bool,
    action: RevealAction,
    second_u: bool,
) -> (Revealed, bool) {
    let rows = key_params_of(clip, audio_lane);
    let filter_in = |pred: &dyn Fn(&KeyParam) -> bool| -> Vec<ClipParam> {
        rows.iter()
            .filter(|r| pred(r))
            .map(KeyParam::param)
            .collect()
    };
    let only = |params: Vec<ClipParam>| Revealed {
        params,
        paths: Vec::new(),
    };
    let paths = |keyed: bool| -> Vec<u32> {
        if audio_lane {
            return Vec::new();
        }
        path_masks(clip)
            .filter(|&mask| !keyed || clip.masks[mask as usize].path.is_animated())
            .collect()
    };
    match action {
        RevealAction::Position => (
            only(filter_in(&|r| r.param() == ClipParam::Position)),
            false,
        ),
        RevealAction::Scale => (only(filter_in(&|r| r.param() == ClipParam::Scale)), false),
        RevealAction::Rotation => (
            only(filter_in(&|r| r.param() == ClipParam::Rotation)),
            false,
        ),
        RevealAction::Opacity => (only(filter_in(&|r| r.param() == ClipParam::Opacity)), false),
        RevealAction::Anchor => (
            only(filter_in(&|r| r.param() == ClipParam::AnchorPoint)),
            false,
        ),
        RevealAction::Effects => (
            only(filter_in(&|r| {
                PropertyGroup::of(&r.spec) == Some(PropertyGroup::Effects)
            })),
            false,
        ),
        RevealAction::Masks => (
            Revealed {
                params: filter_in(&|r| PropertyGroup::of(&r.spec) == Some(PropertyGroup::Masks)),
                paths: paths(false),
            },
            false,
        ),
        RevealAction::Audio => (
            only(filter_in(&|r| {
                matches!(r.param(), ClipParam::Volume | ClipParam::Pan)
            })),
            false,
        ),
        RevealAction::AnimatedOrModified if !second_u => (
            Revealed {
                params: filter_in(&|r| !r.marks.is_empty() || clip.driver(r.param()).is_some()),
                paths: paths(true),
            },
            true,
        ),
        RevealAction::AnimatedOrModified | RevealAction::Modified => {
            let rest = rest_state(clip);
            (
                Revealed {
                    params: filter_in(&|r| is_param_modified(clip, &rest, r)),
                    paths: paths(false),
                },
                false,
            )
        }
        RevealAction::CloseAll => (Revealed::default(), false),
    }
}

/// `clip_id`'s model and whether it sits on an audio lane. The lane, not the
/// clip, decides whether the visual properties are offered, the same answer
/// `SelectedDetail::of` reads.
fn lane_model(project: &aria_models::Project, clip_id: ClipId) -> Option<(Clip, bool)> {
    let timeline = project.timeline();
    let clip = timeline.clip(clip_id)?.clone();
    let audio_lane = timeline
        .track_of(clip_id)
        .and_then(|track| timeline.track(track))
        .is_some_and(|track| track.kind.is_audio());
    Some((clip, audio_lane))
}
