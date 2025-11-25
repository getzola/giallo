use std::env;
use std::fs;

use giallo::{HighlightOptions, Registry};
use giallo::{HtmlRenderer, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    // Check if the correct number of arguments are provided
    if args.len() < 3 {
        eprintln!("Usage: {} <file_path> <language> [class_prefix]", args[0]);
        eprintln!(
            "Example: cargo run --example output_html --features dump -- path/to/file.js javascript"
        );
        std::process::exit(1);
    }

    let file_path = &args[1];
    let language = &args[2];

    let registry = Registry::load_from_file("builtin.msgpack")?;
    let file_content = fs::read_to_string(file_path)?;

    let options = HighlightOptions {
        lang: language,
        theme: "catppuccin-frappe",
        merge_whitespaces: true,
        merge_same_style_tokens: true,
    };

    let highlighted = registry.highlight(&file_content, options)?;
    let render_options = Options::default();
    let rendered = HtmlRenderer::default().render(&highlighted, &render_options);
    println!("{}", rendered);
    Ok(())
}
