//! Object store throughput benchmarks (Phase 8 perf suite).
//!
//! Run: `cargo bench -p gpp-core`. Targets from the roadmap: object-store
//! hot read < 1ms; these establish the baseline.

use criterion::{Criterion, criterion_group, criterion_main};
use gpp_core::{Blob, ObjectStore};

fn bench(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let store = ObjectStore::init(dir.path()).unwrap();
    let payload = vec![0xABu8; 4096];

    c.bench_function("write_4k_blob", |b| {
        let mut n = 0u64;
        b.iter(|| {
            n += 1;
            let mut p = payload.clone();
            p.extend_from_slice(&n.to_le_bytes()); // unique → real write
            std::hint::black_box(store.write(&Blob::new(p)).unwrap());
        });
    });

    let id = store.write(&Blob::new(payload.clone())).unwrap();
    c.bench_function("read_4k_blob_hot", |b| {
        b.iter(|| {
            std::hint::black_box(store.read::<Blob>(&id).unwrap());
        });
    });

    c.bench_function("read_raw_4k", |b| {
        b.iter(|| {
            std::hint::black_box(store.read_raw(&id).unwrap());
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
