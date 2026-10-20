//! Contracts for the preview worker and its frame cache.
//!
//! The class closed here: playback that rendered each frame on a thread of its
//! own once it came due, so a frame was late by construction and nothing
//! rendered the next one early, and a vsync that found no new frame had no rule
//! for what it owed the count of dropped frames. These pin which frame a vsync
//! shows, how every slot that came due and was not shown is counted and why,
//! what the cache holds and lets go, that a clear wins against a frame already
//! rendering, and that every job runs on the one worker thread, which survives
//! a job that panics.
//!
//! Not covered: the rate a machine reaches, which `render/playback_rate.rs`
//! measures, and the drawn indicator.

use std::thread::ThreadId;

use gpui::AppContext as _;

use super::*;
use crate::app::premiere_tests::with_clip;

/// A 2x2 frame for `tick`.
fn frame(tick: i64) -> PreviewFrame {
    sized(tick, Arc::new(RgbaImage::transparent(2, 2)))
}

/// A frame for `tick` over `rgba`, which several frames may share: the cache
/// counts bytes by the buffer each frame holds.
fn sized(tick: i64, rgba: Arc<RgbaImage>) -> PreviewFrame {
    PreviewFrame {
        tick,
        rgba,
        image: None,
        texture: None,
        stats: FrameStats::default(),
        took: Duration::ZERO,
        scopes: None,
    }
}

const STAMP: Stamp = (1, 0);

/// A run from tick 100, `step` ticks per slot, in a 1000-tick sequence.
fn cadence(step: i64) -> Cadence {
    Cadence {
        anchor: 100,
        speed_num: step,
        speed_den: 1,
        end: 1000,
        rate: Rational::new(24, 1),
    }
}

/// A run from tick 100 at rational speed `speed_num / speed_den`.
fn cadence_scaled(speed_num: i64, speed_den: i64) -> Cadence {
    Cadence {
        anchor: 100,
        speed_num,
        speed_den,
        end: 1000,
        rate: Rational::new(24, 1),
    }
}

/// A cache holding the frames for `slots` of `cadence`.
fn holding(cadence: Cadence, slots: &[i64]) -> FrameCache {
    let mut cache = FrameCache::default();
    for &slot in slots {
        cache.insert(STAMP, frame(cadence.tick(slot)), cadence.anchor);
    }
    cache
}

/// Every subset of slots 1..=6 held, presented at every `now` from 0 to 6
/// after slot 0: the frame shown is the newest due one held, every due slot
/// is either shown or counted as dropped with the right cause, and the cache
/// keeps exactly the held slots after `now`, with its byte count matching.
#[test]
fn every_due_slot_is_shown_or_counted_and_the_cache_keeps_only_the_future() {
    let run = cadence(1);
    for held in 0u32..(1 << 6) {
        let slots: Vec<i64> = (1..=6)
            .filter(|slot| held & (1 << (slot - 1)) != 0)
            .collect();
        for now in 0..=6 {
            let mut cache = holding(run, &slots);
            let presented = cache.present(run, 0, now, DropCause::DecodeLate);
            let due: Vec<i64> = slots.iter().copied().filter(|slot| *slot <= now).collect();
            let case = format!("held {slots:?}, now {now}");
            assert_eq!(
                presented.frame.as_ref().map(|frame| frame.tick),
                due.last().map(|slot| run.tick(*slot)),
                "{case}: the wrong frame was shown"
            );
            let shown = u32::from(presented.frame.is_some());
            assert_eq!(
                shown + presented.dropped.total(),
                u32::try_from(now).unwrap_or(0),
                "{case}: a due slot was neither shown nor counted"
            );
            assert_eq!(
                presented.dropped.count(DropCause::PresentLate),
                u32::try_from(due.len()).unwrap_or(0).saturating_sub(1),
                "{case}: a held frame passed over was not counted as present-late"
            );
            assert_eq!(
                presented.dropped.count(DropCause::DecodeLate),
                u32::try_from(now).unwrap_or(0) - u32::try_from(due.len()).unwrap_or(0),
                "{case}: a slot with no frame was not charged to the stated cause"
            );
            let kept: Vec<i64> = slots.iter().copied().filter(|slot| *slot > now).collect();
            assert_eq!(cache.len(), kept.len(), "{case}: the cache kept the past");
            for slot in kept {
                assert!(
                    cache.contains(run.tick(slot)),
                    "{case}: slot {slot} was lost"
                );
            }
            assert_eq!(
                cache.bytes(),
                cache.len() * frame(0).bytes(),
                "{case}: the byte count drifted from the frames held"
            );
        }
    }
}

