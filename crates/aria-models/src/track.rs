use serde::{Deserialize, Serialize};

use crate::Map;
use crate::clip::generator_catalog::generator_spec;
use crate::clip::{Clip, ClipSource, Generator};
use crate::error::ModelError;
use crate::ids::{ClipId, TrackId};
use crate::time::{RationalTime, TimeRange};
use crate::track_catalog::{LaneZone, TrackKindSpec, track_kind_by_id, track_kind_spec};
use crate::track_routing::{AudioSend, TrackOutput};
use crate::transition::Transition;

/// Lane category on the timeline. Drives drag targeting, clip placement rules,
/// and compositor participation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrackKind {
    /// Footage and other imported picture media.
    Video,
    /// Imported sound media.
    Audio,
    /// Titles and captions.
    Text,
    /// Stickers, shapes, and other graphic overlays.
    Sticker,
    /// Motion and composited effects.
    Effect,
    /// Blur, mask, and similar filters.
    Filter,
    /// Color grade / adjustment layers.
    Adjustment,
    /// A bus: holds no clips, sums what other lanes route into it, and carries
    /// the result on to its own output.
    Submix,
}

impl TrackKind {
    /// This lane kind's capability row: what it accepts, where it stacks in the
    /// lane order, what it is called on the wire and on screen.
    ///
    /// Every predicate below reads one column of it. The rows live in
    /// `track_catalog`, one per kind, and that file's exhaustive match
    /// is what stops a new lane kind from shipping without answers.
    pub const fn spec(self) -> &'static TrackKindSpec {
        track_kind_spec(self)
    }

    /// Picture stack lanes (excludes audio).
    pub const fn is_visual(self) -> bool {
        self.spec().is_visual
    }

    /// Sound lanes.
    pub const fn is_audio(self) -> bool {
        self.spec().is_audio
    }

    /// Lanes whose clips composite as frames and can therefore take a
    /// transition at their junctions. Audio has no frame; effect/filter/
    /// adjustment segments resolve to canvas-wide passes, which the renderer
    /// can't nest inside a transition.
    pub const fn supports_transitions(self) -> bool {
        self.spec().supports_transitions
    }

    /// Whether a lane of this kind can be the main track.
    pub const fn can_be_main(self) -> bool {
        self.spec().can_be_main
    }

    /// Whether a media clip's own audio belongs to a lane of this kind.
    /// Callers still layer mute, solo, and detach state on top.
    pub const fn carries_audio(self) -> bool {
        self.spec().carries_audio
    }

    /// Whether "extract audio" is offered for a clip on a lane of this kind.
    pub const fn extracts_audio(self) -> bool {
        self.spec().extracts_audio
    }

    /// Whether the lane holds no clips and exists to sum what is routed into
    /// it. Routing destinations are chosen by this.
    pub const fn is_bus(self) -> bool {
        self.spec().is_bus
    }

    /// Whether the lane appears in the mixer: it holds sound of its own, or it
    /// is a bus that sums other lanes.
    pub const fn in_mixer(self) -> bool {
        self.spec().in_mixer()
    }

    /// Where a lane of this kind sinks or rises to in the lane stack.
    pub const fn zone(self) -> LaneZone {
        self.spec().zone
    }

    /// Stable wire name — what an agent, a binding, or a projection prints.
    pub const fn id(self) -> &'static str {
        self.spec().id
    }

    /// Operator-facing lane label.
    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    /// Name a freshly created lane of this kind gets when the caller has none.
    pub const fn default_name(self) -> &'static str {
        self.spec().default_name
    }

    /// Lane height a freshly created lane of this kind gets.
    pub const fn default_height(self) -> u32 {
        self.spec().default_height
    }

    /// The kind this wire name addresses, or `None` when nothing does.
    pub fn from_id(id: &str) -> Option<Self> {
        track_kind_by_id(id).map(|spec| spec.kind)
    }

    /// Whether `content` may be placed on a track of this kind.
    pub fn accepts_content(self, content: &ClipSource) -> bool {
        self.spec().accepts_content(content)
    }

    /// Whether `clip` may be placed on a track of this kind.
    pub fn accepts_clip(self, clip: &Clip) -> bool {
        self.spec().accepts_clip(clip)
    }

    /// Track kind required for a generated clip variant.
    ///
    /// The generator's own row names its lane, so this and
    /// [`TrackKind::accepts_content`] read the same fact and cannot drift.
    pub const fn for_generator(generator: &Generator) -> Option<Self> {
        Some(generator_spec(generator).lane)
    }
}

