use criterion::{Criterion, criterion_group, criterion_main};
use giallo::registry::{HighlightOptions, Registry};
use std::fs;

fn highlight_jquery_benchmark(c: &mut Criterion) {
    // Load registry once for all benchmarks
    let registry = Registry::load_from_file("builtin.msgpack")
        .expect("Failed to load registry from builtin.msgpack");

    // Read jQuery file content once
    let jquery_content =
        fs::read_to_string("src/fixtures/samples/jquery.js").expect("Failed to read jQuery file");

    let options = HighlightOptions {
        lang: "javascript",
        theme: "vitesse-black", // Assuming this theme is available in the builtin registry
        merge_whitespaces: true,
        merge_same_style_tokens: true,
    };

    c.bench_function("highlight jquery.js", |b| {
        b.iter(|| {
            let result = registry
                .highlight(&jquery_content, options.clone())
                .unwrap();
            std::hint::black_box(result);
        })
    });
}

criterion_group!(benches, highlight_jquery_benchmark);
criterion_main!(benches);
