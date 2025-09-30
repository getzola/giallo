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

    let mut scope_list: Vec<String> = scopes.into_iter().collect();
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
        #[derive(Copy, Clone, Debug, Eq, PartialEq)]\n\
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
