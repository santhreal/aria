//! The references a project read from a file holds before an edit or a render
//! reads it: every clip plays a media, sequence or multicam group the project
//! holds, no sequence contains itself, and every multicam group is valid and
//! plays media the project holds.
//!
//! The edit path keeps each of these ([`Project::add_clip`],
//! [`Project::remove_media`], the multicam group constructors, and the nested
//! sequence cycle check). A file reaches the model through the deserializer
//! instead, so the loader reads these faults once and refuses a file that
//! holds one.

use crate::clip::{Clip, ClipSource};
use crate::ids::SequenceId;
use crate::timeline::Timeline;
use crate::track::Track;

use super::Project;

impl Project {
    /// The first clip on `timeline`, in lane order, that plays a media, a
    /// sequence or a multicam group the project does not hold, or a sequence
    /// that contains `holder`, as a sentence stating it. `holder` is the
    /// sequence `timeline` belongs to, or `None` for the root timeline.
    pub(crate) fn reference_fault(
        &self,
        timeline: &Timeline,
        holder: Option<SequenceId>,
    ) -> Option<String> {
        timeline.tracks_ordered().find_map(|track| {
            track
                .clips_ordered()
                .find_map(|clip| self.clip_reference_fault(track, clip, holder))
        })
    }

    fn clip_reference_fault(
        &self,
        track: &Track,
        clip: &Clip,
        holder: Option<SequenceId>,
    ) -> Option<String> {
        let (kind, raw, held) = match &clip.content {
            ClipSource::Generated(_) => return None,
            ClipSource::Media { media, .. } => {
                ("media", media.raw(), self.media.contains_key(media))
            }
            ClipSource::Multicam { group, .. } => (
                "multicam group",
                group.raw(),
                self.multicam_groups.contains_key(group),
            ),
            ClipSource::Sequence { sequence, .. } => match self.sequences.get(sequence) {
                Some(nested) if self.would_create_sequence_cycle(holder, *sequence) => {
                    return Some(format!(
                        "clip {} on lane '{}' plays sequence '{}', and sequence '{}' contains this sequence",
                        clip.id.raw(),
                        track.name,
                        nested.name,
                        nested.name
                    ));
                }
                held => ("sequence", sequence.raw(), held.is_some()),
            },
        };
        (!held).then(|| {
            format!(
                "clip {} on lane '{}' plays {kind} {raw}, and the project holds no {kind} {raw}",
                clip.id.raw(),
                track.name
            )
        })
    }

    /// The first multicam group, in id order, that breaks a rule
    /// [`crate::multicam::MulticamGroup::validate`] keeps or plays a media the
    /// project does not hold, as a sentence stating it.
    pub(crate) fn multicam_fault(&self) -> Option<String> {
        let mut groups: Vec<_> = self.multicam_groups.values().collect();
        groups.sort_unstable_by_key(|group| group.id);
        groups.into_iter().find_map(|group| {
            let which = || format!("multicam group {} '{}'", group.id.raw(), group.name);
            if let Err(err) = group.validate() {
                return Some(format!("{} is invalid: {err}", which()));
            }
            group
                .angles
                .iter()
                .position(|angle| !self.media.contains_key(&angle.media))
                .map(|index| {
                    let media = group.angles[index].media.raw();
                    format!(
                        "{} angle {} plays media {media}, and the project holds no media {media}",
                        which(),
                        index + 1
                    )
                })
        })
    }
}
