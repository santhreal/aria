//! Tests for thread admission.
//!
//! Closes the class "a job spawned while the manager is at its limit is lost":
//! a spawn past the limit used to fail on the spot, so a caller that spawns its
//! next job from inside the one finishing (the export queue does) dropped work
//! whenever a finishing thread had not yet given up its slot. Every case goes
//! through `spawn`, the one admission path. It does not cover a job that never
//! finishes: the jobs waiting behind it wait with it.

use parking_lot::Condvar;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

use super::*;

const SETTLE: Duration = Duration::from_secs(10);

/// A manager and every event it delivers, in delivery order.
struct Watched {
    manager: JobManager,
    events: mpsc::Receiver<JobSnapshot>,
    seen: Vec<JobSnapshot>,
}

impl Watched {
    fn new(limit: usize) -> Self {
        let manager = JobManager::with_concurrency_limit(limit);
        let (events_tx, events) = mpsc::channel();
        manager.subscribe(move |snapshot| {
            let _ = events_tx.send(snapshot);
        });
        Self {
            manager,
            events,
            seen: Vec::new(),
        }
    }

    /// Read events until each of `ids` has reached a terminal state.
    fn settle(&mut self, ids: &[JobId]) {
        while !ids.iter().all(|id| {
            self.seen
                .iter()
                .any(|seen| seen.id == *id && seen.state.is_terminal())
        }) {
            let next = self
                .events
                .recv_timeout(SETTLE)
                .expect("timed out waiting for job events");
            self.seen.push(next);
        }
    }

    /// The states `id` passed through, in order.
    fn states(&self, id: JobId) -> Vec<JobState> {
        self.seen
            .iter()
            .filter(|seen| seen.id == id)
            .map(|seen| seen.state)
            .collect()
    }

    fn last(&self, id: JobId) -> &JobSnapshot {
        self.seen
            .iter()
            .rev()
            .find(|seen| seen.id == id)
            .expect("no events for the job")
    }
}

/// Assert every slot is freed within [`SETTLE`] and no job waits. A job's
/// thread publishes its terminal state before it passes on or frees its slot,
/// so the last terminal event can arrive while that slot is still held.
fn assert_slots_freed(manager: &JobManager) {
    let deadline = Instant::now() + SETTLE;
    while manager.shared.active_threads.load(Ordering::SeqCst) != 0 {
        assert!(
            Instant::now() < deadline,
            "a slot stayed held after every job finished"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(manager.shared.waiting.lock().is_empty());
}

/// A gate jobs wait on until a test opens it.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn pass(&self) {
        let mut open = self.open.lock();
        let timeout = self
            .changed
            .wait_while_for(&mut open, |open| !*open, SETTLE);
        assert!(!timeout.timed_out(), "the gate never opened");
    }

    fn open(&self) {
        *self.open.lock() = true;
        self.changed.notify_all();
    }
}

/// Spawn a job that announces it started and then waits at `gate`.
fn waiting_job(
    manager: &JobManager,
    label: String,
    gate: &Arc<Gate>,
    started: &mpsc::Sender<String>,
) -> JobId {
    let gate = Arc::clone(gate);
    let started = started.clone();
    manager.spawn(label.clone(), move |_| {
        started.send(label).expect("announce");
        gate.pass();
        Ok(String::new())
    })
}

#[test]
fn exactly_the_limit_runs_at_once_and_every_job_finishes() {
    for limit in [1, 2, 3] {
        let mut watched = Watched::new(limit);
        let gate = Arc::new(Gate::default());
        let (started_tx, started_rx) = mpsc::channel();
        let ids: Vec<JobId> = (0..limit * 3)
            .map(|i| waiting_job(&watched.manager, format!("burst-{i}"), &gate, &started_tx))
            .collect();
        for _ in 0..limit {
            started_rx
                .recv_timeout(SETTLE)
                .unwrap_or_else(|_| panic!("limit {limit}: a slot stayed empty"));
        }
        // Admission is decided in `spawn`: everything past the limit waits,
        // Queued, with its closure not run.
        assert!(
            started_rx.try_recv().is_err(),
            "limit {limit}: a job past the limit started"
        );
        let states: Vec<JobState> = ids
            .iter()
            .map(|id| watched.manager.get(*id).expect("registered").state)
            .collect();
        let count = |state| states.iter().filter(|held| **held == state).count();
        assert_eq!(count(JobState::Running), limit, "{states:?}");
        assert_eq!(count(JobState::Queued), limit * 2, "{states:?}");

        gate.open();
        watched.settle(&ids);
        for id in &ids {
            assert_eq!(watched.last(*id).state, JobState::Done);
        }
        assert_slots_freed(&watched.manager);
    }
    assert!(
        JobManager::new().shared.concurrency_limit.is_some(),
        "the shipped constructor must be bounded"
    );
}

/// A manager at a limit of one whose only slot is held by a gated job.
fn gated() -> (Watched, Arc<Gate>, JobId) {
    let watched = Watched::new(1);
    let gate = Arc::new(Gate::default());
    let (started_tx, started_rx) = mpsc::channel();
    let holder = waiting_job(&watched.manager, "holder".into(), &gate, &started_tx);
    started_rx
        .recv_timeout(SETTLE)
        .expect("the holder did not start");
    (watched, gate, holder)
}

