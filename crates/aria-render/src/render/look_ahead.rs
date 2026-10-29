//! Decoders for the media on screen and the media about to come on screen.
//!
//! The renderer keeps at most [`MAX_DECODERS`] [`AheadDecoder`]s, keyed by the
//! media and the clip that shows it ([`DecoderKey`]), so two clips of one file
//! at different source times keep independent cursors. When multiple layers in
//! one scene request the same media at the same source time, the first layer's
//! decoded CPU frame serves the remaining layers without opening another
//! decoder or decoding again.
//!
//! After each timeline frame, [`Renderer::look_ahead`] reads the step from the
//! previous timeline frame. At a playback step (forward or reverse, up to
//! [`MAX_STEP`] frames) it requests each distinct `(media, source_time)` the
//! next step shows, and opens and decodes on the decode pool the first frame of
//! each distinct `(media, source_time)` entering in the next
//! [`LOOK_AHEAD_SECONDS`], up to [`MAX_DECODERS`] total decoders. A decoder
//! unused for [`RELEASE_AFTER`] scenes drops its held frames; past
//! [`MAX_DECODERS`], the least recently used close.
//!
//! A CPU frame returns to its slot after the composite that drew it
//! ([`Renderer::keep_shown`]). The next request with the same source time
//! takes it back with no decode: a paused preview redrawn for a parameter
//! change, a gesture, or a selection change shows its media frame again
//! instead of seeking and walking the GOP to it. A request for any other time
//! drops it, returning its planes to the decoder plane pool
//! ([`aria_decoder::recycle_frame_planes`]). A GPU surface does not return:
//! it belongs to the decoder's surface pool.

use std::path::PathBuf;

use aria_core::{FrameData, RationalTime, VideoDecoder, VideoFrame};
use aria_decoder::{AheadDecoder, OpeningDecoder, open_ahead, open_video_decoder_at};
use aria_models::{ClipId, MediaId, Project, Timeline};

use crate::error::RenderError;
use crate::resolve::ResolveOverrides;
use crate::scene::{LayerSource, SceneLayer};

use super::{Renderer, SeekPolicy};

/// How far ahead of the playhead clips coming on screen are opened and
/// decoded, in seconds.
pub(super) const LOOK_AHEAD_SECONDS: i64 = 2;

/// The largest step between two timeline frames read as playback: the 8x
/// shuttle. A larger step is a jump, and nothing is looked ahead at.
pub(super) const MAX_STEP: i64 = 8;

/// Clip entrances looked ahead at per frame, nearest first.
const MAX_ENTRANCES: usize = 8;

/// Scenes a decoder may go unused before it drops its held frames. A gesture
/// frame renders three scenes, so a smaller count would drop the frames of a
/// clip between two of its own gesture frames.
pub(super) const RELEASE_AFTER: u64 = 4;

/// Maximum open or opening decoders held by [`Renderer`] at one time.
/// Idle decoders (`RELEASE_AFTER` scenes unused) close first; when active or
/// speculative decoders alone exceed the cap, the least recently used close.
pub(super) const MAX_DECODERS: usize = 16;

/// One use of a media file on screen: the media and the clip showing it.
/// A media layer with no clip (a pool source preview) has its own decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct DecoderKey {
    pub(super) media: MediaId,
    pub(super) clip: Option<ClipId>,
}

impl DecoderKey {
    pub(super) fn of(media: MediaId, layer: &SceneLayer) -> Self {
        Self {
            media,
            clip: layer.clip,
        }
    }
}

/// A decoded frame and the request that produced it.
pub(super) struct Shown {
    pub(super) key: DecoderKey,
    pub(super) source_time: RationalTime,
    /// The frame is the one at `source_time`, rather than the sync frame
    /// before it that a [`SeekPolicy::NearestSync`] request snapped to.
    pub(super) exact: bool,
    pub(super) frame: VideoFrame,
}

impl Shown {
    /// Whether this frame answers a request for `source_time` under `policy`:
    /// the frame at it answers either policy, a snapped one only a snap.
    fn serves(&self, source_time: RationalTime, policy: SeekPolicy) -> bool {
        self.source_time == source_time && (self.exact || policy == SeekPolicy::NearestSync)
    }
}

impl Drop for Shown {
    fn drop(&mut self) {
        aria_decoder::recycle_frame_planes(&mut self.frame);
    }
}

