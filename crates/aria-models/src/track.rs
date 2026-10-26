use serde::{Deserialize, Serialize};

use crate::Map;
use crate::audio_effects::AudioEffectInstance;
use crate::clip::generator_catalog::generator_spec;
use crate::clip::{Clip, ClipSource, Generator};
use crate::content_hash::Kept;
use crate::error::ModelError;
use crate::ids::{ClipId, TrackId};
use crate::param::{Param, default_zero_param, is_zero_param};
use crate::time::{RationalTime, TimeRange};
use crate::track_catalog::{LaneZone, TrackKindSpec, track_kind_by_id, track_kind_spec};
use crate::track_clips::LaneClips;
use crate::track_routing::{AudioSend, AutomationMode, TrackOutput};
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

    /// Which transitions this lane's cuts take: a catalog blend on a lane that
    /// composites frames, an audio crossfade on a sound lane that holds clips,
    /// none elsewhere.
    pub const fn transition_media(self) -> Option<crate::TransitionMedia> {
        if self.supports_transitions() {
            Some(crate::TransitionMedia::Picture)
        } else if self.is_audio() && !self.is_bus() {
            Some(crate::TransitionMedia::Audio)
        } else {
            None
        }
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
/// timeline as it was laid out. Defaults match the shipped lane so old
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
    /// is kept, so expanding restores the lane at its set height.
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
    pub const HEIGHT: std::ops::RangeInclusive<u32> = 20..=200;

    /// Named lane height presets `(label, height_px)` in ascending height order.
    pub const PRESETS: [(&'static str, u32); 4] =
        [("Mini", 20), ("Small", 32), ("Medium", 48), ("Large", 72)];
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

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_pan(pan: &f32) -> bool {
    *pan == 0.0
}

fn default_volume_envelope() -> Param<f32> {
    Param::Constant(1.0)
}

fn is_default_volume_envelope(envelope: &Param<f32>) -> bool {
    envelope.constant() == Some(1.0)
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
    /// Stereo pan (`-1.0` hard left .. `1.0` hard right, `0.0` center).
    #[serde(default, skip_serializing_if = "is_zero_pan")]
    pub pan: f32,
    /// How this strip handles its volume and pan automation curves.
    #[serde(default, skip_serializing_if = "AutomationMode::is_off")]
    pub automation: AutomationMode,
    /// Sequence-tick volume automation curve (linear gain `0.0..=4.0`).
    #[serde(
        default = "default_volume_envelope",
        skip_serializing_if = "is_default_volume_envelope"
    )]
    pub volume_envelope: Param<f32>,
    /// Sequence-tick pan automation curve (`-1.0..=1.0`).
    #[serde(default = "default_zero_param", skip_serializing_if = "is_zero_param")]
    pub pan_envelope: Param<f32>,
    /// Where the lane's own sum goes: the master, or a bus lane.
    #[serde(default, skip_serializing_if = "TrackOutput::is_master")]
    pub output: TrackOutput,
    /// Copies of this lane taken for other buses, each at its own level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<AudioSend>,
    /// The lane's insert chain: audio effects run over the lane's sum, in
    /// order, before its fader and its sends. Keyframes are sequence ticks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_effects: Vec<AudioEffectInstance>,
    /// Saved as `serde_map` saves a map. Inside the lane's revision each clip
    /// is written as the revision the lane keeps for it; see [`LaneClips`].
    #[serde(serialize_with = "LaneClips::serialize")]
    clips: LaneClips,
    #[serde(skip)]
    order: Vec<ClipId>,
    /// Transitions at clip junctions, keyed by the left (outgoing) clip id.
    /// Additive (M4): old projects load with none. A junction lives only
    /// while its pair still abuts; structural edits prune dead ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transitions: Vec<Transition>,
    /// See [`Track::revision`]. Not saved: a loaded lane takes a fresh one.
    #[serde(skip)]
    revision: Kept,
}

#[cfg(test)]
thread_local! {
    /// Lanes [`Track::revision`] hashed on this thread.
    static HASHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Clips [`Track::clip_at`], [`Track::has_overlap`], and [`Track::content_end`]
    /// inspected on this thread.
    static ORDER_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub automation: AutomationMode,
    #[serde(default = "default_volume_envelope")]
    pub volume_envelope: Param<f32>,
    #[serde(default = "default_zero_param")]
    pub pan_envelope: Param<f32>,
    #[serde(default, skip_serializing_if = "TrackOutput::is_master")]
    pub output: TrackOutput,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<AudioSend>,
    #[serde(default)]
    pub audio_effects: Vec<AudioEffectInstance>,
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
            pan: data.pan,
            automation: data.automation,
            volume_envelope: data.volume_envelope,
            pan_envelope: data.pan_envelope,
            output: data.output,
            sends: data.sends,
            audio_effects: data.audio_effects,
            clips: LaneClips::new(data.clips),
            order: Vec::new(),
            transitions: data.transitions,
            revision: Kept::default(),
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
            pan: 0.0,
            automation: AutomationMode::default(),
            volume_envelope: default_volume_envelope(),
            pan_envelope: default_zero_param(),
            output: TrackOutput::default(),
            sends: Vec::new(),
            audio_effects: Vec::new(),
            clips: LaneClips::default(),
            order: Vec::new(),
            transitions: Vec::new(),
            revision: Kept::default(),
        }
    }

    /// A hash of everything the lane saves, so two lanes with one revision
    /// hold the same content, and a write that leaves the content as it was
    /// (a refused edit, a pass over every lane that moves one) leaves it too.
    /// A view of the timeline rebuilds a lane only when this moved.
    ///
    /// The hash is computed on the first read and kept: each `&mut Track` a
    /// [`Timeline`](crate::Timeline) hands out, each lane it takes in, and
    /// each clip the lane hands out for a write drops the kept value, and a
    /// clone keeps it. Reading the revision of a lane nothing wrote to since
    /// the last read costs one load.
    ///
    /// The lane keeps a revision for each clip the same way, and its hash
    /// reads those rather than the clips' content: a write to one clip of a
    /// lane hashes that clip again and no other.
    pub fn revision(&self) -> u64 {
        self.revision.get_or(|| {
            #[cfg(test)]
            HASHED.with(|count| count.set(count.get() + 1));
            crate::content_hash::summary_hash(self)
        })
    }

    /// The revision of the clip `id` on this lane: a hash of everything the
    /// clip saves, kept by the lane as [`Self::revision`] is. A view of the
    /// lane rebuilds a clip only when this moved. `None` for a clip not on the
    /// lane.
    pub fn clip_revision(&self, id: ClipId) -> Option<u64> {
        self.clips.revision(id)
    }

    /// Drop the kept [`revision`](Self::revision), so the next read hashes
    /// the lane again.
    pub(crate) fn touch(&mut self) {
        self.revision.forget();
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
        self.touch();
        self.clips.get_mut(id)
    }

    /// Mutable access for edits that may change a clip's timeline start.
    ///
    /// The ordered index is refreshed when the returned guard is dropped.
    pub fn clip_timeline_mut(&mut self, id: ClipId) -> Option<ClipMut<'_>> {
        let original_start = self.clips.get(&id)?.timeline.start.value;
        self.touch();
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
        self.touch();
        self.clips.values_mut().for_each(&mut f);
        self.sort_order();
    }

    /// Fallible variant of [`Self::for_each_clip_mut`]. The order is restored
    /// even when `f` returns an error after mutating an earlier clip.
    pub fn try_for_each_clip_mut<E>(
        &mut self,
        mut f: impl FnMut(&mut Clip) -> Result<(), E>,
    ) -> Result<(), E> {
        self.touch();
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

    /// Clips overlapping `range`, in timeline start order.
    pub fn clips_overlapping(&self, range: TimeRange) -> impl Iterator<Item = &Clip> {
        let start = range.start.value;
        let end = range.end_tick();
        let hi = self.order.partition_point(|id| {
            #[cfg(test)]
            ORDER_PROBES.with(|c| c.set(c.get() + 1));
            self.clips[id].timeline.start.value < end
        });
        let mut lo = hi;
        while lo > 0 {
            #[cfg(test)]
            ORDER_PROBES.with(|c| c.set(c.get() + 1));
            if self.clips[&self.order[lo - 1]].timeline.end_tick() <= start {
                break;
            }
            lo -= 1;
        }
        self.order[lo..hi].iter().map(move |id| &self.clips[id])
    }

    /// Clips starting at `tick`, in id order: one, or more where zero-length
    /// clips share the tick. Found by the start order, so a lane is not walked.
    pub fn clips_starting_at(&self, tick: i64) -> impl Iterator<Item = &Clip> {
        let from = self.order.partition_point(|id| {
            #[cfg(test)]
            ORDER_PROBES.with(|c| c.set(c.get() + 1));
            self.clips[id].timeline.start.value < tick
        });
        self.order[from..]
            .iter()
            .map(move |id| &self.clips[id])
            .take_while(move |clip| clip.timeline.start.value == tick)
    }

    /// The clip occupying `timeline_pos`, if any. (At most one, since clips on a
    /// track never overlap.)
    pub fn clip_at(&self, timeline_pos: RationalTime) -> Result<Option<&Clip>, ModelError> {
        let Some(&first_id) = self.order.first() else {
            return Ok(None);
        };
        crate::time::check_same_rate(self.clips[&first_id].timeline.start.rate, timeline_pos.rate)?;
        let idx = self.order.partition_point(|id| {
            #[cfg(test)]
            ORDER_PROBES.with(|c| c.set(c.get() + 1));
            self.clips[id].timeline.start.value <= timeline_pos.value
        });
        for &id in self.order[..idx].iter().rev() {
            #[cfg(test)]
            ORDER_PROBES.with(|c| c.set(c.get() + 1));
            let clip = &self.clips[&id];
            if clip.timeline.contains(timeline_pos)? {
                return Ok(Some(clip));
            }
            if clip.timeline.end_tick() <= timeline_pos.value {
                break;
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
        let end = range.end_tick();
        let idx = self.order.partition_point(|id| {
            #[cfg(test)]
            ORDER_PROBES.with(|c| c.set(c.get() + 1));
            self.clips[id].timeline.start.value < end
        });
        for &id in self.order[..idx].iter().rev() {
            if Some(id) == ignore {
                continue;
            }
            #[cfg(test)]
            ORDER_PROBES.with(|c| c.set(c.get() + 1));
            let clip = &self.clips[&id];
            if clip.timeline.overlaps(range)? {
                return Ok(true);
            }
            if clip.timeline.end_tick() <= range.start.value {
                break;
            }
        }
        for &id in &self.order[idx..] {
            if Some(id) == ignore {
                continue;
            }
            #[cfg(test)]
            ORDER_PROBES.with(|c| c.set(c.get() + 1));
            let clip = &self.clips[&id];
            crate::time::check_same_rate(clip.timeline.start.rate, range.start.rate)?;
            break;
        }
        Ok(false)
    }

    /// Exclusive end tick of the last clip (0 if empty), at the timeline rate.
    pub fn content_end(&self) -> i64 {
        self.order
            .last()
            .and_then(|id| {
                #[cfg(test)]
                ORDER_PROBES.with(|c| c.set(c.get() + 1));
                self.clips.get(id)
            })
            .map_or(0, |c| c.timeline.end_tick())
    }

    /// Insert without overlap checking. Returns the displaced clip, if any.
    /// Prefer [`Timeline::add_clip`](crate::Timeline::add_clip) which validates.
    ///
    /// The clip takes its slot in the maintained order by binary search, so a
    /// placement costs a lookup per halving and not a re-sort of the lane.
    pub(crate) fn insert_clip(&mut self, clip: Clip) -> Option<Clip> {
        self.touch();
        let key = (clip.timeline.start.value, clip.id);
        let old = self.clips.insert(clip);
        if old.is_some() {
            self.order.retain(|&x| x != key.1);
        }
        let clips = &self.clips;
        let at = self.order.partition_point(|id| {
            let c = &clips[id];
            (c.timeline.start.value, c.id) < key
        });
        self.order.insert(at, key.1);
        old
    }

    pub(crate) fn remove_clip(&mut self, id: ClipId) -> Option<Clip> {
        self.touch();
        self.order.retain(|&x| x != id);
        self.clips.remove(id)
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
            .get_mut(self.id)
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

impl<'a> IntoIterator for &ClipsOrdered<'a> {
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

#[cfg(test)]
#[path = "track_revision_tests.rs"]
mod revision_tests;
