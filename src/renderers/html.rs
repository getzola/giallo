use crate::registry::HighlightedCode;
use crate::renderers::Options;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, PartialEq, Clone, Default)]
pub struct HtmlRenderer<'h> {
    pub css_class_prefix: Option<&'h str>,
    pub other_metadata: BTreeMap<String, String>,
}

impl<'h> HtmlRenderer<'h> {
    pub fn render(&self, highlighted: &HighlightedCode, _options: &Options) -> String {
        let mut lines = Vec::with_capacity(highlighted.tokens.len() + 4);
        for line_tokens in &highlighted.tokens {
            let mut line = Vec::with_capacity(line_tokens.len());
            for tok in line_tokens {
                line.push(tok.as_html(self.css_class_prefix, &highlighted.theme.default_style));
            }
            lines.push(line.join(""));
        }

        let lines = lines.join("\n");
        let lang = highlighted.language;
        let fg = highlighted
            .theme
            .default_style
            .foreground
            .as_css_color_property();
        let bg = highlighted
            .theme
            .default_style
            .background
            .as_css_bg_color_property();

        format!(
            r#"<pre class="giallo" style="{fg} {bg}"><code data-lang="{lang}">{lines}</code></pre>"#
        )
    }
}

// From syntect
pub(crate) struct HtmlEscaped<'a>(pub &'a str);
impl fmt::Display for HtmlEscaped<'_> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Because the internet is always right, turns out there's not that many
        // characters to escape: http://stackoverflow.com/questions/7381974
        let Self(s) = *self;
        let pile_o_bits = s;
        let mut last = 0;
        for (i, ch) in s.bytes().enumerate() {
            match ch as char {
                '<' | '>' | '&' | '\'' | '"' => {
                    fmt.write_str(&pile_o_bits[last..i])?;
                    let s = match ch as char {
                        '>' => "&gt;",
                        '<' => "&lt;",
                        '&' => "&amp;",
                        '\'' => "&#39;",
                        '"' => "&quot;",
                        _ => unreachable!(),
                    };
                    fmt.write_str(s)?;
                    last = i + 1;
                }
                _ => {}
            }
        }

        if last < s.len() {
            fmt.write_str(&pile_o_bits[last..])?;
        }
        Ok(())
    }
}