fn clone_cpu_frame(frame: &VideoFrame) -> Option<VideoFrame> {
    let FrameData::Cpu(image) = &frame.data else {
        return None;
    };
    let cloned_planes = image
        .planes
        .iter()
        .map(|plane| aria_core::Plane {
            data: aria_decoder::copy_plane_bytes(&plane.data),
            stride: plane.stride,
            rows: plane.rows,
        })
        .collect();
    let mut out = VideoFrame::new(
        frame.pts,
        frame.format,
        frame.color,
        frame.coded_size,
        frame.visible,
        frame.rotation,
        FrameData::Cpu(aria_core::CpuImage::new(cloned_planes)),
    );
    out.duration = frame.duration;
    Some(out)
}

fn clone_shown(shown: &Shown) -> Option<Shown> {
    Some(Shown {
        key: shown.key,
        source_time: shown.source_time,
        exact: shown.exact,
        frame: clone_cpu_frame(&shown.frame)?,
    })
}

/// Append `(key, time)` to `uses` unless the slice `uses[scene_start..]`
/// already contains a request for `key.media` at `time`.
fn push_unique_media_time(
    uses: &mut Vec<(DecoderKey, RationalTime)>,
    scene_start: usize,
    key: DecoderKey,
    time: RationalTime,
) {
    if uses[scene_start..]
        .iter()
        .any(|&(existing, existing_time)| existing.media == key.media && existing_time == time)
    {
        return;
    }
    uses.push((key, time));
}

/// An open decoder, or one opening on the decode pool, and the last scene that
/// used it.
pub(super) struct DecoderSlot {
    decoder: Option<AheadDecoder>,
    /// An open in progress, its first frame requested. Set only while
    /// `decoder` is `None`.
    opening: Option<OpeningDecoder>,
    /// The scene stamp of the last frame that used it or looked ahead at it.
    used: u64,
    /// Its held frames were dropped after it went unused.
    released: bool,
    /// The CPU frame the last composite drew from this decoder.
    shown: Option<Shown>,
}

impl DecoderSlot {
    fn new(decoder: Option<AheadDecoder>, opening: Option<OpeningDecoder>, used: u64) -> Self {
        Self {
            decoder,
            opening,
            used,
            released: false,
            shown: None,
        }
    }

    /// The decoder, waiting for an open in progress to finish.
    fn ready(&mut self) -> Result<&mut AheadDecoder, RenderError> {
        if let Some(opening) = self.opening.take() {
            self.decoder = Some(opening.wait()?);
        }
        self.decoder.as_mut().ok_or_else(|| {
            RenderError::Decode(aria_core::DecodeError::Open(
                "the decoder slot holds no decoder. Reopen the project.".into(),
            ))
        })
    }
}

/// Call `each` for every media use in `layers`, transition sides, matte
/// layers, and the layers of a nest's scene included.
pub(super) fn media_uses(layers: &[SceneLayer], each: &mut impl FnMut(DecoderKey, RationalTime)) {
    for layer in layers {
        match &layer.source {
            LayerSource::Media { media, source_time } => {
                each(DecoderKey::of(*media, layer), *source_time);
            }
            LayerSource::Transition {
                outgoing, incoming, ..
            } => {
                media_uses(std::slice::from_ref(&**outgoing), each);
                media_uses(std::slice::from_ref(&**incoming), each);
            }
            LayerSource::Matte { layers, .. } => media_uses(&layers[..], each),
            LayerSource::Nest { scene, .. } => {
                media_uses(&scene.references, each);
                media_uses(&scene.layers, each);
            }
            LayerSource::Still { .. }
            | LayerSource::Sticker { .. }
            | LayerSource::Lottie { .. }
            | LayerSource::Text { .. }
            | LayerSource::Solid(_)
            | LayerSource::CanvasPass
            | LayerSource::Shape { .. }
            | LayerSource::PathShape { .. } => {}
        }
    }
}

/// Ticks in `timeline.frame_rate` covering [`LOOK_AHEAD_SECONDS`].
fn window_ticks(timeline: &Timeline) -> i64 {
    let rate = timeline.frame_rate;
    if !rate.is_valid() {
        return 0;
    }
    let ticks = i128::from(LOOK_AHEAD_SECONDS) * i128::from(rate.num) / i128::from(rate.den);
    i64::try_from(ticks).unwrap_or(i64::MAX)
}