/// Lane view options persisted with the sequence.
///
/// A lane's height and whether it draws waveforms or filmstrips belong to the
/// sequence, not to the session: reopening a project must give back the
/// timeline the operator laid out. Defaults match the shipped lane so old
/// projects load unchanged and write nothing back.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackDisplay {
    /// Lane height in logical pixels, clamped to [`TrackDisplay::HEIGHT`].
    pub height: u32,
    /// Draw audio waveforms on this lane's clips.
    pub waveform: bool,
    /// Draw video filmstrips on this lane's clips.
    pub filmstrip: bool,
    /// Draw the lane as a summary strip instead of full clip cards. The height
    /// is kept, so expanding restores the lane the operator sized.
    pub collapsed: bool,
}

impl TrackDisplay {
    /// The shipped lane height, and the height an old project loads with.
    pub const DEFAULT_HEIGHT: u32 = 48;
    /// The shipped height of a bus lane. It carries no clips, so it needs only
    /// its name, its fader and its routing — the low end of the accepted range.
    pub const BUS_HEIGHT: u32 = 28;
    /// Accepted lane heights. Below the low end a clip label cannot be read;
    /// above the high end one lane fills the panel.
    pub const HEIGHT: std::ops::RangeInclusive<u32> = 28..=200;

    /// Clamp a requested height into [`TrackDisplay::HEIGHT`].
    pub fn clamp_height(height: u32) -> u32 {
        height.clamp(*Self::HEIGHT.start(), *Self::HEIGHT.end())
    }

    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl Default for TrackDisplay {
    fn default() -> Self {
        Self {
            height: Self::DEFAULT_HEIGHT,
            waveform: true,
            filmstrip: true,
            collapsed: false,
        }
    }
}

fn default_true() -> bool {
    true
}

// `&bool` is the signature `skip_serializing_if` requires.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_true(b: &bool) -> bool {
    *b
}

fn unity_gain() -> f32 {
    1.0
}

// `&f32` is the signature `skip_serializing_if` requires.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_unity_gain(gain: &f32) -> bool {
    *gain == 1.0
}

/// A single lane of the timeline holding non-overlapping [`Clip`]s.
///
/// Clips are stored in a hash map keyed by [`ClipId`] for O(1) lookup, with
/// sorted order maintained for allocation-free ordered iteration via
/// [`clips_ordered`](Track::clips_ordered). Overlap is enforced by the
/// [`Timeline`](crate::Timeline) on insert.
#[derive(Debug, Clone, Serialize)]
pub struct Track {
    pub id: TrackId,
    pub kind: TrackKind,
    pub name: String,
    /// Video: whether the track contributes to the composite. Audio: unused.
    pub enabled: bool,
    /// Audio: whether the track is silenced. Video: unused.
    pub muted: bool,
    /// Whether the lane is soloed. While any lane of a kind is soloed, the other
    /// lanes of that kind do not contribute: audio lanes go silent, visual lanes
    /// drop out of the composite. Additive — old projects load with none soloed,
    /// which is the same as no solo at all.
    #[serde(default)]
    pub solo: bool,
    /// Whether the lane is locked: its clips can't be selected, moved, or
    /// trimmed. Compositing/playback are unaffected (CapCut semantics).
    #[serde(default)]
    pub locked: bool,
    /// When true, an empty lane is not auto-removed by implicit cleanup —
    /// only an explicit user delete removes it (manual "Add track").
    #[serde(default)]
    pub pinned: bool,
    /// CapCut's main track: the single privileged video lane every other
    /// visual lane stacks above. It persists when emptied (the only lane
    /// allowed to exist without clips) and sits directly above the audio
    /// floor. At most one track carries the flag and it must be a
    /// [`TrackKind::Video`] lane — [`Timeline`](crate::Timeline) maintains
    /// both invariants on every structural edit.
    #[serde(default)]
    pub main: bool,
    /// Audio: whether this lane is a sidechain "voice" source — its clips
    /// drive audio ducking (M8 Phase 4), so a "duck under voice" gesture dips
    /// music under the talkers on every lane flagged here. Additive: old
    /// projects load with none. Inert on non-audio lanes.
    #[serde(default)]
    pub duck_source: bool,
    /// Whether an edit with no lane named lands here: the target lane of its
    /// kind. Defaults to true and is written only when cleared, so old projects
    /// load with every lane targeted (which is what they behaved as) and save
    /// byte-for-byte as before.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub targeted: bool,
    /// Whether an insert on another lane moves this lane's clips with it, so
    /// material that was in sync stays in sync. Defaults to true, written only
    /// when cleared, for the same reason as [`Self::targeted`].
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub sync_lock: bool,
    /// Lane height and per-lane strip options. Additive: old projects load
    /// with the shipped defaults and write nothing back.
    #[serde(default, skip_serializing_if = "TrackDisplay::is_default")]
    pub display: TrackDisplay,
    /// The lane's fader, as linear gain. Written only when moved, so a project
    /// that has never been mixed loads and saves as it did.
    ///
    /// Set through [`Timeline::set_track_gain`], which is where the accepted
    /// range lives.
    ///
    /// [`Timeline::set_track_gain`]: crate::Timeline::set_track_gain
    #[serde(default = "unity_gain", skip_serializing_if = "is_unity_gain")]
    pub gain: f32,
    /// Where the lane's own sum goes: the master, or a bus lane.
    #[serde(default, skip_serializing_if = "TrackOutput::is_master")]
    pub output: TrackOutput,
    /// Copies of this lane taken for other buses, each at its own level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<AudioSend>,
    #[serde(with = "crate::serde_map")]
    clips: Map<ClipId, Clip>,
    #[serde(skip)]
    order: Vec<ClipId>,
    /// Transitions at clip junctions, keyed by the left (outgoing) clip id.
    /// Additive (M4): old projects load with none. A junction lives only
    /// while its pair still abuts; structural edits prune dead ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transitions: Vec<Transition>,
}

