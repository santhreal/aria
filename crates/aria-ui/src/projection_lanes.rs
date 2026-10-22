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

use aria_models::{AudioLaneLinks, ClipId, MediaKind, Project, Rational, TrackId};
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
    mixer: Arc<(crate::app::mixer::MixerBasis, crate::app::mixer::MixerFacts)>,
}

/// One lane as the projection built it.
#[derive(Clone)]
struct Built {
    revision: u64,
    clips: Weak<Vec<UiClip>>,
    has_links: bool,
    has_effects: bool,
    has_transitions: bool,
    has_clip_types: bool,
    clip_markers: usize,
    media_in_use: Arc<[u64]>,
}

#[cfg(test)]
std::thread_local! {
    static BUILT_CLIP_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_built_clip_probes() {
    BUILT_CLIP_PROBES.with(|n| n.set(0));
}

#[cfg(test)]
pub(crate) fn built_clip_probes() -> usize {
    BUILT_CLIP_PROBES.with(std::cell::Cell::get)
}

impl Built {
    fn of(track: &aria_models::Track, clips: &UiClips) -> Self {
        let mut has_links = false;
        let mut has_effects = false;
        let mut has_clip_types = false;
        let mut clip_markers = 0usize;
        let mut media: Vec<u64> = Vec::new();
        for clip in track.clips() {
            #[cfg(test)]
            BUILT_CLIP_PROBES.with(|n| n.set(n.get() + 1));
            has_links |= clip.link.is_some();
            has_effects |= !clip.effects.is_empty();
            has_clip_types |= clip.clip_type.is_some();
            clip_markers += clip.markers.len();
            if let Some(m) = clip.media() {
                media.push(m.raw());
            }
        }
        media.sort_unstable();
        media.dedup();
        Self {
            revision: track.revision(),
            clips: clips.downgrade(),
            has_links,
            has_effects,
            has_transitions: !track.transitions().is_empty(),
            has_clip_types,
            clip_markers,
            media_in_use: media.into(),
        }
    }
}

impl LaneBasis {
    pub(crate) fn catalog_usage(&self) -> (bool, bool, bool) {
        let (mut effects, mut transitions, mut clip_types) = (false, false, false);
        for built in self.lanes.values() {
            effects |= built.has_effects;
            transitions |= built.has_transitions;
            clip_types |= built.has_clip_types;
        }
        (effects, transitions, clip_types)
    }

    pub(crate) fn clip_marker_count(&self) -> usize {
        self.lanes.values().map(|b| b.clip_markers).sum()
    }

    pub(crate) fn lane_has_clip_markers(&self, track_id: u64) -> bool {
        self.lanes
            .get(&track_id)
            .is_some_and(|b| b.clip_markers > 0)
    }

    pub(crate) fn media_in_use(&self) -> Vec<u64> {
        let mut used: Vec<u64> = self
            .lanes
            .values()
            .flat_map(|b| b.media_in_use.iter().copied())
            .collect();
        used.sort_unstable();
        used.dedup();
        used
    }

    pub(crate) fn mixer(&self) -> crate::app::mixer::MixerFacts {
        self.mixer.1.clone()
    }
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

/// What a clip reads of the media it places: its name, what it can do (kind,
/// still or moving, sound), and whether the file is there.
#[derive(PartialEq)]
pub(super) struct MediaFact {
    id: u64,
    pub(super) name: SharedString,
    pub(super) path: SharedString,
    kind: MediaKind,
    is_image: bool,
    has_audio: bool,
    pub(super) missing: bool,
}

/// The media facts of a projection by media id: a clip placing media finds its
/// facts with one lookup.
pub(super) type MediaFacts<'a> = aria_models::Map<u64, &'a MediaFact>;

/// What a linked clip reads of its link partners. Read once per projection,
/// and only when a rebuilt lane holds a linked clip; a lane holding none reads
/// the empty default.
#[derive(Default)]
pub(super) struct LinkFacts {
    /// How far each drifted linked clip sits from its partner, by clip.
    pub(super) drifts: HashMap<ClipId, i64>,
    /// The link groups with a member on an audio lane.
    pub(super) audio: AudioLaneLinks,
}

impl LinkFacts {
    fn of(project: &Project) -> Self {
        Self {
            drifts: project.link_drifts(),
            audio: project.timeline().audio_lane_links(),
        }
    }
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
                    name: item.name.clone(),
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
    let mut linked = None;
    let unlinked = LinkFacts::default();
    let mut built = HashMap::with_capacity(timeline.track_count());
    let lanes = {
        let facts: MediaFacts<'_> = globals.media.iter().map(|fact| (fact.id, fact)).collect();
        timeline
            .tracks_ordered()
            .map(|track| {
                let id = track.id.raw();
                let (clips, lane_built) = match kept.get(&id) {
                    Some((clips, prev_built)) => {
                        let cloned = UiClips::clone(clips);
                        let mut b = (*prev_built).clone();
                        b.clips = cloned.downgrade();
                        (cloned, b)
                    }
                    None => {
                        let has_links = track.clips().any(|c| c.link.is_some());
                        let links = if has_links {
                            linked.get_or_insert_with(|| LinkFacts::of(project))
                        } else {
                            &unlinked
                        };
                        let clips = lane_clips(project, track, selection, links, &facts);
                        let b = Built::of(track, &clips);
                        (clips, b)
                    }
                };
                built.insert(id, lane_built);
                lane_to_ui(track, clips)
            })
            .collect()
    };
    let mixer = match previous.filter(|prev| prev.basis.mixer.0.matches(timeline)) {
        Some(prev) => Arc::clone(&prev.basis.mixer),
        None => Arc::new((
            crate::app::mixer::MixerBasis::of(timeline),
            crate::app::mixer::MixerFacts::of_timeline(timeline),
        )),
    };
    let basis = LaneBasis {
        globals,
        lanes: built,
        order: timeline.order().iter().map(|id| id.raw()).collect(),
        selection: selection.to_vec(),
        mixer,
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
) -> HashMap<u64, (&'a UiClips, &'a Built)> {
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
        if let Some((lane, built)) = lane.zip(previous.basis.lanes.get(&id))
            && built.revision == track.revision()
            && lane.clips.is(&built.clips)
        {
            kept.insert(id, (&lane.clips, built));
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
        kept.retain(|id, (_, built)| {
            if !built.has_links {
                return true;
            }
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