/// The ticks within the look-ahead window from `tick` in the direction of
/// `step` at which a clip comes on screen, nearest first: clip starts and
/// transition starts going forward, their last frames going backward.
pub(super) fn entrances(timeline: &Timeline, tick: i64, step: i64, into: &mut Vec<i64>) {
    into.clear();
    let window = window_ticks(timeline);
    let forward = step > 0;
    let mut consider = |at: i64| {
        let ahead = if forward { at - tick } else { tick - at };
        if ahead > 0 && ahead <= window {
            into.push(at);
        }
    };
    for track in timeline.tracks_ordered() {
        if !track.kind.is_visual() || !timeline.track_is_visible(track) {
            continue;
        }
        for clip in track.clips() {
            if forward {
                consider(clip.timeline.start.value);
            } else {
                consider(clip.timeline.end_tick() - 1);
            }
        }
        for transition in track.transitions() {
            let Some(left) = track.clip(transition.left) else {
                continue;
            };
            let cut = left.timeline.end_tick();
            let (start, end) = transition.window(cut);
            if forward {
                consider(start);
            } else {
                consider(end - 1);
            }
        }
    }
    into.sort_unstable_by_key(|at| (at - tick).abs());
    into.dedup();
    into.truncate(MAX_ENTRANCES);
}

impl Renderer {
    /// Check on the stack whether `scene` draws more than one video layer of
    /// the same media at the same source time. Single-track and distinct-media
    /// scenes set `scene_shares_frames = false` with zero heap allocations.
    pub(super) fn note_scene_media(&mut self, scene: &crate::scene::Scene) {
        const INLINE_CAP: usize = 16;
        let mut seen: [Option<(DecoderKey, RationalTime)>; INLINE_CAP] = [None; INLINE_CAP];
        let mut len = 0usize;
        let mut shared = false;
        let mut visit = |key: DecoderKey, time: RationalTime| {
            if shared {
                return;
            }
            for &(prev_key, prev_time) in seen[..len].iter().flatten() {
                if prev_key.media == key.media && prev_time == time && prev_key != key {
                    shared = true;
                    return;
                }
            }
            if len < INLINE_CAP {
                seen[len] = Some((key, time));
                len += 1;
            } else {
                shared = true;
            }
        };
        media_uses(&scene.references, &mut visit);
        media_uses(&scene.layers, &mut visit);
        self.shared_check_stamp = self.scene_stamp;
        self.scene_shares_frames = shared;
    }

    /// Decode the frame of `key.media` at `source_time` for the clip in `key`,
    /// opening (and keeping) its decoder on first sight, over the media's
    /// proxy file when one is registered (see [`set_proxy`](Self::set_proxy)).
    /// Under [`SeekPolicy::NearestSync`] the frame may be the sync point before
    /// `source_time` rather than the exact frame (see [`SeekPolicy`]). The
    /// frame the last composite drew for `key` answers with no decode when it
    /// [`serves`](Shown::serves) the request.
    pub(super) fn decode(
        &mut self,
        project: &Project,
        key: DecoderKey,
        source_time: RationalTime,
        policy: SeekPolicy,
    ) -> Result<Shown, RenderError> {
        let stamp = self.scene_stamp;
        let share_in_scene = self.shared_check_stamp != stamp || self.scene_shares_frames;
        if let Some(slot) = self.decoders.get_mut(&key) {
            slot.used = stamp;
            slot.released = false;
            if let Some(shown) = slot.shown.take()
                && shown.serves(source_time, policy)
            {
                if share_in_scene {
                    slot.shown = clone_shown(&shown);
                }
                return Ok(shown);
            }
        }
        if share_in_scene {
            for slot in self.decoders.values_mut() {
                let matches = slot
                    .shown
                    .as_ref()
                    .is_some_and(|s| s.key.media == key.media && s.serves(source_time, policy));
                if matches && let Some(shown) = slot.shown.as_ref().and_then(clone_shown) {
                    slot.used = stamp;
                    slot.released = false;
                    return Ok(shown);
                }
            }
        }
        if !self.decoders.contains_key(&key) {
            let path = self.decoder_path(project, key.media)?;
            let decoder =
                AheadDecoder::new(open_video_decoder_at(&path, self.decode_mode, source_time)?);
            if self.decoders.len() >= MAX_DECODERS {
                self.evict_decoders_to(MAX_DECODERS - 1, true);
            }
            self.decoders
                .insert(key, DecoderSlot::new(Some(decoder), None, stamp));
        }
        let Some(slot) = self.decoders.get_mut(&key) else {
            return Err(RenderError::MissingMedia(key.media));
        };
        slot.used = stamp;
        slot.released = false;
        let opened = slot.ready();
        let decoder = match opened {
            Ok(decoder) => decoder,
            Err(error) => {
                // A failed open is retried by the next frame that needs it.
                self.decoders.remove(&key);
                return Err(error);
            }
        };
        let mut frame = match policy {
            SeekPolicy::Exact => decoder.frame_at(source_time)?,
            SeekPolicy::NearestSync => decoder.frame_at_nearest(source_time)?,
        };
        // An exact target at the media's very end can overshoot EOF: the
        // pool trusts the container's frame count, which routinely
        // over-reports by one, and proxies additionally run a frame short
        // by construction (generation drops that untrustworthy tail). Show
        // the nearest decodable frame instead of failing the whole
        // composite: transitions pin the outgoing side at the final
        // source frame, so a miss here would error every frame of the
        // window.
        if frame.is_none() {
            frame = decoder.frame_at_nearest(source_time)?;
        }
        let frame = frame.ok_or(RenderError::NoFrame {
            media: key.media,
            time: source_time,
        })?;
        // A snap that rolled forward, or whose sync frame is the target, drew
        // the frame at the target: it answers an exact request too.
        let exact = policy == SeekPolicy::Exact
            || aria_decoder::frame_covers(frame.pts, source_time, decoder.info().frame_rate);
        let shown = Shown {
            key,
            source_time,
            exact,
            frame,
        };
        if share_in_scene
            && let Some(cloned) = clone_shown(&shown)
            && let Some(slot) = self.decoders.get_mut(&key)
        {
            slot.shown = Some(cloned);
        }
        Ok(shown)
    }