#[derive(Deserialize)]
struct TrackData {
    pub id: TrackId,
    pub kind: TrackKind,
    pub name: String,
    pub enabled: bool,
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub main: bool,
    #[serde(default)]
    pub duck_source: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub targeted: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub sync_lock: bool,
    #[serde(default, skip_serializing_if = "TrackDisplay::is_default")]
    pub display: TrackDisplay,
    #[serde(default = "unity_gain", skip_serializing_if = "is_unity_gain")]
    pub gain: f32,
    #[serde(default, skip_serializing_if = "TrackOutput::is_master")]
    pub output: TrackOutput,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<AudioSend>,
    #[serde(with = "crate::serde_map")]
    pub clips: Map<ClipId, Clip>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<Transition>,
}

impl<'de> Deserialize<'de> for Track {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let data = TrackData::deserialize(deserializer)?;
        let mut track = Self {
            id: data.id,
            kind: data.kind,
            name: data.name,
            enabled: data.enabled,
            muted: data.muted,
            solo: data.solo,
            locked: data.locked,
            pinned: data.pinned,
            main: data.main,
            duck_source: data.duck_source,
            targeted: data.targeted,
            sync_lock: data.sync_lock,
            display: data.display,
            gain: data.gain,
            output: data.output,
            sends: data.sends,
            clips: data.clips,
            order: Vec::new(),
            transitions: data.transitions,
        };
        track.sort_order();
        Ok(track)
    }
}
impl Track {
    /// Create a track with a freshly allocated [`TrackId`].
    pub fn new(kind: TrackKind, name: impl Into<String>) -> Self {
        Self {
            id: TrackId::next(),
            kind,
            name: name.into(),
            enabled: true,
            muted: false,
            solo: false,
            locked: false,
            pinned: false,
            main: false,
            duck_source: false,
            targeted: true,
            sync_lock: true,
            display: TrackDisplay::default(),
            gain: 1.0,
            output: TrackOutput::default(),
            sends: Vec::new(),
            clips: Map::default(),
            order: Vec::new(),
            transitions: Vec::new(),
        }
    }

    pub(crate) fn sort_order(&mut self) {
        self.order.clear();
        self.order.extend(self.clips.keys().copied());
        let clips = &self.clips;
        self.order.sort_by_key(|id| {
            let c = &clips[id];
            (c.timeline.start.value, c.id)
        });
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.get(&id)
    }

    pub fn clip_mut(&mut self, id: ClipId) -> Option<&mut Clip> {
        self.clips.get_mut(&id)
    }

