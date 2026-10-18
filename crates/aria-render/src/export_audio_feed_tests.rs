//! The audio feed delivers every frame's mix in order and ends with the export.
//!
//! The class closed here is a mix thread out of step with the frame loop: a
//! block delivered for the wrong frame, a thread that keeps mixing after the
//! loop stopped reading (which holds the export's scope open until the whole
//! timeline is mixed), and a thread that mixes past a block that failed. Not
//! caught: a mix that is slower on its own thread than on the loop's.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use aria_core::{AudioReader, DecodeError};
use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};

use super::*;

/// A source whose every sample is its own frame position, so a block mixed
/// for the wrong frame differs, and which counts its reads and can fail one.
struct Ramp {
    pos: i64,
    reads: Arc<AtomicUsize>,
    fail_at: Option<usize>,
}

impl AudioReader for Ramp {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let call = self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_at == Some(call) {
            return Err(DecodeError::Decode("the source broke".into()));
        }
        let channels = EXPORT_AUDIO_CHANNELS as usize;
        for (k, frame) in out.chunks_exact_mut(channels).enumerate() {
            frame.fill((self.pos + k as i64) as f32 * 1e-5);
        }
        let frames = out.len() / channels;
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

const RATE: Rational = Rational::FPS_24;

/// The mixer of one audio lane holding one `frames`-long clip read from a
/// [`Ramp`].
fn mixer(frames: i64, reads: &Arc<AtomicUsize>, fail_at: Option<usize>) -> ExportAudioMixer {
    let mut project = Project::new("feed", RATE);
    let a1 = project.add_track(TrackKind::Audio, "A1");
    let media = project.add_media(MediaSource::new("/dev/null", 0, 0, RATE, frames, true));
    project
        .add_clip(
            a1,
            media,
            TimeRange::at_rate(0, frames, RATE),
            RationalTime::new(0, RATE),
        )
        .expect("place the clip");
    let mut mixer = ExportAudioMixer::for_project(&project)
        .expect("the routing folds")
        .expect("the lane is audible");
    for span in &mut mixer.spans {
        span.reader = Some(Box::new(Ramp {
            pos: 0,
            reads: reads.clone(),
            fail_at,
        }));
    }
    mixer
}

/// Runs `body` on a thread of its own and fails when it does not return
/// within ten seconds, so a feed that never ends fails rather than hangs.
fn within_ten_seconds(body: impl FnOnce() + Send + 'static) {
    let (done, ended) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        body();
        let _ = done.send(());
    });
    assert!(
        ended.recv_timeout(Duration::from_secs(10)).is_ok(),
        "the feed's scope did not end within ten seconds"
    );
}

#[test]
fn every_block_is_the_mix_of_its_own_frame() {
    const FRAMES: i64 = 40;
    let (fed, in_place) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mut reference = mixer(FRAMES, &in_place, None);
    std::thread::scope(|scope| {
        let feed = AudioFeed::start(scope, mixer(FRAMES, &fed, None), 0, FRAMES as u64, RATE)
            .expect("the mix thread starts");
        for index in 0..FRAMES {
            let block = feed.next().expect("a mixed block");
            let from = sample_boundary(index, RATE.num, RATE.den);
            let to = sample_boundary(index + 1, RATE.num, RATE.den);
            let mut want = vec![0.0; (to - from) as usize * EXPORT_AUDIO_CHANNELS as usize];
            reference.mix_into(from, &mut want).expect("mix in place");
            assert_eq!(block, want, "frame {index}");
            feed.recycle(block);
        }
        assert!(feed.next().is_err(), "a block past the last frame");
    });
}

#[test]
fn a_dropped_feed_stops_mixing_within_its_lead() {
    const FRAMES: i64 = 100_000;
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = reads.clone();
    within_ten_seconds(move || {
        let mixer = mixer(FRAMES, &counted, None);
        std::thread::scope(|scope| {
            let feed = AudioFeed::start(scope, mixer, 0, FRAMES as u64, RATE)
                .expect("the mix thread starts");
            feed.next().expect("the first block");
        });
    });
    // One block taken, AHEAD queued, one mixed and waiting to be queued, one
    // more mixed before the thread saw the feed was gone.
    let reads = reads.load(Ordering::SeqCst);
    assert!(
        reads <= AHEAD + 3,
        "{reads} blocks mixed for an export that took one"
    );
}

#[test]
fn a_block_that_fails_to_mix_is_the_last_one() {
    const FRAMES: i64 = 40;
    const FAILS: usize = 5;
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = reads.clone();
    within_ten_seconds(move || {
        let mixer = mixer(FRAMES, &counted, Some(FAILS));
        std::thread::scope(|scope| {
            let feed = AudioFeed::start(scope, mixer, 0, FRAMES as u64, RATE)
                .expect("the mix thread starts");
            for index in 0..FAILS {
                feed.next()
                    .unwrap_or_else(|err| panic!("block {index} before the failure: {err}"));
            }
            let failed = feed.next().expect_err("the failing block");
            assert!(matches!(failed, RenderError::Decode(_)), "{failed}");
            assert!(feed.next().is_err(), "a block mixed after the failure");
        });
    });
    assert_eq!(
        reads.load(Ordering::SeqCst),
        FAILS + 1,
        "reads after the failing block"
    );
}
