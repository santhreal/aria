//! Scratch directories for the crate's tests: five fixed paths, reused forever.
//!
//! Three designs failed here, all the same way, because all three let the number
//! of directories follow the number of calls:
//!
//! - a fresh `temp_dir()/aria-<prefix>-<pid>-<n>` per call, deleted by nothing,
//!   until a small tmpfs filled and unrelated tests failed with `StorageFull`;
//! - the same, pruned by age, which cannot touch the running suite's own dirs and
//!   left 1448 siblings behind while making every call re-scan the parent;
//! - one `run-<pid>` parent per process, which bounded the parent's entry count
//!   and put 842 children inside it instead.
//!
//! Bounding a flow was the wrong shape. There is no flow now. Every path this
//! module can name is fixed: slot `n` is `temp_dir()/aria-scratch-<n>`, and a
//! test's directory is `aria-scratch-<n>/<prefix>`. Both are created the first
//! time they are used and reused by every run after that, so the thousandth run
//! creates nothing. There is no pid, no counter, and no parent directory, since
//! a parent would be a sixth path.
//!
//! One slot per test thread, leased the first time that thread asks and returned
//! when the thread ends. A thread only ever empties its own directories, so a
//! fast thread cannot delete the files a slow test on another thread is still
//! asserting on. A thread that finds every slot held *waits* for one: two live
//! threads in one slot would wipe each other's files, and the suite is larger
//! than the pool on purpose, so the pool bounds concurrency instead of refusing
//! work. `cargo test` therefore needs no `--test-threads` flag at any width.
//!
//! The lease is the whole design. libtest runs each test on its own thread, so a
//! slot claimed for the life of the process is a slot lost: five tests would
//! pass and the sixth would find the pool empty, which is exactly what happened.
//!
//! A [`Pool`] is a value rather than a set of statics, and that is the fourth
//! design defect being closed. The rules about waiting can only be tested by a
//! test that fills a pool and holds it, and while those tests spoke to the
//! process-wide pool, filling it meant filling the pool the other 1400 tests
//! were queueing on: every one of them blocked for as long as the drain test
//! held all five slots, several hit the deadlock bound and failed, and when one
//! of the drain test's own holder threads lost the race for a slot and panicked
//! at the bound, the untimed barrier it never reached hung the whole suite until
//! the runner's timeout killed it. A test about a full pool now builds its own,
//! with its own root and its own bound, and the process-wide pool is one
//! instance of the same type.
//!
//! The fifth defect was in the wait itself: it was bounded by the waiter's own
//! clock, so a waiter that queued for two minutes behind holders that were
//! working was declared a deadlock and failed. A suite of 1500 tests through
//! five slots queues by design, and a test that bakes a few seconds of video
//! holds a slot for seconds, so the waiter's clock says nothing. What separates
//! a queue from a deadlock is whether the pool is handing slots back at all, so
//! the bound is on that: a [`Stall`] restarts its window every time any holder
//! returns a slot, and only a window in which nothing came back anywhere is
//! called a deadlock.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The fixed name every pool directory starts with.
const NAME: &str = "aria-scratch";

/// How many scratch slots exist. Not a budget, a count: these are all of them,
/// for every run, on every machine.
pub(crate) const SLOTS: usize = 5;

/// How long a thread waits without a single slot coming back before it calls the
/// pool deadlocked.
///
/// The bound is on progress, not on this thread's own wait. A suite far wider
/// than the pool queues on it deliberately, and a test that holds a slot while
/// it encodes a few seconds of video is slow rather than stuck, so "I waited two
/// minutes" says nothing: five tests running long enough to fill this window
/// once is an ordinary busy run. "No slot came back anywhere in the pool for two
/// minutes" is the deadlock, and it still fails loudly inside a bounded wait.
const WAIT: Duration = Duration::from_secs(120);

