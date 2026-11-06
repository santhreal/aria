//! A preview stream that does not start is posted, not only logged.
//!
//! The class closed here: playback whose sound could not start, for a missing
//! output device, a device that refuses its configuration, or a mix that could
//! not be prepared, played the picture on in silence and wrote the reason to the
//! log alone, so a sound that never came read as a silent clip. The pump that
//! tried a stream returns why it did not start, for a play and for a scrub
//! grain, and the shell posts it on the error line.
//!
//! These drive the failure through a mix that could not be prepared, which
//! fails before any device is opened, so they read the same on a machine with
//! speakers and on one with none.
//!
//! Not covered: the copy of each device failure, which is written where cpal
//! reports it, and a device that fails after its stream started.

use std::sync::Arc;

use parking_lot::Mutex;

use super::{AudioHandle, AudioSystem, PlaybackSnapshot, ScrubGrain};

/// A prepared mix slot holding `mix`.
type Prepared = Arc<Mutex<Option<Result<Option<aria_render::ExportAudioMixer>, String>>>>;

fn prepared(mix: Result<Option<aria_render::ExportAudioMixer>, String>) -> Prepared {
    Arc::new(Mutex::new(Some(mix)))
}

fn play(mix: Result<Option<aria_render::ExportAudioMixer>, String>) -> PlaybackSnapshot {
    PlaybackSnapshot {
        mixer: prepared(mix),
        start_tick: 0,
        fps_num: 24,
        fps_den: 1,
        speed_num: 1,
        speed_den: 1,
    }
}

/// The pump that tried a stream reports why it did not start, once: the next
/// pump has nothing new to start and reports nothing. A silent mix needs no
/// device and is no failure. A scrub grain while paused reports the same way.
#[test]
fn a_stream_that_does_not_start_is_reported_by_the_pump_that_tried_it() {
    let handle = AudioHandle::new();
    let mut system = AudioSystem::new(handle.clone());
    assert_eq!(system.pump(), None, "nothing was asked for");

    handle.publish_play(play(Err("the mix failed".into())));
    assert_eq!(system.pump().as_deref(), Some("the mix failed"));
    assert_eq!(
        handle.current_tick(),
        None,
        "a stream that did not start drives no clock"
    );
    assert_eq!(
        system.pump(),
        None,
        "the failure was reported by the pump that tried"
    );

    handle.publish_play(play(Ok(None)));
    assert_eq!(
        system.pump(),
        None,
        "a silent mix starts no stream and fails nothing"
    );

    handle.publish_stop();
    assert_eq!(system.pump(), None, "a stop starts nothing");
    handle.queue_scrub(ScrubGrain {
        mixer: Some(prepared(Err("the grain failed".into()))),
        revision: 0,
        tick: 3,
        fps_num: 24,
        fps_den: 1,
    });
    assert_eq!(system.pump().as_deref(), Some("the grain failed"));
    assert_eq!(
        system.pump(),
        None,
        "the grain was reported by the pump that tried"
    );
}

/// WHY: every scrub step built a mix from the project under the engine lock,
/// and the pump tore down the stream and opened the device again for each
/// grain on the UI thread; with no device each step probed the device and
/// posted the same failure again. A mix is built and tried once per project
/// revision, and an edit, a play or a stop tries again.
///
/// Not caught: a device plugged in during a scrub stays silent until one of
/// those.
#[test]
fn a_scrub_mix_is_tried_once_per_revision() {
    let handle = AudioHandle::new();
    let mut system = AudioSystem::new(handle.clone());
    let grain = |mixer: Option<Prepared>, revision, tick| ScrubGrain {
        mixer,
        revision,
        tick,
        fps_num: 24,
        fps_den: 1,
    };
    assert!(!handle.scrub_mix_ready(7), "nothing tried yet");

    handle.queue_scrub(grain(Some(prepared(Err("no device".into()))), 7, 1));
    assert!(handle.scrub_mix_ready(7), "the pending grain holds the mix");
    assert!(!handle.scrub_mix_ready(8), "a mix is one revision's");
    handle.queue_scrub(grain(None, 8, 2));
    assert!(
        !handle.scrub_mix_ready(8) && !handle.scrub_mix_ready(7),
        "a grain at a new revision drops the old mix"
    );
    handle.queue_scrub(grain(Some(prepared(Err("no device".into()))), 7, 3));
    assert_eq!(system.pump().as_deref(), Some("no device"));
    assert!(handle.scrub_mix_ready(7), "the failure is the tried mix");

    for tick in 4..54 {
        handle.queue_scrub(grain(None, 7, tick));
        assert_eq!(system.pump(), None, "grain {tick} posted the failure again");
    }
    assert!(!handle.scrub_mix_ready(8), "an edit tries again");

    handle.publish_play(play(Ok(None)));
    assert_eq!(system.pump(), None);
    assert!(!handle.scrub_mix_ready(7), "a play tries the device again");
    handle.publish_stop();
    assert_eq!(system.pump(), None);

    handle.queue_scrub(grain(Some(prepared(Ok(None))), 9, 1));
    assert_eq!(system.pump(), None, "a silent mix needs no device");
    assert!(handle.scrub_mix_ready(9));
    handle.queue_scrub(grain(None, 9, 2));
    assert_eq!(system.pump(), None);
    assert!(handle.scrub_mix_ready(9), "a later grain keeps the mix");
}

/// The shell posts what the pump returned on the error line, as a sentence.
///
/// The post notifies the state, which runs the shell's observer, and so the
/// pump, again. The observer settles because that pump reports nothing, which
/// the test above pins: a pump that retried a play it already tried would hang
/// this test rather than fail it.
#[gpui::test]
async fn the_shell_posts_a_stream_that_did_not_start(cx: &mut gpui::TestAppContext) {
    let (state, visual) = crate::test_support::drawn_shell(cx, "audio-start");
    assert_eq!(visual.read(|app| state.read(app).last_error.clone()), None);

    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.audio.publish_play(play(Err(
                "no audio output device. Plug in speakers or a headset.".into(),
            )));
            cx.notify();
        })
    });
    visual.run_until_parked();

    assert_eq!(
        visual
            .read(|app| state.read(app).last_error.clone())
            .as_deref(),
        Some("No audio output device. Plug in speakers or a headset.")
    );
}
