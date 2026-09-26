use std::cell::RefCell;
use std::fs;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use giallo::{HighlightOptions, Registry, ThemeVariant};

const SAMPLES: &[(&str, &str)] = &[
    ("javascript", "src/fixtures/samples/jquery.js"),
    ("javascript", "grammars-themes/samples/javascript.sample"),
    ("typescript", "grammars-themes/samples/typescript.sample"),
    ("tsx", "grammars-themes/samples/tsx.sample"),
    ("rust", "grammars-themes/samples/rust.sample"),
    ("c", "grammars-themes/samples/c.sample"),
    ("markdown", "grammars-themes/samples/markdown.sample"),
    ("python", "grammars-themes/samples/python.sample"),
    ("html", "grammars-themes/samples/html.sample"),
    ("css", "grammars-themes/samples/css.sample"),
    ("ruby", "grammars-themes/samples/ruby.sample"),
    ("go", "grammars-themes/samples/go.sample"),
    ("astro", "grammars-themes/samples/astro.sample"),
    ("c#", "grammars-themes/samples/csharp.sample"),
    ("java", "grammars-themes/samples/java.sample"),
    ("php", "grammars-themes/samples/php.sample"),
    ("json", "grammars-themes/samples/json.sample"),
    ("shellscript", "grammars-themes/samples/shellscript.sample"),
];

fn bench_name(grammar: &'static str, path: &str) -> &'static str {
    if path.contains("jquery") {
        "jquery"
    } else {
        grammar
    }
}

fn highlight_simple_benchmark(c: &mut Criterion) {
    let mut registry =
        Registry::load_from_file("builtin.zst").expect("Failed to load registry from builtin.zst");
    registry.link_grammars();
    let registry = RefCell::new(registry);

    let ts_content = fs::read_to_string("src/fixtures/samples/simple.ts").unwrap();

    let options = HighlightOptions::new("typescript", ThemeVariant::Single("vitesse-black"));

    c.bench_function("highlight simple.ts", |b| {
        b.iter_batched(
            || registry.borrow_mut().clear_caches(),
            |()| {
                let registry = registry.borrow();
                std::hint::black_box(registry.highlight(&ts_content, &options).unwrap());
            },
            BatchSize::PerIteration,
        )
    });
}

fn highlight_multiple_simple_benchmark(c: &mut Criterion) {
    let mut registry =
        Registry::load_from_file("builtin.zst").expect("Failed to load registry from builtin.zst");
    registry.link_grammars();
    let registry = RefCell::new(registry);

    let ts_content = fs::read_to_string("src/fixtures/samples/simple.ts").unwrap();

    let options = HighlightOptions::new("typescript", ThemeVariant::Single("vitesse-black"));

    c.bench_function("highlight multiple simple.ts", |b| {
        b.iter_batched(
            || registry.borrow_mut().clear_caches(),
            |()| {
                // should not be 5x slower than "highlight simple.ts"
                for _ in 0..5 {
                    std::hint::black_box(
                        registry.borrow().highlight(&ts_content, &options).unwrap(),
                    );
                }
            },
            BatchSize::PerIteration,
        )
    });
}

fn highlight_cold_benchmark(c: &mut Criterion) {
    let registry = RefCell::new(Registry::load_from_file("builtin.zst").unwrap());
    let mut group = c.benchmark_group("highlight cold");
    group.sample_size(50);
    for &(grammar, path) in SAMPLES {
        let content = fs::read_to_string(path).unwrap();
        let options = HighlightOptions::new(grammar, ThemeVariant::Single("vitesse-black"));
        registry.borrow().highlight(&content, &options).unwrap();
        group.bench_function(bench_name(grammar, path), |b| {
            b.iter_batched(
                || registry.borrow_mut().clear_caches(),
                |()| {
                    let registry = registry.borrow();
                    std::hint::black_box(registry.highlight(&content, &options).unwrap());
                },
                BatchSize::PerIteration,
            )
        });
    }
    group.finish();
}

fn highlight_warm_benchmark(c: &mut Criterion) {
    let registry = Registry::load_from_file("builtin.zst").unwrap();
    let mut group = c.benchmark_group("highlight warm");
    for &(grammar, path) in SAMPLES {
        let content = fs::read_to_string(path).unwrap();
        let options = HighlightOptions::new(grammar, ThemeVariant::Single("vitesse-black"));
        registry.highlight(&content, &options).unwrap();
        group.bench_function(bench_name(grammar, path), |b| {
            b.iter(|| std::hint::black_box(registry.highlight(&content, &options).unwrap()))
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    highlight_simple_benchmark,
    highlight_multiple_simple_benchmark,
    highlight_cold_benchmark,
    highlight_warm_benchmark,
);
criterion_main!(benches);
