use giallo::{FontStyle, HighlightOptions, HighlightedCode, Registry, ThemeVariant};

fn render_html(highlighted: &HighlightedCode) -> String {
    let ThemeVariant::Single(theme) = &highlighted.theme else {
        panic!("Expected single theme");
    };
    let default_style = &theme.default_style;

    let mut html = format!(
        "<pre><code style=\"color:{};background:{};\">",
        default_style.foreground.as_hex(),
        default_style.background.as_hex()
    );

    for line in &highlighted.tokens {
        for token in line {
            let ThemeVariant::Single(style) = &token.style else {
                continue;
            };

            // Build CSS only for properties that differ from default
            let mut css = String::new();
            if style.foreground != default_style.foreground {
                css.push_str(&format!("color:{};", style.foreground.as_hex()));
            }
            if style.background != default_style.background {
                css.push_str(&format!("background:{};", style.background.as_hex()));
            }
            if style.font_style != default_style.font_style
                && style.font_style.contains(FontStyle::BOLD)
            {
                css.push_str("font-style:bold;");
            }

            // Escape HTML in token text
            let escaped = token
                .text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");

            if css.is_empty() {
                html.push_str(&format!("<span>{}</span>", escaped));
            } else {
                html.push_str(&format!("<span style=\"{}\">{}</span>", css, escaped));
            }
        }
        html.push('\n');
    }

    html.push_str("</code></pre>");
    html
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = Registry::load_from_file("builtin.zst")?;
    registry.link_grammars();
    let content = std::fs::read_to_string("grammars-themes/samples/rust.sample")?;

    let options = HighlightOptions::new("rust", ThemeVariant::Single("vitesse-black"));
    let highlighted = registry.highlight(&content, &options)?;

    let html = render_html(&highlighted);
    println!("{}", html);

    Ok(())
}