/// A fixed set of scratch slots under one root, leased one holder at a time.
pub(crate) struct Pool {
    /// Where this pool's slots live. The process pool's root is the system temp
    /// dir; a test's own pool is rooted inside a slot of the process pool, so a
    /// test that fills a pool fills one it owns.
    root: PathBuf,
    /// How many slots exist. Fixed at construction: a pool does not grow.
    slots: usize,
    /// How long [`Pool::lease`] waits before calling the pool deadlocked.
    wait: Duration,
    /// The slots nobody is holding, and the count of returns that made them
    /// free.
    free: Mutex<Free>,
    /// Signalled when a slot comes back.
    ready: Condvar,
}

impl Pool {
    /// A pool of `slots` directories under `root`, waiting `wait` for one.
    fn new(root: PathBuf, slots: usize, wait: Duration) -> Self {
        Self {
            root,
            slots,
            wait,
            free: Mutex::new(Free {
                slots: (0..slots).rev().collect(),
                released: 0,
            }),
            ready: Condvar::new(),
        }
    }

    /// A slot, waiting for one while every slot is held.
    ///
    /// Panics when no slot comes back inside this pool's [`Pool::wait`]: a test
    /// that cannot get a scratch dir has nothing to assert, and a bounded wait
    /// that names the deadlock beats a suite that stops without saying why. The
    /// window restarts every time any holder returns a slot, so waiting behind a
    /// busy pool is unbounded in time and bounded in stalling.
    fn lease(&self) -> Lease<'_> {
        let mut free = self.free();
        let mut stall = Stall::new(free.released);
        loop {
            if let Some(index) = free.slots.pop() {
                return Lease { pool: self, index };
            }
            free = self
                .ready
                .wait_timeout(free, self.wait)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
            stall.observe(free.released);
            assert!(
                !stall.stalled(self.wait) || !free.slots.is_empty(),
                "no scratch slot came back in {:?}: all {} are held by threads that are \
                 not ending, which is a deadlock rather than a busy suite",
                self.wait,
                self.slots
            );
        }
    }

    /// The free list, readable through a poisoned lock: a slot returned by a
    /// panicking thread is still a free slot, and refusing to hand it out would
    /// turn one failed test into a failed suite.
    fn free(&self) -> MutexGuard<'_, Free> {
        self.free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Slot `index`'s directory, named rather than created.
    fn slot(&self, index: usize) -> PathBuf {
        self.root.join(format!("{NAME}-{index}"))
    }

    /// Every path this pool can name, for the rule that asserts there are five.
    fn paths(&self) -> Vec<PathBuf> {
        (0..self.slots).map(|index| self.slot(index)).collect()
    }

    /// Whether `path` is inside this pool. A caller writing outside the pool is
    /// the defect this module exists to prevent.
    fn holds(&self, path: &Path) -> bool {
        self.paths().iter().any(|slot| path.starts_with(slot))
    }
}

/// A slot held for as long as the lease lives, which for a test thread is as
/// long as the thread runs.
struct Lease<'pool> {
    pool: &'pool Pool,
    index: usize,
}

impl Lease<'_> {
    /// The directory named `prefix` inside this lease's slot, empty.
    ///
    /// The path is the same on every run: nothing here is unique, and nothing
    /// accumulates. Emptying happens on the way in, so a test never sees the
    /// previous occupant's files, and the directory that survives a run is one
    /// of the same fixed paths the next run will reuse.
    fn dir(&self, prefix: &str) -> PathBuf {
        let slot = self.pool.slot(self.index);
        std::fs::create_dir_all(&slot).expect("create a scratch slot");
        let dir = slot.join(prefix);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create a scratch dir");
        dir
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        // A test that panicked still returns its slot: the unwind runs this,
        // and the next thread gets a directory it will empty on the way in.
        let mut free = self.pool.free();
        free.slots.push(self.index);
        // The return is the pool's proof of life, so it is counted where it
        // happens: a waiter reads the count rather than the clock.
        free.released += 1;
        drop(free);
        // One slot came back, so one waiting thread can move.
        self.pool.ready.notify_one();
    }
}

/// The free list and how many leases have ever come back to it.
///
/// The counter exists because "the pool is full" and "the pool is stuck" look
/// identical from a waiter's clock. It only ever increases, so a waiter that
/// sees it move knows some other holder finished, whatever happened to its own
/// wait.
struct Free {
    /// The slots nobody is holding.
    slots: Vec<usize>,
    /// How many leases have been returned, for the whole life of the pool.
    released: u64,
}

