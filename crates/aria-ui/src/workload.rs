//! Synthetic sequences for measuring what a frame costs.
//!
//! The editor's per-frame CPU work outside the GPU is: project the project
//! into the view model, build the magnet index from the result, and query that
//! index once per pointer move. Measuring those needs a sequence big enough to
//! be interesting, and both the bench (`bench_hot_paths.rs`) and the budget
//! test read it from here, so a recorded number and a gated number describe
//! the same work. A workload built twice would drift and the comparison would
//! be worthless.
//!
//! `pub` because a bench is a separate crate. Nothing in the editor calls it:
//! this builds a project nobody edited, which is only ever a measurement.

use aria_models::{
    Marker, MarkerColor, MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind,
};

/// A sequence shaped like a real edit: interleaved video and audio lanes, one
/// media source per lane, clips laid end to end with beats on each, and a
/// marker every few seconds.
///
/// `lanes` counts video/audio pairs, so `sequence(4, 60)` is 8 tracks and 480
/// clips — larger than most timelines and small enough to project in a frame.
pub fn sequence(lanes: usize, clips_per_lane: usize) -> Project {
    let rate = Rational::FPS_24;
    let mut project = Project::new("Measurement", rate);
    // 2s clips, so a 60-clip lane is two minutes of sequence.
    let length = 48;
    for lane in 0..lanes {
        for kind in [TrackKind::Video, TrackKind::Audio] {
            let name = format!("{kind:?}{lane}");
            let track = project.add_track(kind, name);
            let media = project.add_media(MediaSource::new(
                format!("lane{lane}.mp4"),
                1920,
                1080,
                rate,
                (clips_per_lane as i64 + 1) * length,
                kind.is_audio(),
            ));
            for index in 0..clips_per_lane {
                let at = (index as i64) * length;
                let source = TimeRange::at_rate(at, length, rate);
                let clip = project
                    .add_clip(track, media, source, RationalTime::new(at, rate))
                    .expect("a clip laid end to end on an empty lane cannot overlap");
                // Beats are the largest part of a magnet set on a real music
                // edit: eight per clip is a bar at this length.
                if let Some(clip) = project.timeline_mut().clip_mut(clip) {
                    clip.beats = (0..8).map(|beat| at + beat * (length / 8)).collect();
                }
            }
        }
    }
    for index in 0..(clips_per_lane / 4).max(1) {
        let tick = RationalTime::new((index as i64) * length * 4, rate);
        project
            .timeline_mut()
            .add_marker(Marker::new(tick, format!("m{index}"), MarkerColor::Blue))
            .expect("a marker inside the sequence is accepted");
    }
    project
}

/// The workload the recorded numbers describe: 4 lane pairs, 60 clips each.
///
/// Both the bench and the budget test use this, so "projection" means one
/// thing. 480 clips, 3840 beats, 15 markers.
pub fn standard() -> Project {
    sequence(4, 60)
}