    /// Mutable access for edits that may change a clip's timeline start.
    ///
    /// The ordered index is refreshed when the returned guard is dropped.
    pub fn clip_timeline_mut(&mut self, id: ClipId) -> Option<ClipMut<'_>> {
        let original_start = self.clips.get(&id)?.timeline.start.value;
        Some(ClipMut {
            track: self,
            id,
            original_start,
        })
    }

    pub fn clips(&self) -> impl Iterator<Item = &Clip> {
        self.clips.values()
    }

    /// Applies `f` to every clip, then restores the maintained timeline order.
    pub fn for_each_clip_mut(&mut self, mut f: impl FnMut(&mut Clip)) {
        self.clips.values_mut().for_each(&mut f);
        self.sort_order();
    }

    /// Fallible variant of [`Self::for_each_clip_mut`]. The order is restored
    /// even when `f` returns an error after mutating an earlier clip.
    pub fn try_for_each_clip_mut<E>(
        &mut self,
        mut f: impl FnMut(&mut Clip) -> Result<(), E>,
    ) -> Result<(), E> {
        let result = self.clips.values_mut().try_for_each(&mut f);
        self.sort_order();
        result
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Allocation-free iterator over clips sorted by their timeline start frame
    /// (ties broken by `ClipId`).
    pub fn clips_ordered(&self) -> ClipsOrdered<'_> {
        ClipsOrdered::new(self)
    }
    /// The clip occupying `timeline_pos`, if any. (At most one, since clips on a
    /// track never overlap.)
    pub fn clip_at(&self, timeline_pos: RationalTime) -> Result<Option<&Clip>, ModelError> {
        for clip in self.clips.values() {
            if clip.timeline.contains(timeline_pos)? {
                return Ok(Some(clip));
            }
        }
        Ok(None)
    }

    /// Whether `range` would collide with any existing clip, optionally
    /// ignoring one clip (useful when re-placing an existing clip).
    pub fn has_overlap(
        &self,
        range: TimeRange,
        ignore: Option<ClipId>,
    ) -> Result<bool, ModelError> {
        for clip in self.clips.values().filter(|c| Some(c.id) != ignore) {
            if clip.timeline.overlaps(range)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Exclusive end tick of the last clip (0 if empty), at the timeline rate.
    pub fn content_end(&self) -> i64 {
        self.clips
            .values()
            .map(|c| c.timeline.end_tick())
            .max()
            .unwrap_or(0)
    }

    /// Insert without overlap checking. Returns the displaced clip, if any.
    /// Prefer [`Timeline::add_clip`](crate::Timeline::add_clip) which validates.
    pub(crate) fn insert_clip(&mut self, clip: Clip) -> Option<Clip> {
        let old = self.clips.insert(clip.id, clip);
        self.sort_order();
        old
    }

    pub(crate) fn remove_clip(&mut self, id: ClipId) -> Option<Clip> {
        self.order.retain(|&x| x != id);
        self.clips.remove(&id)
    }

    // --- transitions (M4) -------------------------------------------------

    /// All transitions on this track (junction order is not guaranteed).
    pub fn transitions(&self) -> &[Transition] {
        &self.transitions
    }

    /// The transition set, mutable in place.
    ///
    /// For edits that rescale every junction at once — a sequence rate change
    /// resamples each duration — where the set itself does not change shape, so
    /// there is nothing for [`set_transitions`](Self::set_transitions) to
    /// rebuild.
    pub(crate) fn transitions_mut(&mut self) -> &mut [Transition] {
        &mut self.transitions
    }

    /// Replace the whole transition set (used by undo/prune restore).
    pub(crate) fn set_transitions(&mut self, transitions: Vec<Transition>) {
        self.transitions = transitions;
    }

    /// The transition whose outgoing side is `left`, if any.
    pub fn transition_at(&self, left: ClipId) -> Option<&Transition> {
        self.transitions.iter().find(|t| t.left == left)
    }

    pub fn transition_at_mut(&mut self, left: ClipId) -> Option<&mut Transition> {
        self.transitions.iter_mut().find(|t| t.left == left)
    }

    /// Add or replace the transition at the `left` junction.
    pub(crate) fn upsert_transition(&mut self, transition: Transition) {
        if let Some(existing) = self.transition_at_mut(transition.left) {
            *existing = transition;
        } else {
            self.transitions.push(transition);
        }
    }

    /// Remove the transition at the `left` junction, returning it if present.
    pub(crate) fn remove_transition(&mut self, left: ClipId) -> Option<Transition> {
        let idx = self.transitions.iter().position(|t| t.left == left)?;
        Some(self.transitions.remove(idx))
    }

    /// Whether `left` and `right` abut on this track: `left` ends exactly
    /// where `right` starts and both are present.
    pub fn clips_abut(&self, left: ClipId, right: ClipId) -> bool {
        match (self.clip(left), self.clip(right)) {
            (Some(l), Some(r)) => l.timeline.end_tick() == r.timeline.start.value,
            _ => false,
        }
    }

    /// Drop transitions whose junction no longer abuts (a clip moved, trimmed,
    /// split, or was removed). Returns whether anything was pruned.
    pub(crate) fn prune_dead_transitions(&mut self) -> bool {
        let before = self.transitions.len();
        let clips = &self.clips;
        self.transitions
            .retain(|t| match (clips.get(&t.left), clips.get(&t.right)) {
                (Some(l), Some(r)) => l.timeline.end_tick() == r.timeline.start.value,
                _ => false,
            });
        self.transitions.len() != before
    }
}

/// Mutable access to one clip. Timeline-start changes update the ordered index
/// when the borrow ends; edits to other fields pay no sorting cost.
pub struct ClipMut<'a> {
    track: &'a mut Track,
    id: ClipId,
    original_start: i64,
}