/// A vsync before the next slot is due changes nothing, and a slot that came
/// due on an earlier vsync is not counted again.
#[test]
fn a_vsync_with_nothing_newly_due_shows_and_counts_nothing() {
    let run = cadence(1);
    let mut cache = holding(run, &[3, 4]);
    for (shown, now) in [(2, 2), (3, 2), (2, 1)] {
        let presented = cache.present(run, shown, now, DropCause::RenderLate);
        assert!(presented.frame.is_none(), "shown {shown}, now {now}");
        assert!(presented.dropped.is_empty(), "shown {shown}, now {now}");
    }
    assert_eq!(cache.len(), 2, "a vsync with nothing due emptied the cache");
}

/// The cause of a frame that was not ready is the one the caller passes, so
/// the log and the indicator say which stage was slow.
#[test]
fn a_frame_not_ready_is_charged_to_the_cause_given() {
    let run = cadence(1);
    for cause in DropCause::ALL {
        let mut cache = holding(run, &[2]);
        let presented = cache.present(run, 0, 1, cause);
        assert!(presented.frame.is_none());
        assert_eq!(presented.dropped.count(cause), 1, "{cause}");
        assert_eq!(presented.dropped.total(), 1, "{cause}");
        assert!(cache.contains(run.tick(2)), "{cause}: a future frame left");
    }
}

/// A shuttle step shows every `step`th tick, in either direction; a frame off
/// that cadence is never shown and leaves the cache on the next vsync.
#[test]
fn shuttle_steps_show_their_own_ticks_in_either_direction() {
    for step in [2, -2, 4, -8] {
        let run = cadence(step);
        let mut cache = holding(run, &[1, 2]);
        cache.insert(STAMP, frame(run.tick(1) + step.signum()), run.anchor);
        let now = run.slot_at(run.tick(2));
        assert_eq!(now, 2, "step {step}");
        let presented = cache.present(run, 0, now, DropCause::DecodeLate);
        assert_eq!(
            presented.frame.map(|frame| frame.tick),
            Some(run.tick(2)),
            "step {step}"
        );
        assert_eq!(
            presented.dropped.count(DropCause::PresentLate),
            1,
            "step {step}"
        );
        assert!(cache.is_empty(), "step {step}: an off-cadence frame stayed");
    }
}

/// A clock between two slots is at the earlier one, in either direction, and
/// a run with no step never moves.
#[test]
fn the_slot_due_is_the_last_one_the_clock_reached() {
    let forward = cadence(3);
    assert_eq!(forward.slot_at(100), 0);
    assert_eq!(forward.slot_at(102), 0);
    assert_eq!(forward.slot_at(103), 1);
    assert_eq!(forward.slot_at(99), -1);
    let backward = cadence(-3);
    assert_eq!(backward.slot_at(98), 0);
    assert_eq!(backward.slot_at(97), 1);
    assert_eq!(backward.slot_at(101), -1);
    assert_eq!(cadence(0).slot_at(5000), 0);
}

