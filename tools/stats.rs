use std::collections::HashMap;
use std::fs;

use serde_json::Value;
use walkdir::WalkDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting grammar and scope analysis...");

    // Create output directory
    fs::create_dir_all("src/generated")?;

    // Step 1: Extract all scopes for informational purposes
    let (scopes, scope_to_grammar) = extract_all_scopes()?;

    println!("Scope extraction complete!");
    println!("- Found {} unique scopes", scopes.len());

    // Step 2: Perform atom analysis
    println!("\nPerforming atom analysis...");
    let analysis = analyze_scope_atoms(&scopes);
    print_atom_statistics(&analysis);

    // Step 3: Perform capture analysis
    println!("\nPerforming capture analysis...");
    let capture_analysis = analyze_capture_scopes(&scopes);
    print_capture_statistics(&capture_analysis);

    // Step 4: Analyze longest scopes
    println!("\nPerforming longest scopes analysis...");
    analyze_longest_scopes(&scopes, &scope_to_grammar);

    // Step 5: Comparative analysis excluding cpp.json
    compare_with_without_cpp()?;

    // Step 6: Load and serialize all grammars (commented out for now)
    // let grammars = load_all_grammars()?;
    // serialize_grammars(&grammars)?;
    // println!("- Processed {} grammars", grammars.len());

    println!("\nAnalysis complete!");

    Ok(())
}

fn extract_all_scopes() -> Result<(Vec<String>, HashMap<String, String>), Box<dyn std::error::Error>>
{
    extract_scopes_with_exclusions(&[])
}

fn extract_scopes_with_exclusions(
    exclude_grammars: &[&str],
) -> Result<(Vec<String>, HashMap<String, String>), Box<dyn std::error::Error>> {
    let mut scopes = std::collections::HashSet::new();
    let mut scope_to_grammar = HashMap::new();

    // Extract scopes from grammar files
    let grammars_dir = "grammars-themes/packages/tm-grammars/grammars";
    if exclude_grammars.is_empty() {
        println!("Extracting scopes from {}...", grammars_dir);
    } else {
        println!(
            "Extracting scopes from {} (excluding {:?})...",
            grammars_dir, exclude_grammars
        );
    }

    for entry in WalkDir::new(grammars_dir) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.path().extension() == Some("json".as_ref()) {
            let grammar_name = entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Skip excluded grammars
            if exclude_grammars.contains(&grammar_name.as_str()) {
                continue;
            }

            let content = fs::read_to_string(entry.path())?;
            let grammar: Value = serde_json::from_str(&content)?;

            extract_scopes_from_value(&grammar, &mut scopes, &mut scope_to_grammar, &grammar_name);
        }
    }

    println!("Found {} raw grammar scopes", scopes.len());

    // Extract scopes from theme files
    let theme_scopes = extract_theme_scopes()?;
    println!("Found {} raw theme scopes", theme_scopes.len());

    // Combine all scopes
    scopes.extend(theme_scopes);

    // Apply hierarchical splitting to all scopes
    let mut hierarchical_scopes = std::collections::HashSet::new();
    let mut final_scope_to_grammar = HashMap::new();

    for scope_string in &scopes {
        let grammar_name = scope_to_grammar
            .get(scope_string)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        // First split on whitespace to get individual scopes
        for individual_scope in scope_string.split_whitespace() {
            // Track the original scope for each individual scope and its hierarchical parts
            final_scope_to_grammar.insert(individual_scope.to_string(), grammar_name.clone());
            add_hierarchical_scopes(
                individual_scope,
                &mut hierarchical_scopes,
                &mut final_scope_to_grammar,
                &grammar_name,
            );
        }
    }

    println!(
        "Generated {} total scopes (including hierarchy)",
        hierarchical_scopes.len()
    );

    let mut scope_list: Vec<String> = hierarchical_scopes.into_iter().collect();
    scope_list.sort();

    Ok((scope_list, final_scope_to_grammar))
}

