//! Which lanes a projection rebuilds, and which it takes from the last one.
//!
//! A lane's clips are rebuilt when any of these holds, and taken from the
//! previous store otherwise:
//!
//! - the lane is new, or its revision moved: every write the timeline hands
//!   out moves it;
//! - the previous store no longer holds the clips built for the lane, because
//!   a caller wrote to them after the projection;
//! - a clip on the lane entered or left the selection;
//! - the lane holds a clip linked to a clip on a rebuilt or removed lane: a
//!   linked clip's drift and its detached-sound flag read its partners. When
//!   the lane order changed, every lane holding a linked clip is rebuilt,
//!   because drift is measured against the first partner in lane order.
//!
//! Everything else a clip reads from outside its lane is in [`Globals`]: the
//! sequence rate, the effect, transition, clip type and audio effect catalogs,
//! the open sequence, the media pool, the sequence names and the multicam
//! groups. When any of it changed, every lane is rebuilt.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use aria_models::{ClipId, MediaKind, Project, Rational, TrackId};
use gpui::SharedString;

use super::clip::{lane_clips, lane_to_ui};
use crate::types::{
    UiClip, UiClips, UiEditorStore, UiMediaItem, UiMulticamGroup, UiSequenceRef, UiTrack,
};

/// The store the last projection built, and what it was built from.
#[derive(Clone, Copy)]
pub(crate) struct Previous<'a> {
    pub(crate) store: &'a UiEditorStore,
    pub(crate) basis: &'a LaneBasis,
}

/// What a projection read, kept so the next one rebuilds only the lanes that
/// moved.
pub(crate) struct LaneBasis {
    globals: Globals,
    lanes: HashMap<u64, Built>,
    order: Vec<u64>,
    selection: Vec<ClipId>,
}

/// One lane as the projection built it.
struct Built {
    revision: u64,
    clips: Weak<Vec<UiClip>>,
}

/// What a clip reads from outside its own lane.
#[derive(PartialEq)]
pub(super) struct Globals {
    rate: Rational,
    catalogs: [u64; 4],
    open_sequence: Option<u64>,
    media: Vec<MediaFact>,
    sequences: Vec<(u64, SharedString)>,
    multicam: Arc<[UiMulticamGroup]>,
}

/// What a clip reads of the media it places: its name (from the path), what
/// it can do (kind, still or moving, sound), and whether the file is there.
#[derive(PartialEq)]
struct MediaFact {
    id: u64,
    path: SharedString,
    kind: MediaKind,
    is_image: bool,
    has_audio: bool,
    missing: bool,
}

impl Globals {
    /// Read from the parts of the store a projection builds before its lanes.
    /// `media` is the pool as projected from `project`, in pool order.
    pub(super) fn of(
        project: &Project,
        media: &[UiMediaItem],
        sequences: &[UiSequenceRef],
        multicam: &Arc<[UiMulticamGroup]>,
    ) -> Self {
        Self {
            rate: project.timeline().frame_rate,
            catalogs: [
                aria_models::effect_catalog().revision(),
                aria_models::transition_catalog().revision(),
                aria_models::clip_type_catalog().revision(),
                aria_models::audio_effect_catalog().revision(),
            ],
            open_sequence: project.active_sequence().map(|id| id.raw()),
            media: project
                .media_iter()
                .zip(media)
                .map(|(source, item)| MediaFact {
                    id: item.id,
                    path: item.path.clone(),
                    kind: source.kind(),
                    is_image: source.is_image,
                    has_audio: source.has_audio,
                    missing: item.missing,
                })
                .collect(),
            sequences: sequences
                .iter()
                .map(|sequence| (sequence.id, sequence.name.clone()))
                .collect(),
            multicam: Arc::clone(multicam),
        }
    }
}

