use giallo::registry::Registry;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Building Registry with all grammars and themes from grammars-themes folder...");

    let mut registry = Registry::default();
    let mut grammar_count = 0;
    let mut theme_count = 0;
    let mut grammar_errors = 0;
    let mut theme_errors = 0;

    // Load grammars
    println!("\nLoading grammars...");
    let grammars_dir = "grammars-themes/packages/tm-grammars/grammars";

    for entry in fs::read_dir(grammars_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension() == Some("json".as_ref()) {
            let grammar_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap();

            match registry.add_grammar_from_path(&path) {
                Ok(_) => {
                    println!("✓ Loaded grammar: {}", grammar_name);
                    grammar_count += 1;
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
            let theme_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap();

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
    let size_reduction = ((uncompressed_size as f64 - compressed_size as f64) / uncompressed_size as f64) * 100.0;

    println!("\n=== COMPRESSION RESULTS ===");
    println!("Uncompressed MessagePack: {:.2} MB ({} bytes)", uncompressed_mb, uncompressed_size);
    println!("Compressed file:          {:.2} MB ({} bytes)", compressed_mb, compressed_size);
    println!("Compression ratio:        {:.2}x smaller", compression_ratio);
    println!("Size reduction:           {:.1}% smaller", size_reduction);
    println!("✓ Registry saved to builtin.msgpack");

    println!("\nBuild complete!");

    Ok(())
}
