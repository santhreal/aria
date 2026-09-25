use serde::{Deserialize, Serialize};

use crate::Map;
use crate::clip::Clip;
use crate::error::ModelError;
use crate::ids::{ClipId, MarkerId, TrackId};
use crate::time::{Rational, RationalTime, resample};
use crate::track::{Track, TrackKind};
use crate::track_catalog::LaneZone;
use crate::track_routing::LaneRoute;

#[path = "timeline_canvas.rs"]
mod canvas;

pub use canvas::{AudioLayout, CanvasAspect, CanvasSettings, Marker, MarkerColor};

/// What [`Timeline::remove_track`] took out: the lane, and the routing that
/// pointed at it.
///
/// A bus removal changes lanes other than the one removed, so restoring the
/// lane alone would leave those lanes routed to the master. Keeping both in one
/// value is what makes the undo exact.
#[derive(Debug, Clone)]
pub struct RemovedTrack {
    /// The lane that was removed, with its clips.
    pub track: Track,
    /// The routes taken off other lanes because they pointed at it.
    pub detached: Vec<LaneRoute>,
}
/// The single sequence of a [`Project`](crate::Project): an ordered stack of
/// tracks plus a clip-location index.
///
/// - `tracks` is keyed by [`TrackId`] for O(1) lookup.
/// - `order` is the z-stack from bottom (index 0) to top; the topmost enabled
///   video track wins when compositing. The UI renders it top-first, so index 0
///   shows at the *bottom* of the lane list.
/// - **Lane-zone invariant (CapCut):** `order` is always partitioned into
///   zones, bottom to top: every bus lane ([`TrackKind::Submix`]), then every
///   [`TrackKind::Audio`] lane, then the main video track ([`Track::main`]),
///   then every other visual lane (overlay video / sticker / effect / filter /
///   adjustment), then every [`TrackKind::Text`] lane. So the mix floor is at
///   the bottom, text on top, and nothing but audio ever sits below the main
///   track. Maintained by every add/insert/move/restore path.
/// - **Main-track invariant:** at most one track is flagged [`Track::main`],
///   it is always a video lane, and one exists whenever any video lane does
///   (the first video track added is designated automatically; removing the
///   main lane promotes the next bottom-most video lane).
/// - `clip_index` maps every [`ClipId`] to the track containing it, so a clip
///   can be found across the whole timeline in O(1) without scanning tracks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    /// Editing/playback frame rate. Clip `timeline` ranges are in these frames.
    pub frame_rate: Rational,
    #[serde(with = "crate::serde_map")]
    tracks: Map<TrackId, Track>,
    order: Vec<TrackId>,
    #[serde(with = "crate::serde_map")]
    clip_index: Map<ClipId, TrackId>,
    /// Ruler markers in `(tick, id)` order. Optional + defaulted so pre-marker
    /// project files load unchanged and marker-free saves stay byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    markers: Vec<Marker>,
    /// Canvas settings (M1): aspect preset + background color. Optional +
    /// defaulted so pre-canvas project files load unchanged and default
    /// saves stay byte-identical.
    #[serde(default, skip_serializing_if = "CanvasSettings::is_default")]
    canvas: CanvasSettings,
    /// Audio channel layout for the sequence (mono / stereo / 5.1). Optional +
    /// defaulted so pre-audio-layout project files load unchanged and default
    /// saves stay byte-identical.
    #[serde(default, skip_serializing_if = "AudioLayout::is_default")]
    audio_layout: AudioLayout,
}

impl Timeline {
    pub fn new(frame_rate: Rational) -> Self {
        Self {
            frame_rate,
            tracks: Map::default(),
            order: Vec::new(),
            clip_index: Map::default(),
            markers: Vec::new(),
            canvas: CanvasSettings::default(),
            audio_layout: AudioLayout::default(),
        }
    }

    // --- canvas -------------------------------------------------------------

