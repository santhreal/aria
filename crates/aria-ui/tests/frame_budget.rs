//! The frame budget: what the editor is allowed to spend per adopted edit.
//!
//! What these defend: the two CPU costs a frame cannot avoid — projecting the
//! project into the view model, and building the magnet index from it — stay
//! within a ceiling on a stated workload, and grow with the sequence rather
//! than faster than it. Before this, a change that made projection quadratic
//! showed up as a slow editor and nothing else.
//!
//! How they avoid being a flaky clock: the absolute ceilings are the *shape* of
//! the failure (a tenfold regression), taken as the fastest of several runs so
//! a busy machine reports its best rather than its worst, and the scaling
//! assertion compares two arrangements of the *same* amount of work, so the
//! machine's cache, allocator and neighbors cancel out of the ratio. A test
//! that pinned a millisecond count would fail on somebody's laptop and teach
//! everyone to ignore it.
//!
//! A separate test binary, with one clock reading at a time: cargo runs test
//! binaries one after another, so no other test of the suite runs beside these.
//! In the library's binary they ran beside every other test of the crate: on
//! two cores with their SMT siblings the scaling ratio read 1.17x to 1.23x
//! over 40 runs alone, and 0.68x to 1.79x over 200 runs beside the library's
//! tests, 4 of them over the bound.
//!
//! What they do not catch: GPU cost, element construction (that needs a
//! window — `tests_shell.rs` drives it), and the first projection of a project
//! being loaded, which happens once and is not a frame. The magnet index's
//! contents are checked in the library's `magnet_index_tests`.

use parking_lot::Mutex;
use std::time::{Duration, Instant};

use aria_models::{Project, Rational, RationalTime};
use aria_ui::snap::{index_for, threshold_ticks};
use aria_ui::{project_to_ui, workload};

/// Held for the whole of every test here, so no two of them run at once and
/// none times another's setup.
static CLOCK: Mutex<()> = Mutex::new(());

/// The fastest of `runs` calls. The minimum, not the mean: every source of
/// noise on a shared machine adds time, so the floor is the measurement and
/// the spread is the interference.
fn fastest<T>(runs: usize, mut work: impl FnMut() -> T) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..runs {
        let at = Instant::now();
        let out = work();
        let taken = at.elapsed();
        drop(out);
        best = best.min(taken);
    }
    best
}

fn playhead() -> RationalTime {
    RationalTime::new(1_000, Rational::FPS_24)
}

fn project_cost(project: &Project) -> Duration {
    fastest(5, || project_to_ui(project, playhead(), &[]))
}

/// A frame's projection budget on the standard workload (480 clips, 3840
/// beats). Measured at 407 us (`docs/performance.md`); the ceiling is twenty
/// times that, so a machine several times slower than the recorded one passes
/// and a tenfold regression does not.
const PROJECTION_BUDGET: Duration = Duration::from_millis(8);

/// The same for building the magnet index out of a finished projection,
/// measured at 70 us.
const SNAP_INDEX_BUDGET: Duration = Duration::from_millis(2);

/// Ten thousand magnet queries — a drag held for a couple of seconds — at a
/// measured 46 ns each.
const SNAP_QUERY_BUDGET: Duration = Duration::from_millis(10);

#[test]
fn projection_stays_within_the_frame_budget() {
    let _clock = CLOCK.lock();
    let project = workload::standard();
    let cost = project_cost(&project);
    assert!(
        cost <= PROJECTION_BUDGET,
        "projecting the standard workload took {cost:?}, budget {PROJECTION_BUDGET:?}"
    );
}

#[test]
fn building_the_magnet_index_stays_within_the_frame_budget() {
    let _clock = CLOCK.lock();
    let store = project_to_ui(&workload::standard(), playhead(), &[]);
    let cost = fastest(5, || index_for(&store));
    assert!(
        cost <= SNAP_INDEX_BUDGET,
        "building the magnet index took {cost:?}, budget {SNAP_INDEX_BUDGET:?}"
    );
}

#[test]
fn a_drags_worth_of_magnet_queries_stays_within_the_frame_budget() {
    let _clock = CLOCK.lock();
    let store = project_to_ui(&workload::standard(), playhead(), &[]);
    let index = index_for(&store);
    let threshold = threshold_ticks(40.0, 24.0);
    let cost = fastest(3, || {
        let mut hits = 0usize;
        for step in 0..10_000i64 {
            let (_, snapped) = index.nearest((step * 37) % 6_000, threshold, Some(3), &[]);
            hits += usize::from(snapped.is_some());
        }
        hits
    });
    assert!(
        cost <= SNAP_QUERY_BUDGET,
        "10k magnet queries took {cost:?}, budget {SNAP_QUERY_BUDGET:?}"
    );
}

/// The cost of one whole sequence against the cost of projecting the same clips
/// as `split.len()` sequences of a fraction of the size, each taken as its own
/// floor over `rounds`.
///
/// Both sides do the same number of clip projections, touch the same total
/// bytes of input and produce the same total bytes of view model, so allocator
/// behavior and page faults are matched and cancel. What does not cancel is
/// work that grows faster than the clip count: a term quadratic in the size of
/// one sequence costs `split.len()` times more on the whole than on the pieces.
///
/// Every reading is a lower bound on the work, since interference can only add
/// time, so the floor of many readings is the least contaminated estimate of
/// each side. Both sides are projected once before either is timed.
fn equal_work(rounds: usize, whole: &Project, split: &[Project]) -> (f64, Duration, Duration) {
    let pieces = || {
        for one in split {
            drop(project_to_ui(one, playhead(), &[]));
        }
    };
    drop(project_to_ui(whole, playhead(), &[]));
    pieces();
    let mut one = Duration::MAX;
    let mut many = Duration::MAX;
    for _ in 0..rounds {
        one = one.min(project_cost(whole));
        many = many.min(fastest(5, pieces));
    }
    let ratio = one.as_secs_f64() / many.as_secs_f64().max(f64::EPSILON);
    (ratio, one, many)
}

/// **A clip costs the same to project however long the sequence is.**
///
/// Thirty-two lanes of sixty clips against eight separate sequences of four
/// lanes of sixty: the same 3840 clip projections either way, so a projection
/// whose cost follows the clip count reads 1x. A pass that walks the sequence
/// once per clip is sixty-four times the work on the whole against eight times
/// on the pieces, so it cannot hide in a matched comparison the way it hides in
/// a bigger-versus-smaller one, where an eightfold working set moves the number
/// as much as the algorithm does.
///
/// The bound is 1.4 against a baseline that reads 1.02x to 1.13x on this
/// machine, and a per-clip scan over every clip in the sequence reads 2.04x. A
/// truly quadratic term has to be worth about one per cent of the projection at
/// 480 clips to cross 1.4; below that this is blind, which is why the absolute
/// budget above is measured as well.
#[test]
fn projection_cost_follows_the_clip_count() {
    let _clock = CLOCK.lock();
    // A lane is a video/audio pair, so 32 lanes of 60 is 3840 clips, and each
    // of the eight pieces carries an eighth of them.
    let split: Vec<Project> = (0..8).map(|_| workload::sequence(4, 60)).collect();
    let (ratio, whole, pieces) = equal_work(3, &workload::sequence(32, 60), &split);
    assert!(
        ratio < 1.4,
        "one sequence cost {ratio:.2}x eight of an eighth ({whole:?} vs {pieces:?})"
    );
}
