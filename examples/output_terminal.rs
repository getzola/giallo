use std::env;
use std::fs;

use giallo::{HighlightOptions, Registry, ThemeVariant};
use giallo::{RenderOptions, TerminalRenderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!(
            "Usage: {} <file_path> <language> <theme> [dark_theme]",
            args[0]
        );
        eprintln!("Examples:");
        eprintln!(
            "  Single theme:     cargo run --example output_terminal --features dump -- file.js javascript catppuccin-frappe"
        );
        std::process::exit(1);
    }

    let file_path = &args[1];
    let language = &args[2];
    let theme = &args[3];

    let mut registry = Registry::builtin().unwrap();
    registry.link_grammars();

    let file_content = fs::read_to_string(file_path)?;

    let options = HighlightOptions::new(language, ThemeVariant::Single(theme));

    let highlighted = registry.highlight(&file_content, options)?;
    let render_options = RenderOptions {
        show_line_numbers: true,
        ..Default::default()
    };
    let rendered = TerminalRenderer::default().render(&highlighted, &render_options);

    println!("{rendered}");

    Ok(())
}
