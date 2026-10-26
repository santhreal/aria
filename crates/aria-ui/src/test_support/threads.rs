//! Threads stay off the scheduler, and one module hands work to a thread.

use std::path::Path;

use super::collect_rs;

/// A wait that holds the thread it runs on, in the order a reader meets them.
const BLOCKING_WAITS: &[&str] = &[".recv()", ".recv_timeout(", ".recv_blocking(", ".join()"];

/// An expression that puts the lines after it on a scheduler thread.
const REACHES_THE_SCHEDULER: &[&str] = &["background_executor()", "executor.spawn("];

/// The one file allowed to block a task, and why.
///
/// `handoff::on_a_thread` joins a thread it started itself, so the wait is
/// as long as the work and the work cannot need the scheduler back.
const WAIT_OWNER: &str = "crates/aria-ui/src/handoff.rs";

/// **A task never blocks a scheduler thread.**
///
/// The defect this closes cost a day: `watch_jobs` awaited a task that ran
/// `crossbeam`'s blocking `recv()` on the background executor, and
/// `decode_preview` awaited one that ran `JoinHandle::join()`. gpui's test
/// scheduler runs a background task on the thread that ticks the run loop,
/// so both waits held that thread and `Shell::new` never returned: the whole
/// shell suite hung, with every thread parked and no CPU spent.
///
/// Production paid for it too — a scheduler thread spent waiting is a thread
/// not running the next frame's work — so the fix was not a test hack:
/// `crate::handoff` runs a blocking wait on a thread of its own and reports
/// the result as a future.
///
/// Derived from the sources: a blocking call added inside a
/// `background_executor()` expression anywhere in the workspace turns the
/// suite red on the run that introduces it.
#[test]
fn a_task_never_blocks_a_scheduler_thread() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut files = Vec::new();
    for root in ["crates", "apps"] {
        collect_rs(&workspace.join(root), &mut files);
    }
    assert!(
        files.len() > 500,
        "found {} source files under {workspace:?}, which cannot be this workspace: \
         the rule just went vacuous",
        files.len()
    );

    let mut spawns_seen = 0usize;
    let mut owner_waits = 0usize;
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // The census files name the calls and the waits they refuse, so they
        // match themselves.
        if rel.starts_with("crates/aria-ui/src/test_support") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !REACHES_THE_SCHEDULER
            .iter()
            .any(|reach| text.contains(reach))
        {
            continue;
        }
        if rel == WAIT_OWNER {
            // The owner is the one place the wait is allowed, so it is
            // counted rather than skipped: a rename or a rewrite that
            // removes the wait leaves this rule pointing at nothing.
            owner_waits += BLOCKING_WAITS
                .iter()
                .filter(|wait| text.contains(*wait))
                .count();
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !REACHES_THE_SCHEDULER
                .iter()
                .any(|reach| line.contains(reach))
            {
                continue;
            }
            spawns_seen += 1;
            // The expression can span a few lines: the spawn, the async
            // block, the body, the await. Eight covers every shape in the
            // tree and stops well before the next statement's own waits.
            let end = (index + 8).min(lines.len());
            for (offset, ahead) in lines[index..end].iter().enumerate() {
                for wait in BLOCKING_WAITS {
                    if ahead.contains(wait) {
                        offenders.push(format!(
                            "{rel}:{}: {} — {wait} inside the task spawned at line {}",
                            index + offset + 1,
                            ahead.trim(),
                            index + 1
                        ));
                    }
                }
            }
        }
    }
    assert!(
        spawns_seen > 0,
        "no line in the workspace reaches a scheduler thread: one of {REACHES_THE_SCHEDULER:?} \
         was renamed and this rule now matches nothing"
    );
    assert!(
        owner_waits > 0,
        "{WAIT_OWNER} no longer contains any of {BLOCKING_WAITS:?}: the owner of waiting \
         moved, so this rule is exempting a file that does nothing"
    );
    assert!(
        offenders.is_empty(),
        "a blocking wait on the background executor holds a scheduler thread, which \
         under gpui's test scheduler is the thread ticking the run loop: hand the work \
         to aria_ui::handoff::on_a_thread, or take the result as a handoff::wakes \
         queue:\n  {}",
        offenders.join("\n  ")
    );
}

