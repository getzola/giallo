use giallo::registry::Registry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Deserialize, Serialize)]
struct GrammarMetadata {
    name: String,
    aliases: Vec<String>,
    #[serde(rename = "scopeName")]
    scope_name: String,
}

fn load_grammar_metadata() -> Result<HashMap<String, Vec<String>>, Box<dyn std::error::Error>> {
    let metadata_path = "grammar_metadata.json";

    // Check if metadata file exists
    if !std::path::Path::new(metadata_path).exists() {
        println!("⚠️  Grammar metadata file not found at {}", metadata_path);
        println!("   Run 'node scripts/extract-grammar-metadata.js' to generate it");
        return Ok(HashMap::new());
    }

    let metadata_content = fs::read_to_string(metadata_path)?;
    let metadata: Vec<GrammarMetadata> = serde_json::from_str(&metadata_content)?;

    // Create lookup map from grammar name to aliases
    let mut alias_map = HashMap::new();
    for grammar in metadata {
        if !grammar.aliases.is_empty() {
            alias_map.insert(grammar.name.clone(), grammar.aliases);
        }
    }

    println!(
        "📋 Loaded metadata for {} grammars with aliases",
        alias_map.len()
    );
    Ok(alias_map)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Building Registry with all grammars and themes from grammars-themes folder...");

    // Load grammar metadata (aliases)
    let alias_map = load_grammar_metadata()?;

    let mut registry = Registry::default();
    let mut grammar_count = 0;
    let mut theme_count = 0;
    let mut grammar_errors = 0;
    let mut theme_errors = 0;
    let mut aliases_registered = 0;

    // Load grammars
    println!("\nLoading grammars...");
    let grammars_dir = "grammars-themes/packages/tm-grammars/grammars";

    for entry in fs::read_dir(grammars_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension() == Some("json".as_ref()) {
            let grammar_name = path.file_stem().and_then(|s| s.to_str()).unwrap();

            match registry.add_grammar_from_path(&path) {
                Ok(_) => {
                    println!("✓ Loaded grammar: {}", grammar_name);
                    grammar_count += 1;

                    // Register aliases for this grammar if they exist
                    if let Some(aliases) = alias_map.get(grammar_name) {
                        for alias in aliases {
                            registry.add_alias(grammar_name, alias);
                            aliases_registered += 1;
                        }
                        println!(
                            "  └─ Registered {} aliases: [{}]",
                            aliases.len(),
                            aliases.join(", ")
                        );
                    }
                }
                Err(e) => {
                    eprintln!("✗ Failed to load grammar {}: {}", grammar_name, e);
                    grammar_errors += 1;
                }
            }
        }
    }

    // Load themes
    println!("\nLoading themes...");
    let themes_dir = "grammars-themes/packages/tm-themes/themes";

    for entry in fs::read_dir(themes_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension() == Some("json".as_ref()) {
            let theme_name = path.file_stem().and_then(|s| s.to_str()).unwrap();

            match registry.add_theme_from_path(theme_name, &path) {
                Ok(_) => {
                    println!("✓ Loaded theme: {}", theme_name);
                    theme_count += 1;
                }
                Err(e) => {
                    eprintln!("✗ Failed to load theme {}: {}", theme_name, e);
                    theme_errors += 1;
                }
            }
        }
    }

    println!("\nSummary:");
    println!("- Successfully loaded: {} grammars", grammar_count);
    println!("- Failed to load: {} grammars", grammar_errors);
    println!("- Successfully loaded: {} themes", theme_count);
    println!("- Failed to load: {} themes", theme_errors);
    println!("- Registered aliases: {} total", aliases_registered);

    // Serialize Registry to compressed MessagePack format
    println!("\nSerializing Registry with MessagePack + flate2 compression...");

    // Calculate uncompressed MessagePack size for comparison
    let msgpack_data = rmp_serde::to_vec(&registry)?;
    let uncompressed_size = msgpack_data.len();
    let uncompressed_mb = uncompressed_size as f64 / (1024.0 * 1024.0);

    // Save compressed version using Registry's dump_to_file method
    registry.dump_to_file("builtin.msgpack")?;

    // Check compressed file size
    let compressed_metadata = fs::metadata("builtin.msgpack")?;
    let compressed_size = compressed_metadata.len();
    let compressed_mb = compressed_size as f64 / (1024.0 * 1024.0);

    // Calculate compression statistics
    let compression_ratio = uncompressed_size as f64 / compressed_size as f64;
    let size_reduction =
        ((uncompressed_size as f64 - compressed_size as f64) / uncompressed_size as f64) * 100.0;

    println!("\n=== COMPRESSION RESULTS ===");
    println!(
        "Uncompressed MessagePack: {:.2} MB ({} bytes)",
        uncompressed_mb, uncompressed_size
    );
    println!(
        "Compressed file:          {:.2} MB ({} bytes)",
        compressed_mb, compressed_size
    );
    println!(
        "Compression ratio:        {:.2}x smaller",
        compression_ratio
    );
    println!("Size reduction:           {:.1}% smaller", size_reduction);
    println!("✓ Registry saved to builtin.msgpack");

    println!("\nBuild complete!");

    Ok(())
}
