use giallo::{HighlightOptions, Registry, ThemeVariant};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = Registry::load_from_file("builtin.zst")?;
    registry.link_grammars();
    let ts_content = fs::read_to_string("src/fixtures/samples/simple.ts")?;

    let options = HighlightOptions::new("typescript", ThemeVariant::Single("vitesse-black"));

    // Loop to make highlighting dominate the profile
    for _ in 0..10000 {
        let highlighted_tokens = registry.highlight(&ts_content, &options)?;
        std::hint::black_box(highlighted_tokens);
    }

    Ok(())
}