/// Fractional and integer shuttle speeds map slot `n` to `anchor + floor(n * speed)`,
/// `slot_at` inverts `tick`, and duplicate ticks at speeds below 1x render once
/// and show on each slot without false drops.
#[test]
fn fractional_cadence_ticks_inverse_and_duplicate_slots_render_once() {
    let quarter = cadence_scaled(25, 100);
    let seq_quarter: Vec<i64> = (0..=8).map(|s| quarter.tick(s)).collect();
    assert_eq!(
        seq_quarter,
        vec![100, 100, 100, 100, 101, 101, 101, 101, 102]
    );
    for s in [0, 4, 8, 12] {
        assert_eq!(quarter.slot_at(quarter.tick(s)), s, "1/4x slot_at({s})");
    }

    let one_point_one = cadence_scaled(110, 100);
    let seq_110: Vec<i64> = (0..=10).map(|s| one_point_one.tick(s)).collect();
    assert_eq!(
        seq_110,
        vec![100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 111]
    );
    for s in 0..=20 {
        assert_eq!(
            one_point_one.slot_at(one_point_one.tick(s)),
            s,
            "1.1x slot_at({s})"
        );
    }

    let rev_two = cadence_scaled(-200, 100);
    let seq_rev2: Vec<i64> = (0..=4).map(|s| rev_two.tick(s)).collect();
    assert_eq!(seq_rev2, vec![100, 98, 96, 94, 92]);
    for s in 0..=20 {
        assert_eq!(rev_two.slot_at(rev_two.tick(s)), s, "-2x slot_at({s})");
    }

    let eight = cadence_scaled(800, 100);
    let seq_8x: Vec<i64> = (0..=4).map(|s| eight.tick(s)).collect();
    assert_eq!(seq_8x, vec![100, 108, 116, 124, 132]);
    for s in 0..=20 {
        assert_eq!(eight.slot_at(eight.tick(s)), s, "8x slot_at({s})");
    }

    // At 1/4x, slots 1..=3 all show tick 100 and slots 4..=7 all show tick 101.
    // Each distinct tick is planned and rendered once, and shown on every slot
    // with zero dropped frames.
    let mut cache = FrameCache::default();
    let mut rendered_ticks = Vec::new();
    for slot in 0..=7 {
        let plan = Plan {
            cadence: quarter,
            now: slot,
            fit: None,
        };
        while let Some(next_tick) = plan.next(&cache, &[]) {
            rendered_ticks.push(next_tick);
            cache.insert(STAMP, frame(next_tick), quarter.anchor);
        }
        if slot > 0 {
            let presented = cache.present(quarter, slot - 1, slot, DropCause::DecodeLate);
            assert_eq!(
                presented.frame.map(|f| f.tick),
                Some(quarter.tick(slot)),
                "slot {slot} did not show its duplicate tick"
            );
            assert_eq!(
                presented.dropped.total(),
                0,
                "slot {slot} recorded a false drop on a duplicate tick"
            );
        }
    }
    let mut unique = rendered_ticks.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        rendered_ticks.len(),
        unique.len(),
        "duplicate slots rendered a tick more than once: {rendered_ticks:?}"
    );
}

/// Frames rendered from a session state that is gone leave as soon as one
/// from the new state arrives.
#[test]
fn a_frame_from_a_new_session_state_empties_the_cache() {
    let mut cache = holding(cadence(1), &[1, 2, 3]);
    cache.insert((2, 0), frame(104), 100);
    assert_eq!(cache.len(), 1);
    assert!(cache.contains(104));
    cache.insert((2, 1), frame(105), 100);
    assert_eq!(cache.len(), 1, "a proxy change kept frames from before it");
    assert!(cache.contains(105));
}

/// Over budget, the frame farthest from the playhead leaves first, and a frame
/// larger than the whole budget is not held at all.
#[test]
fn the_cache_stays_inside_its_budget_keeping_the_nearest_frames() {
    // Half the budget and a little more: two of these do not fit.
    let half = Arc::new(RgbaImage::new(1, 1, vec![0; CACHE_BYTES / 4 + 1]));
    let mut cache = FrameCache::default();
    cache.insert(STAMP, sized(110, Arc::clone(&half)), 100);
    cache.insert(STAMP, sized(101, Arc::clone(&half)), 100);
    assert!(cache.contains(101) && !cache.contains(110));
    cache.insert(STAMP, sized(100, Arc::clone(&half)), 100);
    assert!(cache.contains(100) && !cache.contains(101));
    assert!(cache.bytes() <= CACHE_BYTES);

    let whole = Arc::new(RgbaImage::new(1, 1, vec![0; CACHE_BYTES / 2 + 1]));
    cache.insert(STAMP, sized(102, whole), 100);
    assert!(!cache.contains(102), "a frame over the budget was held");
    assert!(
        cache.contains(100),
        "a frame over the budget evicted the rest"
    );
}

