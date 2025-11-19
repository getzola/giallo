use std::fs;

use giallo::Renderer;
use giallo::registry::{HighlightOptions, Registry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = Registry::load_from_file("builtin.msgpack")?;
    let jquery_content = fs::read_to_string("grammars-themes/samples/javascript.sample")?;

    let options = HighlightOptions {
        lang: "javascript",
        theme: "catppuccin-frappe",
        merge_whitespaces: true,
        merge_same_style_tokens: true,
    };

    let (default_style, highlighted_tokens) = registry.highlight(&jquery_content, options)?;
    let rendered = registry.render(default_style, highlighted_tokens, Renderer::Html);
    println!("{}", rendered);
    Ok(())
}
