//! The audio host: the sound of a project for plugins.
//!
//! [`AudioHost`] mixes a sequence, or one clip of it, over a time range the way
//! export does: clip, lane and master inserts, sends, buses and faders, at
//! [`EXPORT_AUDIO_RATE`](aria_render::EXPORT_AUDIO_RATE) in interleaved stereo.
//! An analyzer (loudness, beats, silence) reads the blocks and writes what it
//! finds through [`ProjectHost`](crate::ProjectHost). A plugin takes the
//! project from [`ProjectHost::snapshot`](crate::ProjectHost::snapshot).

use std::borrow::Cow;
use std::ops::ControlFlow;

use aria_core::TimeRange;
use aria_models::{ClipId, ModelError, Project, SequenceId};
use aria_render::{EXPORT_AUDIO_CHANNELS, ExportAudioMixer, RenderError, sample_boundary};

use crate::render_host::sequence_holding;

/// Frames in each block a mix hands over, except a shorter last one.
pub const AUDIO_HOST_BLOCK: usize = 4096;

/// What a mix plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSource {
    /// The sequence open for editing.
    Open,
    /// The root sequence (`None`) or a nested sequence.
    Sequence(Option<SequenceId>),
    /// One clip of any sequence, with every other clip of that sequence
    /// removed: through its own inserts, its lane's inserts and fader, and the
    /// buses and master it reaches. The range is on that sequence.
    Clip(ClipId),
}

/// Mixes the sound of a project for plugins. Holds no state: every call opens
/// a mixer of its own, so calls on several threads run side by side.
#[derive(Debug, Default)]
pub struct AudioHost;

impl AudioHost {
    pub fn new() -> Self {
        Self
    }

    /// Mix `range` of `source` and hand each block to `each`, in order, until
    /// it breaks. `each` receives the sample frame the block starts at,
    /// counted from the start of the sequence, and the block. A range with
    /// nothing audible arrives as silence.
    pub fn mix(
        &self,
        project: &Project,
        source: AudioSource,
        range: TimeRange,
        mut each: impl FnMut(i64, &[f32]) -> ControlFlow<()>,
    ) -> Result<(), RenderError> {
        let project = target(project, source)?;
        let rate = range.start.rate;
        let from = sample_boundary(range.start.value, rate.num, rate.den);
        let to = sample_boundary(range.end_tick(), rate.num, rate.den);
        let mut mixer = ExportAudioMixer::for_project(&project)?;
        let channels = usize::from(EXPORT_AUDIO_CHANNELS);
        let mut buffer = vec![0.0f32; AUDIO_HOST_BLOCK * channels];
        let mut at = from;
        while at < to {
            let frames = (to - at).min(AUDIO_HOST_BLOCK as i64) as usize;
            let block = &mut buffer[..frames * channels];
            block.fill(0.0);
            if let Some(mixer) = &mut mixer {
                mixer.mix_into(at, block)?;
            }
            if each(at, block).is_break() {
                break;
            }
            at += frames as i64;
        }
        Ok(())
    }
}

/// The project as `source` plays it: with its sequence open and, for a clip,
/// every other clip of that sequence removed.
fn target(project: &Project, source: AudioSource) -> Result<Cow<'_, Project>, ModelError> {
    let (sequence, keep) = match source {
        AudioSource::Open => (project.active_sequence(), None),
        AudioSource::Sequence(sequence) => (sequence, None),
        AudioSource::Clip(clip) => (sequence_holding(project, clip)?, Some(clip)),
    };
    if sequence == project.active_sequence() && keep.is_none() {
        return Ok(Cow::Borrowed(project));
    }
    let mut opened = project.clone();
    if sequence != opened.active_sequence() {
        opened.set_active_sequence(sequence)?;
    }
    if let Some(keep) = keep {
        let others: Vec<ClipId> = opened
            .timeline()
            .tracks_ordered()
            .flat_map(|track| track.clips().map(|clip| clip.id))
            .filter(|clip| *clip != keep)
            .collect();
        let timeline = opened.timeline_mut();
        for clip in others {
            timeline.remove_clip(clip);
        }
    }
    Ok(Cow::Owned(opened))
}