/// Render-ahead asks for the nearest slot not held and not failed, and nothing
/// at or past the end of the sequence.
#[test]
fn render_ahead_takes_the_nearest_missing_slot_inside_the_sequence() {
    let run = Cadence {
        end: 103,
        ..cadence(1)
    };
    let plan = Plan {
        cadence: run,
        now: 0,
        fit: None,
    };
    let empty = FrameCache::default();
    assert_eq!(plan.next(&empty, &[]), Some(101));
    assert_eq!(plan.next(&holding(run, &[1]), &[]), Some(102));
    assert_eq!(plan.next(&holding(run, &[1]), &[102]), None);
    assert_eq!(
        plan.next(&holding(run, &[1, 2]), &[]),
        None,
        "rendered past the end"
    );
    let far = Plan {
        cadence: cadence(1),
        now: 10,
        fit: None,
    };
    assert_eq!(far.next(&empty, &[]), Some(111));
    assert_eq!(
        far.next(&holding(cadence(1), &[11, 12, 13, 14]), &[]),
        None,
        "rendered more than RENDER_AHEAD past the playhead"
    );
}

/// Pool pictures, Viewer frames and render-ahead all run on one named thread
/// that outlives every job: no thread is started per frame.
#[gpui::test]
async fn every_job_runs_on_the_one_worker_thread(cx: &mut gpui::TestAppContext) {
    let executor = cx.executor();
    executor.allow_parking();
    let (state, _) = with_clip();
    let worker = &state.preview_worker;
    let whose = || {
        let thread = std::thread::current();
        (thread.id(), thread.name().map(str::to_owned))
    };
    let first: Option<(ThreadId, Option<String>)> = worker.run(&executor, whose).await;
    let rendered = worker
        .frame(
            &executor,
            RationalTime::new(0, state.playhead.rate),
            None,
            Cancel::never(),
        )
        .await;
    assert!(
        matches!(rendered, Some(Ok(_))),
        "the Viewer frame did not render: {rendered:?}"
    );
    let second = worker.run(&executor, whose).await;
    let Some((id, name)) = first else {
        panic!("the worker did not run the first picture");
    };
    assert_eq!(name.as_deref(), Some(THREAD_NAME));
    assert_eq!(
        second.map(|(id, _)| id),
        Some(id),
        "a later job ran on a different thread"
    );
}

/// A job that panics costs that job's answer; the worker serves the next one.
#[gpui::test]
async fn a_panicking_job_costs_its_answer_and_the_worker_keeps_serving(
    cx: &mut gpui::TestAppContext,
) {
    let executor = cx.executor();
    executor.allow_parking();
    let (state, _) = with_clip();
    let worker = &state.preview_worker;
    let panicked: Option<u8> = worker.run(&executor, || panic!("a codec fell over")).await;
    assert_eq!(panicked, None);
    assert_eq!(worker.run(&executor, || 7).await, Some(7));
}

/// The Viewer frame that comes back is the tick asked for, with its texture,
/// and a frame asked for at a reduced resolution is rendered at that size.
#[gpui::test]
async fn a_viewer_frame_is_the_tick_asked_for(cx: &mut gpui::TestAppContext) {
    let executor = cx.executor();
    executor.allow_parking();
    let (state, _) = with_clip();
    let at = RationalTime::new(3, state.playhead.rate);
    let rendered = state
        .preview_worker
        .frame(&executor, at, None, Cancel::never())
        .await;
    let Some(Ok(frame)) = rendered else {
        panic!("the frame did not render: {rendered:?}");
    };
    assert_eq!(frame.tick, 3);
    assert!(frame.rgba.width > 0 && frame.rgba.height > 0);
    assert!(frame.image.is_some(), "the frame came back with no texture");
    assert!(
        frame.scopes.is_none(),
        "scopes were measured with the strip closed"
    );
    let full = (frame.rgba.width, frame.rgba.height);
    for level in crate::resolution::Resolution::ALL {
        let fit = level.fit(full);
        let rendered = state
            .preview_worker
            .frame(&executor, at, fit, Cancel::never())
            .await;
        let Some(Ok(reduced)) = rendered else {
            panic!("the {} frame did not render: {rendered:?}", level.label());
        };
        let (width, height) = fit.unwrap_or(full);
        assert!(
            reduced.rgba.width <= width
                && reduced.rgba.height <= height
                && reduced.rgba.width >= full.0 / level.divisor()
                && reduced.rgba.height >= full.1 / level.divisor(),
            "{} of {full:?} rendered {}x{}",
            level.label(),
            reduced.rgba.width,
            reduced.rgba.height
        );
    }
}

