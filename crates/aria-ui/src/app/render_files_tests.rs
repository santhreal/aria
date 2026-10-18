//! Contracts for Render In to Out, Delete Render Files, and the timeline
//! render bar (`RenderQuality::{Green, Yellow, Red}`).
//!
//! These close the class where pre-rendered work-area frames fail to land in
//! the content-addressed `RenderFiles` cache, get re-rendered on a second pass,
//! survive `Delete Render Files` or cache clearing, or leave the timeline
//! render bar stale after an edit, motion-blur toggle, or cache relocation.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aria_commands::{Command, EditCommand};
use aria_jobs::{JobId, JobSnapshot, JobState};
use aria_models::{ClipId, Easing, Keyframe, MotionBlur, Param, Rational, RationalTime};
use aria_plugin::PluginContext;

use super::{RenderQuality, RenderSegment};
use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::scratch_pool::scratch;

const JOB_BOUND: Duration = Duration::from_secs(30);

fn wait_job(state: &AppState, raw_id: u64) -> JobSnapshot {
    let id = JobId::from_raw(raw_id);
    let deadline = Instant::now() + JOB_BOUND;
    loop {
        let snap = state.jobs.get(id).expect("registered job");
        if snap.state.is_terminal() {
            return snap;
        }
        assert!(
            Instant::now() < deadline,
            "job {raw_id} ({}) did not finish within {JOB_BOUND:?}: {:?}",
            snap.label,
            snap.state
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn output_val<'a>(snap: &'a JobSnapshot, key: &str) -> &'a str {
    snap.outputs
        .iter()
        .find(|o| o.name() == key)
        .unwrap_or_else(|| panic!("missing output {key:?} on {snap:?}"))
        .value()
}

fn qoi_count(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut count = 0;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            count += qoi_count(&path);
        } else if path.extension().and_then(|s| s.to_str()) == Some("qoi") {
            count += 1;
        }
    }
    count
}

/// Set up a project with one clip whose transform varies per frame so every
/// frame in `[0, duration)` resolves to a distinct [`aria_render::FrameKey`].
fn animated_clip_state() -> (AppState, ClipId) {
    let (mut state, clip) = with_clip();
    {
        let mut engine = state.engine.lock();
        let mut project = (*engine.project()).clone();
        let mut canvas = project.timeline().canvas();
        canvas.resolution = Some([32, 18]);
        project.timeline_mut().set_canvas(canvas);
        let clip_mut = project.clip_mut(clip).expect("clip");
        clip_mut.transform.position = Param::Keyframed {
            keyframes: vec![
                Keyframe::new(0, [0.0, 0.0], Easing::Linear),
                Keyframe::new(20, [40.0, 20.0], Easing::Linear),
            ],
        };
        engine.reset_project(project);
    }
    state.sync_from_engine();
    (state, clip)
}

#[test]
fn empty_timeline_refuses_render_in_to_out_and_delete_render_files() {
    let mut state = AppState::new(PluginContext::new(), scratch("rf-empty")).expect("state");
    state.new_project().expect("empty project");

    let render_err = state.render_in_to_out().expect_err("empty timeline");
    assert!(
        render_err.contains("empty"),
        "unexpected refusal message: {render_err}"
    );
    let delete_err = state.delete_render_files().expect_err("empty timeline");
    assert!(
        delete_err.contains("empty"),
        "unexpected refusal message: {delete_err}"
    );
    assert!(state.job_view.is_empty(), "refused actions spawned a job");
}

