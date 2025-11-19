use crate::highlight::HighlightedText;
use crate::themes::Style;

pub fn render(default_style: Style, tokens: Vec<Vec<HighlightedText>>) -> String {
    let mut lines = Vec::with_capacity(tokens.len() + 4);
    for line_tokens in tokens {
        let mut line = Vec::with_capacity(line_tokens.len());
        for tok in line_tokens {
            if tok.style == default_style {
                line.push(format!("<span>{}</span>", tok.text));
            } else {
                let mut css_style = String::with_capacity(30);
                if tok.style.foreground != default_style.foreground {
                    css_style.push_str(&format!("color: {};", tok.style.foreground.as_hex()));
                }
                if tok.style.background != default_style.background {
                    css_style.push_str(&format!(
                        "background-color: {};",
                        tok.style.background.as_hex()
                    ));
                }
                for font_attr in tok.style.font_style.css_attributes() {
                    css_style.push_str(font_attr);
                }
                line.push(format!(r#"<span style="{css_style}">{}</span>"#, tok.text));
            }
        }
        lines.push(line.join(""));
    }

    let lines = lines.join("\n");
    format!(
        r#"<pre style="color: {}; background-color: {};"><code>{lines}</code></pre>"#,
        default_style.foreground.as_hex(),
        default_style.background.as_hex()
    )
}
