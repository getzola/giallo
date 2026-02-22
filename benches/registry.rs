use criterion::{Criterion, criterion_group, criterion_main};
use giallo::Registry;

fn registry_benchmark(c: &mut Criterion) {
    let dump = include_bytes!("../builtin.zst");

    c.bench_function("registry deserialization", |b| {
        b.iter(|| {
            let registry = Registry::load(dump).expect("Failed to load registry");
            std::hint::black_box(registry);
        })
    });
}

criterion_group!(benches, registry_benchmark);
criterion_main!(benches);
