use giallo::GIALLO_CSS;
use std::env;
use std::fs;

use giallo::{HighlightOptions, Registry, ThemeVariant};
use giallo::{HtmlRenderer, RenderOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 4 {
        eprintln!(
            "Usage: {} <file_path> <language> <theme> [dark_theme]",
            args[0]
        );
        eprintln!("Examples:");
        eprintln!(
            "  Single theme:     cargo run --example output_html --features dump -- file.js javascript catppuccin-frappe"
        );
        eprintln!(
            "  Light/dark theme: cargo run --example output_html --features dump -- file.js javascript catppuccin-latte catppuccin-frappe"
        );
        std::process::exit(1);
    }

    let file_path = &args[1];
    let language = &args[2];
    let theme = &args[3];
    let dark_theme = args.get(4).map(|s| s.as_str());

    let mut registry = Registry::load_from_file("builtin.zst")?;
    registry.link_grammars();
    let file_content = fs::read_to_string(file_path)?;

    let options = match dark_theme {
        Some(dark) => HighlightOptions::new(language, ThemeVariant::Dual { light: theme, dark }),
        None => HighlightOptions::new(language, ThemeVariant::Single(theme)),
    };

    let highlighted = registry.highlight(&file_content, &options)?;
    let render_options = RenderOptions {
        show_line_numbers: true,
        ..Default::default()
    };
    let rendered = HtmlRenderer::default().render(&highlighted, &render_options);

    println!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Syntax Highlighted Code</title>
    <style>
    html {{
        font-size: 16px;
    }}
    {GIALLO_CSS}
    </style>
</head>
<body>
{rendered}
</body>
</html>"#,
    );

    Ok(())
}
