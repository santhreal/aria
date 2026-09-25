//! Cost of constructing another text renderer after the host font catalog exists.

use aria_text::{TextRenderer, system_font_families};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn consecutive_system_font_access(c: &mut Criterion) {
    // Initialization is process startup. These benchmarks measure the repeated
    // path that every additional engine, renderer, and font picker takes.
    let first = TextRenderer::new();
    black_box(first.font_count());
    black_box(system_font_families());

    let mut group = c.benchmark_group("system_fonts/consecutive");
    group.bench_function("text_renderer", |b| {
        b.iter(|| black_box(TextRenderer::new()))
    });
    group.bench_function("family_list", |b| {
        b.iter(|| black_box(system_font_families()))
    });
    group.finish();
}

criterion_group!(benches, consecutive_system_font_access);
criterion_main!(benches);