#[test]
fn render_in_to_out_stores_four_frames_skips_cached_on_second_run_and_delete_removes_them() {
    let (mut state, _clip) = animated_clip_state();
    let duration = state.store.sequence.duration_ticks;
    assert!(duration >= 8, "fixture clip must span at least 8 frames");

    state.mark_in = Some(2);
    state.mark_out = Some(6);

    let initial = state.render_bar_segments();
    assert_eq!(
        *initial,
        vec![RenderSegment {
            start_tick: 0,
            end_tick: duration,
            quality: RenderQuality::Yellow,
        }],
        "unrendered standard clip should be yellow across the sequence"
    );
    assert_eq!(qoi_count(&state.render_files_cache_dir()), 0);

    let first_job = state.render_in_to_out().expect("start first render");
    let first_snap = wait_job(&state, first_job);
    assert_eq!(
        first_snap.state,
        JobState::Done,
        "first render failed: {}",
        first_snap.detail
    );
    assert_eq!(output_val(&first_snap, "rendered"), "4");
    assert_eq!(output_val(&first_snap, "cached"), "0");
    assert_eq!(qoi_count(&state.render_files_cache_dir()), 4);

    let after_render = state.render_bar_segments();
    assert_eq!(
        *after_render,
        vec![
            RenderSegment {
                start_tick: 0,
                end_tick: 2,
                quality: RenderQuality::Yellow,
            },
            RenderSegment {
                start_tick: 2,
                end_tick: 6,
                quality: RenderQuality::Green,
            },
            RenderSegment {
                start_tick: 6,
                end_tick: duration,
                quality: RenderQuality::Yellow,
            },
        ],
        "rendered work area [2, 6) must turn green while outside stays yellow"
    );

    let second_job = state.render_in_to_out().expect("start second render");
    let second_snap = wait_job(&state, second_job);
    assert_eq!(second_snap.state, JobState::Done);
    assert_eq!(output_val(&second_snap, "rendered"), "0");
    assert_eq!(output_val(&second_snap, "cached"), "4");
    assert_eq!(qoi_count(&state.render_files_cache_dir()), 4);

    let del_job = state.delete_render_files().expect("start delete");
    let del_snap = wait_job(&state, del_job);
    assert_eq!(del_snap.state, JobState::Done);
    assert_eq!(output_val(&del_snap, "removed"), "4");
    assert_eq!(qoi_count(&state.render_files_cache_dir()), 0);

    let after_delete = state.render_bar_segments();
    assert_eq!(
        *after_delete,
        vec![RenderSegment {
            start_tick: 0,
            end_tick: duration,
            quality: RenderQuality::Yellow,
        }],
        "deleting render files must return the bar to yellow"
    );
}

#[test]
fn editing_or_relocating_cache_invalidates_green_bar_and_motion_blur_marks_red() {
    let (mut state, clip) = animated_clip_state();
    let duration = state.store.sequence.duration_ticks;
    state.mark_in = Some(0);
    state.mark_out = Some(2);

    let job = state.render_in_to_out().expect("render [0, 2)");
    let snap = wait_job(&state, job);
    assert_eq!(snap.state, JobState::Done, "render failed: {}", snap.detail);
    assert_eq!(state.render_bar_segments()[0].quality, RenderQuality::Green);

    // Editing the clip changes its scene digest, flipping [0, 2) back to Yellow.
    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: "gaussian_blur".into(),
        }))
        .expect("add effect");
    state.sync_from_engine();
    assert_eq!(
        *state.render_bar_segments(),
        vec![RenderSegment {
            start_tick: 0,
            end_tick: duration,
            quality: RenderQuality::Yellow,
        }],
        "adding an effect must invalidate the previous green segment"
    );

    // Enabling motion blur on an animated clip attaches blur sub-passes, turning
    // unrendered animated frames Red.
    state
        .apply_command(Command::Edit(EditCommand::SetClipMotionBlur {
            clip,
            motion_blur: MotionBlur {
                enabled: true,
                shutter_deg: 180.0,
                samples: 8,
            },
        }))
        .expect("enable motion blur");
    state.sync_from_engine();
    let blurred = state.render_bar_segments();
    assert!(
        blurred.iter().any(|s| s.quality == RenderQuality::Red),
        "unrendered motion-blurred frames must report Red, got {blurred:?}"
    );

    // Rendering [0, 2) with motion blur enabled caches those frames and turns
    // [0, 2) Green even though the underlying scene has motion blur.
    let blur_job = state
        .render_in_to_out()
        .expect("render motion-blurred [0, 2)");
    let blur_snap = wait_job(&state, blur_job);
    assert_eq!(blur_snap.state, JobState::Done);
    assert_eq!(state.render_bar_segments()[0].quality, RenderQuality::Green);

    // Relocating CacheId::RenderFiles points the engine at the new directory and
    // refreshes the bar.
    let relocated = scratch("rf-relocated").join("render-files");
    state
        .relocate_registered_cache("render_files", &relocated)
        .expect("relocate render-files cache");
    assert_eq!(
        state
            .engine
            .lock()
            .render_files()
            .expect("engine render_files")
            .dir(),
        relocated.as_path()
    );

    // Clearing CacheId::RenderFiles removes the files and flips [0, 2) back to Red.
    state
        .clear_registered_cache("render_files")
        .expect("clear render-files cache");
    assert_eq!(qoi_count(&relocated), 0);
    assert_eq!(state.render_bar_segments()[0].quality, RenderQuality::Red);
}