    /// Canvas settings: aspect preset, background color, explicit size.
    pub fn canvas(&self) -> CanvasSettings {
        self.canvas
    }

    pub fn set_canvas(&mut self, settings: CanvasSettings) {
        self.canvas = settings;
    }

    // --- audio layout -------------------------------------------------------

    /// Audio channel layout for the sequence (mono / stereo / 5.1).
    pub fn audio_layout(&self) -> AudioLayout {
        self.audio_layout
    }

    pub fn set_audio_layout(&mut self, layout: AudioLayout) {
        self.audio_layout = layout;
    }

    // --- frame rate & resampling --------------------------------------------

    /// Change the sequence frame rate, resampling all clip timeline placements,
    /// fade envelopes, transitions, marker positions, and clip-relative keyframe
    /// parameters so every timeline tick is expressed at `new_rate`.
    ///
    /// Refuses invalid rates (`num <= 0` or `den <= 0`).
    pub fn resample_frame_rate(&mut self, new_rate: Rational) -> Result<(), ModelError> {
        if !new_rate.is_valid() {
            return Err(ModelError::InvalidParam(format!(
                "frame rate must have positive numerator and denominator (got {}/{})",
                new_rate.num, new_rate.den
            )));
        }
        let old_rate = self.frame_rate;
        if crate::time::rate_eq(old_rate, new_rate) {
            self.frame_rate = new_rate;
            return Ok(());
        }

        // 1. Resample markers
        for marker in &mut self.markers {
            marker.tick = resample(marker.tick, new_rate);
        }

        // 2. Resample tracks (clips, transitions)
        for track in self.tracks.values_mut() {
            for transition in track.transitions_mut() {
                transition.duration =
                    resample(RationalTime::new(transition.duration, old_rate), new_rate)
                        .value
                        .max(1);
            }
            track.for_each_clip_mut(|clip| {
                let new_start = resample(clip.timeline.start, new_rate).value;
                // Through the end, not the duration: resampling both ends and
                // subtracting keeps a clip abutting the one after it, where two
                // independently rounded lengths drift apart by a tick.
                let old_end = RationalTime::new(clip.timeline.end_tick(), old_rate);
                let new_end = resample(old_end, new_rate).value;
                let new_dur = (new_end - new_start).max(1);
                clip.timeline = crate::time::TimeRange::at_rate(new_start, new_dur, new_rate);

                if clip.fade_in > 0 {
                    clip.fade_in = resample(RationalTime::new(clip.fade_in, old_rate), new_rate)
                        .value
                        .max(0)
                        .min(new_dur);
                }
                if clip.fade_out > 0 {
                    clip.fade_out = resample(RationalTime::new(clip.fade_out, old_rate), new_rate)
                        .value
                        .max(0)
                        .min(new_dur);
                }

                // 3. Resample the clip's own keyframe ticks. Without this a
                // keyframe stays on the old grid: tick 24 means one second at
                // 24 fps and four fifths of one at 30, so every animation on
                // the sequence slides when the rate changes.
                clip.resample_timeline_params(old_rate, new_rate);
            });
        }

        self.frame_rate = new_rate;
        Ok(())
    }

    // --- tracks -----------------------------------------------------------

    /// Append a track to the top of the stack. Returns its [`TrackId`].
    ///
    /// The lane-zone invariant (see [`Timeline`]) is re-applied afterwards,
    /// so an audio lane sinks to the audio floor, a text lane rises to the
    /// top, and the first video lane ever added becomes the main track and
    /// lands directly above the audio block.
    pub fn add_track(&mut self, track: Track) -> TrackId {
        let id = track.id;
        self.tracks.insert(id, track);
        self.order.push(id);
        self.designate_main();
        self.enforce_lane_zones();
        id
    }

