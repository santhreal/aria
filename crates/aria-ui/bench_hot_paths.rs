//! What a frame costs before the GPU sees anything.
//!
//! Run: `cargo bench -p aria-ui --bench ui_hot_paths`
//!
//! Three measurements, in the order a frame pays them:
//!
//! - `projection` — the project into the view model, once per adopted edit.
//! - `snap_index` — the magnet set built from that projection.
//! - `snap_query` — one magnet lookup, which a drag pays per pointer move.
//!
//! The workload is [`aria_ui::workload::standard`], the same one the frame
//! budget test asserts against, so a bench number and a gate failure describe
//! the same sequence. Recorded baselines are in `docs/performance.md`.

use aria_models::{Rational, RationalTime};
use aria_ui::snap::{SnapCandidate, SnapKind};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn hot_paths(c: &mut Criterion) {
    let project = aria_ui::workload::standard();
    let playhead = RationalTime::new(1_000, Rational::FPS_24);

    c.bench_function("projection", |b| {
        b.iter(|| {
            black_box(aria_ui::project_to_ui(
                black_box(&project),
                playhead,
                black_box(&[]),
            ))
        })
    });

    let store = aria_ui::project_to_ui(&project, playhead, &[]);
    c.bench_function("snap_index", |b| {
        b.iter(|| black_box(aria_ui::snap::index_for(black_box(&store))))
    });

    let index = aria_ui::snap::index_for(&store);
    let loose = [SnapCandidate::new(1_000, SnapKind::Playhead)];
    let threshold = aria_ui::snap::threshold_ticks(40.0, 24.0);
    c.bench_function("snap_query", |b| {
        // Walking the drag across the sequence rather than querying one tick:
        // a fixed tick would measure one cache line.
        let mut at = 0i64;
        b.iter(|| {
            at = (at + 37) % 6_000;
            black_box(index.nearest(black_box(at), threshold, Some(3), &loose))
        })
    });
}

criterion_group!(benches, hot_paths);
criterion_main!(benches);
