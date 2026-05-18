//! Semantic-diff benchmarks (Phase 8 perf suite). `cargo bench -p gpp-diff`.

use criterion::{Criterion, criterion_group, criterion_main};

fn rust_src(n: usize, tweak: usize) -> Vec<u8> {
    let mut s = String::new();
    for i in 0..n {
        let body = if i == tweak { "n + 1" } else { "n" };
        s.push_str(&format!("fn f{i}(n: u64) -> u64 {{ {body} }}\n"));
    }
    s.into_bytes()
}

fn bench(c: &mut Criterion) {
    let old = rust_src(200, usize::MAX);
    let new = rust_src(200, 100); // one body changed

    c.bench_function("semantic_200_fns_one_change", |b| {
        b.iter(|| {
            std::hint::black_box(gpp_diff::semantic("m.rs", &old, &new).unwrap());
        });
    });

    c.bench_function("line_unified_200_fns", |b| {
        b.iter(|| {
            std::hint::black_box(gpp_diff::unified("m.rs", &old, &new));
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