    /// Return `shown` to its decoder slot after the composite that drew it, so
    /// a repeat request takes it back. A GPU surface, or a frame whose decoder
    /// closed since, is dropped.
    pub(super) fn keep_shown(&mut self, shown: Shown) {
        if !matches!(shown.frame.data, FrameData::Cpu(_)) {
            return;
        }
        if let Some(slot) = self.decoders.get_mut(&shown.key) {
            slot.shown = Some(shown);
        }
    }

    /// The file decoders for `media` read: its proxy while proxies are in use
    /// and one is registered and on disk, else the project's path. Checked once
    /// per decoder open.
    fn decoder_path(&self, project: &Project, media: MediaId) -> Result<PathBuf, RenderError> {
        if self.use_proxies
            && let Some(proxy) = self.proxies.get(&media)
            && proxy.is_file()
        {
            return Ok(proxy.clone());
        }
        let source = project
            .media(media)
            .ok_or(RenderError::MissingMedia(media))?;
        Ok(source.path().to_path_buf())
    }

    /// Ask the decoder for `key` for its frame at `source_time` on the decode
    /// pool, opening it there first when none is open.
    fn prepare(&mut self, project: &Project, key: DecoderKey, source_time: RationalTime) {
        let stamp = self.scene_stamp;
        if let Some(slot) = self.decoders.get_mut(&key) {
            slot.used = stamp;
            slot.released = false;
            if let Some(decoder) = &slot.decoder {
                decoder.request(source_time, false);
            }
            return;
        }
        if self.decoders.len() >= MAX_DECODERS {
            self.evict_decoders_to(MAX_DECODERS - 1, false);
            if self.decoders.len() >= MAX_DECODERS {
                return;
            }
        }
        let Ok(path) = self.decoder_path(project, key.media) else {
            return;
        };
        let mode = self.decode_mode;
        let opening = open_ahead(
            move || open_video_decoder_at(&path, mode, source_time),
            source_time,
            false,
        );
        self.decoders
            .insert(key, DecoderSlot::new(None, Some(opening), stamp));
    }

    /// Start decoding a forward run of timeline frames from `first`, `step`
    /// ticks apart, before `first` is realized: drop the decoders' shown
    /// frames after a backward jump or a rate change, set `last_tick` two
    /// steps before `first`, and run [`Self::look_ahead`] from one step before
    /// it, so `first`'s decoders open and decode on the pool.
    pub(crate) fn prime_look_ahead(&mut self, project: &Project, first: RationalTime, step: i64) {
        if step <= 0 || step > MAX_STEP {
            return;
        }
        if self
            .last_tick
            .is_some_and(|prev| prev.rate != first.rate || first.value < prev.value)
        {
            for slot in self.decoders.values_mut() {
                slot.shown = None;
                if let Some(decoder) = &slot.decoder {
                    decoder.release();
                }
            }
        }
        let two_back = first.value.saturating_sub(step.saturating_mul(2));
        let one_back = first.value.saturating_sub(step);
        self.last_tick = Some(RationalTime::new(two_back, first.rate));
        self.look_ahead(
            project,
            RationalTime::new(one_back, first.rate),
            SeekPolicy::Exact,
        );
    }

