use criterion::{Criterion, criterion_group, criterion_main};
use giallo::{HighlightOptions, Registry, ThemeVariant};
use std::fs;

fn highlight_jquery_benchmark(c: &mut Criterion) {
    // Load registry once for all benchmarks
    let mut registry =
        Registry::load_from_file("builtin.zst").expect("Failed to load registry from builtin.zst");
    registry.link_grammars();

    // Read jQuery file content once
    let jquery_content =
        fs::read_to_string("src/fixtures/samples/jquery.js").expect("Failed to read jQuery file");

    let options = HighlightOptions::new("javascript", ThemeVariant::Single("vitesse-black"));

    c.bench_function("highlight jquery.js", |b| {
        b.iter(|| {
            registry.clear_pattern_cache();
            let result = registry.highlight(&jquery_content, &options).unwrap();
            std::hint::black_box(result);
        })
    });
}

fn highlight_simple_benchmark(c: &mut Criterion) {
    let mut registry =
        Registry::load_from_file("builtin.zst").expect("Failed to load registry from builtin.zst");
    registry.link_grammars();

    let ts_content = fs::read_to_string("src/fixtures/samples/simple.ts").unwrap();

    let options = HighlightOptions::new("typescript", ThemeVariant::Single("vitesse-black"));

    c.bench_function("highlight simple.ts", |b| {
        b.iter(|| {
            registry.clear_pattern_cache();
            let result = registry.highlight(&ts_content, &options).unwrap();
            std::hint::black_box(result);
        })
    });
}

fn highlight_multiple_simple_benchmark(c: &mut Criterion) {
    let mut registry =
        Registry::load_from_file("builtin.zst").expect("Failed to load registry from builtin.zst");
    registry.link_grammars();

    let ts_content = fs::read_to_string("src/fixtures/samples/simple.ts").unwrap();

    let options = HighlightOptions::new("typescript", ThemeVariant::Single("vitesse-black"));

    c.bench_function("highlight multiple simple.ts", |b| {
        b.iter(|| {
            // should not be 5x slower than "highlight simple.ts"
            registry.clear_pattern_cache();
            let result = registry.highlight(&ts_content, &options).unwrap();
            std::hint::black_box(result);
            let result = registry.highlight(&ts_content, &options).unwrap();
            std::hint::black_box(result);
            let result = registry.highlight(&ts_content, &options).unwrap();
            std::hint::black_box(result);
            let result = registry.highlight(&ts_content, &options).unwrap();
            std::hint::black_box(result);
            let result = registry.highlight(&ts_content, &options).unwrap();
            std::hint::black_box(result);
        })
    });
}

criterion_group!(
    benches,
    highlight_jquery_benchmark,
    highlight_simple_benchmark,
    highlight_multiple_simple_benchmark
);
criterion_main!(benches);
