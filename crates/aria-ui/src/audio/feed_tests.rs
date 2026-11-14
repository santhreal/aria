use crossbeam_channel::bounded;

use super::*;

#[test]
fn consumed_mix_blocks_return_to_the_feeder_pool() {
    let (tx, rx) = bounded(1);
    let (recycle_tx, recycle_rx) = bounded(1);
    let block = vec![0.25, -0.25, 0.5, -0.5];
    let allocation = block.as_ptr();
    tx.send(block).expect("queue block");
    let mut pending = Vec::new();
    let mut pending_at = 0;
    let mut output = [0.0; 4];

    fill_output(
        &mut output,
        2,
        &rx,
        &mut pending,
        &mut pending_at,
        &recycle_tx,
    );
    assert_eq!(output, [0.25, -0.25, 0.5, -0.5]);

    fill_output(
        &mut output,
        2,
        &rx,
        &mut pending,
        &mut pending_at,
        &recycle_tx,
    );
    let returned = recycle_rx.try_recv().expect("block returned");
    assert_eq!(
        returned.as_ptr(),
        allocation,
        "the callback allocated a replacement block"
    );
}

#[test]
fn fill_output_empty_receiver_underflow_zero_fills() {
    let (_tx, rx) = bounded(1);
    let (recycle_tx, _recycle_rx) = bounded(1);
    let mut pending = Vec::new();
    let mut pending_at = 0;
    let mut output = [1.0f32; 8];

    fill_output(
        &mut output,
        2,
        &rx,
        &mut pending,
        &mut pending_at,
        &recycle_tx,
    );
    assert_eq!(output, [0.0; 8]);
}

#[test]
fn fill_output_mono_and_multichannel_alignment() {
    let (tx, rx) = bounded(1);
    let (recycle_tx, _recycle_rx) = bounded(1);
    let block = vec![0.3, -0.4, 0.7, -0.8]; // 2 stereo frames: (0.3, -0.4) and (0.7, -0.8)
    tx.send(block).expect("send block");
    let mut pending = Vec::new();
    let mut pending_at = 0;

    // Mono output (1 channel): reads left channel of each frame
    let mut mono_out = [0.0f32; 2];
    fill_output(
        &mut mono_out,
        1,
        &rx,
        &mut pending,
        &mut pending_at,
        &recycle_tx,
    );
    assert_eq!(mono_out, [0.3, 0.7]);

    // 6-channel output (5.1): maps L/R to first two channels, zeros remaining
    let (tx6, rx6) = bounded(1);
    tx6.send(vec![0.5, -0.5]).expect("send stereo frame");
    let mut pending6 = Vec::new();
    let mut pending_at6 = 0;
    let mut out6 = [9.0f32; 6];
    fill_output(
        &mut out6,
        6,
        &rx6,
        &mut pending6,
        &mut pending_at6,
        &recycle_tx,
    );
    assert_eq!(out6, [0.5, -0.5, 0.0, 0.0, 0.0, 0.0]);
}

/// A source whose every stereo frame is `0.5`, so a scrub grain mixed from
/// it publishes a known non-zero post-fader peak on its lane.
struct Tone {
    pos: i64,
}

impl aria_core::AudioReader for Tone {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, aria_core::DecodeError> {
        out.fill(0.5);
        let frames = out.len() / usize::from(EXPORT_AUDIO_CHANNELS);
        self.pos += frames as i64;
        Ok(frames)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), aria_core::DecodeError> {
        self.pos = frame;
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        Some(self.pos)
    }
}

