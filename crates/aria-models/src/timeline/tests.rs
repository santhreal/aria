use super::*;
use crate::clip::{Clip, Generator};
use crate::time::TimeRange;
use crate::track::{Track, TrackKind};

const R24: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, R24)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, R24)
}

fn generated_clip(start: i64, duration: i64) -> Clip {
    Clip::generated(Generator::Adjustment, tr(start, duration))
}

fn timeline_with_track() -> (Timeline, TrackId) {
    let mut timeline = Timeline::new(R24);
    let track = timeline.add_track(Track::new(TrackKind::Adjustment, "FX"));
    (timeline, track)
}

#[path = "tests_canvas.rs"]
mod canvas;
#[path = "tests_clips.rs"]
mod clips;
#[path = "tests_markers.rs"]
mod markers;
#[path = "tests_tracks.rs"]
mod tracks;
