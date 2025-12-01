use criterion::{Criterion, criterion_group, criterion_main};
use giallo::Registry;

fn registry_benchmark(c: &mut Criterion) {
    c.bench_function("registry load from file", |b| {
        b.iter(|| {
            let registry =
                Registry::load_from_file("builtin.msgpack").expect("Failed to load registry");
            std::hint::black_box(registry);
        })
    });
}

criterion_group!(benches, registry_benchmark);
criterion_main!(benches);