fn extract_scopes_from_value(
    value: &Value,
    scopes: &mut std::collections::HashSet<String>,
    scope_to_grammar: &mut HashMap<String, String>,
    grammar_name: &str,
) {
    match value {
        Value::Object(obj) => {
            // Extract 'name' fields (these are scopes)
            if let Some(Value::String(name)) = obj.get("name") {
                scopes.insert(name.clone());
                scope_to_grammar.insert(name.clone(), grammar_name.to_string());
            }

            // Extract 'contentName' fields
            if let Some(Value::String(content_name)) = obj.get("contentName") {
                scopes.insert(content_name.clone());
                scope_to_grammar.insert(content_name.clone(), grammar_name.to_string());
            }

            // Extract 'scopeName' fields
            if let Some(Value::String(scope_name)) = obj.get("scopeName") {
                scopes.insert(scope_name.clone());
                scope_to_grammar.insert(scope_name.clone(), grammar_name.to_string());
            }

            // Recursively process all values
            for (_, v) in obj {
                extract_scopes_from_value(v, scopes, scope_to_grammar, grammar_name);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                extract_scopes_from_value(item, scopes, scope_to_grammar, grammar_name);
            }
        }
        _ => {}
    }
}

fn extract_theme_scopes() -> Result<std::collections::HashSet<String>, Box<dyn std::error::Error>> {
    // For now, return empty set - can be extended later to parse theme files
    Ok(std::collections::HashSet::new())
}

fn add_hierarchical_scopes(
    scope: &str,
    hierarchical_scopes: &mut std::collections::HashSet<String>,
    scope_to_grammar: &mut HashMap<String, String>,
    grammar_name: &str,
) {
    let parts: Vec<&str> = scope.split('.').collect();

    // Generate all hierarchical combinations
    // e.g., "source.rust.function" -> "source", "source.rust", "source.rust.function"
    for i in 1..=parts.len() {
        let hierarchical_scope = parts[..i].join(".");
        hierarchical_scopes.insert(hierarchical_scope.clone());
        scope_to_grammar.insert(hierarchical_scope, grammar_name.to_string());
    }
}

#[derive(Debug)]
struct AtomAnalysis {
    unique_atoms: std::collections::HashSet<String>,
    atom_counts: HashMap<String, usize>,
    scopes_by_atom_count: HashMap<usize, usize>,
    total_scopes: usize,
}

fn analyze_scope_atoms(scopes: &[String]) -> AtomAnalysis {
    let mut unique_atoms = std::collections::HashSet::new();
    let mut atom_counts = HashMap::new();
    let mut scopes_by_atom_count = HashMap::new();
    let mut total_individual_scopes = 0;

    for scope_string in scopes {
        // First split on whitespace to get individual scopes
        for individual_scope in scope_string.split_whitespace() {
            let atoms: Vec<&str> = individual_scope.split('.').collect();
            let atom_count = atoms.len();
            total_individual_scopes += 1;

            // Count scopes by number of atoms
            *scopes_by_atom_count.entry(atom_count).or_insert(0) += 1;

            // Collect unique atoms and count their frequency
            for atom in atoms {
                unique_atoms.insert(atom.to_string());
                *atom_counts.entry(atom.to_string()).or_insert(0) += 1;
            }
        }
    }

    AtomAnalysis {
        unique_atoms,
        atom_counts,
        scopes_by_atom_count,
        total_scopes: total_individual_scopes,
    }
}

fn print_atom_statistics(analysis: &AtomAnalysis) {
    println!("\n=== ATOM ANALYSIS ===");
    println!("Total unique atoms: {}", analysis.unique_atoms.len());
    println!("Total scopes analyzed: {}", analysis.total_scopes);

    // Calculate average atoms per scope
    let total_atom_instances: usize = analysis
        .scopes_by_atom_count
        .iter()
        .map(|(atom_count, scope_count)| atom_count * scope_count)
        .sum();
    let average_atoms = if analysis.total_scopes > 0 {
        total_atom_instances as f64 / analysis.total_scopes as f64
    } else {
        0.0
    };
    println!("Average atoms per scope: {:.2}", average_atoms);

    println!("\n--- Distribution by Atom Count ---");
    let mut atom_count_keys: Vec<usize> = analysis.scopes_by_atom_count.keys().cloned().collect();
    atom_count_keys.sort();

    for atom_count in atom_count_keys {
        let scope_count = analysis.scopes_by_atom_count[&atom_count];
        let percentage = (scope_count as f64 / analysis.total_scopes as f64) * 100.0;
        println!(
            "Scopes with {} atom{}: {} ({:.1}%)",
            atom_count,
            if atom_count == 1 { "" } else { "s" },
            scope_count,
            percentage
        );
    }

    println!("\n--- Most Common Atoms (Top 20) ---");
    let mut atom_frequency: Vec<(String, usize)> = analysis
        .atom_counts
        .iter()
        .map(|(atom, count)| (atom.clone(), *count))
        .collect();
    atom_frequency.sort_by(|a, b| b.1.cmp(&a.1));

    for (i, (atom, count)) in atom_frequency.iter().take(20).enumerate() {
        println!("{}. {} ({})", i + 1, atom, count);
    }
}