    /// Look ahead from the timeline frame at `tick`, just rendered under
    /// `policy`: at a playback step from the previous timeline frame, request
    /// the next step's frames and open the clips coming on screen within
    /// [`LOOK_AHEAD_SECONDS`]. A scrub ([`SeekPolicy::NearestSync`]) looks
    /// nowhere.
    pub(super) fn look_ahead(&mut self, project: &Project, tick: RationalTime, policy: SeekPolicy) {
        let Some(previous) = self.last_tick.replace(tick) else {
            return;
        };
        if policy != SeekPolicy::Exact || previous.rate != tick.rate {
            return;
        }
        let step = tick.value.saturating_sub(previous.value);
        if step == 0 || step.abs() > MAX_STEP {
            return;
        }
        let mut uses = std::mem::take(&mut self.look_uses);
        uses.clear();
        let mut look_scene = std::mem::replace(
            &mut self.look_ahead_scene,
            crate::scene::Scene::empty(0, 0, [0, 0, 0, 255]),
        );
        if crate::resolve::resolve_with_into(
            project,
            RationalTime::new(tick.value + step, tick.rate),
            ResolveOverrides::default(),
            &mut look_scene,
            &mut self.look_ahead_pool,
        )
        .is_ok()
        {
            media_uses(&look_scene.references, &mut |key, time| {
                push_unique_media_time(&mut uses, 0, key, time);
            });
            media_uses(&look_scene.layers, &mut |key, time| {
                push_unique_media_time(&mut uses, 0, key, time);
            });
        }
        let next_uses = uses.len();
        let mut ticks = std::mem::take(&mut self.look_ticks);
        entrances(project.timeline(), tick.value, step, &mut ticks);
        for &at in &ticks {
            if crate::resolve::resolve_with_into(
                project,
                RationalTime::new(at, tick.rate),
                ResolveOverrides::default(),
                &mut look_scene,
                &mut self.look_ahead_pool,
            )
            .is_err()
            {
                continue;
            }
            let entrance_start = uses.len();
            media_uses(&look_scene.references, &mut |key, time| {
                push_unique_media_time(&mut uses, entrance_start, key, time);
            });
            media_uses(&look_scene.layers, &mut |key, time| {
                push_unique_media_time(&mut uses, entrance_start, key, time);
            });
        }
        self.look_ahead_pool.recycle_scene(&mut look_scene);
        self.look_ahead_scene = look_scene;
        // The next frame's uses are always requested. An entrance is skipped
        // for a clip this frame or the next one already uses: it is on screen,
        // and its decoder's position belongs to that use.
        let stamp = self.scene_stamp;
        for (index, &(key, time)) in uses.iter().enumerate() {
            let in_use = self
                .decoders
                .get(&key)
                .is_some_and(|slot| slot.used == stamp);
            if index < next_uses || !in_use {
                self.prepare(project, key, time);
            }
        }
        self.look_ticks = ticks;
        self.look_uses = uses;
    }

    /// Drop the held frames of decoders no recent scene used, and close the
    /// least recently used past [`MAX_DECODERS`].
    pub(super) fn retire_decoders(&mut self) {
        let stamp = self.scene_stamp;
        for slot in self.decoders.values_mut() {
            if slot.used.saturating_add(RELEASE_AFTER) >= stamp {
                continue;
            }
            if !slot.released {
                slot.released = true;
                slot.shown = None;
                if let Some(decoder) = &slot.decoder {
                    decoder.release();
                }
            }
        }
        if self.decoders.len() > MAX_DECODERS {
            self.evict_decoders_to(MAX_DECODERS, true);
        }
    }

    /// Close decoders until at most `cap` remain. Released decoders close
    /// first (oldest `used` first), then decoders from earlier scenes, then
    /// speculative unopened slots when `allow_current_scene` holds.
    fn evict_decoders_to(&mut self, cap: usize, allow_current_scene: bool) {
        let stamp = self.scene_stamp;
        while self.decoders.len() > cap {
            let victim = self
                .decoders
                .iter()
                .filter(|(_, slot)| allow_current_scene || slot.used < stamp)
                .min_by_key(|(_, slot)| {
                    (
                        !slot.released,
                        slot.used == stamp,
                        slot.decoder.is_some(),
                        slot.used,
                    )
                })
                .map(|(key, _)| *key);
            let Some(key) = victim else {
                break;
            };
            self.decoders.remove(&key);
        }
    }

    /// Open decoders.
    #[cfg(test)]
    pub(super) fn decoder_keys(&self) -> impl Iterator<Item = &DecoderKey> {
        self.decoders.keys()
    }
}

#[cfg(test)]
#[path = "look_ahead_tests.rs"]
mod tests;
