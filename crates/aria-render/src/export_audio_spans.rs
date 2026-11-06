//! A timeline resolved into the mixer's graph and spans: every audible clip
//! becomes a span over its recorded sound, and a nested sequence clip a span
//! over that sequence's own mix.

use std::path::PathBuf;

use aria_core::{AudioReader, DecodeError};
use aria_models::{
    AudioEffectInstance, Clip, ClipParam, ModelError, Project, SequenceId, TimeRange, Timeline,
};

use super::{BusNode, CHANNELS, ExportAudioMixer, Span, inserts, ticks_to_samples, xfade};
use crate::audio_dsp::{DenoiseReader, SpanWarp};
use crate::driver::driven_envelope;
use crate::error::RenderError;

impl ExportAudioMixer {
    /// Audible spans: clips on lanes whose sound still arrives at the master,
    /// summed through the project's own mix graph. CapCut-style, a video clip
    /// keeps its own sound, so video lanes are audible too; only a clip
    /// detached to a linked audio lane defers its audio there. `None` when the
    /// timeline is silent, so callers can skip the audio track entirely.
    ///
    /// Routing is read once, from [`Timeline::audio_buses`]: a lane behind a
    /// muted bus is dropped here rather than decoded and multiplied by zero,
    /// and a project whose routing is cyclic is refused by name instead of
    /// being mixed in whatever order the fold happened to take.
    ///
    /// Reversed clips mix backward. Other retimed and denoised clips mix
    /// through the same warped-resample path. A nested sequence clip plays its
    /// sequence's mix, built the same way from that sequence's own lanes,
    /// routing and master; a sequence that shows itself, directly or through
    /// another, is [`ModelError::SequenceCycle`].
    pub fn for_project(project: &Project) -> Result<Option<Self>, RenderError> {
        Self::for_timeline(project, project.timeline(), &mut Vec::new())
    }