/// A file allowed to start a thread of its own, and why.
const THREAD_OWNERS: &[(&str, &str)] = &[
    (
        "crates/aria-ui/src/handoff.rs",
        "the owner: one thread per blocking wait, reported back as a future",
    ),
    (
        "crates/aria-ui/src/audio.rs",
        "the mixer feeder: one long-lived device thread, joined when playback stops",
    ),
    (
        "crates/aria-ui/src/app/models.rs",
        "a scoped progress reader inside a job, never on the window's thread",
    ),
    (
        "crates/aria-ui/src/test_support/threads.rs",
        "the rule here names the calls it refuses, and would match itself",
    ),
    (
        "crates/aria-ui/src/scratch_pool.rs",
        "the pool's own rules: a thread that finds every slot held, and a \
         holder that panics while it has one",
    ),
    (
        "crates/aria-ui/src/app/view_events.rs",
        "a plugin's context menu row: plugin code that may edit through the \
         project host or block, started off the window's thread and never \
         waited for",
    ),
    (
        "crates/aria-ui/src/preview_worker.rs",
        "the preview worker: one render thread per session, fed by a job queue \
         and stopped when the session drops its handle",
    ),
    (
        "crates/aria-ui/src/app/render_bar.rs",
        "the render bar: at most one thread drains the latest segment request, \
         is never joined, and reports through a handoff wake",
    ),
    (
        "crates/aria-ui/src/app/recovery.rs",
        "the autosave writer: at most one recovery copy in flight, joined on \
         the window's thread before the copy is rewritten or removed and when \
         the session drops; the write never needs the scheduler",
    ),
    (
        "crates/aria-ui/src/app/export_options.rs",
        "the codec probe waiter: at most one per session, started when the \
         export sheet opens before the encoder's probe has answered, never \
         joined, and reports through the preview worker's handoff wake",
    ),
];

/// **Handing work to a thread belongs to one module.**
///
/// Four modules used to spawn a thread and then await its `join()` on the
/// executor — the preview decode, the filmstrip, the pool thumbnail and the
/// waveform — which is four copies of the deadlock above. A wait has one
/// owner now, so the fifth caller inherits the fix instead of the defect.
///
/// A new thread is not forbidden, it is declared: adding one turns this red
/// until its reason is written down, which is the point.
#[test]
fn only_one_module_hands_work_to_a_thread() {
    let crate_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut files = Vec::new();
    collect_rs(&crate_src, &mut files);
    collect_rs(&workspace.join("apps"), &mut files);
    assert!(
        files.len() > 200,
        "found {} files under {crate_src:?}, which cannot be this crate: the rule just \
         went vacuous",
        files.len()
    );

    let mut owners_seen = 0usize;
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // A `_tests.rs` file is test-only by this crate's naming rule: a
        // test driving a host from another thread is the test's own turn to
        // spend, not a window's.
        if rel.ends_with("_tests.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let starts = ["thread::spawn", "thread::Builder", "thread::scope"];
        if !starts.iter().any(|start| text.contains(start)) {
            continue;
        }
        if THREAD_OWNERS.iter().any(|(name, _)| rel == *name) {
            owners_seen += 1;
            continue;
        }
        for (number, line) in text.lines().enumerate() {
            if starts.iter().any(|start| line.contains(start)) {
                offenders.push(format!("{rel}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert_eq!(
        owners_seen,
        THREAD_OWNERS.len(),
        "{} of the {} declared thread owners still start one; a renamed owner leaves a \
         rule that matches nothing",
        owners_seen,
        THREAD_OWNERS.len()
    );
    assert!(
        offenders.is_empty(),
        "work that blocks goes to aria_ui::handoff::on_a_thread, which reports the \
         result as a future (owners declared: {}):\n  {}",
        THREAD_OWNERS.len(),
        offenders.join("\n  ")
    );
}
