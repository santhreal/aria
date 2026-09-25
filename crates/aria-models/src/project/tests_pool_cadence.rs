//! Which pool entries carry a frame cadence.
//!
//! Closes defect class: a source whose `frame_rate` field is a tick base rather
//! than a shot cadence being read as footage rate. A still uses
//! [`STILL_TICK_RATE`] (1000/1) so its duration math stays exact, and an
//! audio-only file is stored the same way; either one read as a cadence claims
//! the project holds 1000 fps footage, and the settings dialog then reports a
//! mismatch against footage that has no cadence at all.
//!
//! Honest limit: model level. The dialog's wording for a real mismatch is
//! asserted in `aria-ui`.

use super::*;
use crate::media::{MediaKind, STILL_TICK_RATE};

/// One pool entry per [`MediaKind`], with the cadence that kind should report.
///
/// The match is exhaustive on purpose: a new kind does not compile until
/// someone states here whether it carries a cadence, so the answer can never
/// default to "whatever `frame_rate` happens to hold".
fn sample_of(kind: MediaKind) -> (MediaSource, Option<Rational>) {
    match kind {
        MediaKind::Video => (
            MediaSource::new("/tmp/shot.mp4", 1920, 1080, R24, 48, true),
            Some(R24),
        ),
        // Probing reports an audio-only file with zero dimensions and
        // millisecond ticks.
        MediaKind::Audio => (
            MediaSource::new("/tmp/score.wav", 0, 0, STILL_TICK_RATE, 5_000, true),
            None,
        ),
        // Deliberately larger than the video above, so area alone would elect it.
        MediaKind::Image => (MediaSource::image("/tmp/card.png", 4000, 3000), None),
    }
}

const KINDS: [MediaKind; 3] = [MediaKind::Video, MediaKind::Audio, MediaKind::Image];

#[test]
fn only_a_video_source_reports_a_cadence() {
    for kind in KINDS {
        let (source, expected) = sample_of(kind);
        assert_eq!(source.kind(), kind, "fixture is not the kind it claims");

        let mut project = Project::new("cadence", R24);
        project.add_media(source);

        assert_eq!(
            project.largest_source_frame_rate(),
            expected,
            "{kind:?} reported the wrong pool cadence"
        );
    }
}

#[test]
fn a_pool_without_footage_never_reports_a_mismatch() {
    for kind in KINDS {
        let (source, expected) = sample_of(kind);
        if expected.is_some() {
            continue;
        }
        for rate in [R24, R30, Rational::new(1000, 1)] {
            let mut project = Project::new("cadence", rate);
            project.add_media(source.to_owned());
            assert_eq!(
                project.frame_rate_mismatch(),
                None,
                "{kind:?} claimed a cadence mismatch at {rate:?}"
            );
        }
    }
}

#[test]
fn a_larger_still_does_not_outvote_the_footage_it_sits_beside() {
    let mut project = Project::new("cadence", R30);
    let (still, _) = sample_of(MediaKind::Image);
    let (video, _) = sample_of(MediaKind::Video);
    project.add_media(still);
    project.add_media(video);

    assert_eq!(
        project.largest_source_frame_rate(),
        Some(R24),
        "the 4000x3000 still won the area comparison against the footage"
    );
    assert_eq!(
        project.frame_rate_mismatch(),
        Some(R24),
        "a 30fps sequence over 24fps footage is a mismatch"
    );
}

#[test]
fn footage_at_the_sequence_rate_is_not_a_mismatch() {
    let mut project = Project::new("cadence", R24);
    let (video, _) = sample_of(MediaKind::Video);
    project.add_media(video);
    assert_eq!(project.frame_rate_mismatch(), None);
}