    /// The mix of `timeline`, resolved inside the sequences in `nesting`.
    fn for_timeline(
        project: &Project,
        timeline: &Timeline,
        nesting: &mut Vec<SequenceId>,
    ) -> Result<Option<Self>, RenderError> {
        let fps = timeline.frame_rate;
        let graph = timeline.audio_buses()?;
        let index_of = |track| graph.iter().position(|node| node.track == track);
        // Feed order is what the fold relies on: a lane only ever writes
        // forward, so its destinations have to sit after it. The model
        // guarantees that; this turns the guarantee into a checked precondition
        // instead of an index the fold would have to second-guess per block.
        let mut buses: Vec<BusNode> = Vec::with_capacity(graph.len());
        for (i, node) in graph.iter().enumerate() {
            let forward = |target| match index_of(target) {
                Some(j) if j > i => Ok(j),
                _ => Err(RenderError::Model(ModelError::RoutingCycle(node.track))),
            };
            let mut sends = Vec::with_capacity(node.sends.len());
            for send in node.sends {
                sends.push((forward(send.to)?, send.level, send.pre_fader));
            }
            buses.push(BusNode {
                gain: node.gain,
                pan: node.pan,
                automation: node.automation,
                volume_envelope: node.volume_envelope.clone(),
                pan_envelope: node.pan_envelope.clone(),
                output: node.output.bus().map(forward).transpose()?,
                sends,
                live: node.reaches_master,
            });
        }
        let mut spans = Vec::new();
        // Each span's clip chain, by span.
        let mut clip_inserts: Vec<&[AudioEffectInstance]> = Vec::new();
        // One lane's spans, for its crossfades.
        let mut lane = Vec::new();
        // Read once: the per-clip detach query scans the audio lanes for each
        // linked clip.
        let links = timeline.audio_lane_links();
        for track in timeline.tracks_ordered() {
            let Some(bus) = index_of(track.id) else {
                continue;
            };
            if !graph[bus].reaches_master {
                continue;
            }
            lane.clear();
            for clip in track.clips_ordered() {
                if clip.is_silent() {
                    continue;
                }
                if !links.carries_own_audio(track, clip) {
                    continue;
                }
                let Some(sound) = clip_source(project, clip, nesting)? else {
                    continue;
                };
                let source = sound.window;
                let warp = if clip.has_speed_curve() {
                    SpanWarp::Curved {
                        curve: clip.speed_curve.clone(),
                        source_len: ticks_to_samples(
                            source.duration.value,
                            source.start.rate.num,
                            source.start.rate.den,
                        ),
                        curve_total: clip.speed_curve_average(),
                    }
                } else if clip.speed.num != clip.speed.den {
                    SpanWarp::FlatSpeed {
                        num: clip.speed.num,
                        den: clip.speed.den,
                    }
                } else {
                    SpanWarp::Linear
                };
                // A driven envelope replaces the keyframed one, evaluated once
                // per frame of the clip.
                let volume = driven_envelope(project, clip, ClipParam::Volume);
                let pan = driven_envelope(project, clip, ClipParam::Pan);
                spans.push(Span {
                    path: sound.path,
                    start: ticks_to_samples(clip.timeline.start.value, fps.num, fps.den),
                    end: ticks_to_samples(clip.timeline.end_tick(), fps.num, fps.den),
                    source_start: ticks_to_samples(
                        source.start.value,
                        source.start.rate.num,
                        source.start.rate.den,
                    ),
                    source_len: ticks_to_samples(
                        source.duration.value,
                        source.start.rate.num,
                        source.start.rate.den,
                    ),
                    reversed: clip.reversed,
                    warp,
                    preserve_pitch: clip.preserve_pitch,
                    denoise: clip.denoise,
                    volume: volume
                        .as_ref()
                        .unwrap_or(&clip.volume)
                        .map_ticks(|tick| ticks_to_samples(tick, fps.num, fps.den)),
                    pan: pan
                        .as_ref()
                        .unwrap_or(&clip.pan)
                        .map_ticks(|tick| ticks_to_samples(tick, fps.num, fps.den)),
                    fade_in: ticks_to_samples(clip.fade_in, fps.num, fps.den),
                    fade_out: ticks_to_samples(clip.fade_out, fps.num, fps.den),
                    crossfade_in: None,
                    crossfade_out: None,
                    reader: sound.reader,
                    exhausted: false,
                    file_len: sound.file_len,
                    bus,
                });
                lane.push(xfade::LaneSpan {
                    clip: clip.id,
                    span: spans.len() - 1,
                    media_len: sound.len,
                });
                clip_inserts.push(&clip.audio_effects);
            }
            xfade::crossfade_lane(track, &lane, &mut spans, fps);
        }
        if spans.is_empty() {
            return Ok(None);
        }
        let inserts = inserts::InsertGraph::build(
            &graph,
            &buses,
            timeline.master_audio_effects(),
            &clip_inserts,
            fps,
        )
        .map(Box::new);
        Ok(Some(Self {
            bus_buffers: vec![Vec::new(); buses.len()],
            lanes: graph.iter().map(|node| node.track).collect(),
            peaks: vec![[0.0; 2]; buses.len()],
            fps,
            master_gain: timeline.master_gain(),
            master_pan: timeline.master_pan(),
            master_muted: timeline.master_muted(),
            master_automation: timeline.master_automation(),
            master_volume: timeline.master_volume_envelope().clone(),
            master_pan_envelope: timeline.master_pan_envelope().clone(),
            master_peak: [0.0; 2],
            buses,
            spans,
            scratch: Vec::new(),
            warp_scratch: Vec::new(),
            weight_scratch: Vec::new(),
            shuttle_scratch: Vec::new(),
            inserts,
        }))
    }
}

/// What a clip's span reads.
struct SpanSource {
    /// The file, or the nested sequence's name, for errors.
    path: PathBuf,
    /// The window of the source the clip covers.
    window: TimeRange,
    /// The source's length in output frames.
    len: i64,
    /// A nested sequence's mix; a file is opened on first overlap instead.
    reader: Option<Box<dyn AudioReader>>,
    /// [`Span::file_len`].
    file_len: Option<i64>,
}