/// Every lane of `project` in stacking order, bottom to top, and the basis the
/// next projection compares against. With `previous`, a lane that did not move
/// keeps the clips the previous store holds for it.
pub(super) fn lanes(
    project: &Project,
    selection: &[ClipId],
    globals: Globals,
    previous: Option<Previous<'_>>,
) -> (Vec<UiTrack>, LaneBasis) {
    let timeline = project.timeline();
    let kept = previous
        .filter(|previous| previous.basis.globals == globals)
        .map(|previous| kept(project, selection, previous))
        .unwrap_or_default();
    let mut drifts = None;
    let mut built = HashMap::with_capacity(timeline.track_count());
    let lanes = timeline
        .tracks_ordered()
        .map(|track| {
            let id = track.id.raw();
            let clips = match kept.get(&id) {
                Some(clips) => UiClips::clone(clips),
                None => {
                    let drifts = drifts.get_or_insert_with(|| project.link_drifts());
                    lane_clips(project, track, selection, drifts)
                }
            };
            built.insert(
                id,
                Built {
                    revision: track.revision(),
                    clips: clips.downgrade(),
                },
            );
            lane_to_ui(track, clips)
        })
        .collect();
    let basis = LaneBasis {
        globals,
        lanes: built,
        order: timeline.order().iter().map(|id| id.raw()).collect(),
        selection: selection.to_vec(),
    };
    (lanes, basis)
}

/// Write `selection` into the clip flags of `store`, touching only the lanes
/// holding a clip whose flag changes. With the basis `store` was projected
/// with, the lanes written are recorded in it, so the next projection still
/// takes them: a selection changes nothing a clip reads but its flag.
pub(crate) fn reselect(
    store: &mut UiEditorStore,
    selection: &[ClipId],
    mut basis: Option<&mut LaneBasis>,
) {
    let ids: HashSet<u64> = selection.iter().map(|id| id.raw()).collect();
    for lane in &mut store.sequence.tracks {
        if lane
            .clips
            .iter()
            .all(|clip| clip.selected == ids.contains(&clip.id))
        {
            continue;
        }
        let built = basis
            .as_deref_mut()
            .and_then(|basis| basis.lanes.get_mut(&lane.id))
            .filter(|built| lane.clips.is(&built.clips));
        for clip in lane.clips.to_mut() {
            clip.selected = ids.contains(&clip.id);
        }
        if let Some(built) = built {
            built.clips = lane.clips.downgrade();
        }
    }
    if let Some(basis) = basis {
        basis.selection.clear();
        basis.selection.extend_from_slice(selection);
    }
}

/// The lanes whose clips carry over from `previous`, by lane id.
fn kept<'a>(
    project: &Project,
    selection: &[ClipId],
    previous: Previous<'a>,
) -> HashMap<u64, &'a UiClips> {
    let timeline = project.timeline();
    let held: HashMap<u64, &UiTrack> = previous
        .store
        .sequence
        .tracks
        .iter()
        .map(|lane| (lane.id, lane))
        .collect();
    let mut kept = HashMap::with_capacity(held.len());
    // Links held by a rebuilt or removed lane, before and after the edit.
    let mut links: HashSet<u64> = HashSet::new();
    for track in timeline.tracks_ordered() {
        let id = track.id.raw();
        let lane = held.get(&id);
        let unmoved = lane
            .zip(previous.basis.lanes.get(&id))
            .is_some_and(|(lane, built)| {
                built.revision == track.revision() && lane.clips.is(&built.clips)
            });
        if unmoved {
            if let Some(lane) = lane {
                kept.insert(id, &lane.clips);
            }
            continue;
        }
        links.extend(
            track
                .clips()
                .filter_map(|clip| clip.link)
                .map(|link| link.raw()),
        );
        if let Some(lane) = lane {
            links.extend(lane.clips.iter().filter_map(|clip| clip.linked_to));
        }
    }
    for lane in held.values() {
        if timeline.track(TrackId::from_raw(lane.id)).is_none() {
            links.extend(lane.clips.iter().filter_map(|clip| clip.linked_to));
        }
    }
    let was: HashSet<ClipId> = previous.basis.selection.iter().copied().collect();
    let now: HashSet<ClipId> = selection.iter().copied().collect();
    for clip in now.symmetric_difference(&was) {
        if let Some(lane) = timeline.track_of(*clip) {
            kept.remove(&lane.raw());
        }
    }
    let reordered = !timeline.order().iter().map(|id| id.raw()).eq(previous
        .basis
        .order
        .iter()
        .copied());
    if reordered || !links.is_empty() {
        kept.retain(|id, _| {
            timeline.track(TrackId::from_raw(*id)).is_some_and(|track| {
                !track.clips().any(|clip| {
                    clip.link
                        .is_some_and(|link| reordered || links.contains(&link.raw()))
                })
            })
        });
    }
    kept
}

#[cfg(test)]
#[path = "projection_lanes_tests.rs"]
mod tests;