impl std::fmt::Debug for ClipMut<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipMut")
            .field("id", &self.id)
            .field("original_start", &self.original_start)
            .finish()
    }
}

impl std::ops::Deref for ClipMut<'_> {
    type Target = Clip;

    fn deref(&self) -> &Self::Target {
        self.track
            .clips
            .get(&self.id)
            .expect("clip remains present while mutably borrowed")
    }
}

impl std::ops::DerefMut for ClipMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.track
            .clips
            .get_mut(&self.id)
            .expect("clip remains present while mutably borrowed")
    }
}

impl Drop for ClipMut<'_> {
    fn drop(&mut self) {
        if self
            .track
            .clips
            .get(&self.id)
            .is_some_and(|clip| clip.timeline.start.value != self.original_start)
        {
            self.track.sort_order();
        }
    }
}

/// An allocation-free ordered iterator over a track's clips in timeline start order.
#[derive(Debug, Clone)]
pub struct ClipsOrdered<'a> {
    track: &'a Track,
    start_idx: usize,
    end_idx: usize,
}

impl<'a> ClipsOrdered<'a> {
    #[inline]
    pub fn new(track: &'a Track) -> Self {
        Self {
            track,
            start_idx: 0,
            end_idx: track.order.len(),
        }
    }

    #[inline]
    pub fn iter(&self) -> Self {
        self.clone()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.end_idx.saturating_sub(self.start_idx)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start_idx >= self.end_idx
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<&'a Clip> {
        let order_idx = self.start_idx.checked_add(index)?;
        if order_idx < self.end_idx {
            let id = self.track.order.get(order_idx)?;
            self.track.clips.get(id)
        } else {
            None
        }
    }
}

impl<'a> Iterator for ClipsOrdered<'a> {
    type Item = &'a Clip;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while self.start_idx < self.end_idx {
            let id = self.track.order[self.start_idx];
            self.start_idx += 1;
            if let Some(clip) = self.track.clips.get(&id) {
                return Some(clip);
            }
        }
        None
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<'a> DoubleEndedIterator for ClipsOrdered<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        while self.start_idx < self.end_idx {
            self.end_idx -= 1;
            let id = self.track.order[self.end_idx];
            if let Some(clip) = self.track.clips.get(&id) {
                return Some(clip);
            }
        }
        None
    }
}

impl<'a> ExactSizeIterator for ClipsOrdered<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.len()
    }
}

impl<'b, 'a> IntoIterator for &'b ClipsOrdered<'a> {
    type Item = &'a Clip;
    type IntoIter = ClipsOrdered<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.clone()
    }
}

impl<'a> std::ops::Index<usize> for ClipsOrdered<'a> {
    type Output = Clip;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index)
            .expect("index out of bounds in ClipsOrdered")
    }
}

#[cfg(test)]
#[path = "track_tests.rs"]
mod tests;
