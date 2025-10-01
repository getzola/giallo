use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};

use phf_codegen::Map;
use serde_json::Value;
use walkdir::WalkDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting grammar and scope generation...");

    // Create output directory
    fs::create_dir_all("src/generated")?;

    // Step 1: Extract all scopes and generate PHF map
    let scopes = extract_all_scopes()?;
    generate_scope_map(&scopes)?;

    // Step 2: Load and serialize all grammars
    // let grammars = load_all_grammars()?;
    // serialize_grammars(&grammars)?;

    println!("Generation complete!");
    println!("- Generated {} unique scopes", scopes.len());
    // println!("- Processed {} grammars", grammars.len());

    Ok(())
}

fn extract_all_scopes() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut scopes = std::collections::HashSet::new();

    // Extract scopes from grammar files
    let grammars_dir = "grammars-themes/packages/tm-grammars/grammars";
    println!("Extracting scopes from {}...", grammars_dir);

    for entry in WalkDir::new(grammars_dir) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.path().extension() == Some("json".as_ref()) {
            let content = fs::read_to_string(entry.path())?;
            let grammar: Value = serde_json::from_str(&content)?;

            extract_scopes_from_value(&grammar, &mut scopes);
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
    for scope in &scopes {
        add_hierarchical_scopes(scope, &mut hierarchical_scopes);
    }

    println!(
        "Generated {} total scopes (including hierarchy)",
        hierarchical_scopes.len()
    );

    let mut scope_list: Vec<String> = hierarchical_scopes.into_iter().collect();
    scope_list.sort();

    Ok(scope_list)
}

fn extract_scopes_from_value(value: &Value, scopes: &mut std::collections::HashSet<String>) {
    match value {
        Value::Object(obj) => {
            // Extract 'name' fields (these are scopes)
            if let Some(Value::String(name)) = obj.get("name") {
                scopes.insert(name.clone());
            }

            // Extract 'contentName' fields
            if let Some(Value::String(content_name)) = obj.get("contentName") {
                scopes.insert(content_name.clone());
            }

            // Extract 'scopeName' fields
            if let Some(Value::String(scope_name)) = obj.get("scopeName") {
                scopes.insert(scope_name.clone());
            }

            // Recursively process all values
            for (_, v) in obj {
                extract_scopes_from_value(v, scopes);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                extract_scopes_from_value(item, scopes);
            }
        }
        _ => {}
    }
}

/// Extract scopes from theme files
fn extract_theme_scopes() -> Result<std::collections::HashSet<String>, Box<dyn std::error::Error>> {
    let mut scopes = std::collections::HashSet::new();
    let themes_dir = "grammars-themes/packages/tm-themes/themes";

    println!("Extracting scopes from {}...", themes_dir);

    for entry in WalkDir::new(themes_dir) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.path().extension() == Some("json".as_ref()) {
            let content = fs::read_to_string(entry.path()).unwrap_or_default();
            if let Ok(theme) = serde_json::from_str::<Value>(&content) {
                extract_theme_scopes_from_value(&theme, &mut scopes);
            }
        }
    }

    Ok(scopes)
}

/// Extract scope strings from theme JSON structure
fn extract_theme_scopes_from_value(value: &Value, scopes: &mut std::collections::HashSet<String>) {
    match value {
        Value::Object(obj) => {
            // Look for tokenColors array
            if let Some(Value::Array(token_colors)) = obj.get("tokenColors") {
                for token_rule in token_colors {
                    if let Value::Object(rule) = token_rule {
                        if let Some(scope_value) = rule.get("scope") {
                            match scope_value {
                                // Handle string scope
                                Value::String(scope) => {
                                    scopes.insert(scope.clone());
                                }
                                // Handle array of scopes
                                Value::Array(scope_array) => {
                                    for scope_item in scope_array {
                                        if let Value::String(scope) = scope_item {
                                            scopes.insert(scope.clone());
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Recursively process other values (in case of nested structures)
            for (_, v) in obj {
                extract_theme_scopes_from_value(v, scopes);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                extract_theme_scopes_from_value(item, scopes);
            }
        }
        _ => {}
    }
}

/// Add hierarchical scopes for a given scope string
/// e.g., "entity.name.function.js" -> ["entity", "entity.name", "entity.name.function", "entity.name.function.js"]
fn add_hierarchical_scopes(scope: &str, scopes: &mut std::collections::HashSet<String>) {
    let parts: Vec<&str> = scope.split('.').collect();
    let mut accumulated = String::new();

    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            accumulated.push('.');
        }
        accumulated.push_str(part);
        scopes.insert(accumulated.clone());
    }
}

fn generate_scope_map(scopes: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    println!("Generating PHF scope map...");

    let mut phf_map = Map::new();

    for (i, scope) in scopes.iter().enumerate() {
        phf_map.entry(scope, i.to_string());
    }

    let output_path = "src/generated/scopes.rs";
    let mut file = BufWriter::new(File::create(output_path)?);

    writeln!(
        file,
        "// Auto-generated scope mappings - do not edit manually\n\
        #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]\n\
        pub struct ScopeId(pub u32);\n\n\
        pub static SCOPE_MAP: ::phf::Map<&'static str, u32> = {};\n",
        phf_map.build()
    )?;

    writeln!(
        file,
        "#[inline]\npub fn get_scope_id(scope: &str) -> Option<ScopeId> {{\n\
        \x20   SCOPE_MAP.get(scope).map(|&id| ScopeId(id))\n\
        }}"
    )?;

    println!("Generated scope map at {}", output_path);
    Ok(())
}

// fn load_all_grammars() -> Result<HashMap<String, Value>, Box<dyn std::error::Error>> {
//     let mut grammars = HashMap::new();
//     let grammars_dir = "grammars-themes/packages/tm-grammars/grammars";
//
//     println!("Loading grammars from {}...", grammars_dir);
//
//     for entry in WalkDir::new(grammars_dir) {
//         let entry = entry?;
//         if entry.file_type().is_file() && entry.path().extension() == Some("json".as_ref()) {
//             let content = fs::read_to_string(entry.path())?;
//             let grammar: Value = serde_json::from_str(&content)?;
//
//             // Use filename (without extension) as key
//             let filename = entry
//                 .path()
//                 .file_stem()
//                 .and_then(|s| s.to_str())
//                 .unwrap_or("unknown")
//                 .to_string();
//
//             grammars.insert(filename, grammar);
//         }
//     }
//
//     Ok(grammars)
// }
//
// fn serialize_grammars(grammars: &HashMap<String, Value>) -> Result<(), Box<dyn std::error::Error>> {
//     println!("Serializing grammars to binary...");
//
//     let output_path = "src/generated/grammars.bin";
//     let encoded = bincode::serialize(grammars)?;
//     let size = encoded.len();
//     fs::write(output_path, encoded)?;
//
//     println!(
//         "Generated grammar binary at {} ({} bytes)",
//         output_path, size
//     );
//     Ok(())
// }