/// Starting a run renders the next [`RENDER_AHEAD`] slots; a clear empties the
/// cache at once; an edit renders them again from the new project.
///
/// A pool picture is the barrier: it runs only once the worker has nothing
/// left to render ahead.
#[gpui::test]
async fn a_run_renders_ahead_and_an_edit_renders_again(cx: &mut gpui::TestAppContext) {
    let executor = cx.executor();
    executor.allow_parking();
    let (mut state, _) = with_clip();
    state.playing = true;
    let run = state.arm_presenting();
    assert!(
        run.end > RENDER_AHEAD,
        "the fixture clip is too short for this test: {run:?}"
    );
    assert_eq!(state.preview_worker.run(&executor, || ()).await, Some(()));
    assert_eq!(state.preview_worker.cached(), RENDER_AHEAD as usize);
    for slot in 1..=RENDER_AHEAD {
        assert!(
            state.preview_worker.has_cached(run.tick(slot)),
            "slot {slot}"
        );
    }
    let before = state.engine().lock().revision();

    state.add_marker_at_playhead().expect("a marker lands");
    assert_ne!(
        state.engine().lock().revision(),
        before,
        "the edit moved nothing"
    );
    assert_eq!(
        state.preview_worker.cached(),
        0,
        "frames rendered before the edit stayed in the cache"
    );
    assert_eq!(state.preview_worker.run(&executor, || ()).await, Some(()));
    assert_eq!(
        state.preview_worker.cached(),
        RENDER_AHEAD as usize,
        "the run did not render ahead again after the edit"
    );

    state.stop_presenting();
    assert_eq!(
        state.preview_worker.cached(),
        0,
        "a stop kept the run's frames"
    );
}

/// When the window shares its `wgpu` device, Viewer frames and playback
/// render-ahead copy directly into pooled GPU textures with zero GPU-to-CPU
/// frame readbacks, with scopes closed or open: open scopes count the texture
/// on the device. A replaced device is re-adopted cleanly.
#[gpui::test]
async fn shared_wgpu_device_presents_without_cpu_readback_and_recovers(
    cx: &mut gpui::TestAppContext,
) {
    let executor = cx.executor();
    executor.allow_parking();
    let (mut state, _) = with_clip();
    let Some(shared_gpu) = crate::test_support::mounted_gpu(&state.ctx) else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let dev_a = GpuDevice::new((
        shared_gpu.instance.clone(),
        shared_gpu.adapter.clone(),
        shared_gpu.device.clone(),
        shared_gpu.queue.clone(),
    ));
    let adapter = shared_gpu.adapter.get_info().name;
    // The worker started before the render plugin mounted, so the session
    // had no adapter to name.
    assert_eq!(state.preview_worker.device_name(), None);
    assert!(state.preview_worker.set_gpu_device(Some(dev_a.clone())));
    assert!(!state.preview_worker.set_gpu_device(Some(dev_a)));
    // The status bar names the adapter the Viewer renders on.
    assert_eq!(
        state.preview_worker.device_name().as_deref(),
        Some(adapter.as_str())
    );

    let at = RationalTime::new(0, state.playhead.rate);
    let first = state
        .preview_worker
        .frame(&executor, at, Some((320, 180)), Cancel::never())
        .await
        .expect("worker alive")
        .expect("frame rendered");
    let readbacks_after_first = state.preview_worker.readback_count();
    assert!(
        first.texture.is_some(),
        "shared wgpu device should produce a GPU texture"
    );
    assert!(
        first.image.is_none() && first.rgba.pixels.is_empty(),
        "GPU texture path must not allocate or read back CPU RGBA/BGRA buffers"
    );
    assert_eq!((first.rgba.width, first.rgba.height), (320, 180));
    state
        .preview_worker
        .set_shown_texture(first.texture.clone());

    // Render ahead RENDER_AHEAD frames during playback and verify zero readbacks.
    state.playing = true;
    let run = state.arm_presenting();
    assert_eq!(state.preview_worker.run(&executor, || ()).await, Some(()));
    assert_eq!(state.preview_worker.cached(), RENDER_AHEAD as usize);
    assert_eq!(
        state.preview_worker.readback_count(),
        readbacks_after_first,
        "render-ahead on a shared wgpu window must perform zero GPU-to-CPU readbacks"
    );
    let presented = state.preview_worker.present(run, 0, 1);
    let shown_slot_1 = presented.frame.expect("slot 1 was rendered ahead");
    assert!(shown_slot_1.texture.is_some());
    assert_ne!(
        shown_slot_1.texture, first.texture,
        "pooled texture must not overwrite the currently shown texture"
    );

    // Device recovery: a new GpuDevice handle drops cached textures and re-adopts.
    let dev_b = GpuDevice::new((
        shared_gpu.instance.clone(),
        shared_gpu.adapter.clone(),
        shared_gpu.device.clone(),
        shared_gpu.queue.clone(),
    ));
    assert!(state.preview_worker.set_gpu_device(Some(dev_b)));
    assert!(state.preview_worker.shown_texture().is_none());
    assert_eq!(state.preview_worker.cached(), 0);
    let recovered = state
        .preview_worker
        .frame(&executor, at, None, Cancel::never())
        .await
        .expect("worker alive")
        .expect("recovered frame rendered");
    assert!(recovered.texture.is_some());
    assert_eq!(
        state.preview_worker.readback_count(),
        readbacks_after_first,
        "recovered GPU texture render must still perform zero CPU readbacks"
    );

    // Opening scopes counts the texture on the device: the frame stays a
    // texture and is measured, and no frame is read back.
    state.preview_worker.set_scopes(true);
    let with_scopes = state
        .preview_worker
        .frame(&executor, at, None, Cancel::never())
        .await
        .expect("worker alive")
        .expect("scoped frame rendered");
    assert!(
        with_scopes.texture.is_some() && with_scopes.rgba.pixels.is_empty(),
        "open scopes read the frame back instead of counting the texture"
    );
    let on_device = with_scopes
        .scopes
        .as_ref()
        .expect("the texture frame was not measured");
    assert_eq!(on_device.tick, 0);
    assert_eq!(
        state.preview_worker.readback_count(),
        readbacks_after_first,
        "scopes on a texture frame read the frame back"
    );

    // The same frame read back and counted on the CPU samples the same pixels.
    assert!(state.preview_worker.set_gpu_device(None));
    let read_back = state
        .preview_worker
        .frame(&executor, at, None, Cancel::never())
        .await
        .expect("worker alive")
        .expect("read-back frame rendered");
    assert!(read_back.texture.is_none());
    let on_cpu = read_back
        .scopes
        .as_ref()
        .expect("the read-back frame was not measured");
    assert_eq!(
        (on_device.sampled, on_device.tick),
        (on_cpu.sampled, on_cpu.tick),
        "the device and the CPU sampled different pixels of one frame"
    );
    assert!(on_cpu.sampled > 0);

    // A window that gives its device back takes its adapter name with it; a
    // worker started once the session has an adapter names that one.
    assert_eq!(state.preview_worker.device_name(), None);
    let later = PreviewWorker::spawn(Arc::clone(state.engine()), (*state.ctx).clone())
        .expect("second worker");
    assert_eq!(later.device_name().as_deref(), Some(adapter.as_str()));
}

