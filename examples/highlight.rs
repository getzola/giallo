use giallo::{HighlightStyle, HtmlRenderer, Theme};
use std::fs::read_to_string;
use std::io::Write;

fn main() {
    let theme = Theme::new();
    let content = read_to_string("src/highlight.rs").unwrap();
    let mut renderer = HtmlRenderer::new(HighlightStyle::Inline, &theme);
    renderer.render("rs", content.as_bytes());
    let output = std::str::from_utf8(&renderer.html).unwrap();
    let mut file = std::fs::File::create("out.html").unwrap();
    write!(file, "{}", output).unwrap();
}