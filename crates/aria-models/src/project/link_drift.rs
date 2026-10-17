//! How far a linked clip has slipped from a partner placing the same file.

use std::collections::HashMap;

use crate::clip::{Clip, ClipSource};
use crate::ids::{ClipId, LinkId, MediaId};
use crate::project::Project;
use crate::time::Rational;

impl Project {
    /// The frames each linked clip sits away from the first other member of
    /// its link that places the same file, keyed by clip. Positive when the
    /// clip plays a given frame of the file later on the timeline than its
    /// partner does. Only clips with a nonzero offset are listed.
    ///
    /// A retimed clip (a speed other than 100 %, a speed ramp, reversed, or a
    /// freeze frame) maps the file to the timeline by more than one offset, so
    /// it is neither measured nor measured against.
    pub fn link_drifts(&self) -> HashMap<ClipId, i64> {
        let timeline = self.timeline();
        let rate = timeline.frame_rate;
        let mut members: HashMap<(LinkId, MediaId), Vec<(ClipId, i64)>> = HashMap::new();
        for clip in timeline
            .tracks_ordered()
            .flat_map(|track| track.clips_ordered())
        {
            let (Some(link), Some((media, anchor))) = (clip.link, sync_anchor(clip, rate)) else {
                continue;
            };
            members
                .entry((link, media))
                .or_default()
                .push((clip.id, anchor));
        }
        let mut drifts = HashMap::new();
        for group in members.values() {
            for (index, &(clip, anchor)) in group.iter().enumerate() {
                let partner = group
                    .iter()
                    .enumerate()
                    .find(|&(other, _)| other != index)
                    .map(|(_, &(_, partner))| partner);
                if let Some(partner) = partner
                    && anchor != partner
                {
                    drifts.insert(clip, anchor - partner);
                }
            }
        }
        drifts
    }
}

/// The timeline tick at which `clip` would place its file's first frame, for
/// a media clip that plays its file at one fixed offset.
fn sync_anchor(clip: &Clip, rate: Rational) -> Option<(MediaId, i64)> {
    let ClipSource::Media { media, source } = &clip.content else {
        return None;
    };
    let retimed = clip.reversed
        || clip.freeze_frame
        || clip.has_speed_curve()
        || clip.speed.num != clip.speed.den;
    if retimed {
        return None;
    }
    let source_start = crate::time::resample(source.start, rate).value;
    Some((*media, clip.timeline.start.value - source_start))
}
