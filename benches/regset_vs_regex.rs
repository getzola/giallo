use criterion::{Criterion, criterion_group, criterion_main};
use onig::{RegSet, Regex, RegexOptions, Syntax};

fn individual_regex_benchmark(c: &mut Criterion) {
    // Test input - small string with some JavaScript-like content
    let test_input = "async function test() { return true; }";

    // Create 10 individual regex patterns (from JavaScript TextMate grammar)
    let patterns = vec![
        r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))abstract|declare|override|public|protected|private|readonly|static(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
        r"(?:(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))\b(async)\s+)?([$_[:alpha:]][$_[:alnum:]]*)\s*(?==>)",
        r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))async(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
        r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))true(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
        r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))false(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
        r"[$_[:alpha:]][$_[:alnum:]]*",
        r"([$_[:alpha:]][$_[:alnum:]]*)\s*(?:(\.)|(\?\.(?!\s*\d)))(?=\s*[$_[:alpha:]][$_[:alnum:]]*(\s*\??\.\s*[$_[:alpha:]][$_[:alnum:]]*)*\s*)",
        r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))catch|finally|throw|try(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
        r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))break|continue|goto\s+([$_[:alpha:]][$_[:alnum:]]*)(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
        r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))break|continue|do|goto|while(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
    ];

    // Compile regex objects outside the benchmark
    let compiled_regexes: Vec<Regex> = patterns
        .iter()
        .map(|pattern| Regex::with_options(pattern, RegexOptions::REGEX_OPTION_CAPTURE_GROUP, Syntax::default()).expect("Failed to compile regex"))
        .collect();

    c.bench_function("individual regex iteration", |b| {
        b.iter(|| {
            let mut total_matches = 0;
            for regex in &compiled_regexes {
                if let Some(_captures) = regex.captures(test_input) {
                    total_matches += 1;
                }
            }
            std::hint::black_box(total_matches);
        })
    });
}

fn regset_benchmark(c: &mut Criterion) {
    // Test input - same as individual regex benchmark
    let test_input = "async function test() { return true; }";

    c.bench_function("regset batch matching", |b| {
        b.iter(|| {
            // Same 10 patterns as individual benchmark
            let patterns = vec![
                r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))abstract|declare|override|public|protected|private|readonly|static(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
                r"(?:(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))\b(async)\s+)?([$_[:alpha:]][$_[:alnum:]]*)\s*(?==>)",
                r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))async(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
                r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))true(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
                r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))false(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
                r"[$_[:alpha:]][$_[:alnum:]]*",
                r"([$_[:alpha:]][$_[:alnum:]]*)\s*(?:(\.)|(\?\.(?!\s*\d)))(?=\s*[$_[:alpha:]][$_[:alnum:]]*(\s*\??\.\s*[$_[:alpha:]][$_[:alnum:]]*)*\s*)",
                r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))catch|finally|throw|try(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
                r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))break|continue|goto\s+([$_[:alpha:]][$_[:alnum:]]*)(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
                r"(?<![$_[:alnum:]])(?:(?<=\.\.\.)|(?<!\.))break|continue|do|goto|while(?![$_[:alnum:]])(?:(?=\.\.\.)|(?!\.))",
            ];

            // Create RegSet with capture groups enabled outside the benchmark
            let regset = RegSet::with_options(&patterns, RegexOptions::REGEX_OPTION_CAPTURE_GROUP)
                .expect("Failed to create RegSet");

            // RegSet search returns matches with positions
            if let Some((_pattern_index, captures)) = regset.captures_with_encoding(
                test_input,
                0,
                test_input.len(),
                onig::RegSetLead::Position,
                onig::SearchOptions::SEARCH_OPTION_NONE,
            ) {
                std::hint::black_box(captures.len());
            } else {
                std::hint::black_box(0);
            }
        })
    });
}

criterion_group!(benches, individual_regex_benchmark, regset_benchmark);
criterion_main!(benches);
