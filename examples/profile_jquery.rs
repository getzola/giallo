use giallo::{HighlightOptions, Registry, ThemeVariant};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = Registry::load_from_file("builtin.zst")?;
    registry.link_grammars();
    let jquery_content = fs::read_to_string("src/fixtures/samples/jquery.js")?;

    let options = HighlightOptions::new("javascript", ThemeVariant::Single("vitesse-black"));

    let highlighted_tokens = registry.highlight(&jquery_content, &options)?;

    // Use the result to prevent optimization from removing the work
    std::hint::black_box(highlighted_tokens);

    Ok(())
}
