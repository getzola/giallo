use criterion::{criterion_group, criterion_main, Criterion};
use giallo::grammars::RawGrammar;
use giallo::tokenizer::Tokenizer;

fn criterion_benchmark(c: &mut Criterion) {
    let json_input = r#"{"name": "John", "age": 30, "active": true, "score": 95.5, "tags": ["developer", "rust"], "address": null}"#;
    let json_grammar_path = "grammars-themes/packages/tm-grammars/grammars/json.json";
    let raw_grammar = RawGrammar::load_from_file(json_grammar_path).unwrap();
    let compiled_grammar = raw_grammar.compile().unwrap();

    c.bench_function("json tokenization", |b| {
        b.iter(|| {
            let mut tokenizer = Tokenizer::new(&compiled_grammar);
            let result = tokenizer
                .tokenize_line(json_input)
                .expect("Tokenization should succeed");
            std::hint::black_box(result);
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);