#[derive(Debug)]
struct CaptureAnalysis {
    scopes_with_captures: usize,
    regular_scopes: usize,
    downcase_count: usize,
    upcase_count: usize,
    capitalize_count: usize,
}

fn analyze_capture_scopes(scopes: &[String]) -> CaptureAnalysis {
    let mut scopes_with_captures = 0;
    let mut downcase_count = 0;
    let mut upcase_count = 0;
    let mut capitalize_count = 0;
    let mut total_individual_scopes = 0;

    for scope_string in scopes {
        // First split on whitespace to get individual scopes
        for individual_scope in scope_string.split_whitespace() {
            total_individual_scopes += 1;

            // Check if scope contains any capture references ($0, $1, ${1:/downcase}, etc.)
            if individual_scope.contains('$') {
                scopes_with_captures += 1;

                // Count specific transformations
                if individual_scope.contains(":/downcase}") {
                    downcase_count += 1;
                }
                if individual_scope.contains(":/upcase}") {
                    upcase_count += 1;
                }
                if individual_scope.contains(":/capitalize}") {
                    capitalize_count += 1;
                }
            }
        }
    }

    let regular_scopes = total_individual_scopes - scopes_with_captures;

    CaptureAnalysis {
        scopes_with_captures,
        regular_scopes,
        downcase_count,
        upcase_count,
        capitalize_count,
    }
}

fn print_capture_statistics(analysis: &CaptureAnalysis) {
    let total_scopes = analysis.scopes_with_captures + analysis.regular_scopes;
    let capture_percentage = if total_scopes > 0 {
        (analysis.scopes_with_captures as f64 / total_scopes as f64) * 100.0
    } else {
        0.0
    };
    let regular_percentage = if total_scopes > 0 {
        (analysis.regular_scopes as f64 / total_scopes as f64) * 100.0
    } else {
        0.0
    };

    println!("\n=== CAPTURE SCOPE ANALYSIS ===");
    println!(
        "Total scopes with captures: {} ({:.1}% of all scopes)",
        analysis.scopes_with_captures, capture_percentage
    );
    println!(
        "Regular scopes: {} ({:.1}%)",
        analysis.regular_scopes, regular_percentage
    );

    println!("\n--- Transformation Usage ---");
    println!("/downcase: {} scopes", analysis.downcase_count);
    println!("/upcase: {} scopes", analysis.upcase_count);
    println!("/capitalize: {} scopes", analysis.capitalize_count);
}

fn analyze_longest_scopes(scopes: &[String], scope_to_grammar: &HashMap<String, String>) {
    let mut scope_lengths = Vec::new();

    for scope_string in scopes {
        // First split on whitespace to get individual scopes
        for individual_scope in scope_string.split_whitespace() {
            let atoms: Vec<&str> = individual_scope.split('.').collect();
            let atom_count = atoms.len();

            let grammar_name = scope_to_grammar
                .get(individual_scope)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());

            scope_lengths.push((individual_scope.to_string(), atom_count, grammar_name));
        }
    }

    // Sort by atom count in descending order
    scope_lengths.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    println!("\n=== LONGEST SCOPES ANALYSIS ===");
    println!("Top 20 scopes with most atoms:\n");

    for (i, (scope, atom_count, grammar)) in scope_lengths.iter().take(20).enumerate() {
        println!(
            "{}. {} ({} atoms) - from {}.json",
            i + 1,
            scope,
            atom_count,
            grammar
        );
    }
}