/// Closes the class "a scrub stream that stays open between pointer moves
/// leaves the last grain's lane peaks in `LaneMeters` after the grain has
/// finished playing, so mixer strips decay toward a stale level forever".
/// Not caught here: the repaint clock's ballistics in `app::meter`.
#[test]
fn scrub_grain_publishes_lane_peaks_then_silences_after_grain_duration() {
    use aria_models::{Param, TrackId};
    use aria_render::export_audio_bench::{ExportAudioMixer, Span};

    let span = Span::linear(
        0,
        i64::from(EXPORT_AUDIO_RATE),
        0,
        Param::Constant(1.0),
        Param::Constant(0.0),
        0,
        0,
        0,
        Some(Box::new(Tone { pos: 0 })),
    );
    let mixer = ExportAudioMixer::of_spans(vec![span]);
    let lane = TrackId::from_raw(0);
    let meters = Arc::new(LaneMeters::for_lanes(std::iter::once(lane)));
    let slot = Arc::new(Mutex::new(Some((0_i64, 24, 1))));
    let (tx, rx) = bounded(2);
    let (_recycle_tx, recycle_rx) = bounded(2);
    let stop = Arc::new(AtomicBool::new(false));

    let handle = {
        let slot = Arc::clone(&slot);
        let stop = Arc::clone(&stop);
        let meters = Arc::clone(&meters);
        thread::spawn(move || {
            feed_mixer(mixer, 0, 1, 1, Some(slot), tx, recycle_rx, stop, meters);
        })
    };

    let grain = rx
        .recv_timeout(Duration::from_millis(500))
        .expect("the scrub loop delivers the grain within 500 ms");
    assert!(!grain.is_empty());
    let published_at = Instant::now();
    let peak = meters.peak(lane);
    assert!(
        peak[0] > 0.0 && peak[1] > 0.0,
        "the lane meter reads the grain's level while the grain is heard, got {peak:?}"
    );

    let bound = Duration::from_millis(500);
    while meters.peak(lane) != [0.0, 0.0] && published_at.elapsed() < bound {
        thread::sleep(Duration::from_millis(4));
    }
    let elapsed = published_at.elapsed();
    stop.store(true, Ordering::Release);
    handle.join().expect("the scrub thread exits cleanly");

    assert_eq!(
        meters.peak(lane),
        [0.0, 0.0],
        "the lane meter must fall to silence after the scrub grain finishes"
    );
    assert!(
        elapsed >= Duration::from_millis(u64::from(SCRUB_GRAIN_MS) / 2),
        "the lane meter silenced too early ({elapsed:?}), before the {SCRUB_GRAIN_MS} ms grain was heard"
    );
    assert!(
        elapsed < bound,
        "the lane meter did not silence within the {bound:?} bound (took {elapsed:?})"
    );
}

/// Closes the class "an audio open that fails after its mix thread started
/// leaves the thread running": nothing could stop it, a scrub thread waited
/// for grains for the life of the process, and the next tear-down joined it
/// forever, which hung the device thread and every window close behind it.
/// Every `?` past the spawn in `AudioSystem::start` drops the [`MixThread`];
/// this drops one over a scrub thread whose stream never opened. Not caught:
/// a path of `start` that takes the thread out of the guard before it fails.
#[test]
fn a_mix_thread_dropped_before_its_stream_opened_stops_and_joins() {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = bounded(2);
    let (_recycle_tx, recycle_rx) = bounded(2);
    let slot = Arc::new(Mutex::new(None));
    let meters = Arc::new(LaneMeters::for_lanes(std::iter::empty::<
        aria_models::TrackId,
    >()));
    let mixer = ExportAudioMixer::of_spans(Vec::new());
    let run_stop = Arc::clone(&stop);
    let thread = thread::spawn(move || {
        feed_mixer(mixer, 0, 1, 1, Some(slot), tx, recycle_rx, run_stop, meters);
    });
    let mix = MixThread::new(thread, Arc::clone(&stop));
    // The stream that would have read the grains failed to build.
    drop(rx);
    let (dropped, wait) = bounded(1);
    thread::spawn(move || {
        drop(mix);
        let _ = dropped.send(());
    });
    wait.recv_timeout(Duration::from_secs(2))
        .expect("dropping the mix thread stops and joins it within 2 s");
    assert!(stop.load(Ordering::Acquire), "the drop set the stop flag");
}

/// A kept mix thread is the stream's: the guard no longer stops it, and
/// `keep` hands back the flag that does.
#[test]
fn a_kept_mix_thread_runs_until_its_tear_down() {
    let stop = Arc::new(AtomicBool::new(false));
    let run_stop = Arc::clone(&stop);
    let thread = thread::spawn(move || {
        while !run_stop.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
    });
    let (kept_stop, thread) = MixThread::new(thread, Arc::clone(&stop)).keep();
    let (kept_stop, thread) = (
        kept_stop.expect("the stop flag"),
        thread.expect("the thread"),
    );
    thread::sleep(Duration::from_millis(20));
    assert!(!stop.load(Ordering::Acquire), "keep set the stop flag");
    assert!(!thread.is_finished(), "the kept thread ended on its own");
    kept_stop.store(true, Ordering::Release);
    thread.join().expect("the kept thread exits");
}
