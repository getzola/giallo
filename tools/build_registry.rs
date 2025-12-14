use giallo::Registry;
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

    // Create lookup map from grammar name to aliases (include all grammars)
    let mut alias_map = HashMap::new();
    for grammar in metadata {
        alias_map.insert(grammar.name.clone(), grammar.aliases);
    }

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
    let grammars_dir = "grammars-themes/packages/tm-grammars/grammars";

    for entry in fs::read_dir(grammars_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension() == Some("json".as_ref()) {
            let grammar_name = path.file_stem().and_then(|s| s.to_str()).unwrap();

            match registry.add_grammar_from_path(&path) {
                Ok(_) => {
                    grammar_count += 1;

                    // Register aliases for this grammar if they exist
                    if let Some(aliases) = alias_map.get(grammar_name) {
                        for alias in aliases {
                            registry.add_alias(grammar_name, alias);
                            aliases_registered += 1;
                        }
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
    let themes_dir = "grammars-themes/packages/tm-themes/themes";
    let mut theme_names: Vec<String> = Vec::new();

    for entry in fs::read_dir(themes_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension() == Some("json".as_ref()) {
            let theme_name = path.file_stem().and_then(|s| s.to_str()).unwrap();

            match registry.add_theme_from_path(&path) {
                Ok(_) => {
                    theme_names.push(theme_name.to_string());
                    theme_count += 1;
                }
                Err(e) => {
                    eprintln!("✗ Failed to load theme {}: {}", theme_name, e);
                    theme_errors += 1;
                }
            }
        }
    }

    registry.add_plain_grammar(&["txt"])?;

    // Build grammars list string
    let mut grammar_entries: Vec<_> = alias_map.iter().collect();
    grammar_entries.sort_by_key(|(name, _)| name.as_str());

    let mut grammars_list = String::new();
    for (name, aliases) in grammar_entries {
        if aliases.is_empty() {
            grammars_list.push_str(&format!("- {}\n", name));
        } else {
            grammars_list.push_str(&format!("- {} -> {}\n", name, aliases.join(", ")));
        }
    }

    // Build themes list string
    theme_names.sort();
    let mut themes_list = String::new();
    for name in &theme_names {
        themes_list.push_str(&format!("- {}\n", name));
    }

    // Print lists
    println!("\nSyntaxes:\n{}", grammars_list);
    println!("Themes:\n{}", themes_list);

    println!("Summary:");
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

    update_readme(&grammars_list, &themes_list)?;

    Ok(())
}

fn update_readme(grammars_list: &str, themes_list: &str) -> Result<(), Box<dyn std::error::Error>> {
    let readme_path = "README.md";
    let mut readme_content = fs::read_to_string(readme_path)?;

    let mut replace_content = |start_marker: &str, end_marker: &str, text: &str| {
        let start = readme_content.find(start_marker).expect("to find marker");
        let end = readme_content.find(end_marker).expect("to find marker");
        let before = &readme_content[..start + start_marker.len()];
        let after = &readme_content[end..];
        readme_content = format!("{before}\n{text}{after}");
    };

    replace_content(
        "<!-- GRAMMARS_START -->",
        "<!-- GRAMMARS_END -->",
        grammars_list,
    );
    replace_content("<!-- THEMES_START -->", "<!-- THEMES_END -->", themes_list);

    fs::write(readme_path, readme_content)?;
    println!("\n✓ Updated README.md");

    Ok(())
}
