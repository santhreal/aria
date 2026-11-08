use crate::clip::{Clip, ClipSource, CropRect, GeneratedRefusal, SlipRefusal, split_speed_curve};
use crate::error::ModelError;
use crate::ids::{ClipId, TrackId};
use crate::param::{Easing, Param};
use crate::time::{Rational, RationalTime, TimeRange, check_same_rate, resample};
use crate::timeline::{AudioLayout, TimeDisplay};

use super::Project;

impl Project {
    // --- editing primitives ----------------------------------------------

    pub fn remove_clip(&mut self, clip_id: ClipId) -> Option<Clip> {
        self.timeline_mut().remove_clip(clip_id)
    }

    /// Split the clip at timeline position `at` into two abutting clips.
    pub fn split_clip(&mut self, clip_id: ClipId, at: RationalTime) -> Result<ClipId, ModelError> {
        let clip = self
            .timeline()
            .clip(clip_id)
            .cloned()
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let tl = clip.timeline;
        let tl_rate = self.timeline().frame_rate;
        check_same_rate(at.rate, tl_rate)?;
        self.split_check(clip_id, at.value)?;

        let left_tl = TimeRange::at_rate(tl.start.value, at.value - tl.start.value, tl_rate);
        let right_tl = TimeRange::at_rate(at.value, tl.end_tick() - at.value, tl_rate);
        let split_fraction = left_tl.duration.value as f64 / tl.duration.value as f64;
        let (left_speed_curve, right_speed_curve) =
            split_speed_curve(&clip.speed_curve, split_fraction)?;

        let new_left_source = match clip.content.clone() {
            ClipSource::Media { media, source } => {
                let media_fps = self
                    .media
                    .get(&media)
                    .ok_or(ModelError::UnknownMedia(media))?
                    .frame_rate;
                if clip.freeze_frame {
                    let held = TimeRange::at_rate(source.start.value, 1, media_fps);
                    Some((held, held))
                } else {
                    Some(split_source_window(&clip, source, at, tl_rate, media_fps)?)
                }
            }
            ClipSource::Multicam { source, .. } => Some(split_source_window(
                &clip,
                source,
                at,
                tl_rate,
                source.start.rate,
            )?),
            ClipSource::Sequence { sequence, source } => {
                let seq_fps = self
                    .sequences
                    .get(&sequence)
                    .ok_or(ModelError::UnknownSequence(sequence))?
                    .timeline
                    .frame_rate;
                Some(split_source_window(&clip, source, at, tl_rate, seq_fps)?)
            }
            ClipSource::Generated(_) => None,
        };

        // Start from the original value so newly-added serde-default fields
        // automatically survive future splits. Only split identity, placement,
        // linkage, and clip-time-anchored fields are changed below.
        let mut new_clip = clip;
        new_clip.id = ClipId::next();
        new_clip.timeline = right_tl;
        // The tail starts unlinked: a link holds clips that move together, and
        // its partners still run on past the cut unless they were cut there
        // too. [`Project::split_tail_links`] pairs the tails of one cut.
        new_clip.link = None;
        if let (
            Some((_, right_source)),
            ClipSource::Media { source, .. }
            | ClipSource::Multicam { source, .. }
            | ClipSource::Sequence { source, .. },
        ) = (new_left_source, &mut new_clip.content)
        {
            *source = right_source;
        }
        // Ordinary animation keyframes are clip-relative. Moving every tail
        // keyframe left by the split offset makes sampling tail tick `t`
        // identical to sampling original tick `split + t`.
        new_clip.shift_timeline_params(-left_tl.duration.value)?;
        new_clip.speed_curve = right_speed_curve;
        // Edge-anchored properties stay on the corresponding outer edge.
        new_clip.fade_in = 0;
        if new_clip.animation_combo.is_none() {
            new_clip.animation_in = None;
        }
        if let Some((_, right_source)) = new_left_source {
            let right_start = right_source.start.value;
            let right_end = right_source.end_tick();
            new_clip.markers.retain_mut(|m| {
                if m.tick.value < right_start || m.tick.value >= right_end {
                    return false;
                }
                if m.tick.value + m.duration.value > right_end {
                    m.duration.value = (right_end - m.tick.value).max(0);
                }
                true
            });
        } else {
            let split_local = left_tl.duration.value;
            let right_len = right_tl.duration.value;
            new_clip.markers.retain_mut(|m| {
                if m.tick.value < split_local || m.tick.value >= split_local + right_len {
                    return false;
                }
                m.tick.value -= split_local;
                if m.tick.value + m.duration.value > right_len {
                    m.duration.value = (right_len - m.tick.value).max(0);
                }
                true
            });
        }

        let track_id = self
            .timeline()
            .track_of(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;

        {
            let mut left = self
                .timeline_mut()
                .clip_timeline_mut(clip_id)
                .expect("clip existence checked above");
            // The head keeps its start and gives up everything from the cut:
            // shortening it here is what makes room for the tail added below,
            // and without it the insert collides with the clip being split.
            left.timeline = left_tl;
            // The tail owns properties anchored to the original right edge.
            left.fade_out = 0;
            if left.animation_combo.is_none() {
                left.animation_out = None;
            }
            if let (
                Some((left_source, _)),
                ClipSource::Media { source, .. }
                | ClipSource::Multicam { source, .. }
                | ClipSource::Sequence { source, .. },
            ) = (new_left_source, &mut left.content)
            {
                *source = left_source;
            }
            left.speed_curve = left_speed_curve;
            if let Some((left_source, _)) = new_left_source {
                let left_start = left_source.start.value;
                let left_end = left_source.end_tick();
                left.markers.retain_mut(|m| {
                    if m.tick.value < left_start || m.tick.value >= left_end {
                        return false;
                    }
                    if m.tick.value + m.duration.value > left_end {
                        m.duration.value = (left_end - m.tick.value).max(0);
                    }
                    true
                });
            } else {
                let left_len = left_tl.duration.value;
                left.markers.retain_mut(|m| {
                    if m.tick.value < 0 || m.tick.value >= left_len {
                        return false;
                    }
                    if m.tick.value + m.duration.value > left_len {
                        m.duration.value = (left_len - m.tick.value).max(0);
                    }
                    true
                });
            }
        }
        let new_id = self.timeline_mut().add_clip(track_id, new_clip)?;
        if let Some(track) = self.timeline_mut().track_mut(track_id) {
            track.carry_transitions_across_split(clip_id, new_id);
            track.prune_dead_transitions();
        }
        Ok(new_id)
    }

    /// Set the clip's timeline placement to `new_timeline` (trim/extend).
    pub fn trim_clip(
        &mut self,
        clip_id: ClipId,
        new_timeline: TimeRange,
    ) -> Result<(), ModelError> {
        let (track_id, new_source) = self.plan_trim(clip_id, new_timeline, true)?;
        self.timeline_mut()
            .clip_timeline_mut(clip_id)
            .expect("plan_trim found the clip")
            .trim_to(new_timeline, new_source);
        if let Some(track) = self.timeline_mut().track_mut(track_id) {
            track.prune_dead_transitions();
        }
        Ok(())
    }

    /// The lane [`Self::trim_clip`] leaves the clip on and the source window
    /// it gives the clip for `new_timeline`, refused as `trim_clip` refuses
    /// it, with nothing written. With `lane` false, neither a neighbor the new
    /// range covers nor the sequence start refuses it.
    pub(super) fn plan_trim(
        &self,
        clip_id: ClipId,
        new_timeline: TimeRange,
        lane: bool,
    ) -> Result<(TrackId, Option<TimeRange>), ModelError> {
        check_same_rate(new_timeline.start.rate, self.timeline().frame_rate)?;
        check_same_rate(new_timeline.duration.rate, self.timeline().frame_rate)?;
        new_timeline.end()?;
        if new_timeline.is_empty() || (lane && new_timeline.start.value < 0) {
            return Err(ModelError::InvalidRange);
        }
        let track_id = self
            .timeline()
            .track_of(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let clip = self.timeline().require_clip(clip_id)?;

        if lane
            && self
                .timeline()
                .track(track_id)
                .expect("clip is on a track")
                .has_overlap(new_timeline, Some(clip_id))?
        {
            return Err(ModelError::Overlap(track_id));
        }

        Ok((track_id, self.trim_source(clip, new_timeline)?))
    }

    /// The source window a trim of `clip` to `new_timeline` gives it, refused
    /// as [`Self::trim_clip`] refuses a window past the take. `None` for a
    /// generated clip, which plays no take. `clip` is read as it is, so it
    /// may be one the caller has already edited.
    pub fn trim_source(
        &self,
        clip: &Clip,
        new_timeline: TimeRange,
    ) -> Result<Option<TimeRange>, ModelError> {
        let tl_rate = self.timeline().frame_rate;
        check_same_rate(new_timeline.start.rate, tl_rate)?;
        let window = match &clip.content {
            ClipSource::Media { media: _, source } if clip.freeze_frame => {
                Some(TimeRange::at_rate(source.start.value, 1, source.start.rate))
            }
            ClipSource::Media { media, source } => {
                let media = self
                    .media
                    .get(media)
                    .ok_or(ModelError::UnknownMedia(*media))?;
                let (start, duration) =
                    trimmed_window(clip, *source, new_timeline, media.frame_rate);
                // Stills extend freely past the pool's default 5s window —
                // the one frame repeats and decode ignores the window, so
                // the source range is duration bookkeeping only. Clamp the
                // start to 0 so extensions stay canonical.
                if media.is_image {
                    Some(TimeRange::at_rate(start.max(0), duration, media.frame_rate))
                } else {
                    let end = start
                        .checked_add(duration)
                        .ok_or(ModelError::TimeOverflow)?;
                    if start < 0 || end > media.duration.value {
                        return Err(ModelError::SourceOutOfBounds);
                    }
                    Some(TimeRange::at_rate(start, duration, media.frame_rate))
                }
            }
            ClipSource::Multicam { group, source } => {
                if !self.multicam_groups.contains_key(group) {
                    return Err(ModelError::UnknownMulticamGroup(*group));
                }
                let rate = source.start.rate;
                let (start, duration) = trimmed_window(clip, *source, new_timeline, rate);
                if start < 0 {
                    return Err(ModelError::SourceOutOfBounds);
                }
                Some(TimeRange::at_rate(start, duration, rate))
            }
            ClipSource::Sequence { sequence, source } => {
                let seq = self
                    .sequences
                    .get(sequence)
                    .ok_or(ModelError::UnknownSequence(*sequence))?;
                let seq_fps = seq.timeline.frame_rate;
                let (start, duration) = trimmed_window(clip, *source, new_timeline, seq_fps);
                if start < 0 {
                    return Err(ModelError::SourceOutOfBounds);
                }
                Some(TimeRange::at_rate(start, duration, seq_fps))
            }
            ClipSource::Generated(_) => None,
        };
        // Edges saturate at the last tick, and a window that rounds to
        // nothing keeps one frame past its start, which can end past it.
        window.map(|w| w.end()).transpose()?;
        Ok(window)
    }

    /// Slip a clip's source window by `by` source ticks, keeping its
    /// timeline placement and length: the window [`Self::slip_source`] gives.
    ///
    /// Trimming derives the source window from the timeline, so it cannot
    /// express "same place on the timeline, different part of the take". A slip
    /// is that gesture and nothing else: neighbours are untouched, so no overlap
    /// is possible and no other clip moves.
    pub fn slip_clip(&mut self, clip_id: ClipId, by: i64) -> Result<(), ModelError> {
        let window = self.slip_source(self.timeline().require_clip(clip_id)?, by)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .expect("clip existence checked above");
        let timeline = clip.timeline;
        clip.trim_to(timeline, Some(window));
        Ok(())
    }

    /// The source window a slip of `clip` by `by` source ticks gives it. `clip`
    /// is read as it is, so it may be one the caller has already edited.
    ///
    /// Clamped to the take rather than refused at the ends: a drag that hits
    /// the head of the take stops there. Refused for a generated clip, a freeze
    /// frame, a still, and a clip that already spans its whole take.
    pub fn slip_source(&self, clip: &Clip, by: i64) -> Result<TimeRange, ModelError> {
        let (source, (back, on)) = self.slip_room(clip)?;
        Ok(TimeRange::at_rate(
            source.start.value.saturating_add(by.clamp(back, on)),
            source.duration.value,
            source.start.rate,
        ))
    }

    /// `clip`'s source window, and how far its take may slip, in the ticks of
    /// the take: back to the head of the take and on to its tail. A nested
    /// sequence shorter than the window has no tail, and it slips on until the
    /// window ends at the last tick a time holds.
    ///
    /// Refused for a generated clip (no source), a freeze frame (one held
    /// frame), a still (whose window is duration bookkeeping, not a position
    /// in a take), and a clip that already spans its whole take.
    fn slip_room(&self, clip: &Clip) -> Result<(TimeRange, (i64, i64)), ModelError> {
        if clip.freeze_frame {
            return Err(ModelError::Slip(SlipRefusal::FreezeFrame));
        }
        let whole = || ModelError::Slip(SlipRefusal::WholeTake);
        let (source, limit) = match &clip.content {
            ClipSource::Media { media, source } => {
                let media = self
                    .media
                    .get(media)
                    .ok_or(ModelError::UnknownMedia(*media))?;
                if media.is_image {
                    return Err(ModelError::Slip(SlipRefusal::Still));
                }
                let limit = media.duration.value - source.duration.value;
                if limit <= 0 {
                    return Err(whole());
                }
                (*source, limit)
            }
            ClipSource::Sequence { sequence, source } => {
                let seq = self
                    .sequences
                    .get(sequence)
                    .ok_or(ModelError::UnknownSequence(*sequence))?;
                let limit = seq.timeline.duration().value - source.duration.value;
                (*source, if limit > 0 { limit } else { i64::MAX })
            }
            ClipSource::Multicam { group, source } => {
                let limit =
                    self.multicam_group_span(*group)?.duration.value - source.duration.value;
                if limit <= 0 {
                    return Err(whole());
                }
                (*source, limit)
            }
            ClipSource::Generated(_) => return Err(ModelError::Slip(SlipRefusal::Generated)),
        };
        let back = source.start.value.saturating_neg();
        let on = if limit == i64::MAX {
            i64::MAX - source.start.value.saturating_add(source.duration.value)
        } else {
            limit - source.start.value
        };
        Ok((source, (back, on.max(back))))
    }

    /// Each clip a slip of `clip` by `by` of its source ticks moves, and the
    /// source ticks it moves by. Each clip linked to `clip` on an unlocked
    /// lane ([`Self::linked_followers`]) moves by the same
    /// stretch of source time in the ticks of its own take, and the slip stops
    /// where the first of their takes runs out, so a linked pair placing files
    /// of different rates and lengths keeps its sync. Refused as
    /// [`Self::slip_source`] refuses any clip it reaches.
    pub fn slip_reach(&self, clip: ClipId, by: i64) -> Result<Vec<(ClipId, i64)>, ModelError> {
        let primary = self.timeline().require_clip(clip)?;
        if primary.link.is_none() {
            return Ok(vec![(clip, by)]);
        }
        let from = self.slip_room(primary)?.0.start.rate;
        let members = self
            .linked_followers(clip)
            .into_iter()
            .map(|member| {
                let (source, room) = self.slip_room(self.timeline().require_clip(member)?)?;
                Ok((member, source.start.rate, room))
            })
            .collect::<Result<Vec<_>, ModelError>>()?;
        // Each member's room in `clip`'s ticks, rounded toward zero so the
        // share a member takes back stays inside its take.
        let (back, on) = members.iter().fold(
            (i64::MIN, i64::MAX),
            |(back, on), &(_, rate, (low, high))| {
                let shared = |ticks| RationalTime::new(ticks, rate).within(from).value;
                (back.max(shared(low)), on.min(shared(high)))
            },
        );
        let by = by.clamp(back, on.max(back));
        Ok(members
            .into_iter()
            .map(|(member, rate, _)| {
                let ticks = if member == clip {
                    by
                } else {
                    resample(RationalTime::new(by, from), rate).value
                };
                (member, ticks)
            })
            .collect())
    }

    /// Maximum timeline ticks `clip_id` can occupy from its start before
    /// reaching the next clip on its lane, or `None` when nothing follows it.
    pub fn clip_room_after(&self, clip_id: ClipId) -> Option<i64> {
        let track_id = self.timeline().track_of(clip_id)?;
        let clip = self.timeline().clip(clip_id)?;
        let end = clip.timeline.end_tick();
        let next = self
            .timeline()
            .track(track_id)?
            .clips_ordered()
            .find(|other| other.id != clip_id && other.timeline.start.value >= end)?;
        Some((next.timeline.start.value - clip.timeline.start.value).max(1))
    }

    /// Retime a media clip (CapCut speed, M1): keep its timeline start and
    /// source window, set `speed`/`reversed`, and re-derive the timeline
    /// duration. Rejected on generated clips, non-positive speeds, and overlap.
    pub fn set_clip_speed(
        &mut self,
        clip_id: ClipId,
        speed: Rational,
        reversed: bool,
    ) -> Result<(), ModelError> {
        self.set_clip_speed_bounded(clip_id, speed, reversed, None)
    }

    /// [`Self::set_clip_speed`], clamping the retimed out point to `max_duration`.
    pub fn set_clip_speed_bounded(
        &mut self,
        clip_id: ClipId,
        speed: Rational,
        reversed: bool,
        max_duration: Option<i64>,
    ) -> Result<(), ModelError> {
        let tl_rate = self.timeline().frame_rate;
        let mut probe = self.timeline().require_clip(clip_id)?.clone();
        probe.retime(speed, reversed, tl_rate)?;
        self.apply_retimed_clip(clip_id, probe, max_duration)
    }

    /// Set (or clear) a media clip's playback-rate ramp (CapCut speed curves,
    /// M2): store `curve` (`None` clears it) and re-derive duration from
    /// `source ÷ (base_speed × average_curve)`.
    pub fn set_clip_speed_curve(
        &mut self,
        clip_id: ClipId,
        curve: Option<Param<f32>>,
    ) -> Result<(), ModelError> {
        self.set_clip_speed_curve_bounded(clip_id, curve, None)
    }

    /// [`Self::set_clip_speed_curve`], clamping the retimed out point to `max_duration`.
    pub fn set_clip_speed_curve_bounded(
        &mut self,
        clip_id: ClipId,
        curve: Option<Param<f32>>,
        max_duration: Option<i64>,
    ) -> Result<(), ModelError> {
        if let Some(curve) = &curve {
            crate::clip::validate_speed_curve(curve)?;
        }
        let clip = self.timeline().require_clip(clip_id)?;
        if clip.freeze_frame {
            return Err(ModelError::InvalidParam(
                "freeze-frame clips cannot use speed curves".into(),
            ));
        }
        if clip.source_range().is_none() {
            return Err(ModelError::Generated(GeneratedRefusal::Speed));
        }
        let new_curve = curve.unwrap_or(Param::Constant(1.0));
        let new_dur = self
            .speed_curve_duration(clip_id, &new_curve)
            .expect("clip and source range checked above");
        let mut probe = clip.clone();
        probe.speed_curve = new_curve;
        probe.timeline = TimeRange::at_rate(
            clip.timeline.start.value,
            new_dur,
            self.timeline().frame_rate,
        );
        self.apply_retimed_clip(clip_id, probe, max_duration)
    }

    fn apply_retimed_clip(
        &mut self,
        clip_id: ClipId,
        probe: Clip,
        max_duration: Option<i64>,
    ) -> Result<(), ModelError> {
        let track_id = self
            .timeline()
            .track_of(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let full = probe.timeline;
        let (new_timeline, clamped_source) = match max_duration.filter(|&m| full.duration.value > m)
        {
            Some(limit) => {
                let clamped = TimeRange::at_rate(full.start.value, limit.max(1), full.start.rate);
                let src = self.trim_source(&probe, clamped)?;
                (clamped, Some(src))
            }
            None => (full, None),
        };
        if self
            .timeline()
            .track(track_id)
            .expect("clip is on a track")
            .has_overlap(new_timeline, Some(clip_id))?
        {
            return Err(ModelError::Overlap(track_id));
        }
        {
            let mut held = self
                .timeline_mut()
                .clip_timeline_mut(clip_id)
                .expect("clip existence checked above");
            held.speed = probe.speed;
            held.reversed = probe.reversed;
            held.speed_curve = probe.speed_curve;
            held.timeline = new_timeline;
            if let Some(src) = clamped_source {
                held.trim_to(new_timeline, src);
            }
        }
        if let Some(track) = self.timeline_mut().track_mut(track_id) {
            track.prune_dead_transitions();
        }
        Ok(())
    }

    /// Timeline ticks the clip would occupy under `curve`, without applying it.
    ///
    /// `None` when the clip is unknown or generated (a generator has no source
    /// window to retime). A shell that draws a ramp shows the length it implies
    /// before committing, and [`Self::set_clip_speed_curve`] derives its new
    /// extent from this same call, so a preview cannot drift from the edit.
    pub fn speed_curve_duration(&self, clip_id: ClipId, curve: &Param<f32>) -> Option<i64> {
        self.timeline()
            .clip(clip_id)?
            .duration_under(curve, self.timeline().frame_rate)
    }

    /// Toggle whether a retimed media clip preserves pitch while it plays
    /// (CapCut's "pitch" switch, M8 Phase 3): `true` time-stretches so the
    /// audio keeps its pitch, `false` lets pitch ride the speed ("chipmunk").
    /// Pure audio property — it changes no duration, so there is no overlap
    /// check. Rejected on generated clips (nothing to hear).
    pub fn set_clip_pitch(
        &mut self,
        clip_id: ClipId,
        preserve_pitch: bool,
    ) -> Result<(), ModelError> {
        self.edit_clip(clip_id, |clip| {
            if clip.source_range().is_none() {
                return Err(ModelError::InvalidParam(
                    "pitch lock requires a media-backed clip".into(),
                ));
            }
            clip.preserve_pitch = preserve_pitch;
            Ok(())
        })
    }

    /// Toggle noise reduction on a media clip (CapCut "Reduce noise", M8
    /// Phase 5), returning the previous flag for the inverse. The mixers run
    /// the clip's audio through RNNoise when set. Rejected on generated clips
    /// (no source audio to clean).
    pub fn set_clip_denoise(&mut self, clip_id: ClipId, denoise: bool) -> Result<bool, ModelError> {
        self.edit_clip(clip_id, |clip| {
            if clip.source_range().is_none() {
                return Err(ModelError::InvalidParam(
                    "noise reduction requires a media-backed clip".into(),
                ));
            }
            Ok(std::mem::replace(&mut clip.denoise, denoise))
        })
    }

    /// Set (or clear with `None`) a clip's organizational label colour,
    /// returning the previous value for the inverse.
    ///
    /// Accepted on every clip kind: a label describes an editorial grouping,
    /// not the content, so a title and a sound effect can share one.
    pub fn set_clip_label(
        &mut self,
        clip_id: ClipId,
        label: Option<crate::LabelColor>,
    ) -> Result<Option<crate::LabelColor>, ModelError> {
        self.edit_clip(clip_id, |clip| {
            Ok(std::mem::replace(&mut clip.label, label))
        })
    }

    /// Set (or clear with `None`) a clip's operator-given name, returning the
    /// previous value for the inverse.
    ///
    /// A blank or whitespace-only name is stored as `None`: an empty card name
    /// is not a name, and clearing it must give the source's own name back.
    /// Accepted on every clip kind.
    pub fn set_clip_name(
        &mut self,
        clip_id: ClipId,
        name: Option<String>,
    ) -> Result<Option<String>, ModelError> {
        let name = name.filter(|text| !text.trim().is_empty());
        self.edit_clip(clip_id, |clip| Ok(std::mem::replace(&mut clip.name, name)))
    }

    /// Set (or with `None` clear) the clip type `clip_id` is, returning the
    /// previous one. Refused when the id is empty, longer than
    /// [`crate::CLIP_TYPE_ID_MAX_BYTES`], or holds a space or a control
    /// character. The type need not be registered: a project opened without the
    /// plugin keeps the id.
    pub fn set_clip_type(
        &mut self,
        clip_id: ClipId,
        clip_type: Option<String>,
    ) -> Result<Option<String>, ModelError> {
        if let Some(id) = &clip_type {
            let fault = if id.is_empty() {
                Some("a clip type id is empty".to_owned())
            } else if id.len() > crate::CLIP_TYPE_ID_MAX_BYTES {
                Some(format!(
                    "a clip type id is limited to {} bytes",
                    crate::CLIP_TYPE_ID_MAX_BYTES
                ))
            } else if id.chars().any(|c| c.is_whitespace() || c.is_control()) {
                Some(format!(
                    "the clip type id `{id}` holds a space or a control character"
                ))
            } else {
                None
            };
            if let Some(fault) = fault {
                return Err(ModelError::InvalidParam(fault));
            }
        }
        self.edit_clip(clip_id, |clip| {
            Ok(std::mem::replace(&mut clip.clip_type, clip_type))
        })
    }

    /// Set (or with `None` clear) the property `clip_id`'s bar on the timeline
    /// draws as its line, returning the previous one. Refused when the clip
    /// has no such property, and for a picture property of a clip on an audio
    /// lane.
    pub fn set_clip_line_param(
        &mut self,
        clip_id: ClipId,
        param: Option<crate::ClipParam>,
    ) -> Result<Option<crate::ClipParam>, ModelError> {
        let audio_lane = self
            .timeline()
            .track_of(clip_id)
            .and_then(|track| self.timeline().track(track))
            .is_some_and(|track| track.kind.is_audio());
        self.edit_clip(clip_id, |clip| {
            if let Some(param) = param
                && clip.line_row(param, audio_lane).is_none()
            {
                return Err(ModelError::InvalidParam(format!(
                    "the clip has no property {param:?} to draw on its bar; choose one from its fx badge menu"
                )));
            }
            Ok(std::mem::replace(&mut clip.line_param, param))
        })
    }

    /// Set a media clip's audio mix (CapCut volume + fades): `volume` is
    /// `Some` to set a flat gain (`0` mutes, `1` unchanged, up to
    /// [`crate::MAX_CLIP_VOLUME`]× boost), overwriting any M8 envelope
    /// (CapCut's basic slider), or `None` to keep the current gain and only
    /// update the fades — so a fade edit never flattens an envelope. Fades
    /// are linear in/out durations at the timeline rate. Rejected on
    /// generated clips (nothing to hear), out-of-range volume, negative
    /// fades, and fades longer than the clip.
    pub fn set_clip_audio(
        &mut self,
        clip_id: ClipId,
        volume: Option<f32>,
        fade_in: RationalTime,
        fade_out: RationalTime,
    ) -> Result<(), ModelError> {
        if let Some(volume) = volume {
            crate::clip::validate_volume(volume)?;
        }
        let tl_rate = self.timeline().frame_rate;
        check_same_rate(fade_in.rate, tl_rate)?;
        check_same_rate(fade_out.rate, tl_rate)?;

        let clip = self.timeline().require_clip(clip_id)?;
        if clip.is_generated() {
            return Err(ModelError::Generated(GeneratedRefusal::Sound));
        }
        let duration = clip.timeline.duration.value;
        for (name, fade) in [("fade_in", fade_in.value), ("fade_out", fade_out.value)] {
            if fade < 0 {
                return Err(ModelError::InvalidParam(format!("{name} must be ≥ 0")));
            }
            if fade > duration {
                return Err(ModelError::InvalidParam(format!(
                    "{name} ({:.3} s) is longer than the clip ({:.3} s)",
                    RationalTime::new(fade, tl_rate).seconds(),
                    RationalTime::new(duration, tl_rate).seconds()
                )));
            }
        }

        self.edit_clip(clip_id, |clip| {
            // `Some` is the basic volume slider: a flat level that flattens any
            // envelope (CapCut). `None` keeps the gain (constant or envelope)
            // and touches only the fades, so a fade edit never destroys
            // automation; envelopes are otherwise drawn through the volume
            // keyframe commands (`ClipParam::Volume`).
            if let Some(volume) = volume {
                clip.volume = Param::Constant(volume);
            }
            clip.fade_in = fade_in.value;
            clip.fade_out = fade_out.value;
            Ok(())
        })
    }

    /// Set a clip's framing (CapCut crop, M1): the normalized kept region
    /// plus horizontal/vertical mirroring. Visual clips only — audio has no
    /// frame to crop. Rejected on a degenerate or out-of-frame crop rect.
    ///
    /// `at` composes with animation CapCut-style (same as [`Self::set_transform`]):
    /// `Some(playhead)` writes a keyframe when crop is already animated, and
    /// leaves the curve as it is when `crop` is the rect in force there, so a
    /// mirror toggle on an animated crop adds no keyframe; `None` (or a
    /// never-animated crop) flattens to a constant.
    pub fn set_clip_crop(
        &mut self,
        clip_id: ClipId,
        crop: CropRect,
        flip_h: bool,
        flip_v: bool,
        at: Option<RationalTime>,
    ) -> Result<(), ModelError> {
        self.require_visual(clip_id)?;
        if let Some(at) = at {
            check_same_rate(at.rate, self.timeline().frame_rate)?;
        }
        self.edit_clip(clip_id, |clip| {
            // The rect in force at an animated frame is written nowhere, so it
            // is not held to the valid set: an eased curve may overshoot
            // between keys.
            match at {
                Some(at) if clip.crop.is_animated() => {
                    let tick = clip.param_tick(at.value);
                    if clip.crop.sample(tick) != crop {
                        crop.validate()?;
                        clip.crop.set_keyframe(tick, crop, Easing::Linear);
                    }
                }
                _ => {
                    crop.validate()?;
                    clip.crop.set_constant(crop);
                }
            }
            clip.flip_h = flip_h;
            clip.flip_v = flip_v;
            Ok(())
        })
    }

    /// Replace a clip's detected beat markers (M8 Phase 6), returning the
    /// previous list for the inverse. Beats are source ticks at the media
    /// frame rate; this only stores what detection (or a clear) produced —
    /// the engine owns the analysis. Media clips only (generated content has
    /// no audio to analyze). Stored sorted + de-duplicated and clamped to the
    /// source window so a stale list can't snap to phantom positions.
    pub fn set_clip_beats(
        &mut self,
        clip_id: ClipId,
        beats: Vec<i64>,
    ) -> Result<Vec<i64>, ModelError> {
        let clip = self.timeline().require_clip(clip_id)?;
        let source = clip
            .source_range()
            .ok_or_else(|| ModelError::InvalidParam("beats require a media-backed clip".into()))?;
        let (lo, hi) = (source.start.value, source.end()?.value);
        let mut beats: Vec<i64> = beats
            .into_iter()
            .filter(|&b| (lo..hi).contains(&b))
            .collect();
        beats.sort_unstable();
        beats.dedup();

        self.edit_clip(clip_id, |clip| {
            Ok(std::mem::replace(&mut clip.beats, beats))
        })
    }

    pub fn audio_layout(&self) -> AudioLayout {
        self.timeline().audio_layout()
    }

    pub fn set_audio_layout(&mut self, layout: AudioLayout) {
        self.timeline_mut().set_audio_layout(layout);
    }

    pub fn time_display(&self) -> TimeDisplay {
        self.timeline().time_display()
    }

    pub fn set_time_display(&mut self, display: TimeDisplay) {
        self.timeline_mut().set_time_display(display);
    }

    pub fn resample_frame_rate(&mut self, new_rate: Rational) -> Result<(), ModelError> {
        self.timeline_mut().resample_frame_rate(new_rate)
    }
}

/// The source windows, in `rate`, of the halves of `clip` cut at timeline
/// position `at`, read from the clip's own source position on each side of
/// the cut. Exact rational base speed, integrated speed ramps, mixed source
/// and timeline rates, and reversal all resolve through
/// [`Clip::source_time_at`].
///
/// Fails with [`ModelError::InvalidRange`] when `source` is shorter than two
/// frames, or when adjacent timeline frames resolve to the same source frame,
/// so no two disjoint non-empty windows preserve both halves.
fn split_source_window(
    clip: &Clip,
    source: TimeRange,
    at: RationalTime,
    tl_rate: Rational,
    rate: Rational,
) -> Result<(TimeRange, TimeRange), ModelError> {
    if source.duration.value < 2 {
        return Err(ModelError::InvalidRange);
    }
    let boundary = clip.source_time_at(at)?.ok_or(ModelError::InvalidRange)?;
    let previous_at = RationalTime::new(
        at.value.checked_sub(1).ok_or(ModelError::TimeOverflow)?,
        tl_rate,
    );
    let previous = clip
        .source_time_at(previous_at)?
        .ok_or(ModelError::InvalidRange)?;
    let source_last = source.start.value + source.duration.value - 1;
    let left_src_dur = if clip.reversed {
        if previous.value <= boundary.value {
            return Err(ModelError::InvalidRange);
        }
        source_last - boundary.value
    } else {
        if previous.value >= boundary.value {
            return Err(ModelError::InvalidRange);
        }
        boundary.value - source.start.value
    };
    if left_src_dur <= 0 || left_src_dur >= source.duration.value {
        return Err(ModelError::InvalidRange);
    }
    // A reversed clip plays its window backward: the timeline's left half
    // shows the source window's top, so the split hands the window bottom to
    // the right clip.
    let (left_src_start, right_src_start) = if clip.reversed {
        (
            source.start.value + source.duration.value - left_src_dur,
            source.start.value,
        )
    } else {
        (source.start.value, source.start.value + left_src_dur)
    };
    Ok((
        TimeRange::at_rate(left_src_start, left_src_dur, rate),
        TimeRange::at_rate(right_src_start, source.duration.value - left_src_dur, rate),
    ))
}

/// The source window, as `(start, duration)` in `rate`, that `clip` plays
/// over `new_timeline`. Each source edge moves by its own timeline edge's
/// delta, scaled by the clip's speed and resampled to `rate` as
/// [`Clip::source_time_at`] maps a position. A head trimmed to a tick starts
/// the window where a split there starts its right half, an edge the trim
/// leaves in place keeps its source frame, and a trim inside the clip stays
/// inside its window. A reversed clip plays its window backward: its timeline
/// head moves the window's end and its tail the window's start. A window that
/// rounds to nothing keeps one frame beside the edge that stayed.
fn trimmed_window(
    clip: &Clip,
    source: TimeRange,
    new_timeline: TimeRange,
    rate: Rational,
) -> (i64, i64) {
    let shift = |ticks: i64| {
        let scaled = if clip.has_speed_curve() {
            ((ticks as f64)
                * (f64::from(clip.speed.num) / f64::from(clip.speed.den))
                * clip.speed_curve_average())
            .round() as i64
        } else {
            clip.scale_by_speed(ticks)
        };
        resample(RationalTime::new(scaled, new_timeline.start.rate), rate).value
    };
    let head = shift(
        new_timeline
            .start
            .value
            .saturating_sub(clip.timeline.start.value),
    );
    let tail = shift(
        new_timeline
            .end_tick()
            .saturating_sub(clip.timeline.end_tick()),
    );
    let (low, high) = if clip.reversed {
        (tail.saturating_neg(), head.saturating_neg())
    } else {
        (head, tail)
    };
    let start = source.start.value.saturating_add(low);
    let end = source.end_tick().saturating_add(high);
    if end > start {
        (start, end.saturating_sub(start))
    } else if high == 0 {
        (end.saturating_sub(1), 1)
    } else {
        (start, 1)
    }
}
