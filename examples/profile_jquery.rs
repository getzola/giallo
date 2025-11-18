use giallo::registry::{HighlightOptions, Registry};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = Registry::load_from_file("builtin.msgpack")?;
    let jquery_content = fs::read_to_string("src/fixtures/samples/jquery.js")?;

    let options = HighlightOptions {
        lang: "javascript",
        theme: "vitesse-black",
        merge_whitespaces: true,
        merge_same_style_tokens: true,
    };

    let highlighted_tokens = registry.highlight(&jquery_content, options)?;

    // Use the result to prevent optimization from removing the work
    std::hint::black_box(highlighted_tokens);

    Ok(())
}