#[test]
fn cadence_slots_step_fractional_and_reverse_speeds_and_invert_with_slot_at() {
    // 1/4x: 4 slots per tick (100, 100, 100, 100, 101, 101, 101, 101, 102)
    let quarter = cadence_scaled(25, 100);
    let quarter_ticks: Vec<i64> = (0..=8).map(|s| quarter.tick(s)).collect();
    assert_eq!(
        quarter_ticks,
        vec![100, 100, 100, 100, 101, 101, 101, 101, 102]
    );
    assert_eq!(quarter.slot_at(100), 0);
    assert_eq!(quarter.slot_at(101), 4);
    assert_eq!(quarter.slot_at(102), 8);

    // 1.1x: anchor + floor(n * 110 / 100)
    let one_ten = cadence_scaled(110, 100);
    let one_ten_ticks: Vec<i64> = (0..=10).map(|s| one_ten.tick(s)).collect();
    assert_eq!(
        one_ten_ticks,
        vec![100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 111]
    );
    for s in 0..=10 {
        let tick = one_ten.tick(s);
        let inv = one_ten.slot_at(tick);
        assert_eq!(one_ten.tick(inv), tick, "slot_at inverse at slot {s}");
    }

    // -2x: steps backward by 2 ticks per slot
    let rev_two = cadence_scaled(-200, 100);
    let rev_two_ticks: Vec<i64> = (0..=4).map(|s| rev_two.tick(s)).collect();
    assert_eq!(rev_two_ticks, vec![100, 98, 96, 94, 92]);
    for s in 0..=4 {
        assert_eq!(rev_two.slot_at(rev_two.tick(s)), s);
    }

    // 8x: steps forward by 8 ticks per slot
    let eight = cadence_scaled(800, 100);
    let eight_ticks: Vec<i64> = (0..=4).map(|s| eight.tick(s)).collect();
    assert_eq!(eight_ticks, vec![100, 108, 116, 124, 132]);
    for s in 0..=4 {
        assert_eq!(eight.slot_at(eight.tick(s)), s);
    }
}