#[test]
fn a_waiting_job_runs_through_its_whole_lifecycle_once_a_slot_frees() {
    let (mut watched, gate, holder) = gated();
    let ran = Arc::new(AtomicBool::new(false));
    let waiting = watched.manager.spawn("waits", {
        let ran = Arc::clone(&ran);
        move |_| {
            ran.store(true, Ordering::SeqCst);
            Ok("waited".into())
        }
    });
    let snapshot = watched.manager.get(waiting).expect("registered");
    assert_eq!(snapshot.state, JobState::Queued);
    assert!(snapshot.cancellable);
    assert!(!ran.load(Ordering::SeqCst), "a job past the limit ran");

    gate.open();
    watched.settle(&[holder, waiting]);
    assert_eq!(
        watched.states(waiting),
        [JobState::Queued, JobState::Running, JobState::Done]
    );
    assert_eq!(watched.last(waiting).detail, "waited");
    // It started only once the holder had finished.
    let holder_done = watched
        .seen
        .iter()
        .position(|seen| seen.id == holder && seen.state.is_terminal());
    let waiting_ran = watched
        .seen
        .iter()
        .position(|seen| seen.id == waiting && seen.state == JobState::Running);
    assert!(holder_done < waiting_ran, "{:?}", watched.seen);
}

#[test]
fn waiting_jobs_start_in_spawn_order() {
    let (mut watched, gate, holder) = gated();
    let order = Arc::new(Mutex::new(Vec::new()));
    let ids: Vec<JobId> = (0..5)
        .map(|i| {
            let order = Arc::clone(&order);
            watched.manager.spawn(format!("ordered-{i}"), move |_| {
                order.lock().push(i);
                Ok(String::new())
            })
        })
        .collect();
    gate.open();
    watched.settle(&ids);
    watched.settle(&[holder]);
    assert_eq!(*order.lock(), [0, 1, 2, 3, 4]);
}

#[test]
fn a_job_cancelled_while_it_waits_never_runs() {
    let (mut watched, gate, holder) = gated();
    let cancelled = watched
        .manager
        .spawn("cancelled", |_| -> Result<String, String> {
            panic!("a job cancelled while waiting ran");
        });
    let after = watched.manager.spawn("after", |_| Ok("ran".into()));
    assert!(watched.manager.cancel(cancelled));
    assert!(watched.manager.get(cancelled).unwrap().cancel_requested);

    gate.open();
    watched.settle(&[holder, cancelled, after]);
    // Panic wins over cancellation, so a closure that ran would end Failed.
    assert_eq!(
        watched.states(cancelled),
        [JobState::Queued, JobState::Cancelled]
    );
    let last = watched.last(cancelled);
    assert_eq!(last.detail, admission::CANCELLED_WHILE_WAITING);
    assert!(!last.cancellable);
    // The slot passed on: the job behind it still ran.
    assert_eq!(watched.last(after).state, JobState::Done);
    assert_eq!(watched.last(after).detail, "ran");
    assert_slots_freed(&watched.manager);
}

/// The export queue's shape: each job spawns the next from inside its own
/// work, before its thread gives up its slot. At a limit of one, every spawn
/// lands while the slot is held.
#[test]
fn a_job_spawned_from_a_finishing_job_at_the_limit_still_runs() {
    fn chain(manager: JobManager, left: usize, done: mpsc::Sender<usize>) {
        let next = manager.clone();
        let _ = manager.spawn(format!("chain-{left}"), move |_| {
            done.send(left).expect("report");
            if left > 0 {
                chain(next, left - 1, done);
            }
            Ok(String::new())
        });
    }
    let manager = JobManager::with_concurrency_limit(1);
    let (done_tx, done_rx) = mpsc::channel();
    chain(manager, 20, done_tx);
    let reached: Vec<usize> = (0..=20)
        .map(|_| done_rx.recv_timeout(SETTLE).expect("the chain stopped"))
        .collect();
    assert_eq!(reached, (0..=20).rev().collect::<Vec<_>>());
}

/// Bursts from many threads at once never leave a job waiting with no
/// running job to start it, never exceed the limit, and keep the registry in
/// id order: an id taken before the registry lock let a later id land first,
/// and the binary search behind every state change then lost the job.
#[test]
fn concurrent_spawners_never_strand_a_job() {
    let mut watched = Watched::new(2);
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let ids: Vec<JobId> = std::thread::scope(|scope| {
        let spawners: Vec<_> = (0..8)
            .map(|spawner| {
                let manager = watched.manager.clone();
                let running = Arc::clone(&running);
                let peak = Arc::clone(&peak);
                scope.spawn(move || {
                    (0..16)
                        .map(|i| {
                            let running = Arc::clone(&running);
                            let peak = Arc::clone(&peak);
                            manager.spawn(format!("s{spawner}-{i}"), move |_| {
                                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                                peak.fetch_max(now, Ordering::SeqCst);
                                std::thread::yield_now();
                                running.fetch_sub(1, Ordering::SeqCst);
                                Ok(String::new())
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        spawners
            .into_iter()
            .flat_map(|spawner| spawner.join().expect("spawner"))
            .collect()
    });
    watched.settle(&ids);
    let listed: Vec<JobId> = watched.manager.snapshot().iter().map(|s| s.id).collect();
    assert!(
        listed.windows(2).all(|pair| pair[0] > pair[1]),
        "registry out of id order: {listed:?}"
    );
    assert!(
        ids.iter()
            .all(|id| watched.last(*id).state == JobState::Done)
    );
    assert!(peak.load(Ordering::SeqCst) <= 2);
    assert_slots_freed(&watched.manager);
}