/// Split the clip under `at` there, so the timeline gains a cut at `at`.
fn cut_at(state: &mut AppState, at: i64) {
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.timeline_start_ticks < at && at < clip.timeline_end_ticks)
        .map(|clip| ClipId::from_raw(clip.id))
        .expect("a clip under the cut");
    state.select_clip(clip);
    state.set_playhead(at);
    state.split_at_playhead().expect("split");
}

/// Render Selection renders only the frames the selected clips cover: spans of
/// abutting clips join, a frame between two selected clips that neither covers
/// stays unrendered, and an empty selection is refused without a job.
#[test]
fn render_selection_renders_only_the_frames_the_selected_clips_cover() {
    let (mut state, _) = animated_clip_state();
    let duration = state.store.sequence.duration_ticks;
    assert!(duration > 6, "fixture clip must span more than 6 frames");
    for at in [2, 4, 6] {
        cut_at(&mut state, at);
    }
    let mut clips: Vec<(i64, ClipId)> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| (clip.timeline_start_ticks, ClipId::from_raw(clip.id)))
        .collect();
    clips.sort_unstable_by_key(|(start, _)| *start);
    let [(0, a), (2, b), (4, c), (6, d)] = clips[..] else {
        panic!("the cuts must leave four clips at 0, 2, 4 and 6: {clips:?}");
    };

    state.clear_selection();
    assert_eq!(state.selection_spans(), Vec::new());
    assert_eq!(
        state.render_selection().expect_err("nothing selected"),
        "Select the clips to render."
    );
    assert!(state.job_view.is_empty(), "a refused render spawned a job");

    state.selection = vec![d, b, a];
    assert_eq!(state.selection_spans(), vec![(0, 4), (6, duration)]);
    state.selection = vec![c, b];
    assert_eq!(state.selection_spans(), vec![(2, 6)]);

    state.selection = vec![c, a];
    assert_eq!(state.selection_spans(), vec![(0, 2), (4, 6)]);
    let job = state.render_selection().expect("start the render");
    let snap = wait_job(&state, job);
    assert_eq!(snap.state, JobState::Done, "render failed: {}", snap.detail);
    assert_eq!(snap.label, "Rendering Selection");
    assert_eq!(output_val(&snap, "rendered"), "4");
    assert_eq!(output_val(&snap, "cached"), "0");
    assert_eq!(qoi_count(&state.render_files_cache_dir()), 4);
    let segment = |start_tick, end_tick, quality| RenderSegment {
        start_tick,
        end_tick,
        quality,
    };
    assert_eq!(
        *state.render_bar_segments(),
        vec![
            segment(0, 2, RenderQuality::Green),
            segment(2, 4, RenderQuality::Yellow),
            segment(4, 6, RenderQuality::Green),
            segment(6, duration, RenderQuality::Yellow),
        ]
    );
}

/// The render bar is computed off the `aria-preview` thread: with that thread
/// held busy, the segments for the current revision still arrive. Closes the
/// class where the whole-timeline segment pass queued on the preview worker
/// and every Viewer frame after an edit waited behind it. Does not measure how
/// long the pass takes.
#[test]
fn render_bar_completes_while_the_preview_worker_is_busy() {
    let (mut state, _) = animated_clip_state();
    let duration = state.store.sequence.duration_ticks;
    let (release, held) = std::sync::mpsc::channel::<()>();
    let (entered, busy) = std::sync::mpsc::channel::<()>();
    state.preview_worker.occupy(move || {
        let _ = entered.send(());
        let _ = held.recv();
    });
    busy.recv_timeout(JOB_BOUND)
        .expect("the preview worker took the holding job");

    // A work area the bar has not seen yet, so this asks for a new pass.
    state.mark_in = Some(1);
    state.mark_out = Some(3);
    let _ = state.render_bar();
    let seq = state.render_bar_state.latest_seq();
    let segments = state
        .render_bar_state
        .wait_within(seq, JOB_BOUND)
        .expect("render bar segments arrive while the preview worker is busy");
    drop(release);
    assert_eq!(
        *segments,
        vec![RenderSegment {
            start_tick: 0,
            end_tick: duration,
            quality: RenderQuality::Yellow,
        }]
    );
}