#[test]
fn sub_1x_cadence_renders_duplicate_tick_once_and_presents_on_each_slot() {
    let quarter = cadence_scaled(25, 100);
    let mut cache = FrameCache::default();
    cache.insert(STAMP, frame(100), quarter.anchor);

    // Plan at slot 0: slots 1, 2, 3 all map to tick 100 (already in cache),
    // so next_missing skips duplicate tick 100 and returns tick 101 (slot 4).
    let plan = Plan {
        cadence: quarter,
        now: 0,
        fit: None,
    };
    assert_eq!(plan.next(&cache, &[]), Some(101));

    // Presenting slots 1, 2, 3 in order shows tick 100 on each slot with 0 drops,
    // keeping tick 100 in the cache until slot 3 (the last slot for tick 100).
    let p1 = cache.present(quarter, 0, 1, DropCause::RenderLate);
    assert_eq!(p1.frame.as_ref().map(|f| f.tick), Some(100));
    assert_eq!(p1.dropped.total(), 0);
    assert!(cache.contains(100));

    let p2 = cache.present(quarter, 1, 2, DropCause::RenderLate);
    assert_eq!(p2.frame.as_ref().map(|f| f.tick), Some(100));
    assert_eq!(p2.dropped.total(), 0);
    assert!(cache.contains(100));

    let p3 = cache.present(quarter, 2, 3, DropCause::RenderLate);
    assert_eq!(p3.frame.as_ref().map(|f| f.tick), Some(100));
    assert_eq!(p3.dropped.total(), 0);
    assert!(!cache.contains(100));
}

/// With the transparency grid on, the Viewer frame is read back with the
/// uncovered canvas at alpha 0, and the image on screen is that frame laid
/// over the grid: opaque, the first grey in the top-left square and the second
/// one square to the right.
#[gpui::test]
async fn a_transparent_viewer_frame_is_drawn_over_the_grid(cx: &mut gpui::TestAppContext) {
    let executor = cx.executor();
    executor.allow_parking();
    let mut state = crate::app::AppState::new(
        aria_plugin::PluginContext::new(),
        crate::scratch_pool::scratch("preview-grid"),
    )
    .expect("state");
    state.new_project().expect("project");
    state.engine().lock().set_preview_transparency(true);
    let rendered = state
        .preview_worker
        .frame(
            &executor,
            RationalTime::new(0, state.playhead.rate),
            Some((64, 36)),
            Cancel::never(),
        )
        .await;
    let Some(Ok(frame)) = rendered else {
        panic!("the frame did not render: {rendered:?}");
    };
    assert!(
        frame.texture.is_none(),
        "a transparent frame skipped the grid"
    );
    assert!(
        frame
            .rgba
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| *px == [0, 0, 0, 0]),
        "the empty canvas was not left transparent"
    );
    let image = frame.image.expect("the frame came back with no image");
    let bgra = image.as_bytes(0).expect("one frame");
    let width = frame.rgba.width as usize;
    let at = |x: usize, y: usize| {
        let i = (y * width + x) * 4;
        [bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]]
    };
    let cell = crate::frame::GRID_CELL;
    let [first, second] = crate::frame::GRID_GREYS;
    assert_eq!(at(0, 0), [first[2], first[1], first[0], 255]);
    assert_eq!(at(cell, 0), [second[2], second[1], second[0], 255]);
    assert_eq!(at(cell, cell), [first[2], first[1], first[0], 255]);
}

