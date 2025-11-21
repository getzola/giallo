use std::fs;

use giallo::{HighlightOptions, Registry};
use giallo::{HtmlRenderer, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = Registry::load_from_file("builtin.msgpack")?;
    let jquery_content = fs::read_to_string("grammars-themes/samples/javascript.sample")?;

    let options = HighlightOptions {
        lang: "javascript",
        theme: "catppuccin-frappe",
        merge_whitespaces: true,
        merge_same_style_tokens: true,
    };

    let highlighted = registry.highlight(&jquery_content, options)?;
    let render_options = Options::default();
    let rendered = HtmlRenderer::default().render(&highlighted, &render_options);
    println!("{}", rendered);
    Ok(())
}
