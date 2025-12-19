use giallo::{HighlightOptions, HtmlRenderer, Registry, RenderOptions, ThemeVariant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the pre-built registry
    let mut registry = Registry::builtin()?;
    registry.link_grammars();

    let code = "let x = 42;";
    let options = HighlightOptions::new("javascript", ThemeVariant::Single("catppuccin-frappe"));
    let highlighted = registry.highlight(code, options)?;

    // Render to HTML
    let html = HtmlRenderer::default().render(&highlighted, &RenderOptions::default());
    println!("{html}");

    Ok(())
}