/// Render-ahead measures at most [`SCOPE_RATE_HZ`] frames a second of playback
/// at every rate and speed, and at 1x and faster every [`scope_pace`]-th slot,
/// so the scopes keep up. The pace is the fewest slots that keep the bound.
///
/// Caught: measuring every frame, a pace rounded down (60 fps measured at 60
/// a second), and a pace counted in ticks rather than slots, which measures
/// every frame at 2x. Not caught: the wall-clock rate, which follows from the
/// slot clock.
#[test]
fn render_ahead_measures_at_most_thirty_frames_a_second() {
    let rates = [
        Rational::FPS_23_976,
        Rational::FPS_24,
        Rational::FPS_25,
        Rational::FPS_29_97,
        Rational::FPS_30,
        Rational::new(30_001, 1000),
        Rational::FPS_50,
        Rational::FPS_59_94,
        Rational::FPS_60,
        Rational::new(120, 1),
        Rational::new(240, 1),
    ];
    let limit = SCOPE_RATE_HZ as f64;
    for rate in rates {
        let pace = scope_pace(rate);
        let measured_per_second = rate.as_f64() / pace as f64;
        assert!(
            measured_per_second <= limit,
            "{rate:?} measures {measured_per_second} frames a second"
        );
        if pace > 1 {
            assert!(
                rate.as_f64() / (pace - 1) as f64 > limit,
                "{rate:?} paces {pace} slots where {} keep the bound",
                pace - 1
            );
        }
        for (speed_num, speed_den) in [(1, 4), (1, 2), (1, 1), (2, 1), (8, 1), (-1, 1), (-2, 1)] {
            let run = Cadence {
                anchor: 5000,
                speed_num,
                speed_den,
                end: 100_000,
                rate,
            };
            let slots = 600;
            let mut cache = FrameCache::default();
            let mut measured = 0;
            for now in 0..slots {
                let plan = Plan {
                    cadence: run,
                    now,
                    fit: None,
                };
                while let Some(tick) = plan.next(&cache, &[]) {
                    measured += i64::from(plan.scoped(tick));
                    cache.insert(STAMP, frame(tick), run.tick(now));
                }
            }
            // Slots 1 to `slots - 1 + RENDER_AHEAD` were planned.
            let planned = slots - 1 + RENDER_AHEAD;
            assert!(
                measured <= planned / pace,
                "{rate:?} at {speed_num}/{speed_den} measured {measured} frames in {planned} slots"
            );
            if speed_num.abs() >= speed_den {
                assert_eq!(
                    measured,
                    planned / pace,
                    "{rate:?} at {speed_num}/{speed_den} skipped a due measurement"
                );
            }
        }
    }
}

/// A frame shown during playback without scopes keeps the reading the
/// scopes hold rather than asking for a measurement of its own, and the frame
/// playback stops on is measured.
///
/// Caught: a show that measures every paced frame on its own, which undoes
/// the pace, and a stop that leaves the scopes reading an earlier frame.
#[gpui::test]
async fn playback_holds_the_scope_reading_and_a_stop_measures_the_frame_on_screen(
    cx: &mut gpui::TestAppContext,
) {
    let executor = cx.executor();
    executor.allow_parking();
    let (mut state, _) = with_clip();
    state.scopes_open = true;
    state.sync_scope_demand();
    let at = RationalTime::new(2, state.playhead.rate);
    let rendered = state
        .preview_worker
        .frame(&executor, at, None, Cancel::never())
        .await
        .expect("worker alive")
        .expect("frame rendered");
    assert!(
        rendered.scopes.is_some(),
        "a frame outside playback was not measured"
    );
    let paced = PreviewFrame {
        scopes: None,
        ..rendered
    };
    state.playing = true;
    state.arm_presenting();
    let state = cx.new(|_| state);
    let barrier = state.update(cx, |state, _| state.preview_worker.run(&executor, || ()));
    assert_eq!(barrier.await, Some(()));
    state.update(cx, |state, _| state.preview_worker.take_measured());

    state.update(cx, |state, cx| state.show_frame(paced, cx));
    let barrier = state.update(cx, |state, _| state.preview_worker.run(&executor, || ()));
    assert_eq!(barrier.await, Some(()));
    let asked = state.update(cx, |state, _| state.preview_worker.take_measured());
    assert!(
        asked.is_none(),
        "a frame shown under playback asked for a measurement of its own"
    );

    state.update(cx, |state, _| state.stop_presenting());
    let barrier = state.update(cx, |state, _| state.preview_worker.run(&executor, || ()));
    assert_eq!(barrier.await, Some(()));
    let measured = state.update(cx, |state, _| state.preview_worker.take_measured());
    assert_eq!(
        measured.map(|read| read.tick),
        Some(2),
        "the frame playback stopped on was not measured"
    );
}