fn compare_with_without_cpp() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== COMPARATIVE ANALYSIS (With vs Without C++ Grammars) ===");

    // Analyze with all grammars
    let (all_scopes, all_scope_to_grammar) = extract_scopes_with_exclusions(&[])?;
    let all_analysis = analyze_scope_atoms(&all_scopes);

    // Analyze without all C++ related grammars
    let cpp_grammars = ["c", "cpp", "cpp-macro", "objective-c", "objective-cpp"];
    let (no_cpp_scopes, no_cpp_scope_to_grammar) = extract_scopes_with_exclusions(&cpp_grammars)?;
    let no_cpp_analysis = analyze_scope_atoms(&no_cpp_scopes);

    // Calculate max atoms for each
    let all_max_atoms = all_analysis.scopes_by_atom_count.keys().max().unwrap_or(&0);
    let no_cpp_max_atoms = no_cpp_analysis
        .scopes_by_atom_count
        .keys()
        .max()
        .unwrap_or(&0);

    println!(
        "\n{:<25} | {:>12} | {:>15} | {:>12}",
        "Metric", "All Grammars", "Excluding C++", "Difference"
    );
    println!("{}", "=".repeat(70));

    // Total scopes
    let scope_diff = all_analysis.total_scopes as i32 - no_cpp_analysis.total_scopes as i32;
    let scope_diff_pct = if all_analysis.total_scopes > 0 {
        (scope_diff as f64 / all_analysis.total_scopes as f64) * 100.0
    } else {
        0.0
    };
    println!(
        "{:<25} | {:>12} | {:>15} | {:>+6} ({:+.1}%)",
        "Total scopes",
        all_analysis.total_scopes,
        no_cpp_analysis.total_scopes,
        scope_diff,
        scope_diff_pct
    );

    // Unique atoms
    let atom_diff =
        all_analysis.unique_atoms.len() as i32 - no_cpp_analysis.unique_atoms.len() as i32;
    let atom_diff_pct = if all_analysis.unique_atoms.len() > 0 {
        (atom_diff as f64 / all_analysis.unique_atoms.len() as f64) * 100.0
    } else {
        0.0
    };
    println!(
        "{:<25} | {:>12} | {:>15} | {:>+6} ({:+.1}%)",
        "Unique atoms",
        all_analysis.unique_atoms.len(),
        no_cpp_analysis.unique_atoms.len(),
        atom_diff,
        atom_diff_pct
    );

    // Average atoms per scope
    let all_avg = if all_analysis.total_scopes > 0 {
        all_analysis
            .scopes_by_atom_count
            .iter()
            .map(|(count, freq)| count * freq)
            .sum::<usize>() as f64
            / all_analysis.total_scopes as f64
    } else {
        0.0
    };

    let no_cpp_avg = if no_cpp_analysis.total_scopes > 0 {
        no_cpp_analysis
            .scopes_by_atom_count
            .iter()
            .map(|(count, freq)| count * freq)
            .sum::<usize>() as f64
            / no_cpp_analysis.total_scopes as f64
    } else {
        0.0
    };

    let avg_diff = all_avg - no_cpp_avg;
    println!(
        "{:<25} | {:>12.2} | {:>15.2} | {:>+12.2}",
        "Average atoms per scope", all_avg, no_cpp_avg, avg_diff
    );

    // Max atoms per scope
    let max_diff = all_max_atoms - no_cpp_max_atoms;
    println!(
        "{:<25} | {:>12} | {:>15} | {:>+12}",
        "Max atoms per scope", all_max_atoms, no_cpp_max_atoms, max_diff
    );

    // Show longest scopes without C++ grammars
    println!("\n--- Longest Scopes (Excluding All C++ Grammars) ---");
    let mut no_cpp_scope_lengths = Vec::new();

    for scope_string in &no_cpp_scopes {
        for individual_scope in scope_string.split_whitespace() {
            let atoms: Vec<&str> = individual_scope.split('.').collect();
            let atom_count = atoms.len();

            let grammar_name = no_cpp_scope_to_grammar
                .get(individual_scope)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());

            no_cpp_scope_lengths.push((individual_scope.to_string(), atom_count, grammar_name));
        }
    }

    no_cpp_scope_lengths.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    for (i, (scope, atom_count, grammar)) in no_cpp_scope_lengths.iter().take(10).enumerate() {
        println!(
            "{}. {} ({} atoms) - from {}.json",
            i + 1,
            scope,
            atom_count,
            grammar
        );
    }

    Ok(())
}