/// How long a waiter has gone without the pool handing anything back.
///
/// This is the whole difference between a queue and a deadlock, and it is a
/// value so it can be asserted on directly. The state it describes cannot be
/// built to order out of threads: a waiter that is denied a slot for longer than
/// the bound *while the pool keeps cycling* needs to lose every handoff in that
/// window, and who wins a handoff is the scheduler's decision, not a test's.
struct Stall {
    /// The release count when the pool was last seen making progress.
    seen: u64,
    /// When that was.
    since: Instant,
}

impl Stall {
    /// A window that starts now, against a pool that has released `released`.
    fn new(released: u64) -> Self {
        Self {
            seen: released,
            since: Instant::now(),
        }
    }

    /// Read the pool's release count, restarting the window when it moved.
    fn observe(&mut self, released: u64) {
        if released != self.seen {
            self.seen = released;
            self.since = Instant::now();
        }
    }

    /// Whether nothing has come back for `wait`. Measured from the last release
    /// rather than from the start of the wait, so a waiter behind two hundred
    /// slow tests is queueing and a waiter behind five stopped threads is not.
    fn stalled(&self, wait: Duration) -> bool {
        self.since.elapsed() >= wait
    }
}

/// The one pool the suite shares: five slots in the system temp dir.
static POOL: LazyLock<Pool> = LazyLock::new(|| Pool::new(std::env::temp_dir(), SLOTS, WAIT));

/// The shared pool, borrowed for the life of the process.
fn pool() -> &'static Pool {
    &POOL
}

thread_local! {
    /// This thread's lease on the shared pool, taken once and returned on exit.
    static SLOT: OnceCell<Lease<'static>> = const { OnceCell::new() };
}

/// The directory named `prefix` inside the calling thread's slot, empty.
pub(crate) fn scratch(prefix: &str) -> PathBuf {
    SLOT.with(|slot| slot.get_or_init(|| pool().lease()).dir(prefix))
}

/// Every scratch path that exists, for the rule that asserts there are five.
pub(crate) fn slot_paths() -> Vec<PathBuf> {
    pool().paths()
}