    /// Insert a track at `order_index` in the stack (0 = bottom layer),
    /// clamped to the current stack height. Returns its [`TrackId`].
    ///
    /// The lane-zone invariant (see [`Timeline`]) is re-applied afterwards:
    /// the requested index only decides the lane's position *within its
    /// zone* — a visual track requested below the audio block or the main
    /// track is lifted just above them, a text lane rises above the overlay
    /// block, and an audio track requested above visual lanes sinks back
    /// down.
    pub fn insert_track(&mut self, track: Track, order_index: usize) -> TrackId {
        let id = track.id;
        self.tracks.insert(id, track);
        let idx = order_index.min(self.order.len());
        self.order.insert(idx, id);
        self.designate_main();
        self.enforce_lane_zones();
        id
    }

    /// The main track (CapCut's magnetic lane): the single video lane flagged
    /// [`Track::main`]. `None` until a video track exists.
    pub fn main_track(&self) -> Option<TrackId> {
        self.order
            .iter()
            .copied()
            .find(|id| self.tracks.get(id).is_some_and(|t| t.main))
    }

    /// Re-derive the lane-zone and main-track invariants from scratch —
    /// the chokepoint for projects loaded from disk (files written before
    /// the main-track flag existed, or edited externally).
    pub fn normalize_lanes(&mut self) {
        self.designate_main();
        self.enforce_lane_zones();
    }

    /// Keep the main-track invariant: clear the flag from non-video lanes,
    /// keep only the bottom-most flagged video lane when several claim it,
    /// and designate the bottom-most video lane when none does.
    fn designate_main(&mut self) {
        let mut seen_main = false;
        let mut first_video: Option<TrackId> = None;
        for id in self.order.clone() {
            let Some(track) = self.tracks.get_mut(&id) else {
                continue;
            };
            if !track.kind.can_be_main() {
                track.main = false;
                continue;
            }
            if first_video.is_none() {
                first_video = Some(id);
            }
            if track.main {
                if seen_main {
                    track.main = false;
                } else {
                    seen_main = true;
                }
            }
        }
        if !seen_main && let Some(id) = first_video {
            self.tracks.get_mut(&id).expect("track exists").main = true;
        }
    }