/// The sound `clip` plays: its recorded sound ([`Project::clip_sound`]), or for
/// a nested sequence clip that sequence's mix. `None` for a clip with nothing
/// to hear.
fn clip_source(
    project: &Project,
    clip: &Clip,
    nesting: &mut Vec<SequenceId>,
) -> Result<Option<SpanSource>, RenderError> {
    let Some(sequence) = clip.sequence() else {
        return Ok(project.clip_sound(clip).map(|(media, window)| {
            let len = ticks_to_samples(
                media.duration.value,
                media.duration.rate.num,
                media.duration.rate.den,
            );
            SpanSource {
                path: media.path().to_path_buf(),
                window,
                len,
                reader: None,
                file_len: Some(len),
            }
        }));
    };
    if nesting.contains(&sequence) {
        return Err(RenderError::Model(ModelError::SequenceCycle(sequence)));
    }
    let (Some(inner), Some(window)) = (project.sequence(sequence), clip.source_range()) else {
        return Ok(None);
    };
    nesting.push(sequence);
    let mixed = ExportAudioMixer::for_timeline(project, &inner.timeline, nesting);
    nesting.pop();
    let Some(mixer) = mixed? else {
        return Ok(None);
    };
    let duration = inner.timeline.duration();
    let mut reader: Box<dyn AudioReader> = Box::new(NestReader { mixer, pos: 0 });
    if clip.denoise {
        reader = Box::new(DenoiseReader::new(reader, CHANNELS));
    }
    Ok(Some(SpanSource {
        path: PathBuf::from(&inner.name),
        window,
        len: ticks_to_samples(duration.value, duration.rate.num, duration.rate.den),
        reader: Some(reader),
        file_len: None,
    }))
}

/// A nested sequence's mix read as a source: frame `n` is the sequence's own
/// output frame `n`, silence wherever none of its clips sound.
struct NestReader {
    mixer: ExportAudioMixer,
    pos: i64,
}

impl AudioReader for NestReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let frames = out.len() / CHANNELS;
        self.mixer
            .mix_into(self.pos, &mut out[..frames * CHANNELS])?;
        self.pos += frames as i64;
        Ok(frames)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        if frame != self.pos {
            // A span whose source ran out further along still sounds before
            // that point, so a seek elsewhere re-arms it.
            for span in &mut self.mixer.spans {
                span.exhausted = false;
            }
            self.pos = frame;
        }
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        Some(self.pos)
    }

    fn set_realtime(&mut self, realtime: bool) {
        self.mixer.set_realtime(realtime);
    }
}

#[cfg(test)]
mod tests {
    use aria_models::Param;

    use super::*;

    /// Two hundred frames at half scale, then the end of the source.
    struct Short {
        pos: i64,
    }

    impl AudioReader for Short {
        fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
            let frames = (out.len() / CHANNELS).min((200 - self.pos).max(0) as usize);
            out[..frames * CHANNELS].fill(0.5);
            self.pos += frames as i64;
            Ok(frames)
        }

        fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
            self.pos = frame;
            Ok(())
        }

        fn position(&self) -> Option<i64> {
            Some(self.pos)
        }
    }

    /// A reversed nest clip reads its sequence backward, one seek per block.
    /// A nested span whose source ran out on a later block has to sound again
    /// on an earlier one, or the nest goes silent from the first block that
    /// reached the end of a file.
    #[test]
    fn a_nested_span_that_ran_out_sounds_again_after_a_seek_back() {
        let span = Span::linear(
            0,
            400,
            0,
            Param::Constant(1.0),
            Param::Constant(0.0),
            0,
            0,
            0,
            Some(Box::new(Short { pos: 0 })),
        );
        let mut nest = NestReader {
            mixer: ExportAudioMixer::of_spans(vec![span]),
            pos: 300,
        };
        let mut block = vec![0.0; 100 * CHANNELS];
        assert_eq!(nest.read(&mut block).expect("past the source"), 100);
        assert!(
            block.iter().all(|s| *s == 0.0),
            "past the source is silence"
        );

        nest.seek_to_frame(0).expect("back to the start");
        assert_eq!(nest.read(&mut block).expect("the source"), 100);
        assert_eq!(nest.position(), Some(100));
        assert!(
            block.iter().all(|s| *s > 0.1),
            "the source sounds again after a seek back"
        );
    }
}
