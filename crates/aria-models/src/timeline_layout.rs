//! The placement rules a timeline read from a file holds before an edit or a
//! render reads it.
//!
//! [`Timeline::add_clip`] keeps these rules for every placement, and each
//! clip setter keeps the rule of the value it stores. A file reaches the model
//! through the deserializer instead, so the loader rebuilds what the file
//! states twice ([`Timeline::rederive_order_and_index`]), then reads
//! [`Timeline::layout_fault`] once per sequence and refuses a file that breaks
//! a rule. Overlapping clips are not refused: an edit separates them, and a
//! refusal would leave the project unopenable.

use rustc_hash::FxHashSet;

use crate::CanvasSettings;
use crate::clip::Clip;
use crate::ids::TrackId;
use crate::time::{Rational, TimeRange, check_same_rate};
use crate::track::Track;

use super::Timeline;

impl Timeline {
    /// Re-derive what a file states twice, from its lanes. The lane order keeps
    /// each lane once, at its first place, drops an id no lane has, and appends
    /// the lanes it leaves out, in id order; the clip index is rebuilt from the
    /// lanes, in that order. A lane left out of the order is drawn and rendered
    /// nowhere, and a clip the index lacks is found by no edit.
    pub(super) fn rederive_order_and_index(&mut self) {
        let tracks = &self.tracks;
        let mut seen = FxHashSet::default();
        self.order
            .retain(|id| tracks.contains_key(id) && seen.insert(*id));
        if self.order.len() < tracks.len() {
            let mut left_out: Vec<TrackId> = tracks
                .keys()
                .copied()
                .filter(|id| !seen.contains(id))
                .collect();
            left_out.sort_unstable();
            self.order.extend(left_out);
        }
        self.clip_index.clear();
        for id in &self.order {
            for clip in self.tracks[id].clips() {
                self.clip_index.insert(clip.id, *id);
            }
        }
    }

    /// The first placement rule this timeline breaks, as the field under the
    /// timeline that holds the fault (`frame_rate`, `canvas`, a master strip
    /// field, or `tracks`) and a sentence stating it, or `None` when the
    /// sequence rate is valid, an explicit canvas size is one the canvas edit
    /// accepts ([`CanvasSettings::resolution_is_valid`]), the master strip and
    /// every lane hold mix values their setters accept
    /// ([`Track::check_mix_values`]), and every clip is timed at the rate,
    /// with ranges and a speed [`Timeline::add_clip`] and the speed setter
    /// accept and values its setters accept ([`Clip::check_values`]).
    pub(crate) fn layout_fault(&self) -> Option<(&'static str, String)> {
        let rate = self.frame_rate;
        if !rate.is_valid() {
            return Some((
                "frame_rate",
                format!(
                    "the sequence runs at {}, and a rate needs a numerator and denominator above zero",
                    shown(rate)
                ),
            ));
        }
        if let Some([width, height]) = self.canvas.resolution
            && !CanvasSettings::resolution_is_valid([width, height])
        {
            return Some((
                "canvas",
                format!(
                    "the canvas is {width}x{height} pixels, and each edge needs to be even and \
                     from {} to {} pixels",
                    CanvasSettings::MIN_DIM,
                    CanvasSettings::MAX_DIM
                ),
            ));
        }
        if let Err((field, err)) = self.check_master_mix_values() {
            return Some((
                field,
                format!("the master strip holds a value an edit rejects: {err}"),
            ));
        }
        self.tracks_ordered().find_map(|track| {
            let lane = track
                .check_mix_values()
                .err()
                .map(|err| format!("lane '{}' holds a value an edit rejects: {err}", track.name));
            lane.or_else(|| {
                track.clips_ordered().find_map(|clip| {
                    self.shared_id_fault(track, clip)
                        .or_else(|| clip_fault(track, clip, rate))
                })
            })
            .map(|reason| ("tracks", reason))
        })
    }

    /// A clip whose id a clip on another lane also has. The index holds one
    /// lane per id, the last in lane order, so the earlier lane's clip is the
    /// one it does not name.
    fn shared_id_fault(&self, track: &Track, clip: &Clip) -> Option<String> {
        let other = self
            .clip_index
            .get(&clip.id)
            .filter(|lane| **lane != track.id)?;
        let other = self.tracks.get(other).map_or("", |lane| lane.name.as_str());
        Some(format!(
            "clip {} is on lane '{}' and on lane '{other}'",
            clip.id.raw(),
            track.name
        ))
    }
}

fn clip_fault(track: &Track, clip: &Clip, rate: Rational) -> Option<String> {
    let which = || format!("clip {} on lane '{}'", clip.id.raw(), track.name);
    let span = clip.timeline;
    for timed in [span.start.rate, span.duration.rate] {
        if check_same_rate(timed, rate).is_err() {
            return Some(format!(
                "{} is timed at {}, and the sequence runs at {}",
                which(),
                shown(timed),
                shown(rate)
            ));
        }
    }
    if let Some(fault) = range_fault(span) {
        return Some(format!("{} {fault} on the timeline", which()));
    }
    if let Some(fault) = clip.source_range().and_then(range_fault) {
        return Some(format!("{} {fault} in its source", which()));
    }
    if !clip.speed.is_valid() {
        return Some(format!(
            "{} plays at speed {}, and a speed needs a numerator and denominator above zero",
            which(),
            shown(clip.speed)
        ));
    }
    clip.check_values()
        .err()
        .map(|err| format!("{} holds a value an edit rejects: {err}", which()))
}

fn range_fault(range: TimeRange) -> Option<&'static str> {
    if range.start.value < 0 || range.duration.value < 0 {
        Some("has a negative start or length")
    } else if range.end().is_err() {
        Some("ends past the last tick a time holds")
    } else {
        None
    }
}

fn shown(rate: Rational) -> String {
    format!("{}/{}", rate.num, rate.den)
}