    /// Stable-partition `order` into the four CapCut zones (bottom to top):
    /// audio, the main video track, other visual lanes, text. Relative order
    /// *within* each zone is preserved, so the only movement is lanes sinking
    /// or rising to their zone boundary.
    ///
    /// This is the single chokepoint that guarantees the lane-zone invariant
    /// for every add/insert/move/restore (and therefore for AI commands,
    /// drag-drop, and undo/redo). O(n log n) on the track count — a cold,
    /// per-track-edit path.
    fn enforce_lane_zones(&mut self) {
        // `sort_by_cached_key` reads each track's zone once, so neither a rank
        // map nor a hash lookup per comparison is needed, and the sort stays
        // stable.
        let tracks = &self.tracks;
        self.order.sort_by_cached_key(|id| match tracks.get(id) {
            Some(t) => t.kind.spec().zone_rank(t.main),
            None => LaneZone::Stack.rank(),
        });
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.get(&id)
    }

    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.get_mut(&id)
    }

    /// Track IDs from bottom to top of the stack.
    pub fn order(&self) -> &[TrackId] {
        &self.order
    }

    /// Tracks in stacking order (bottom to top).
    pub fn tracks_ordered(&self) -> impl Iterator<Item = &Track> {
        self.order.iter().filter_map(move |id| self.tracks.get(id))
    }

    /// Whether any lane of `kind` is soloed. While one is, the others of that kind
    /// do not contribute.
    pub fn has_solo(&self, kind: TrackKind) -> bool {
        self.tracks_ordered()
            .any(|track| track.kind == kind && track.solo)
    }

    /// Whether a lane contributes to audio: not muted, and not shadowed by a solo
    /// on another audio lane. The single owner of the solo rule, so the mixer, the
    /// exporter, and the UI cannot disagree about it.
    pub fn track_is_audible(&self, track: &Track) -> bool {
        !track.muted && (track.solo || !self.has_solo(track.kind))
    }

    /// Whether a visual lane contributes to the composite: enabled, and not
    /// shadowed by a solo on another visual lane of the same kind.
    pub fn track_is_visible(&self, track: &Track) -> bool {
        track.enabled && (track.solo || !self.has_solo(track.kind))
    }

    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Remove a track and all its clips (also purging the clip index).
    ///
    /// Removing the main track promotes the next bottom-most video lane.
    /// Removing a bus lane takes the routes that pointed at it off the lanes
    /// that held them, so nothing is left addressing a lane that is gone; the
    /// returned [`RemovedTrack`] carries them, which is how an undo puts the
    /// mix back the way it was.
    pub fn remove_track(&mut self, id: TrackId) -> Option<RemovedTrack> {
        let detached = if self.track(id).is_some_and(|lane| lane.kind.is_bus()) {
            self.detach_routes_to(id)
        } else {
            Vec::new()
        };
        let track = self.tracks.remove(&id)?;
        self.order.retain(|t| *t != id);
        for clip in track.clips() {
            self.clip_index.remove(&clip.id);
        }
        if track.main {
            self.designate_main();
            self.enforce_lane_zones();
        }
        Some(RemovedTrack { track, detached })
    }

    /// Reorder a track within the stack. The lane-zone invariant is
    /// re-applied afterwards, so the move only takes effect within the
    /// track's zone — in particular, the main track never moves, and no
    /// visual lane can cross below it or below the audio floor.
    pub fn move_track(&mut self, id: TrackId, new_index: usize) -> Result<(), ModelError> {
        let current = self
            .order
            .iter()
            .position(|&t| t == id)
            .ok_or(ModelError::UnknownTrack(id))?;
        if current == new_index {
            return Ok(());
        }
        self.order.remove(current);
        let idx = new_index.min(self.order.len());
        self.order.insert(idx, id);
        self.enforce_lane_zones();
        Ok(())
    }

    /// Re-insert a removed track at its prior stack position (undo of
    /// [`remove_track`](Timeline::remove_track)).
    ///
    /// `removed` is what that call returned, so the routes a bus removal took
    /// off other lanes come back with it.
    pub fn restore_track(
        &mut self,
        removed: RemovedTrack,
        order_index: usize,
    ) -> Result<TrackId, ModelError> {
        let RemovedTrack { track, detached } = removed;
        let id = track.id;
        if self.tracks.contains_key(&id) {
            return Err(ModelError::InvalidRange);
        }
        for clip in track.clips() {
            if self.clip_index.contains_key(&clip.id) {
                return Err(ModelError::InvalidRange);
            }
        }
        let clip_ids: Vec<ClipId> = track.clips().map(|c| c.id).collect();
        let idx = order_index.min(self.order.len());
        self.order.insert(idx, id);
        self.tracks.insert(id, track);
        // A restored main lane may collide with an interim promotion (undo of
        // remove-main); `designate_main` keeps the bottom-most claim and
        // clears the other, so the invariant holds either way.
        self.designate_main();
        self.enforce_lane_zones();
        for clip_id in clip_ids {
            self.clip_index.insert(clip_id, id);
        }
        self.restore_routes(detached);
        Ok(id)
    }

    // --- clips ------------------------------------------------------------

    /// Place `clip` on `track_id`, rejecting unknown tracks, duplicate ids,
    /// and overlaps.
    pub fn add_clip(&mut self, track_id: TrackId, clip: Clip) -> Result<ClipId, ModelError> {
        // Defense in depth against id collisions: a plain map insert would
        // silently overwrite an existing clip (and its index entry), which is
        // how a cross-session id clash used to destroy a clip. Reject instead.
        if self.clip_index.contains_key(&clip.id) {
            return Err(ModelError::DuplicateClip(clip.id));
        }
        let track = self
            .tracks
            .get(&track_id)
            .ok_or(ModelError::UnknownTrack(track_id))?;
        if !track.kind.accepts_clip(&clip) {
            return Err(ModelError::IncompatibleTrackKind {
                track: track_id,
                kind: track.kind,
            });
        }

        let track = self.tracks.get_mut(&track_id).expect("track exists");

        if track.has_overlap(clip.timeline, None)? {
            return Err(ModelError::Overlap(track_id));
        }

        let clip_id = clip.id;
        track.insert_clip(clip);
        self.clip_index.insert(clip_id, track_id);
        Ok(clip_id)
    }

    /// Remove a clip by ID from wherever it lives.
    pub fn remove_clip(&mut self, clip_id: ClipId) -> Option<Clip> {
        let track_id = self.clip_index.remove(&clip_id)?;
        self.tracks.get_mut(&track_id)?.remove_clip(clip_id)
    }
    /// Replace an existing clip in place, preserving its id and lane.
    ///
    /// The replacement is validated against the lane and neighboring clips,
    /// then the maintained order and transition set are refreshed atomically.
    pub fn replace_clip(&mut self, clip: Clip) -> Result<Clip, ModelError> {
        let track_id = *self
            .clip_index
            .get(&clip.id)
            .ok_or(ModelError::UnknownClip(clip.id))?;
        let track = self
            .tracks
            .get_mut(&track_id)
            .ok_or(ModelError::UnknownTrack(track_id))?;
        if !track.kind.accepts_clip(&clip) {
            return Err(ModelError::IncompatibleTrackKind {
                track: track_id,
                kind: track.kind,
            });
        }
        if track.has_overlap(clip.timeline, Some(clip.id))? {
            return Err(ModelError::Overlap(track_id));
        }
        let old = track
            .insert_clip(clip)
            .expect("the clip index and track storage agree");
        track.prune_dead_transitions();
        Ok(old)
    }

    /// Find a clip by ID across all tracks in O(1).
    pub fn clip(&self, clip_id: ClipId) -> Option<&Clip> {
        let track_id = *self.clip_index.get(&clip_id)?;
        self.tracks.get(&track_id)?.clip(clip_id)
    }

    pub fn clip_mut(&mut self, clip_id: ClipId) -> Option<&mut Clip> {
        let track_id = *self.clip_index.get(&clip_id)?;
        self.tracks.get_mut(&track_id)?.clip_mut(clip_id)
    }

    pub fn clip_timeline_mut(&mut self, clip_id: ClipId) -> Option<crate::ClipMut<'_>> {
        let track_id = *self.clip_index.get(&clip_id)?;
        self.tracks.get_mut(&track_id)?.clip_timeline_mut(clip_id)
    }

    /// The track that contains `clip_id`, if any.
    pub fn track_of(&self, clip_id: ClipId) -> Option<TrackId> {
        self.clip_index.get(&clip_id).copied()
    }

    /// Whether `clip_id`'s media audio is heard from its *own* lane.
    ///
    /// CapCut keeps a video's sound on the video clip itself; a dropped clip
    /// is audible without a second lane. `true` for audio-lane media clips and
    /// for video-lane media clips, with two exceptions: freeze-frame clips are
    /// semantically silent, and a video clip whose sound has been *detached*
    /// to a linked audio-lane partner (CapCut "separate audio") goes silent on
    /// its own lane — the partner carries it instead. `false` for lanes that
    /// never carry media audio (text/effect/etc.).
    ///
    /// Callers still layer mute / silence / `has_audio` on top; this answers
    /// only "does this clip's media audio belong to *this* lane?". The detach
    /// scan only runs for *linked* video clips, so undetached drops (the common
    /// case, `link == None`) short-circuit.
    pub fn carries_own_audio(&self, clip_id: ClipId) -> bool {
        if self.clip(clip_id).is_none_or(|clip| clip.freeze_frame) {
            return false;
        }
        let Some(track) = self.track_of(clip_id).and_then(|t| self.track(t)) else {
            return false;
        };
        if !track.kind.carries_audio() {
            return false;
        }
        track.kind.is_audio() || !self.detached_to_audio_lane(clip_id)
    }

    /// Whether `clip_id` shares a link group with a clip on an audio lane — the
    /// CapCut "separate audio" companion that took over its sound.
    pub fn detached_to_audio_lane(&self, clip_id: ClipId) -> bool {
        let Some(link) = self.clip(clip_id).and_then(|c| c.link) else {
            return false;
        };
        self.tracks_ordered()
            .any(|t| t.kind.is_audio() && t.clips().any(|c| c.link == Some(link)))
    }

    pub fn clip_count(&self) -> usize {
        self.clip_index.len()
    }

    // --- markers ------------------------------------------------------------

    /// Ruler markers in `(tick, id)` order.
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    pub fn marker(&self, id: MarkerId) -> Option<&Marker> {
        self.markers.iter().find(|m| m.id == id)
    }

    pub fn marker_count(&self) -> usize {
        self.markers.len()
    }

    /// Insert a marker, keeping `(tick, id)` order. The tick is resampled to
    /// the timeline rate so every stored marker shares it. Rejects negative
    /// positions and duplicate ids (undo restores must not double-insert).
    pub fn add_marker(&mut self, mut marker: Marker) -> Result<MarkerId, ModelError> {
        if marker.tick.value < 0 || !marker.tick.rate.is_valid() {
            return Err(ModelError::InvalidRange);
        }
        if self.marker(marker.id).is_some() {
            return Err(ModelError::InvalidRange);
        }
        marker.tick = resample(marker.tick, self.frame_rate);
        let id = marker.id;
        let at = self
            .markers
            .partition_point(|m| (m.tick.value, m.id) <= (marker.tick.value, marker.id));
        self.markers.insert(at, marker);
        Ok(id)
    }

    /// Remove a marker by id, returning it for undo capture.
    pub fn remove_marker(&mut self, id: MarkerId) -> Option<Marker> {
        let index = self.markers.iter().position(|m| m.id == id)?;
        Some(self.markers.remove(index))
    }

    /// Move / rename / recolor / re-annotate a marker in one shot (the
    /// `SetMarker` command and its undo both funnel through here). Re-sorts on
    /// tick change; rejects unknown ids and negative positions.
    pub fn set_marker(
        &mut self,
        id: MarkerId,
        tick: RationalTime,
        name: String,
        color: MarkerColor,
        comment: String,
    ) -> Result<(), ModelError> {
        if tick.value < 0 || !tick.rate.is_valid() {
            return Err(ModelError::InvalidRange);
        }
        let resampled_tick = resample(tick, self.frame_rate);
        if let Some(m) = self.markers.iter_mut().find(|m| m.id == id) {
            if m.tick == resampled_tick {
                m.name = name;
                m.color = color;
                m.comment = comment;
                return Ok(());
            }
        }
        let mut marker = self
            .remove_marker(id)
            .ok_or(ModelError::UnknownMarker(id))?;
        let before = marker.clone();
        marker.tick = tick;
        marker.name = name;
        marker.color = color;
        marker.comment = comment;
        match self.add_marker(marker) {
            Ok(_) => Ok(()),
            Err(e) => {
                // Validation failed: put the original back untouched.
                let restored = self.add_marker(before);
                debug_assert!(restored.is_ok(), "re-inserting the removed marker");
                Err(e)
            }
        }
    }

    /// Total timeline length: the end of the last-ending clip at [`frame_rate`](Self::frame_rate).
    pub fn duration(&self) -> RationalTime {
        let tick = self
            .tracks
            .values()
            .map(Track::content_end)
            .max()
            .unwrap_or(0);
        RationalTime::new(tick, self.frame_rate)
    }
}

#[cfg(test)]
mod tests;