/// A segment pass stops at the first frame after a newer request arrives, so
/// a run of edits costs one full pass, not one per edit. Does not cover the
/// thread hand-off between requests.
#[test]
fn a_superseded_segment_pass_stops_at_the_next_frame() {
    let (state, _) = animated_clip_state();
    let duration = state.store.sequence.duration_ticks;
    assert!(duration > 4, "fixture clip must span more than 4 frames");
    let project = state.host.snapshot();
    let files = state.render_files();

    let checks = std::cell::Cell::new(0);
    let abandoned = crate::app::render_bar::segments_unless(&project, &files, || {
        checks.set(checks.get() + 1);
        checks.get() > 3
    });
    assert_eq!(abandoned, None);
    assert_eq!(checks.get(), 4, "the pass ran past the superseding request");

    let full = crate::app::render_bar::segments_unless(&project, &files, || false);
    assert_eq!(
        full.as_deref(),
        Some(&*crate::app::render_bar::compute_render_segments(
            &project, &files
        ))
    );
}

/// A sequence whose segment pass outlasts the edit a test makes after
/// requesting it: 9600 frames over 4 lanes.
fn long_sequence() -> AppState {
    crate::app::premiere_tests::with_sequence(2, 200)
}

fn add_marker() -> Command {
    Command::Edit(EditCommand::AddMarker {
        at: RationalTime::new(24, Rational::FPS_24),
        name: "during the pass".into(),
        color: None,
    })
}

/// An edit made while a render-bar pass runs changes the project in place.
/// The pass reads the project under the engine lock one frame at a time and
/// keeps no snapshot, so the edit's copy-on-write finds the project unshared.
/// The project is sampled for 20 ms after the pass thread takes its request,
/// so a pass that holds a snapshot is seen holding it whatever the thread's
/// timing. Closes the class where the always-running segment pass held an
/// `Arc<Project>` across every edit and each edit of a long sequence copied
/// the whole project. Does not cover a render or export job, which keeps the
/// snapshot it renders until it ends.
#[test]
fn an_edit_during_a_render_bar_pass_changes_the_project_in_place() {
    let mut state = long_sequence();
    let _ = state.render_bar();
    let seq = state.render_bar_state.latest_seq();
    let deadline = Instant::now() + JOB_BOUND;
    while state.render_bar_state.is_pending() {
        assert!(
            Instant::now() < deadline,
            "no render-bar thread took the request within {JOB_BOUND:?}"
        );
        std::thread::yield_now();
    }
    let sampled = Instant::now() + Duration::from_millis(20);
    while Instant::now() < sampled {
        let holders = Arc::strong_count(&state.engine.lock().snapshot());
        assert_eq!(
            holders, 2,
            "the project is held beside the engine and this sample while the pass runs"
        );
    }
    let before = Arc::as_ptr(&state.host.snapshot());
    state.apply_command(add_marker()).expect("the marker lands");
    let after = Arc::as_ptr(&state.host.snapshot());
    assert!(
        state
            .render_bar_state
            .wait_within(seq, Duration::ZERO)
            .is_none(),
        "the pass completed before the edit, so the edit did not run during it"
    );
    assert_eq!(
        before, after,
        "the edit copied the project: the render-bar pass held a snapshot of it"
    );
}

/// A segment pass abandons once the engine moves past the revision it was
/// requested at, rather than record segments read from two projects. Does not
/// cover how soon the next request replaces it.
#[test]
fn a_render_bar_pass_abandons_once_the_project_moves_on() {
    let mut state = long_sequence();
    let _ = state.render_bar();
    let seq = state.render_bar_state.latest_seq();
    state
        .engine
        .lock()
        .apply(add_marker())
        .expect("the marker lands");
    let bar = state.render_bar_state;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(bar.wait_for(seq).is_some());
    });
    let completed = rx
        .recv_timeout(JOB_BOUND)
        .unwrap_or_else(|_| panic!("the pass did not end within {JOB_BOUND:?}"));
    assert!(
        !completed,
        "a pass recorded segments after the engine moved past its revision"
    );
}