/// Whether `path` is inside the shared pool.
pub(crate) fn is_pooled(path: &Path) -> bool {
    pool().holds(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A pool of this test's own, rooted inside one slot of the shared pool and
    /// waiting `wait` for a slot of its own.
    ///
    /// Every rule about a *full* pool is tested against one of these, and this
    /// is the only place in the tests that claims a shared slot. Filling the
    /// shared pool instead is what hung the suite: it made the other 1400 tests
    /// queue, and the ones that queued past the bound failed.
    fn own_pool(prefix: &str, slots: usize, wait: Duration) -> Pool {
        Pool::new(scratch(prefix), slots, wait)
    }

    /// Long enough that a slot handed over on another thread arrives, short
    /// enough that a pool which never hands one over fails inside a test run.
    const SOON: Duration = Duration::from_millis(500);

    /// The defect this module shipped three times, as an invariant: the number
    /// of directories does not follow the number of calls. Fifty claims, five
    /// paths, and the paths are the ones the pool can name.
    ///
    /// Every slot is touched first, because a sequential claim always gets the
    /// slot the last one gave back: without the first pass this would pass with
    /// one directory, and would still pass if the pool had a thousand slots.
    #[test]
    fn the_pool_never_grows() {
        let pool = own_pool("growth", SLOTS, SOON);
        let touched: Vec<Lease<'_>> = (0..SLOTS).map(|_| pool.lease()).collect();
        for lease in &touched {
            lease.dir("touch");
        }
        drop(touched);
        for round in 0..SLOTS * 10 {
            let dir = pool.lease().dir(&format!("growth-{}", round % 3));
            std::fs::write(dir.join("f"), b"x").expect("write inside a slot");
        }
        let mut found: Vec<String> = std::fs::read_dir(&pool.root)
            .expect("read the pool root")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        let expected: Vec<String> = (0..SLOTS).map(|index| format!("{NAME}-{index}")).collect();
        assert_eq!(
            found,
            expected,
            "fifty claims made {} directories, so the count still follows the calls",
            found.len()
        );
    }

    /// A lease is a loan, not a spend: the same pool answers forever. Ten times
    /// the pool through a pool of one is the shape of a suite bigger than the
    /// pool, without making anything else wait for it.
    #[test]
    fn a_lease_comes_back_when_it_drops() {
        let pool = own_pool("reclaimed", 1, SOON);
        for round in 0..SLOTS * 10 {
            let dir = pool.lease().dir(&format!("round-{}", round % 3));
            assert!(pool.holds(&dir), "{dir:?} came from outside the pool");
        }
    }

    /// The defect the lease closes: libtest runs each test on its own thread, so
    /// a slot held for the life of the process is a slot lost — five tests
    /// passed and the sixth found the pool empty. One extra thread at a time, so
    /// this proves the release without holding a second slot.
    ///
    /// This is the one place in the crate that leases a *shared* slot from a
    /// thread it spawned, because thread exit is the thing being asserted. The
    /// rule below counts the site.
    #[test]
    fn a_slot_comes_back_when_its_thread_ends() {
        for round in 0..SLOTS + 1 {
            let dir = std::thread::spawn(move || scratch(&format!("lease-{}", round % 3)))
                .join()
                .expect("the thread took a slot");
            assert!(is_pooled(&dir), "{dir:?} came from outside the pool");
        }
    }

    /// And a holder that panics returns its slot: one failing test must not cost
    /// the rest of the suite a directory to write in.
    #[test]
    fn a_panicking_holder_returns_its_slot() {
        let pool = own_pool("panicking", 1, SOON);
        std::thread::scope(|scope| {
            for _ in 0..SLOTS * 2 {
                let panicked = scope
                    .spawn(|| {
                        let _lease = pool.lease();
                        panic!("a test failing while it holds a slot");
                    })
                    .join();
                assert!(panicked.is_err(), "the thread was supposed to panic");
            }
            let dir = pool.lease().dir("after-the-panics");
            assert!(pool.holds(&dir), "the panics leaked the pool");
        });
    }

    /// A suite wider than the pool is the normal case, so a thread that arrives
    /// when every slot is held waits for one instead of being refused. Before
    /// the wait existed this panicked, and `cargo test -p aria-ui` only passed
    /// with `--test-threads 5` or fewer — a flag nobody passes by accident.
    ///
    /// Deterministic and bounded in both directions: the pool is full because
    /// this thread is holding every lease in it, the waiter is proved to be
    /// waiting by a receive that must time out, and the slot it then gets is
    /// proved to arrive by a receive that must not. Nothing here can hang,
    /// which the barrier handshake this replaced could and did.
    #[test]
    fn a_thread_that_finds_the_pool_empty_waits_for_a_slot() {
        let pool = own_pool("waiting", 2, Duration::from_secs(5));
        let held: Vec<Lease<'_>> = (0..2).map(|_| pool.lease()).collect();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let lease = pool.lease();
                let _ = tx.send(lease.dir("late"));
            });
            assert!(
                rx.recv_timeout(Duration::from_millis(150)).is_err(),
                "a thread got a slot out of a pool whose every slot is held here"
            );
            drop(held);
            let dir = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("the waiting thread was handed the returned slot");
            assert!(pool.holds(&dir), "{dir:?} came from outside the pool");
        });
    }

    /// The wait ends, and it ends when this pool said it would. A pool nobody
    /// returns a slot to fails by name inside its own bound rather than stopping
    /// the run: a suite that hangs is read as a hung machine, and the last one
    /// cost an hour and told nobody anything.
    ///
    /// The elapsed time is asserted, not just the panic. A wait that ignored the
    /// pool's bound and used the process pool's two minutes would still fail
    /// here, two minutes later, and a rule that cannot tell those apart is how a
    /// suite ends up hanging again.
    #[test]
    fn a_pool_nobody_returns_a_slot_to_fails_inside_its_bound() {
        let pool = own_pool("deadlocked", 1, Duration::from_millis(50));
        let _held = pool.lease();
        let started = std::time::Instant::now();
        let refused = std::thread::scope(|scope| scope.spawn(|| pool.lease().index).join());
        let took = started.elapsed();
        let panic = refused.expect_err("a lease nobody can satisfy has to fail");
        let said = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or_default();
        assert!(
            said.contains("deadlock"),
            "the bound was hit but the message does not name a deadlock: {said:?}"
        );
        assert!(
            took < Duration::from_secs(2),
            "the pool was given a 50ms bound and took {took:?}, so it is waiting on \
             somebody else's clock"
        );
    }

    /// And the other side of that bound, which is the defect it shipped with: a
    /// pool that is merely *busy* is not deadlocked, however long a waiter sits
    /// in it. The first version timed the waiter's own wait, so five tests that
    /// each held a slot through a real encode were indistinguishable from five
    /// threads that had stopped, and `cargo test -p aria-ui --lib` failed five
    /// census tests at the two-minute bound on a run where nothing was stuck:
    /// 1497 tests passed around them, which is what a queue looks like.
    ///
    /// The window is asserted here rather than raced for, because the state that
    /// separates the two cases cannot be built out of threads on demand: a
    /// waiter that is denied for longer than the bound while the pool keeps
    /// cycling has to lose every handoff in that window, and the scheduler
    /// decides that, not the test. So the window is a value, and this presses
    /// both of its answers against a clock it controls.
    #[test]
    fn a_release_anywhere_in_the_pool_restarts_the_deadlock_window() {
        let bound = Duration::from_millis(30);

        // Nothing came back, and the bound has passed: stuck.
        let quiet = Stall::new(7);
        std::thread::sleep(bound * 2);
        assert!(
            quiet.stalled(bound),
            "no release in twice the bound is the deadlock this is here to name"
        );

        // A release lands inside the window: busy, and the window restarts from
        // the release rather than from the start of the wait.
        let mut cycling = Stall::new(7);
        std::thread::sleep(bound * 2);
        cycling.observe(8);
        assert!(
            !cycling.stalled(bound),
            "a pool that handed a slot back was read as a pool that had stopped"
        );

        // And the same count twice is not progress: a holder that re-took the
        // slot it returned leaves the count where a second waiter last saw it.
        let mut repeated = Stall::new(8);
        repeated.observe(8);
        std::thread::sleep(bound * 2);
        assert!(
            repeated.stalled(bound),
            "an unchanged count was read as a release"
        );
    }

    /// The same rule through the real path: a pool whose one slot is handed
    /// around serves a waiter instead of refusing it, and the run ends.
    ///
    /// Bounded in both directions — the cycling stops after a fixed number of
    /// rounds, so a waiter that is genuinely stuck fails the receive rather than
    /// hanging the suite, and the bound is far below the recv timeout.
    #[test]
    fn a_waiter_on_a_pool_that_keeps_cycling_is_served() {
        let bound = Duration::from_millis(20);
        let pool = own_pool("busy", 1, bound);
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            // A holder that keeps taking the one slot and handing it back: the
            // pool is full most of the time, and it is working.
            scope.spawn(|| {
                for _ in 0..40 {
                    let lease = pool.lease();
                    std::thread::sleep(bound / 4);
                    drop(lease);
                }
            });
            scope.spawn(|| {
                let lease = pool.lease();
                let _ = tx.send(lease.index);
            });
            let index = rx
                .recv_timeout(Duration::from_secs(30))
                .expect("a waiter on a busy pool is served, not refused");
            assert_eq!(index, 0, "the pool has one slot");
        });
    }

    /// A prefix names one directory, reused. Two calls with the same prefix are
    /// the same path, emptied, rather than two paths.
    #[test]
    fn a_prefix_is_one_directory_reused() {
        let first = scratch("reused");
        std::fs::write(first.join("left-behind"), b"x").expect("write");
        let second = scratch("reused");
        assert_eq!(first, second, "the same prefix is the same directory");
        assert_eq!(
            std::fs::read_dir(&second).unwrap().count(),
            0,
            "claiming a directory empties it"
        );
    }

    /// Two prefixes are two directories, so a test that needs a source and a
    /// destination can hold both at once.
    #[test]
    fn two_prefixes_are_two_directories() {
        let a = scratch("source");
        let b = scratch("destination");
        assert_ne!(a, b);
        std::fs::write(a.join("f"), b"a").expect("write a");
        std::fs::write(b.join("f"), b"b").expect("write b");
        assert_eq!(
            std::fs::read(a.join("f")).unwrap(),
            b"a",
            "b did not clobber a"
        );
    }

    /// Every path handed out is inside the pool. A path outside it is the class
    /// of defect that filled the disk.
    #[test]
    fn every_scratch_path_is_inside_the_pool() {
        let dir = scratch("pooled");
        assert!(is_pooled(&dir), "{dir:?} is outside the pool");
        assert!(!is_pooled(&std::env::temp_dir().join("aria-elsewhere")));
        assert_eq!(slot_paths().len(), SLOTS);
    }

    /// An expression that puts the lines after it on a thread of its own. The
    /// same three `only_one_module_hands_work_to_a_thread` refuses, so "starts a
    /// thread" has one definition: a scoped spawn is reached through the
    /// `thread::scope` that has to contain it, whatever the binder is called.
    const SPAWNS: &[&str] = &["thread::spawn", "thread::Builder", "thread::scope"];

    /// How far past a spawn a claim counts as being inside it.
    const REACH: usize = 10;

    /// The test that is allowed to lease a shared slot from a thread, because
    /// the release on thread exit is the thing it asserts.
    const OWNER: &str = "a_slot_comes_back_when_its_thread_ends";

    /// **Only one test leases a shared slot from a thread it spawned.**
    ///
    /// A test that spawns threads which each call [`scratch`] holds more than one
    /// of the five shared slots at once, and while it holds them every other
    /// test in a 1400-test suite queues behind it. One such test existed: it
    /// held all five deliberately, to prove the wait, and it made the rest of
    /// the suite wait for real, failed several tests at the deadlock bound, and
    /// eventually hung the run outright when one of its own threads lost the
    /// race for a slot. Filling a pool is legitimate; filling *this* pool is
    /// not, so a rule about the full pool builds its own with [`Pool::new`].
    ///
    /// Source-derived and exact rather than a list of known call sites: a second
    /// site goes red the moment it exists, and losing the first one goes red
    /// too, so the rule cannot end up pointing at nothing.
    #[test]
    fn only_the_thread_exit_rule_leases_a_shared_slot_from_a_thread() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        crate::test_support::collect_rs(&src, &mut files);
        assert!(
            files.len() > 50,
            "found {} source files under {src:?}, which cannot be this crate: the rule \
             just went vacuous",
            files.len()
        );
        let mut sites = Vec::new();
        for file in &files {
            let rel = file
                .strip_prefix(&src)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            let lines: Vec<&str> = text.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                if !SPAWNS.iter().any(|start| line.contains(start)) {
                    continue;
                }
                let end = (index + 1 + REACH).min(lines.len());
                for (offset, ahead) in lines[index..end].iter().enumerate() {
                    if ahead.contains("scratch(") {
                        sites.push(format!(
                            "{rel}:{}: {}",
                            index + offset + 1,
                            ahead.trim().trim_end_matches(|c: char| c == '{')
                        ));
                        break;
                    }
                }
            }
        }
        assert_eq!(
            sites.len(),
            1,
            "a shared slot is leased from a spawned thread in {} places; exactly one is \
             allowed, `{OWNER}`, and any other one makes the rest of the suite queue behind \
             it — build a pool with `Pool::new` instead:\n  {}",
            sites.len(),
            sites.join("\n  ")
        );
        assert!(
            sites[0].starts_with("scratch_pool.rs:"),
            "the one allowed site moved out of this module, so the rule is exempting \
             something it was not written for: {}",
            sites[0]
        );
    }
}
